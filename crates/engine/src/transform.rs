//! Move / free transform: lift the layer (or the selected part), preview it
//! through an affine matrix, then commit as one undo step.

use super::*;
use rayon::prelude::*;

/// 2-D affine map: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(x: f64, y: f64) -> Self {
        Affine { e: x, f: y, ..Self::IDENTITY }
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Affine { a: sx, d: sy, ..Self::IDENTITY }
    }

    pub fn rotate(r: f64) -> Self {
        let (s, c) = r.sin_cos();
        Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    pub fn scale_about(sx: f64, sy: f64, p: [f64; 2]) -> Self {
        Self::translate(-p[0], -p[1]).then(&Self::scale(sx, sy)).then(&Self::translate(p[0], p[1]))
    }

    pub fn rotate_about(r: f64, p: [f64; 2]) -> Self {
        Self::translate(-p[0], -p[1]).then(&Self::rotate(r)).then(&Self::translate(p[0], p[1]))
    }

    /// `self` followed by `next`.
    pub fn then(&self, n: &Affine) -> Affine {
        Affine {
            a: n.a * self.a + n.c * self.b,
            b: n.b * self.a + n.d * self.b,
            c: n.a * self.c + n.c * self.d,
            d: n.b * self.c + n.d * self.d,
            e: n.a * self.e + n.c * self.f + n.e,
            f: n.b * self.e + n.d * self.f + n.f,
        }
    }

    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        [self.a * p[0] + self.c * p[1] + self.e, self.b * p[0] + self.d * p[1] + self.f]
    }

    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine { a, b, c, d, e: -(a * self.e + c * self.f), f: -(b * self.e + d * self.f) })
    }

    /// Bounding box of `r` after the map.
    pub fn map_rect(&self, r: Rect) -> Rect {
        let (x0, y0, x1, y1) = (r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64);
        let pts = [self.apply([x0, y0]), self.apply([x1, y0]), self.apply([x0, y1]), self.apply([x1, y1])];
        let (mut mx, mut my, mut nx, mut ny) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in pts {
            mx = mx.min(p[0]);
            my = my.min(p[1]);
            nx = nx.max(p[0]);
            ny = ny.max(p[1]);
        }
        Rect::covering(mx as f32, my as f32, nx as f32, ny as f32)
    }
}

/// One layer's lifted content in a transform.
pub(crate) struct Lift {
    pub layer: NodeId,
    /// The layer with the lifted part taken out (shown once the box moves).
    pub remainder: Surface,
    /// Layer pixels before the transform (for cancel and the undo entry).
    pub original: Surface,
    /// Dense premultiplied copy of the lifted pixels over `bounds`.
    pub src: Vec<f32>,
    /// What this layer lifted (inside the shared box).
    pub bounds: Rect,
    pub preview: Surface,
    pub preview_bounds: Rect,
    /// A whole path or text layer: an affine commit maps its objects
    /// (Free Transform Path, PE:2330-2345) instead of baking pixels.
    pub vector: bool,
}

/// Lifted content being transformed: one or more layers moving as one
/// (happypaint `transformState.targets`, v0.168 / v0.205.1), under one box.
pub(crate) struct Floating {
    /// The active layer first: it previews through the compositor's overlay;
    /// the others show their result in their own pixels.
    pub lifts: Vec<Lift>,
    /// The handle box: everything that moves.
    pub bounds: Rect,
    pub matrix: Affine,
    /// The selection that was lifted (cleared while transforming, restored on cancel).
    pub saved_selection: Option<Arc<onepaint_selection::Mask>>,
    /// Clamp samples to the source edge instead of fading to transparent (image resize).
    pub clamp_edges: bool,
    /// Distort or mesh warp, replacing `matrix` while set.
    pub warp: Option<WarpMode>,
}

/// Free 4-corner distort (TL, TR, BR, BL) or a WARP_N² mesh lattice, doc px.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WarpMode {
    Quad([[f64; 2]; 4]),
    Mesh(Vec<[f64; 2]>),
}

/// `top` source-over `base`, tile by tile.
fn over(base: &Surface, top: &Surface) -> Surface {
    let mut out = base.clone();
    for ((tx, ty), t) in top.tiles() {
        let s = t.to_f32();
        let mut buf = out.tile(tx, ty).map(|t| t.to_f32()).unwrap_or_else(|| vec![0.0; TILE_PX * 4]);
        for i in (0..buf.len()).step_by(4) {
            let k = 1.0 - s[i + 3];
            for c in 0..4 {
                buf[i + c] = s[i + c] + buf[i + c] * k;
            }
        }
        let t = Tile::from_f32(&buf);
        out.set_tile(tx, ty, (!t.is_empty()).then(|| Arc::new(t)));
    }
    out
}

impl Lift {
    fn sample(&self, x: f64, y: f64, clamp: bool) -> [f32; 4] {
        // Bilinear on texel centres, transparent outside.
        let (fx, fy) = (x - 0.5 - self.bounds.x as f64, y - 0.5 - self.bounds.y as f64);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let (w, h) = (self.bounds.w as i64, self.bounds.h as i64);
        let at = |xi: i64, yi: i64| -> [f32; 4] {
            let (xi, yi) = if clamp { (xi.clamp(0, w - 1), yi.clamp(0, h - 1)) } else { (xi, yi) };
            if xi < 0 || yi < 0 || xi >= w || yi >= h {
                return [0.0; 4];
            }
            let i = ((yi * w + xi) * 4) as usize;
            [self.src[i], self.src[i + 1], self.src[i + 2], self.src[i + 3]]
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (p00, p10, p01, p11) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        let mut o = [0.0; 4];
        for c in 0..4 {
            let top = p00[c] + (p10[c] - p00[c]) * tx;
            let bot = p01[c] + (p11[c] - p01[c]) * tx;
            o[c] = top + (bot - top) * ty;
        }
        o
    }

    /// Map each destination cell back to its source uv rect over the shared
    /// box `b` (perspective-correct).
    fn render_cells(&mut self, clip: Rect, b: Rect, clamp: bool, cells: &[([[f64; 2]; 4], [f64; 4])]) {
        use crate::warp::{invert, quad_homography};
        self.preview = Surface::new();
        let pts = cells.iter().flat_map(|c| c.0);
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in pts {
            (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
        }
        let Some(target) = Rect::covering(x0 as f32, y0 as f32, x1 as f32, y1 as f32).intersect(&clip) else {
            self.preview_bounds = Rect::default();
            return;
        };
        self.preview_bounds = target;
        let (tw, th) = (target.w as usize, target.h as usize);
        let mut buf = vec![0.0f32; tw * th * 4];
        let mut filled = vec![false; tw * th];
        for (quad, uv) in cells {
            let Some(inv) = invert(&quad_homography(quad)) else { continue };
            let (mut cx0, mut cy0, mut cx1, mut cy1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in quad {
                (cx0, cy0, cx1, cy1) = (cx0.min(p[0]), cy0.min(p[1]), cx1.max(p[0]), cy1.max(p[1]));
            }
            let Some(cr) = Rect::covering(cx0 as f32, cy0 as f32, cx1 as f32, cy1 as f32).intersect(&target) else { continue };
            for y in cr.y..cr.bottom() {
                for x in cr.x..cr.right() {
                    let k = ((y - target.y) as usize) * tw + (x - target.x) as usize;
                    if filled[k] {
                        continue;
                    }
                    let [u, v] = crate::warp::apply(&inv, x as f64 + 0.5, y as f64 + 0.5);
                    if !(-1e-6..=1.0 + 1e-6).contains(&u) || !(-1e-6..=1.0 + 1e-6).contains(&v) {
                        continue;
                    }
                    let (su, sv) = (uv[0] + (uv[2] - uv[0]) * u, uv[1] + (uv[3] - uv[1]) * v);
                    let p = self.sample(b.x as f64 + su * b.w as f64, b.y as f64 + sv * b.h as f64, clamp);
                    filled[k] = true;
                    buf[k * 4..k * 4 + 4].copy_from_slice(&p);
                }
            }
        }
        self.preview.write_rect(target, &buf);
        self.preview.prune_empty();
    }

    /// Resample the lifted pixels into `preview`, clipped to `clip`.
    fn render(&mut self, clip: Rect, b: Rect, matrix: Affine, warp: Option<&WarpMode>, clamp: bool) {
        match warp {
            Some(WarpMode::Quad(q)) => return self.render_cells(clip, b, clamp, &[(*q, [0.0, 0.0, 1.0, 1.0])]),
            Some(WarpMode::Mesh(pts)) => {
                use crate::warp::{WARP_GRID, sample_warp};
                let g = WARP_GRID as f64;
                let mut cells = Vec::with_capacity(WARP_GRID * WARP_GRID);
                for gy in 0..WARP_GRID {
                    for gx in 0..WARP_GRID {
                        let (u0, u1, v0, v1) = (gx as f64 / g, (gx + 1) as f64 / g, gy as f64 / g, (gy + 1) as f64 / g);
                        let q = [sample_warp(pts, u0, v0), sample_warp(pts, u1, v0), sample_warp(pts, u1, v1), sample_warp(pts, u0, v1)];
                        cells.push((q, [u0, v0, u1, v1]));
                    }
                }
                return self.render_cells(clip, b, clamp, &cells);
            }
            None => {}
        }
        let inv = matrix.inverse();
        let target = matrix.map_rect(self.bounds).intersect(&clip);
        self.preview = Surface::new();
        self.preview_bounds = target.unwrap_or_default();
        let (Some(inv), Some(target)) = (inv, target) else {
            return;
        };
        let keys: Vec<_> = target.tiles().collect();
        let me = &*self;
        let tiles: Vec<_> = keys
            .par_iter()
            .filter_map(|&(tx, ty)| {
                let ix = target.intersect(&Rect::from_tile(tx, ty))?;
                let mut buf = vec![0.0f32; TILE_PX * 4];
                let mut any = false;
                for y in ix.y..ix.bottom() {
                    for x in ix.x..ix.right() {
                        let s = inv.apply([x as f64 + 0.5, y as f64 + 0.5]);
                        let p = me.sample(s[0], s[1], clamp);
                        if p[3] > 0.0 {
                            any = true;
                            let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
                            buf[i..i + 4].copy_from_slice(&p);
                        }
                    }
                }
                any.then(|| ((tx, ty), Arc::new(Tile::from_f32(&buf))))
            })
            .collect();
        for ((tx, ty), t) in tiles {
            self.preview.set_tile(tx, ty, Some(t));
        }
    }
}

/// `src` (dense premultiplied pixels over `bounds`) through `matrix`,
/// bilinear, clipped to `clip`; `clamp` repeats the edge instead of fading
/// out (an image resize).
pub(crate) fn resample(src: Vec<f32>, bounds: Rect, matrix: Affine, clip: Rect, clamp: bool) -> Surface {
    let mut l = Lift {
        layer: String::new(),
        remainder: Surface::new(),
        original: Surface::new(),
        src,
        bounds,
        preview: Surface::new(),
        preview_bounds: bounds,
        vector: false,
    };
    l.render(clip, bounds, matrix, None, clamp);
    l.preview
}

impl Floating {
    /// Untouched raster content: the layer shows as it was, with no preview over it.
    fn lift_at_rest(&self, l: &Lift) -> bool {
        !l.vector && self.warp.is_none() && self.matrix == Affine::IDENTITY
    }

    /// Nothing anywhere has moved.
    pub(crate) fn at_rest(&self) -> bool {
        self.lifts.iter().all(|l| self.lift_at_rest(l))
    }

    /// The previews' union.
    pub(crate) fn preview_bounds(&self) -> Rect {
        self.lifts.iter().fold(Rect::default(), |r, l| r.union(&l.preview_bounds))
    }

    /// Everything the transform has touched: the lifted and the previewed areas.
    fn reach(&self) -> Rect {
        self.lifts.iter().fold(self.bounds, |r, l| r.union(&l.bounds).union(&l.preview_bounds))
    }

    pub(crate) fn render(&mut self, clip: Rect) {
        let (b, m, clamp) = (self.bounds, self.matrix, self.clamp_edges);
        let warp = self.warp.clone();
        for l in &mut self.lifts {
            l.render(clip, b, m, warp.as_ref(), clamp);
        }
    }
}

impl Session {
    /// Transforms keep content that leaves the canvas (layer overflow): they
    /// render into a generous margin around it instead of clipping.
    pub(crate) fn transform_clip(&self) -> Rect {
        let m = 8192;
        let d = self.doc.bounds();
        Rect::new(d.x - m, d.y - m, d.w + 2 * m, d.h + 2 * m)
    }

    pub fn transform_active(&self) -> bool {
        self.transform.is_some()
    }

    /// Current matrix and the box around everything that moves (doc px), while transforming.
    pub fn transform_state(&self) -> Option<(Affine, Rect)> {
        self.transform.as_ref().map(|f| (f.matrix, f.bounds))
    }

    /// A transform is open and its box has moved, distorted or warped.
    pub fn transform_pending(&self) -> bool {
        self.transform.as_ref().is_some_and(|f| f.matrix != Affine::IDENTITY || f.warp.is_some())
    }

    /// The layers the transform in progress moves, the active one first.
    pub fn transform_layers(&self) -> Vec<NodeId> {
        self.transform.as_ref().map(|f| f.lifts.iter().map(|l| l.layer.clone()).collect()).unwrap_or_default()
    }

    pub(crate) fn transform_overlay(&self) -> Option<StrokeOverlay<'_>> {
        let f = self.transform.as_ref()?;
        let l = f.lifts.first().filter(|l| !f.lift_at_rest(l))?;
        Some(StrokeOverlay {
            mask: false,
            layer_id: &l.layer,
            stroke: &l.preview,
            opacity: 1.0,
            mode: StrokeMode::Paint(BlendMode::Normal),
        })
    }

    /// Lift the active layer (or its selected part), or with `ids` the
    /// layers picked in the Layers panel: they move as one, and a group
    /// means its children (happypaint `beginTransform(ids)`,
    /// `selectedTransformableIds`). Hidden and locked layers stay put.
    pub(crate) fn begin_transform(&mut self, ids: Vec<NodeId>) -> Result<(), EngineError> {
        self.commit_transform();
        let active = self.active.clone();
        let picked = if ids.is_empty() { vec![active.clone()] } else { ids };
        let mut targets = self.transformable_ids(&picked);
        if let Some(i) = targets.iter().position(|t| *t == active) {
            let a = targets.remove(i);
            targets.insert(0, a);
        }
        let clip = self.clip.clone();
        let region = clip.as_ref().and_then(|m| m.bounds()).map(|(x, y, w, h)| Rect::new(x as i32, y as i32, w as i32, h as i32));
        self.stroke_cancel();
        let mut lifts = Vec::new();
        for id in targets {
            let vector = clip.is_none() && (self.is_path_layer(&id) || self.is_text_layer(&id));
            if !vector {
                // A selection lifts pixels: text and paths become pixels first.
                if self.is_text_layer(&id) {
                    let _ = self.rasterize_text(&id);
                }
                if self.is_path_layer(&id) {
                    let _ = self.rasterize_path(&id);
                }
            }
            if let Some(l) = self.lift(&id, clip.as_deref(), region, vector) {
                lifts.push(l);
            }
        }
        if lifts.is_empty() {
            return Ok(());
        }
        let bounds = lifts.iter().fold(Rect::default(), |r, l| r.union(&l.bounds));
        let saved_selection = self.selection.clone();
        let mut f = Floating { lifts, bounds, matrix: Affine::IDENTITY, saved_selection, clamp_edges: false, warp: None };
        f.render(self.transform_clip());
        let lifted_selection = f.saved_selection.is_some();
        self.transform = Some(f);
        self.show_floating();
        if lifted_selection {
            // happypaint clears the marquee while transforming (PE:2016).
            self.set_selection_raw(None);
        }
        self.mark_dirty(bounds);
        Ok(())
    }

    /// Take layer `id`'s content (× the selection mask, inside `region`) out
    /// of it; `None` when it has nothing there to move.
    fn lift(&mut self, id: &str, mask: Option<&onepaint_selection::Mask>, region: Option<Rect>, vector: bool) -> Option<Lift> {
        let layer = self.doc.layer_mut(id).filter(|l| !l.locked && l.adjustment.is_none())?;
        let original = layer.pixels.clone();
        let content = original.content_bounds()?;
        let bounds = match region {
            Some(r) => content.intersect(&r)?,
            None => content,
        };
        let mut src = original.read_rect(bounds);
        if let Some(m) = mask {
            // Lift layer × mask; leave layer × (1 − mask).
            for y in bounds.y..bounds.bottom() {
                for x in bounds.x..bounds.right() {
                    let cov = m.coverage(x, y);
                    let i = (((y - bounds.y) * bounds.w + (x - bounds.x)) * 4) as usize;
                    for c in 0..4 {
                        src[i + c] *= cov;
                    }
                }
            }
            let rest: Vec<f32> = original.read_rect(bounds).iter().zip(&src).map(|(o, l)| o - l).collect();
            layer.pixels.write_rect(bounds, &rest);
            for (tx, ty) in bounds.tiles() {
                layer.pixels.prune_tile(tx, ty);
            }
        } else {
            layer.pixels.clear();
        }
        let remainder = layer.pixels.clone();
        Some(Lift { layer: id.to_string(), remainder, original, src, bounds, preview: Surface::new(), preview_bounds: bounds, vector })
    }

    /// The distort or warp in progress, if any.
    pub fn transform_warp(&self) -> Option<&WarpMode> {
        self.transform.as_ref().and_then(|f| f.warp.as_ref())
    }

    /// Switch to 4-corner distort or mesh warp, seeded from the current affine
    /// so the content doesn't jump.
    pub(crate) fn begin_warp(&mut self, mesh: bool) {
        let doc = self.transform_clip();
        let Some(f) = self.transform.as_mut() else { return };
        let b = f.bounds;
        let m = f.matrix;
        let (x, y, w, h) = (b.x as f64, b.y as f64, b.w as f64, b.h as f64);
        f.warp = Some(if mesh {
            WarpMode::Mesh(crate::warp::lattice(x, y, w, h).into_iter().map(|p| m.apply(p)).collect())
        } else {
            WarpMode::Quad([m.apply([x, y]), m.apply([x + w, y]), m.apply([x + w, y + h]), m.apply([x, y + h])])
        });
        f.matrix = Affine::IDENTITY;
        self.rerender(doc);
    }

    pub(crate) fn set_warp(&mut self, w: WarpMode) {
        let doc = self.transform_clip();
        let Some(f) = self.transform.as_mut() else { return };
        let ok = match (&f.warp, &w) {
            (Some(WarpMode::Quad(_)), WarpMode::Quad(_)) => true,
            (Some(WarpMode::Mesh(_)), WarpMode::Mesh(p)) => p.len() == crate::warp::WARP_N * crate::warp::WARP_N,
            _ => false,
        };
        if !ok {
            return;
        }
        f.warp = Some(w);
        self.rerender(doc);
    }

    pub(crate) fn set_transform(&mut self, m: Affine) {
        let doc = self.transform_clip();
        let Some(f) = self.transform.as_mut() else {
            return;
        };
        f.matrix = m;
        self.rerender(doc);
    }

    /// Re-render every preview after the box changed, and show it.
    fn rerender(&mut self, clip: Rect) {
        let Some(f) = self.transform.as_mut() else { return };
        let old = f.preview_bounds();
        f.render(clip);
        let new = f.preview_bounds();
        self.show_floating();
        self.mark_dirty(old.union(&new));
    }

    /// Each layer as the box stands: untouched content shows the layer as it
    /// was (a soft selection's lifted part and remainder don't recombine
    /// exactly). Moved, the first layer shows its remainder under the
    /// compositor's overlay; the others show the result in their pixels.
    fn show_floating(&mut self) {
        let Some(f) = self.transform.as_ref() else { return };
        let shown: Vec<(NodeId, Option<Surface>)> = f
            .lifts
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let px = if f.lift_at_rest(l) {
                    Some(l.original.clone())
                } else if i == 0 {
                    (!l.vector).then(|| l.remainder.clone())
                } else {
                    Some(over(&l.remainder, &l.preview))
                };
                (l.layer.clone(), px)
            })
            .collect();
        for (id, px) in shown {
            if let (Some(px), Some(l)) = (px, self.doc.layer_mut(&id)) {
                l.pixels = px;
            }
        }
    }

    pub(crate) fn cancel_transform(&mut self) {
        let Some(f) = self.transform.take() else {
            return;
        };
        let dirty = f.reach();
        for l in f.lifts {
            if let Some(layer) = self.doc.layer_mut(&l.layer) {
                layer.pixels = l.original;
            }
        }
        if f.saved_selection.is_some() {
            self.set_selection_raw(f.saved_selection.clone());
        }
        self.mark_dirty(dirty);
    }

    /// Commit every layer's move as one undo step.
    pub(crate) fn commit_transform(&mut self) {
        let Some(mut f) = self.transform.take() else {
            return;
        };
        let dirty = f.reach();
        self.mark_dirty(dirty);
        // Nothing moved: every layer is exactly as it was. Recombining the
        // lifted part with the remainder would lose alpha under a soft
        // selection, with nothing to undo (happypaint 0.195.1).
        if f.at_rest() {
            for l in f.lifts {
                if let Some(layer) = self.doc.layer_mut(&l.layer) {
                    layer.pixels = l.original;
                }
            }
            return;
        }
        let lifts = std::mem::take(&mut f.lifts);
        let restore = lifts.iter().any(|l| l.vector) && f.saved_selection.is_some();
        self.history.begin_transaction();
        for l in lifts {
            if l.vector {
                self.commit_vector_lift(&f, l);
            } else {
                self.commit_raster_lift(&f, l);
            }
        }
        self.history.end_transaction();
        if restore {
            self.set_selection_raw(f.saved_selection.clone());
        }
    }

    fn commit_raster_lift(&mut self, f: &Floating, l: Lift) {
        let result = over(&l.remainder, &l.preview);
        let mapped = self.mapped_vector_mask(f, &l.layer);
        let Some(layer) = self.doc.layer_mut(&l.layer) else {
            return;
        };
        let mut keys: BTreeSet<(i32, i32)> = result.tile_keys().into_iter().collect();
        keys.extend(l.original.tile_keys());
        let mut changes: Vec<TileChange> = Vec::new();
        let mut bytes = 0;
        for (tx, ty) in keys {
            let after = result.tile(tx, ty).cloned();
            let before = l.original.tile(tx, ty).cloned();
            if before.as_deref() != after.as_deref() {
                bytes += tile_bytes(&before) + tile_bytes(&after);
                changes.push(((tx, ty), before, after));
            }
        }
        layer.pixels = result;
        if !changes.is_empty() {
            self.record(Change::Tiles { layer: l.layer.clone(), plane: Plane::Pixels, tiles: changes }, bytes);
            self.doc.touch();
            self.history_changed();
        }
        if let Some(v) = mapped {
            let id = l.layer.clone();
            let _ = self.tree_edit(None, |s| {
                if let Some(layer) = s.doc.layer_mut(&id) {
                    layer.set_vector_mask(Some(v));
                }
                Ok(())
            });
        }
    }

    /// The layer's vector mask through an affine transform's map: `None`
    /// without one, when it is unlinked, or for a warp or distort, which
    /// leave it in place (happypaint `commitVectorMaskTransform`).
    fn mapped_vector_mask(&self, f: &Floating, id: &str) -> Option<onepaint_doc::VectorMask> {
        let m = f.matrix;
        if f.warp.is_some() || m == Affine::IDENTITY {
            return None;
        }
        let v = self.doc.layer(id)?.vector_mask().filter(|v| !v.unlinked)?;
        Some(v.transformed([m.a, m.b, m.c, m.d, m.e, m.f]))
    }

    /// Commit a transform of a whole path or text layer. Affine maps keep
    /// paths editable; text keeps its objects under a rotation plus uniform
    /// scale. A distort, warp, stretch or mirror of text bakes to pixels
    /// (PE:2324-2345).
    fn commit_vector_lift(&mut self, f: &Floating, l: Lift) {
        let m = f.matrix;
        // Put the layer back, so the undo step starts from the untouched layer.
        if let Some(layer) = self.doc.layer_mut(&l.layer) {
            layer.pixels = l.original.clone();
        }
        if f.warp.is_none() && m == Affine::IDENTITY {
            return;
        }
        let (w, h, space) = (self.doc.width, self.doc.height, self.doc.space());
        let id = l.layer.clone();
        let uniform = (m.a - m.d).abs() < 1e-6 && (m.b + m.c).abs() < 1e-6 && m.a * m.d - m.b * m.c > 0.0;
        let mapped = self.mapped_vector_mask(f, &id);
        let warped = f.warp.is_some();
        let preview = l.preview;
        let _ = self.tree_edit(None, move |s| {
            if let (Some(v), Some(layer)) = (mapped, s.doc.layer_mut(&id)) {
                layer.set_vector_mask(Some(v));
            }
            let paths = s.paths(&id);
            let texts = s.texts(&id);
            let Some(layer) = s.doc.layer_mut(&id) else { return Ok(()) };
            if !warped && !paths.is_empty() {
                let pm = onepaint_stroke::bezier::Affine { a: m.a, b: m.b, c: m.c, d: m.d, e: m.e, f: m.f };
                let mapped: Vec<_> = paths.iter().map(|p| onepaint_stroke::bezier::affine(p, &pm)).collect();
                path_ops::write_paths(layer, &mapped, w, h, space);
            } else if !warped && !texts.is_empty() && uniform {
                let mapped: Vec<_> = texts.iter().map(|t| transform_text(t, &m)).collect();
                text_ops::write_texts(layer, &mapped, w, h, space);
            } else {
                // Bake: the objects can't express this map.
                layer.extra.remove("paths");
                layer.extra.remove("texts");
                layer.extra.remove("text");
                layer.pixels = preview;
            }
            Ok(())
        });
    }
}

/// A text object under a rotation plus uniform scale `m`: the anchor moves
/// with the pixels; size, tracking and wrap width scale (PE:2447-2464).
pub(crate) fn transform_text(t: &onepaint_text::TextSpec, m: &Affine) -> onepaint_text::TextSpec {
    let k = m.a.hypot(m.b);
    let [x, y] = m.apply([t.x, t.y]);
    let mut out = t.clone();
    out.x = x;
    out.y = y;
    out.font_size_px *= k;
    if let Some(runs) = out.runs.as_mut() {
        for r in runs {
            if let Some(s) = r.font_size_px.as_mut() {
                *s *= k;
            }
        }
    }
    out.letter_spacing *= k;
    out.box_width = out.box_width.map(|b| b * k);
    out.rotation += m.b.atan2(m.a);
    out
}

/// Which part of the transform box a pointer grabbed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// 0 TL, 1 TR, 2 BR, 3 BL.
    Corner(u8),
    /// 0 top, 1 right, 2 bottom, 3 left.
    Edge(u8),
    Rotate,
    Body,
}

/// Transformed handle positions (doc px).
#[derive(Clone, Copy, Debug)]
pub struct Handles {
    pub corners: [[f64; 2]; 4],
    pub edges: [[f64; 2]; 4],
    pub centre: [f64; 2],
}

/// happypaint's transform parameters: pivot at the document centre (PE:464-476).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformParams {
    pub dx: f64,
    pub dy: f64,
    pub sx: f64,
    pub sy: f64,
    pub rotation: f64,
}

impl Default for TransformParams {
    fn default() -> Self {
        Self { dx: 0.0, dy: 0.0, sx: 1.0, sy: 1.0, rotation: 0.0 }
    }
}

fn box_points(b: Rect) -> ([[f64; 2]; 4], [[f64; 2]; 4], [f64; 2]) {
    let (x0, y0, x1, y1) = (b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
    let (mx, my) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    ([[x0, y0], [x1, y0], [x1, y1], [x0, y1]], [[mx, y0], [x1, my], [mx, y1], [x0, my]], [mx, my])
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn len(v: [f64; 2]) -> f64 {
    v[0].hypot(v[1])
}

fn clamp_scale(s: f64) -> f64 {
    if s.abs() < 0.02 { 0.02 * if s < 0.0 { -1.0 } else { 1.0 } } else { s }
}

impl TransformParams {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    pub fn to_affine(&self, doc: [f64; 2]) -> Affine {
        let (cx, cy) = (doc[0] / 2.0, doc[1] / 2.0);
        let (s, c) = self.rotation.sin_cos();
        let (a, b, cc, d) = (c * self.sx, s * self.sx, -s * self.sy, c * self.sy);
        Affine { a, b, c: cc, d, e: cx + self.dx - (a * cx + cc * cy), f: cy + self.dy - (b * cx + d * cy) }
    }

    pub fn handles(&self, doc: [f64; 2], b: Rect) -> Handles {
        let m = self.to_affine(doc);
        let (cs, es, c) = box_points(b);
        Handles { corners: cs.map(|p| m.apply(p)), edges: es.map(|p| m.apply(p)), centre: m.apply(c) }
    }

    /// The rotate knob: `offset` doc px beyond the top edge, away from the centre.
    pub fn rotate_knob(&self, doc: [f64; 2], b: Rect, offset: f64) -> [f64; 2] {
        let h = self.handles(doc, b);
        let v = sub(h.edges[0], h.centre);
        let l = len(v).max(1e-9);
        [h.edges[0][0] + v[0] / l * offset, h.edges[0][1] + v[1] / l * offset]
    }

    /// Hit order: rotate, corners, edges, else body (`tol` and `rot_offset` in doc px).
    pub fn hit(&self, doc: [f64; 2], b: Rect, p: [f64; 2], tol: f64, rot_offset: f64) -> Handle {
        if len(sub(p, self.rotate_knob(doc, b, rot_offset))) <= tol {
            return Handle::Rotate;
        }
        let h = self.handles(doc, b);
        for (i, c) in h.corners.iter().enumerate() {
            if len(sub(p, *c)) <= tol {
                return Handle::Corner(i as u8);
            }
        }
        for (i, e) in h.edges.iter().enumerate() {
            if len(sub(p, *e)) <= tol {
                return Handle::Edge(i as u8);
            }
        }
        Handle::Body
    }

    /// Shift dx/dy so that original point `o` lands on `target`.
    fn pin(mut self, doc: [f64; 2], o: [f64; 2], target: [f64; 2]) -> Self {
        let now = self.to_affine(doc).apply(o);
        self.dx += target[0] - now[0];
        self.dy += target[1] - now[1];
        self
    }

    /// Scale by dragging a corner or edge from `grab` to `p` (transform-drag.ts).
    #[allow(clippy::too_many_arguments)]
    pub fn scale_drag(&self, doc: [f64; 2], b: Rect, handle: Handle, grab: [f64; 2], p: [f64; 2], shift: bool, alt: bool) -> Self {
        let (cs, es, centre) = box_points(b);
        let m = self.to_affine(doc);
        let (orig_handle, orig_opposite, axis_x) = match handle {
            Handle::Corner(i) => (cs[i as usize], cs[(i as usize + 2) % 4], None),
            Handle::Edge(i) => (es[i as usize], es[(i as usize + 2) % 4], Some(i % 2 == 1)),
            _ => return *self,
        };
        let pivot_o = if alt { centre } else { orig_opposite };
        let pivot = m.apply(pivot_o);
        let handle_t = m.apply(orig_handle);
        let _ = grab;
        let hv = sub(handle_t, pivot);
        let hl = len(hv).max(1e-9);
        let mut next = *self;
        match axis_x {
            None => {
                let r = len(sub(p, pivot)) / hl;
                next.sx = clamp_scale(self.sx * r);
                next.sy = clamp_scale(self.sy * r);
            }
            Some(is_x) => {
                let u = [hv[0] / hl, hv[1] / hl];
                let d = sub(p, pivot);
                let r = (d[0] * u[0] + d[1] * u[1]) / hl;
                if shift {
                    next.sx = clamp_scale(self.sx * r);
                    next.sy = clamp_scale(self.sy * r);
                } else if is_x {
                    next.sx = clamp_scale(self.sx * r);
                } else {
                    next.sy = clamp_scale(self.sy * r);
                }
            }
        }
        next.pin(doc, pivot_o, pivot)
    }

    /// Rotate about the transformed box centre by the pointer's angle change.
    pub fn rotate_drag(&self, doc: [f64; 2], b: Rect, grab: [f64; 2], p: [f64; 2]) -> Self {
        let (_, _, centre_o) = box_points(b);
        let c = self.to_affine(doc).apply(centre_o);
        let a0 = (grab[1] - c[1]).atan2(grab[0] - c[0]);
        let a1 = (p[1] - c[1]).atan2(p[0] - c[0]);
        let next = Self { rotation: self.rotation + (a1 - a0), ..*self };
        next.pin(doc, centre_o, c)
    }

    /// Body move; Shift snaps to 45° rays from the press point.
    pub fn move_drag(&self, grab: [f64; 2], p: [f64; 2], shift: bool) -> Self {
        let p = if shift { onepaint_selection::constrain_45(grab, p) } else { p };
        Self { dx: self.dx + p[0] - grab[0], dy: self.dy + p[1] - grab[1], ..*self }
    }
}
