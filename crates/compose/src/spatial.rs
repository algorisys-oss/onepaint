//! Spatial adjustment layers (blur, sharpen, distort…) rendered as pixels:
//! the filtered backdrop of their container (spec 52's oracle). The engine
//! renders them this way for the CPU compositor, and the GPU path is checked
//! against it.

use onepaint_doc::{Document, Layer, Node};
use onepaint_filters::{Frame, GLOBAL_REACH, Kind, Params, apply_spatial_in, is_identity, prepare_in, reach};
use onepaint_raster::{Rect, Surface, TILE, Tile};
use rayon::prelude::*;
use std::sync::Arc;

/// The kind of a live spatial adjustment layer, if `l` is one.
pub fn spatial_kind(l: &Layer) -> Option<Kind> {
    let k = l.adjustment.as_ref().and_then(|a| Kind::parse(&a.kind))?;
    (l.visible && !l.clipped && !k.is_color()).then_some(k)
}

/// `nodes` flattened as a filter's source: f16 tiles over the document, no
/// paper tooth (filter sources are unshaded paint).
pub fn backdrop(doc: &Document, nodes: &[Node]) -> Surface {
    let mut d = doc.clone_shallow();
    d.root = nodes.to_vec();
    d.source = None;
    d.set_paper(None);
    let flat = crate::flatten(&d, None);
    let mut s = Surface::new();
    s.write_rect(doc.bounds(), &flat);
    s.prune_empty();
    s
}

/// A filter over a whole surface (the engine's `apply_filter` without a
/// selection): pixelwise kinds per tile, spatial kinds over the content plus
/// their reach, clipped to the document.
pub fn apply(src: &Surface, doc: Rect, frame: Frame, kind: Kind, params: &Params) -> Surface {
    let mut out = src.clone();
    let Some(content) = src.content_bounds().and_then(|b| b.intersect(&doc)) else { return out };
    if is_identity(kind, params) {
        return out;
    }
    if kind.is_pixelwise() {
        let prepared = prepare_in(kind, params, frame);
        let keys: Vec<_> = src.tile_keys().into_iter().filter(|&(tx, ty)| Rect::from_tile(tx, ty).intersect(&doc).is_some()).collect();
        let tiles: Vec<_> = keys
            .par_iter()
            .filter_map(|&(tx, ty)| {
                let mut buf = src.tile(tx, ty)?.to_f32();
                for (p, px) in buf.chunks_exact_mut(4).enumerate() {
                    let (x, y) = (tx * TILE + p as i32 % TILE, ty * TILE + p as i32 / TILE);
                    let o = prepared.color_px([px[0], px[1], px[2], px[3]], x, y);
                    px.copy_from_slice(&o);
                }
                Some(((tx, ty), Arc::new(Tile::from_f32(&buf))))
            })
            .collect();
        for ((tx, ty), t) in tiles {
            out.set_tile(tx, ty, Some(t));
        }
        return out;
    }
    let r = reach(kind, params);
    let region = if r >= GLOBAL_REACH {
        doc
    } else {
        Rect::new(content.x - r, content.y - r, content.w + 2 * r, content.h + 2 * r).intersect(&doc).unwrap_or(content)
    };
    let buf = src.read_rect(region);
    let rf = Frame::region(region.x, region.y, frame.doc_w, frame.doc_h, frame.dpi).in_space(frame.space);
    let res = apply_spatial_in(kind, params, &buf, region.w as usize, region.h as usize, rf);
    out.write_rect(region, &res);
    for (tx, ty) in region.tiles() {
        out.prune_tile(tx, ty);
    }
    out
}

/// One spatial layer's pixels: its kind over the backdrop of `below`.
pub fn render_layer(doc: &Document, below: &[Node], kind: Kind, params: &Params) -> Surface {
    apply(&backdrop(doc, below), doc.bounds(), crate::doc_frame(doc), kind, params)
}

/// `doc` with every live spatial adjustment layer replaced by its rendered
/// pixels (in groups too, against the group's own content).
pub fn rendered(doc: &Document) -> Document {
    fn walk(doc: &Document, nodes: &mut [Node]) {
        for i in 0..nodes.len() {
            if let Node::Group(g) = &mut nodes[i] {
                walk(doc, &mut g.children);
                continue;
            }
            let Node::Layer(l) = &nodes[i] else { continue };
            let Some(kind) = spatial_kind(l) else { continue };
            let params = l.adjustment.as_ref().map(|a| a.params.clone()).unwrap_or_default();
            let pixels = render_layer(doc, &nodes[..i], kind, &params);
            if let Node::Layer(l) = &mut nodes[i] {
                l.adjustment = None;
                l.pixels = pixels;
            }
        }
    }
    let mut out = doc.clone();
    walk(doc, &mut out.root);
    out
}

// ── The GPU path (spec 52) ──

/// Whether the filter has a GPU pass with these params. Oil paint stays on the CPU: its quadrant choice
/// turns on variances that cancel to rounding noise, so GPUs pick other
/// quadrants at edges and pixels jump by whole values.
pub fn gpu_supported(kind: Kind, _p: &Params) -> bool {
    matches!(
        kind,
        Kind::GaussianBlur
            | Kind::MotionBlur
            | Kind::Sharpen
            | Kind::Emboss
            | Kind::Mosaic
            | Kind::Ripple
            | Kind::Halftone
            | Kind::Crystallize
            | Kind::Bloom
            | Kind::LineArt
    )
}

/// Whether a whole-canvas kind (`GLOBAL_REACH`) has a GPU pass (spec 53).
pub fn whole_supported(kind: Kind, _p: &Params) -> bool {
    matches!(
        kind,
        Kind::Twirl
            | Kind::Pinch
            | Kind::PolarCoordinates
            | Kind::ChromaticAberration
            | Kind::RadialBlur
            | Kind::SpinBlur
            | Kind::TiltShift
            | Kind::IrisBlur
            | Kind::FieldBlur
    )
}

/// A whole-canvas spatial layer (spec 53): its filter over the backdrop of
/// every tile, planned once per frame and shared by every tile's program.
#[derive(Debug)]
pub struct Whole {
    /// The layer's id.
    pub id: String,
    pub kind: Kind,
    pub params: Params,
    /// The document as a filter frame.
    pub frame: Frame,
    /// The backdrop (the layers below) on every tile of the document.
    pub backdrop: Vec<((i32, i32), Arc<crate::program::TileProgram>)>,
    /// Changes with anything the result depends on.
    pub key: u64,
    filtered: std::sync::OnceLock<Surface>,
}

impl Whole {
    pub(crate) fn new(
        id: String,
        kind: Kind,
        params: Params,
        frame: Frame,
        backdrop: Vec<((i32, i32), Arc<crate::program::TileProgram>)>,
    ) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (kind.as_str(), serde_json::to_string(&params).unwrap_or_default(), frame.doc_w, frame.doc_h).hash(&mut h);
        for ((x, y), p) in &backdrop {
            (x, y, crate::program::segment_key(&p.ops, p.origin())).hash(&mut h);
        }
        Self { id, kind, params, frame, backdrop, key: h.finish(), filtered: Default::default() }
    }

    /// The filtered document, as the oracle stores it: the backdrop tiles as
    /// f16, filtered over the whole document.
    pub fn filtered(&self) -> &Surface {
        self.filtered.get_or_init(|| {
            let tiles: Vec<_> = self
                .backdrop
                .par_iter()
                .filter_map(|((x, y), p)| {
                    let t = Tile::from_f32(&crate::program::run(p));
                    (!t.is_empty()).then(|| ((*x, *y), Arc::new(t)))
                })
                .collect();
            let mut src = Surface::new();
            for ((x, y), t) in tiles {
                src.set_tile(x, y, Some(t));
            }
            let doc = Rect::new(0, 0, self.frame.doc_w as i32, self.frame.doc_h as i32);
            apply(&src, doc, self.frame, self.kind, &self.params)
        })
    }
}

/// A spatial layer's pixels on one tile, as a program computes them: its
/// filter over the backdrop programs of the tiles within reach.
#[derive(Debug)]
pub struct SpatialSrc {
    pub kind: Kind,
    pub params: Params,
    pub reach: i32,
    pub tx: i32,
    pub ty: i32,
    /// Where the filter reads: the tile ± reach, inside the document.
    pub region: Rect,
    pub frame: Frame,
    /// The backdrop (the layers below) on each tile the region touches.
    pub backdrop: Vec<((i32, i32), Arc<crate::program::TileProgram>)>,
    /// Changes with anything the result depends on.
    pub key: u64,
    /// A whole-canvas layer's shared filter (spec 53); `backdrop` is then
    /// empty, as the whole backdrop is `whole`'s.
    pub whole: Option<Arc<Whole>>,
    /// A ghost group seen through its peg (spec 55): `[dx, dy, rot, scale]`.
    /// The backdrop is the group's content on the tiles the inverse
    /// transform reaches, and the filter resamples it; `kind` is unused.
    pub peg: Option<[f32; 4]>,
    /// A styled layer or group (spec 56); the backdrop list is then the
    /// node's own content on the tiles around this one.
    pub styled: Option<Arc<Styled>>,
    filtered: std::sync::OnceLock<Vec<f32>>,
}

/// A styled node's styles and what it blends its effects against: the
/// layers below it in its container, on this tile.
#[derive(Debug)]
pub struct Styled {
    pub styles: onepaint_doc::styles::LayerStyles,
    pub under: Arc<crate::program::TileProgram>,
}

impl SpatialSrc {
    pub(crate) fn new(
        kind: Kind,
        params: Params,
        (tx, ty): (i32, i32),
        region: Rect,
        frame: Frame,
        backdrop: Vec<((i32, i32), Arc<crate::program::TileProgram>)>,
    ) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (kind.as_str(), serde_json::to_string(&params).unwrap_or_default(), tx, ty).hash(&mut h);
        for ((x, y), p) in &backdrop {
            (x, y, crate::program::segment_key(&p.ops, p.origin())).hash(&mut h);
        }
        let reach = reach(kind, &params);
        Self {
            kind,
            params,
            reach,
            tx,
            ty,
            region,
            frame,
            backdrop,
            key: h.finish(),
            whole: None,
            peg: None,
            styled: None,
            filtered: Default::default(),
        }
    }

    /// Tile `(tx, ty)` of a ghost group through `peg`, reading `backdrop`
    /// (the group's content) over `region`.
    pub(crate) fn of_peg(
        peg: [f32; 4],
        (tx, ty): (i32, i32),
        region: Rect,
        frame: Frame,
        backdrop: Vec<((i32, i32), Arc<crate::program::TileProgram>)>,
    ) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ("peg", peg.map(f32::to_bits), tx, ty, frame.doc_w, frame.doc_h).hash(&mut h);
        for ((x, y), p) in &backdrop {
            (x, y, crate::program::segment_key(&p.ops, p.origin())).hash(&mut h);
        }
        Self {
            kind: Kind::GaussianBlur,
            params: Params::default(),
            reach: 0,
            tx,
            ty,
            region,
            frame,
            backdrop,
            key: h.finish(),
            whole: None,
            peg: Some(peg),
            styled: None,
            filtered: Default::default(),
        }
    }

    /// Tile `(tx, ty)` of a whole-canvas layer.
    pub(crate) fn of_whole(whole: Arc<Whole>, (tx, ty): (i32, i32)) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (whole.key, tx, ty).hash(&mut h);
        let region = Rect::new(0, 0, whole.frame.doc_w as i32, whole.frame.doc_h as i32);
        Self {
            kind: whole.kind,
            params: whole.params.clone(),
            reach: GLOBAL_REACH,
            tx,
            ty,
            region,
            frame: whole.frame,
            backdrop: Vec::new(),
            key: h.finish(),
            whole: Some(whole),
            peg: None,
            styled: None,
            filtered: Default::default(),
        }
    }

    /// Tile `(tx, ty)` of a styled node: `content` is its own programs on the
    /// tiles of `region` (the tile ± the styles' margin).
    pub(crate) fn of_styled(
        styled: Styled,
        (tx, ty): (i32, i32),
        region: Rect,
        frame: Frame,
        content: Vec<((i32, i32), Arc<crate::program::TileProgram>)>,
    ) -> Self {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        ("styled", styled.styles.to_json().to_string(), tx, ty, frame.doc_w, frame.doc_h).hash(&mut h);
        crate::program::segment_key(&styled.under.ops, styled.under.origin()).hash(&mut h);
        for ((x, y), p) in &content {
            (x, y, crate::program::segment_key(&p.ops, p.origin())).hash(&mut h);
        }
        Self {
            kind: Kind::GaussianBlur,
            params: Params::default(),
            reach: styled.styles.reach(),
            tx,
            ty,
            region,
            frame,
            backdrop: content,
            key: h.finish(),
            whole: None,
            peg: None,
            styled: Some(Arc::new(styled)),
            filtered: Default::default(),
        }
    }

    /// A styled node's tile (spec 56): the oracle's style pass over its
    /// content and backdrop, both through f16 as the GPU's cache images.
    fn styled(&self, st: &Styled) -> Vec<f32> {
        let tiles: std::collections::HashMap<(i32, i32), Vec<f32>> =
            self.backdrop.iter().map(|(at, p)| (*at, Tile::from_f32(&crate::program::run(p)).to_f32())).collect();
        let content = |x: i32, y: i32| tiles.get(&(x, y)).cloned();
        let under = Tile::from_f32(&crate::program::run(&st.under)).to_f32();
        crate::styles::styled_tile(&st.styles, (self.tx, self.ty), self.frame, &content, None, Some(&under))
    }

    /// A ghost group's tile through its peg, as `compose::pegged` draws it:
    /// each pixel samples the content through the inverse transform,
    /// bilinear, transparent outside the tiles composited.
    fn pegged(&self, [dx, dy, rot, scale]: [f32; 4]) -> Vec<f32> {
        use onepaint_raster::TILE_PX;
        let tiles: std::collections::HashMap<(i32, i32), Vec<f32>> =
            self.backdrop.iter().map(|(at, p)| (*at, Tile::from_f32(&crate::program::run(p)).to_f32())).collect();
        let at = |x: i32, y: i32| -> [f32; 4] {
            let Some(t) = tiles.get(&(x.div_euclid(TILE), y.div_euclid(TILE))) else { return [0.0; 4] };
            let o = (y.rem_euclid(TILE) * TILE + x.rem_euclid(TILE)) as usize * 4;
            [t[o], t[o + 1], t[o + 2], t[o + 3]]
        };
        let inv = crate::peg_inverse(self.frame, [dx, dy, rot, scale]);
        let (x0, y0) = ((self.tx * TILE) as f32, (self.ty * TILE) as f32);
        let mut out = vec![0.0; TILE_PX * 4];
        for py in 0..TILE {
            for pxl in 0..TILE {
                let (sx, sy) = inv(x0 + pxl as f32 + 0.5, y0 + py as f32 + 0.5);
                let (fx, fy) = (sx - 0.5, sy - 0.5);
                let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
                let (wx, wy) = (fx - ix as f32, fy - iy as f32);
                let (a, b, c, d) = (at(ix, iy), at(ix + 1, iy), at(ix, iy + 1), at(ix + 1, iy + 1));
                let o = ((py * TILE + pxl) * 4) as usize;
                for ch in 0..4 {
                    let top = a[ch] + (b[ch] - a[ch]) * wx;
                    let bot = c[ch] + (d[ch] - c[ch]) * wx;
                    out[o + ch] = top + (bot - top) * wy;
                }
            }
        }
        out
    }

    /// The filtered tile (premultiplied, `TILE × TILE` RGBA), as the oracle
    /// stores it (f16): the backdrop tiles as f16, filtered over the region.
    pub fn filtered(&self) -> &[f32] {
        self.filtered.get_or_init(|| {
            if let Some(w) = &self.whole {
                return w.filtered().tile(self.tx, self.ty).map_or_else(|| vec![0.0; onepaint_raster::TILE_PX * 4], |t| t.to_f32());
            }
            if let Some(peg) = self.peg {
                return self.pegged(peg);
            }
            if let Some(st) = &self.styled {
                return self.styled(st);
            }
            let r = self.region;
            let mut src = Surface::new();
            for ((x, y), p) in &self.backdrop {
                let t = Tile::from_f32(&crate::program::run(p));
                if !t.is_empty() {
                    src.set_tile(*x, *y, Some(Arc::new(t)));
                }
            }
            let buf = src.read_rect(r);
            let rf = Frame::region(r.x, r.y, self.frame.doc_w, self.frame.doc_h, self.frame.dpi).in_space(self.frame.space);
            let res = apply_spatial_in(self.kind, &self.params, &buf, r.w as usize, r.h as usize, rf);
            let mut out = Surface::new();
            out.write_rect(r, &res);
            out.tile(self.tx, self.ty).map_or_else(|| vec![0.0; onepaint_raster::TILE_PX * 4], |t| t.to_f32())
        })
    }
}
