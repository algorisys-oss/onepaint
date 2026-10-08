//! CPU reference compositor: walks the layer tree per 256² tile.
//!
//! Semantics follow happypaint `engine/compositor.ts:1462-2091` (isolated
//! groups, clip families, masks, inherit alpha). See `docs/specs/02-compositor.md`.

pub mod blend;
pub mod relief;

use blend::{blend_px, blend_px_alpha_lock, clip_over};
use onepaint_doc::{BlendMode, Document, Layer, Node};
use onepaint_filters::Frame;
use onepaint_panel::coverage::{TileCoverage, tile_coverage};
use onepaint_raster::{Px, Rect, Surface, TILE, TILE_PX, Tile};
use rayon::prelude::*;

type Buf = Vec<f32>;

/// How a stroke buffer lands on its layer (live preview and flatten).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrokeMode {
    Paint(BlendMode),
    Erase,
    AlphaLock(BlendMode),
}

/// Apply one stroke pixel to a layer pixel (happypaint `compositor.ts:2191-2264`).
#[inline]
pub fn apply_stroke_px(dst: Px, src: Px, opacity: f32, mode: StrokeMode) -> Px {
    match mode {
        StrokeMode::Paint(m) => blend_px(m, dst, src, opacity),
        StrokeMode::Erase => {
            let k = 1.0 - src[3] * opacity;
            dst.map(|v| v * k)
        }
        StrokeMode::AlphaLock(m) => blend_px_alpha_lock(m, dst, src, opacity),
    }
}

/// A stroke in progress, composited live onto its layer.
#[derive(Clone, Copy)]
pub struct StrokeOverlay<'a> {
    pub layer_id: &'a str,
    pub stroke: &'a Surface,
    pub opacity: f32,
    pub mode: StrokeMode,
    /// Paint into the layer's mask instead of its pixels.
    pub mask: bool,
}

#[inline]
fn px(b: &[f32], i: usize) -> Px {
    [b[i], b[i + 1], b[i + 2], b[i + 3]]
}

fn zeros() -> Buf {
    vec![0.0; TILE_PX * 4]
}

/// A layer's pixels on one tile, with any live stroke applied and its mask multiplied in.
fn layer_content(l: &Layer, tx: i32, ty: i32, ov: Option<&StrokeOverlay>, frame: Frame) -> Option<Buf> {
    let base = l.pixels.tile(tx, ty);
    let on_layer = ov.filter(|o| o.layer_id == l.id);
    let stroke = on_layer.filter(|o| !o.mask).and_then(|o| o.stroke.tile(tx, ty).map(|t| (o, t)));
    let mask_stroke = on_layer.filter(|o| o.mask).and_then(|o| o.stroke.tile(tx, ty).map(|t| (o, t)));
    if base.is_none() && stroke.is_none() {
        return None;
    }
    let mut buf = base.map(|t| t.to_f32()).unwrap_or_else(zeros);
    if let Some((o, st)) = stroke {
        let s = st.to_f32();
        for i in (0..buf.len()).step_by(4) {
            let r = apply_stroke_px(px(&buf, i), px(&s, i), o.opacity, o.mode);
            buf[i..i + 4].copy_from_slice(&r);
        }
    }
    // Relief lights the paint before the mask, opacity and blend (C:1330).
    if let Some(h) = &l.height {
        relief::shade(&mut buf, h, tx, ty, Rect::new(0, 0, frame.doc_w as i32, frame.doc_h as i32));
    }
    if l.mask_enabled
        && let Some(mask) = &l.mask
    {
        let mut m = match (mask.tile(tx, ty), &mask_stroke) {
            (Some(t), _) => t.to_f32(),
            (None, Some(_)) => zeros(),
            (None, None) => return None,
        };
        if let Some((o, st)) = mask_stroke {
            let s = st.to_f32();
            for i in (0..m.len()).step_by(4) {
                let r = apply_stroke_px(px(&m, i), px(&s, i), o.opacity, o.mode);
                m[i..i + 4].copy_from_slice(&r);
            }
        }
        for i in (0..buf.len()).step_by(4) {
            let k = m[i];
            for c in 0..4 {
                buf[i + c] *= k;
            }
        }
    }
    Some(buf)
}

fn blend_into(target: &mut [f32], src: &[f32], mode: BlendMode, opacity: f32) {
    if opacity <= 0.0 {
        return;
    }
    if mode == BlendMode::Normal {
        for i in (0..target.len()).step_by(4) {
            let sa = src[i + 3] * opacity;
            if sa <= 0.0 && src[i] == 0.0 && src[i + 1] == 0.0 && src[i + 2] == 0.0 {
                continue;
            }
            let k = 1.0 - sa;
            for c in 0..4 {
                target[i + c] = src[i + c] * opacity + target[i + c] * k;
            }
        }
        return;
    }
    for i in (0..target.len()).step_by(4) {
        let r = blend_px(mode, px(target, i), px(src, i), opacity);
        target[i..i + 4].copy_from_slice(&r);
    }
}

fn clip_into(target: &mut [f32], src: &[f32], opacity: f32) {
    for i in (0..target.len()).step_by(4) {
        let r = clip_over(px(target, i), px(src, i), opacity);
        target[i..i + 4].copy_from_slice(&r);
    }
}

/// The clipped layers that directly follow `nodes[i]`.
fn riders(nodes: &[Node], i: usize) -> usize {
    let mut j = i + 1;
    while j < nodes.len() && matches!(&nodes[j], Node::Layer(l) if l.clipped) {
        j += 1;
    }
    j
}

fn draw_clipped(fam: &mut [f32], riders: &[Node], tx: i32, ty: i32, ov: Option<&StrokeOverlay>, frame: Frame) {
    for r in riders {
        let Node::Layer(c) = r else { continue };
        if !c.visible || c.opacity <= 0.0 {
            continue;
        }
        if let Some(cb) = layer_content(c, tx, ty, ov, frame) {
            // A clipped layer's own blend mode is ignored (happypaint behaviour).
            clip_into(fam, &cb, c.opacity);
        }
    }
}

fn composite_nodes(nodes: &[Node], tx: i32, ty: i32, target: &mut Buf, ov: Option<&StrokeOverlay>, frame: Frame) {
    let mut i = 0;
    while i < nodes.len() {
        // An adjustment layer is never a clip base: following clipped layers render plainly.
        if let Node::Layer(l) = &nodes[i]
            && let Some(adj) = &l.adjustment
        {
            draw_adjustment(target, l, adj, tx, ty, frame);
            i += 1;
            continue;
        }
        let j = riders(nodes, i);
        match &nodes[i] {
            Node::Group(g) => {
                let clip = panel_clip(g, tx, ty);
                if g.visible && g.opacity > 0.0 && !g.children.is_empty() && clip != Some(TileCoverage::Empty) {
                    let mut buf = zeros();
                    composite_nodes(&g.children, tx, ty, &mut buf, ov, frame);
                    // A panel's frame clips the group before its riders go on (C:1540-1548).
                    if let Some(TileCoverage::Partial(cov)) = &clip {
                        for (p, k) in buf.chunks_exact_mut(4).zip(cov) {
                            p.iter_mut().for_each(|v| *v *= k);
                        }
                    }
                    draw_clipped(&mut buf, &nodes[i + 1..j], tx, ty, ov, frame);
                    ghost_tint(&mut buf, g);
                    blend_into(target, &buf, g.blend_mode, g.opacity);
                }
            }
            Node::Layer(l) => {
                if l.visible && l.opacity > 0.0 {
                    let family = &nodes[i + 1..j];
                    if family.is_empty() {
                        draw_layer(target, l, tx, ty, ov, frame);
                    } else {
                        let mut fam = layer_content(l, tx, ty, ov, frame).unwrap_or_else(zeros);
                        draw_clipped(&mut fam, family, tx, ty, ov, frame);
                        if l.inherit_alpha {
                            clip_into(target, &fam, l.opacity);
                        } else {
                            blend_into(target, &fam, l.blend_mode, l.opacity);
                        }
                    }
                }
            }
        }
        i = j;
    }
}

/// The clip of a panel group over one tile (`None` for a plain group). The
/// mask is derived from the spec, never stored (spec 30).
fn panel_clip(g: &onepaint_doc::Group, tx: i32, ty: i32) -> Option<TileCoverage> {
    g.extra.get("panel")?;
    onepaint_panel::panel_of(g).map(|spec| tile_coverage(&spec, tx, ty))
}

/// Key on a view-only group that tints its result like an onion-skin ghost.
pub const GHOST_TINT_KEY: &str = "onepaint:ghostTint";

/// Onion ghost tint (compositor.ts:200): `rgb = mix(rgb, tint·a, .65)`, premultiplied.
fn ghost_tint(buf: &mut [f32], g: &onepaint_doc::Group) {
    let Some(t) = g.extra.get(GHOST_TINT_KEY).and_then(|v| serde_json::from_value::<[f32; 3]>(v.clone()).ok()) else { return };
    for p in buf.chunks_exact_mut(4) {
        for c in 0..3 {
            p[c] += (t[c] * p[3] - p[c]) * 0.65;
        }
    }
}

/// Adjustment layer (happypaint `applyAdjustmentLayer`, C:1604-1638): adjust the
/// backdrop, apply the layer mask, then composite with the layer's mode and opacity.
/// Only per-pixel kinds can run per tile; spatial kinds are skipped for now.
/// Fill kinds ignore the backdrop and cover it (happypaint fill layers).
fn draw_adjustment(target: &mut Buf, l: &Layer, adj: &onepaint_doc::Adjustment, tx: i32, ty: i32, frame: Frame) {
    if !l.visible || l.opacity <= 0.0 {
        return;
    }
    let Some(kind) = onepaint_filters::Kind::parse(&adj.kind).filter(|k| k.is_color()) else { return };
    let prepared = onepaint_filters::prepare_in(kind, &adj.params, frame);
    let mask = if l.mask_enabled { l.mask.as_ref().map(|m| m.tile(tx, ty).map(|t| t.to_f32())) } else { None };
    if let Some(None) = mask {
        return; // fully masked out on this tile
    }
    let mut out = vec![0.0; target.len()];
    for i in (0..target.len()).step_by(4) {
        let p = (i / 4) as i32;
        let (x, y) = (tx * TILE + p % TILE, ty * TILE + p / TILE);
        let mut o = prepared.color_px(px(target, i), x, y);
        if let Some(Some(m)) = &mask {
            let k = m[i];
            o = o.map(|v| v * k);
        }
        out[i..i + 4].copy_from_slice(&o);
    }
    blend_into(target, &out, l.blend_mode, l.opacity);
}

fn draw_layer(target: &mut Buf, l: &Layer, tx: i32, ty: i32, ov: Option<&StrokeOverlay>, frame: Frame) {
    let Some(mut c) = layer_content(l, tx, ty, ov, frame) else {
        return;
    };
    if l.inherit_alpha {
        for i in (0..c.len()).step_by(4) {
            let a = target[i + 3];
            for k in 0..4 {
                c[i + k] *= a;
            }
        }
    }
    blend_into(target, &c, l.blend_mode, l.opacity);
}

/// The document as a filter frame (size and DPI for tone and fill layers).
fn doc_frame(doc: &Document) -> Frame {
    Frame::region(0, 0, doc.width, doc.height, doc.dpi as f32)
}

/// Composite one tile as premultiplied linear f32.
pub fn composite_tile(doc: &Document, tx: i32, ty: i32, ov: Option<&StrokeOverlay>) -> Buf {
    let mut buf = zeros();
    composite_nodes(&doc.root, tx, ty, &mut buf, ov, doc_frame(doc));
    buf
}

/// Composite many tiles in parallel.
pub fn composite_tiles(doc: &Document, keys: &[(i32, i32)], ov: Option<&StrokeOverlay>) -> Vec<((i32, i32), Tile)> {
    keys.par_iter().map(|&(tx, ty)| ((tx, ty), Tile::from_f32(&composite_tile(doc, tx, ty, ov)))).collect()
}

/// Tiles covering the canvas.
pub fn doc_tiles(doc: &Document) -> Vec<(i32, i32)> {
    doc.bounds().tiles().collect()
}

/// One composited pixel (for tests and the eyedropper).
pub fn composite_px(doc: &Document, x: i32, y: i32) -> Px {
    let (tx, ty) = onepaint_raster::tile_of(x, y);
    let b = composite_tile(doc, tx, ty, None);
    let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
    px(&b, i)
}

/// The whole canvas as row-major premultiplied linear f32 RGBA.
pub fn flatten(doc: &Document, ov: Option<&StrokeOverlay>) -> Vec<f32> {
    let (w, h) = (doc.width as usize, doc.height as usize);
    let tiles: Vec<_> = doc_tiles(doc).into_par_iter().map(|(tx, ty)| ((tx, ty), composite_tile(doc, tx, ty, ov))).collect();
    let mut out = vec![0.0; w * h * 4];
    let bounds = doc.bounds();
    for ((tx, ty), b) in tiles {
        let Some(ix) = bounds.intersect(&Rect::from_tile(tx, ty)) else {
            continue;
        };
        for y in ix.y..ix.bottom() {
            let src = (((y - ty * TILE) * TILE + (ix.x - tx * TILE)) * 4) as usize;
            let dst = ((y as usize * w) + ix.x as usize) * 4;
            let n = ix.w as usize * 4;
            out[dst..dst + n].copy_from_slice(&b[src..src + n]);
        }
    }
    out
}
