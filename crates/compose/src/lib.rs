//! CPU reference compositor: walks the layer tree per 256² tile.
//!
//! Semantics follow happypaint `engine/compositor.ts:1462-2091` (isolated
//! groups, clip families, masks, inherit alpha). See `docs/specs/02-compositor.md`.

pub mod blend;
pub mod feather;
pub mod program;
pub mod relief;
pub mod spatial;
#[doc(hidden)]
pub mod testdocs;
pub mod vmask;

use blend::{blend_px_alpha_lock_at, blend_px_alpha_lock_in, blend_px_at, blend_px_if, blend_px_in, clip_over};
use onepaint_color::Space;
use onepaint_doc::{BlendIf, BlendMode, Document, Layer, Node, mask_k};
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
    apply_stroke_px_in(Space::Linear, dst, src, opacity, mode)
}

/// [`apply_stroke_px`] in the document's `space` (spec 50).
pub fn apply_stroke_px_in(space: Space, dst: Px, src: Px, opacity: f32, mode: StrokeMode) -> Px {
    match mode {
        StrokeMode::Paint(m) => blend_px_in(space, m, dst, src, opacity),
        StrokeMode::Erase => {
            let k = 1.0 - src[3] * opacity;
            dst.map(|v| v * k)
        }
        StrokeMode::AlphaLock(m) => blend_px_alpha_lock_in(space, m, dst, src, opacity),
    }
}

/// [`apply_stroke_px_in`] at doc pixel (x, y), where a Dissolve stroke dithers.
#[inline]
pub fn apply_stroke_px_at(space: Space, dst: Px, src: Px, opacity: f32, mode: StrokeMode, x: i32, y: i32) -> Px {
    match mode {
        StrokeMode::Paint(m) => blend_px_at(space, m, dst, src, opacity, x, y),
        StrokeMode::AlphaLock(m) => blend_px_alpha_lock_at(space, m, dst, src, opacity, x, y),
        StrokeMode::Erase => apply_stroke_px_in(space, dst, src, opacity, mode),
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
            let p = (i / 4) as i32;
            let (x, y) = (tx * TILE + p % TILE, ty * TILE + p / TILE);
            let r = apply_stroke_px_at(frame.space, px(&buf, i), px(&s, i), o.opacity, o.mode, x, y);
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
        let density = l.mask_density();
        // Feathered, except while the mask is being painted (as happypaint previews it).
        let feathered = if mask_stroke.is_none() { feathered_mask(l, mask, tx, ty, frame) } else { None };
        let mut m = match (feathered.or_else(|| mask.tile(tx, ty).map(|t| t.to_f32())), &mask_stroke) {
            (Some(t), _) => t,
            (None, Some(_)) => zeros(),
            (None, None) if density >= 1.0 => return None,
            (None, None) => zeros(),
        };
        if let Some((o, st)) = mask_stroke {
            let s = st.to_f32();
            for i in (0..m.len()).step_by(4) {
                // Masks hold coverage, not colour: no space.
                let r = apply_stroke_px(px(&m, i), px(&s, i), o.opacity, o.mode);
                m[i..i + 4].copy_from_slice(&r);
            }
        }
        for i in (0..buf.len()).step_by(4) {
            let k = mask_k(density, m[i]);
            for c in 0..4 {
                buf[i + c] *= k;
            }
        }
    }
    // The vector mask multiplies after the pixel mask (density never touches it).
    match vector_cover(l, tx, ty) {
        Some(vmask::Cover::Empty) => return None,
        Some(vmask::Cover::Partial(v)) => {
            for (i, p) in buf.chunks_exact_mut(4).enumerate() {
                let k = v.get(i % TILE as usize, i / TILE as usize)[0];
                p.iter_mut().for_each(|c| *c *= k);
            }
        }
        Some(vmask::Cover::Full) | None => {}
    }
    Some(buf)
}

/// `l`'s active vector mask over tile `(tx, ty)`.
fn vector_cover(l: &Layer, tx: i32, ty: i32) -> Option<vmask::Cover> {
    let v = l.vector_mask().filter(|v| v.active())?;
    Some(vmask::cover(&vmask::flatten(&v), tx, ty))
}

/// `l`'s mask on tile `(tx, ty)` blurred by its feather, as RGBA: `None`
/// without a feather (or when the mask reaches nothing here, coverage 0
/// like a missing tile).
fn feathered_mask(l: &Layer, mask: &Surface, tx: i32, ty: i32, frame: Frame) -> Option<Buf> {
    let f = l.mask_feather();
    if f <= 0.0 {
        return None;
    }
    Some(feather::feathered_mask_tile(mask, f, tx, ty, [frame.doc_w, frame.doc_h]).map_or_else(zeros, |t| t.to_f32()))
}

/// Blend a tile's worth of `src` onto `target`; the tile's origin places
/// Dissolve's dither.
fn blend_into(target: &mut [f32], src: &[f32], mode: BlendMode, opacity: f32, space: Space, (tx, ty): (i32, i32)) {
    blend_into_if(target, src, mode, opacity, space, (tx, ty), None);
}

/// [`blend_into`] through the layer's Blend If, when it has one.
fn blend_into_if(
    target: &mut [f32],
    src: &[f32],
    mode: BlendMode,
    opacity: f32,
    space: Space,
    (tx, ty): (i32, i32),
    bif: Option<&BlendIf>,
) {
    if opacity <= 0.0 {
        return;
    }
    let mode = mode.effective();
    if let Some(b) = bif {
        for i in (0..target.len()).step_by(4) {
            let p = (i / 4) as i32;
            let r = blend_px_if(space, mode, px(target, i), px(src, i), opacity, tx * TILE + p % TILE, ty * TILE + p / TILE, b);
            target[i..i + 4].copy_from_slice(&r);
        }
        return;
    }
    if mode == BlendMode::Dissolve {
        for i in (0..target.len()).step_by(4) {
            let p = (i / 4) as i32;
            let r = blend_px_at(space, mode, px(target, i), px(src, i), opacity, tx * TILE + p % TILE, ty * TILE + p / TILE);
            target[i..i + 4].copy_from_slice(&r);
        }
        return;
    }
    if mode == BlendMode::Normal && space == Space::Linear {
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
        let r = blend_px_in(space, mode, px(target, i), px(src, i), opacity);
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
pub(crate) fn riders(nodes: &[Node], i: usize) -> usize {
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

/// Punch `l`'s masked shape through `target` (knockout): where it covers,
/// the target becomes `reveal` (transparency when `None`), `mix(cur, bg,
/// a · opacity)` (happypaint `compositor.ts` KNOCKOUT_FS).
fn knockout(target: &mut Buf, l: &Layer, reveal: Option<&[f32]>, tx: i32, ty: i32, ov: Option<&StrokeOverlay>, frame: Frame) {
    let Some(cov) = layer_content(l, tx, ty, ov, frame) else { return };
    for (i, p) in target.chunks_exact_mut(4).enumerate() {
        let k = cov[i * 4 + 3] * l.opacity;
        if k <= 0.0 {
            continue;
        }
        for c in 0..4 {
            let bg = reveal.map_or(0.0, |r| r[i * 4 + c]);
            p[c] += (bg - p[c]) * k;
        }
    }
}

/// Whether the top level's knockouts reveal the Background: the bottom
/// layer when it is a visible, unclipped, locked paint layer and a top-level
/// layer knocks out. happypaint snapshots it as the loop reaches node 1, so
/// a Background with clipped layers on it reveals transparency instead.
pub(crate) fn background_reveals(nodes: &[Node]) -> bool {
    let Some(Node::Layer(first)) = nodes.first() else { return false };
    first.locked
        && !first.clipped
        && first.visible
        && first.opacity > 0.0
        && first.adjustment.is_none()
        && riders(nodes, 0) == 1
        && nodes.iter().any(|n| matches!(n, Node::Layer(l) if l.knockout().is_some()))
}

/// The document's top level: knockouts there reveal the Background
/// snapshot, else transparency.
fn composite_root(nodes: &[Node], tx: i32, ty: i32, target: &mut Buf, ov: Option<&StrokeOverlay>, frame: Frame) {
    if background_reveals(nodes) {
        composite_nodes(&nodes[..1], tx, ty, target, ov, frame, None);
        let bg = target.clone();
        composite_nodes(&nodes[1..], tx, ty, target, ov, frame, Some(&bg));
    } else {
        composite_nodes(nodes, tx, ty, target, ov, frame, None);
    }
}

/// Composite `nodes` onto `target`. A knocked-out layer's hole shows
/// `reveal`: the Background at the top level, a Pass Through group's
/// backdrop, transparency (`None`) in an isolated group.
fn composite_nodes(nodes: &[Node], tx: i32, ty: i32, target: &mut Buf, ov: Option<&StrokeOverlay>, frame: Frame, reveal: Option<&[f32]>) {
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
                if passes_through(g, &nodes[i + 1..j]) {
                    if g.visible && g.opacity > 0.0 {
                        pass_through(&g.children, g.opacity, tx, ty, target, ov, frame);
                    }
                } else if g.visible && g.opacity > 0.0 && !g.children.is_empty() && clip != Some(TileCoverage::Empty) {
                    let mut buf = match ghost_peg(g) {
                        Some(peg) => pegged(&g.children, peg, tx, ty, ov, frame),
                        None => {
                            let mut buf = zeros();
                            composite_nodes(&g.children, tx, ty, &mut buf, ov, frame, None);
                            buf
                        }
                    };
                    // A panel's frame clips the group before its riders go on (C:1540-1548).
                    if let Some(TileCoverage::Partial(cov)) = &clip {
                        for (p, k) in buf.chunks_exact_mut(4).zip(cov) {
                            p.iter_mut().for_each(|v| *v *= k);
                        }
                    }
                    draw_clipped(&mut buf, &nodes[i + 1..j], tx, ty, ov, frame);
                    ghost_tint(&mut buf, g);
                    blend_into(target, &buf, g.blend_mode, g.opacity, frame.space, (tx, ty));
                }
            }
            Node::Layer(l) => {
                if l.visible && l.opacity > 0.0 {
                    // Punch first; the layer (or its family) then draws into the hole.
                    if l.knockout().is_some() {
                        knockout(target, l, reveal, tx, ty, ov, frame);
                    }
                    let family = &nodes[i + 1..j];
                    if family.is_empty() {
                        draw_layer(target, l, tx, ty, ov, frame);
                    } else {
                        let mut fam = layer_content(l, tx, ty, ov, frame).unwrap_or_else(zeros);
                        draw_clipped(&mut fam, family, tx, ty, ov, frame);
                        if l.inherit_alpha {
                            clip_into(target, &fam, l.opacity);
                        } else {
                            // The base's Blend If applies to the whole family.
                            blend_into_if(target, &fam, l.blend_mode, l.opacity, frame.space, (tx, ty), l.blend_if().as_ref());
                        }
                    }
                }
            }
        }
        i = j;
    }
}

/// Whether a Pass Through group composites its children straight onto the
/// backdrop. A group that has to be one picture first (a panel frame, a
/// view-only ghost, layers clipped to it) stays isolated and blends as
/// Normal (happypaint `compositeNodes`, v0.200.1).
pub(crate) fn passes_through(g: &onepaint_doc::Group, riders: &[Node]) -> bool {
    g.blend_mode == BlendMode::PassThrough
        && !g.extra.contains_key("panel")
        && !g.extra.contains_key(GHOST_PEG_KEY)
        && !g.extra.contains_key(GHOST_TINT_KEY)
        && !riders.iter().any(|r| r.visible() && r.opacity() > 0.0)
}

/// A Pass Through group (`Compositor.passThroughGroup`): the children
/// composite onto the target; below 100% the result fades with the
/// backdrop, `out = backdrop·(1 − o) + result·o`.
fn pass_through(children: &[Node], opacity: f32, tx: i32, ty: i32, target: &mut Buf, ov: Option<&StrokeOverlay>, frame: Frame) {
    // A knocked-out child reveals the backdrop the group started from.
    let knocks = children.iter().any(|n| matches!(n, Node::Layer(l) if l.knockout().is_some()));
    if opacity >= 1.0 && !knocks {
        composite_nodes(children, tx, ty, target, ov, frame, None);
        return;
    }
    let backdrop = target.clone();
    composite_nodes(children, tx, ty, target, ov, frame, Some(&backdrop));
    if opacity >= 1.0 {
        return;
    }
    for (o, b) in target.iter_mut().zip(&backdrop) {
        *o = b * (1.0 - opacity) + *o * opacity;
    }
}

/// The clip of a panel group over one tile (`None` for a plain group). The
/// mask is derived from the spec, never stored (spec 30).
pub(crate) fn panel_clip(g: &onepaint_doc::Group, tx: i32, ty: i32) -> Option<TileCoverage> {
    g.extra.get("panel")?;
    onepaint_panel::panel_of(g).map(|spec| tile_coverage(&spec, tx, ty))
}

/// Key on a view-only group that tints its result like an onion-skin ghost.
pub const GHOST_TINT_KEY: &str = "onepaint:ghostTint";

/// Key on a view-only ghost group holding its out-of-pegs offset
/// `[dx, dy, rot, scale]` (doc px, radians about the document centre).
pub const GHOST_PEG_KEY: &str = "onepaint:ghostPeg";

pub(crate) fn ghost_peg(g: &onepaint_doc::Group) -> Option<[f32; 4]> {
    g.extra.get(GHOST_PEG_KEY).and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// The group's content seen through its peg (`compositor.ts` `pegMatrix`):
/// each pixel samples the unmoved content through the inverse transform,
/// bilinear. The source tiles it reaches are composited once each.
/// A peg's inverse (`compositor.ts` `pegMatrix`): where a doc-px position
/// reads the unmoved content.
pub(crate) fn peg_inverse(frame: Frame, [dx, dy, rot, scale]: [f32; 4]) -> impl Fn(f32, f32) -> (f32, f32) {
    let (cx, cy) = (frame.doc_w as f32 / 2.0, frame.doc_h as f32 / 2.0);
    let k = 1.0 / scale.clamp(0.1, 4.0);
    let (c, s) = (rot.cos() * k, rot.sin() * k);
    move |x: f32, y: f32| {
        let (qx, qy) = (x - cx - dx, y - cy - dy);
        (cx + c * qx + s * qy, cy - s * qx + c * qy)
    }
}

/// The content tiles tile `(tx, ty)` reads through a peg: columns and rows,
/// inside the document (only content over it is shown); empty if none.
pub(crate) fn peg_span(frame: Frame, peg: [f32; 4], tx: i32, ty: i32) -> (std::ops::RangeInclusive<i32>, std::ops::RangeInclusive<i32>) {
    use onepaint_raster::TILE;
    let inv = peg_inverse(frame, peg);
    let (x0, y0) = ((tx * TILE) as f32, (ty * TILE) as f32);
    let t = TILE as f32;
    let corners = [inv(x0, y0), inv(x0 + t, y0), inv(x0, y0 + t), inv(x0 + t, y0 + t)];
    let fold = |f: fn(f32, f32) -> f32, pick: fn(&(f32, f32)) -> f32, init: f32| corners.iter().map(pick).fold(init, f);
    let (min_x, max_x) = (fold(f32::min, |p| p.0, f32::MAX), fold(f32::max, |p| p.0, f32::MIN));
    let (min_y, max_y) = (fold(f32::min, |p| p.1, f32::MAX), fold(f32::max, |p| p.1, f32::MIN));
    let (dtx, dty) = ((frame.doc_w as i32 - 1).div_euclid(TILE), (frame.doc_h as i32 - 1).div_euclid(TILE));
    let tile_span = |lo: f32, hi: f32, max: i32| {
        ((lo - 1.0).floor() as i32).div_euclid(TILE).max(0)..=((hi + 1.0).ceil() as i32).div_euclid(TILE).min(max)
    };
    (tile_span(min_x, max_x, dtx), tile_span(min_y, max_y, dty))
}

fn pegged(children: &[Node], peg: [f32; 4], tx: i32, ty: i32, ov: Option<&StrokeOverlay>, frame: Frame) -> Buf {
    use onepaint_raster::TILE;
    let inv = peg_inverse(frame, peg);
    let (x0, y0) = ((tx * TILE) as f32, (ty * TILE) as f32);
    let (xs, ys) = peg_span(frame, peg, tx, ty);
    let mut out = zeros();
    if xs.is_empty() || ys.is_empty() {
        return out;
    }
    let (sx0, sy0) = (*xs.start(), *ys.start());
    let cols = (xs.end() - sx0 + 1) as usize;
    let src: Vec<Buf> = ys
        .clone()
        .flat_map(|y| xs.clone().map(move |x| (x, y)))
        .map(|(x, y)| {
            let mut b = zeros();
            composite_nodes(children, x, y, &mut b, ov, frame, None);
            b
        })
        .collect();
    let rows = src.len() / cols;
    let at = |x: i32, y: i32| -> Px {
        let (t_x, t_y) = (x.div_euclid(TILE) - sx0, y.div_euclid(TILE) - sy0);
        if t_x < 0 || t_y < 0 || t_x as usize >= cols || t_y as usize >= rows {
            return [0.0; 4];
        }
        px(&src[t_y as usize * cols + t_x as usize], (y.rem_euclid(TILE) * TILE + x.rem_euclid(TILE)) as usize * 4)
    };
    for py in 0..TILE {
        for pxl in 0..TILE {
            let (sx, sy) = inv(x0 + pxl as f32 + 0.5, y0 + py as f32 + 0.5);
            let (fx, fy) = (sx - 0.5, sy - 0.5);
            let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
            let (wx, wy) = (fx - ix as f32, fy - iy as f32);
            let (a, b, cc, d) = (at(ix, iy), at(ix + 1, iy), at(ix, iy + 1), at(ix + 1, iy + 1));
            let o = ((py * TILE + pxl) * 4) as usize;
            for ch in 0..4 {
                let top = a[ch] + (b[ch] - a[ch]) * wx;
                let bot = cc[ch] + (d[ch] - cc[ch]) * wx;
                out[o + ch] = top + (bot - top) * wy;
            }
        }
    }
    out
}

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
    let density = l.mask_density();
    let mask = if l.mask_enabled {
        l.mask.as_ref().map(|m| feathered_mask(l, m, tx, ty, frame).or_else(|| m.tile(tx, ty).map(|t| t.to_f32())))
    } else {
        None
    };
    if let Some(None) = mask
        && density >= 1.0
    {
        return; // fully masked out on this tile
    }
    let vcover = vector_cover(l, tx, ty);
    if let Some(vmask::Cover::Empty) = vcover {
        return;
    }
    let mut out = vec![0.0; target.len()];
    for i in (0..target.len()).step_by(4) {
        let p = (i / 4) as i32;
        let (x, y) = (tx * TILE + p % TILE, ty * TILE + p / TILE);
        let mut o = prepared.color_px(px(target, i), x, y);
        match &mask {
            Some(Some(m)) => o = o.map(|v| v * mask_k(density, m[i])),
            Some(None) => o = o.map(|v| v * (1.0 - density)),
            None => {}
        }
        if let Some(vmask::Cover::Partial(v)) = &vcover {
            let k = v.get((p % TILE) as usize, (p / TILE) as usize)[0];
            o = o.map(|c| c * k);
        }
        out[i..i + 4].copy_from_slice(&o);
    }
    let bif = if l.clipped { None } else { l.blend_if() };
    blend_into_if(target, &out, l.blend_mode, l.opacity, frame.space, (tx, ty), bif.as_ref());
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
    // Inherit alpha has no Blend If (happypaint `compositeInheritAlpha`).
    let bif = if l.inherit_alpha { None } else { l.blend_if() };
    blend_into_if(target, &c, l.blend_mode, l.opacity, frame.space, (tx, ty), bif.as_ref());
}

/// The document as a filter frame (size, DPI and space for tone and fill
/// layers and the blends).
pub fn doc_frame(doc: &Document) -> Frame {
    Frame::region(0, 0, doc.width, doc.height, doc.dpi as f32).in_space(doc.space())
}

/// Composite one tile as premultiplied f32 in the document's space (linear
/// light, or sRGB values in 8-bit documents; spec 50).
pub fn composite_tile(doc: &Document, tx: i32, ty: i32, ov: Option<&StrokeOverlay>) -> Buf {
    let mut buf = zeros();
    composite_root(&doc.root, tx, ty, &mut buf, ov, doc_frame(doc));
    // The paper's visible tooth shades the finished composite (C:214-233).
    if let Some(p) = doc.paper()
        && let Some(tex) = onepaint_brush::paper::PaperTexture::new(&p)
    {
        onepaint_brush::paper::shade_tile(&mut buf, &tex, p.strength(), tx, ty, doc.space());
    }
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
