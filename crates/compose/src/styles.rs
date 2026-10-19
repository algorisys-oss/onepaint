//! Layer styles' oracle (spec 56): happypaint's `STYLE_SRC`
//! (`engine/styles/style-renderer.ts`) on the CPU, over one tile, from the
//! node's own content on the tiles around it.
//!
//! The distance field is exact (a Euclidean distance transform) where
//! happypaint flood-fills (JFA+1); both measure between pixel centres, with
//! the edge at ±½, and are refined to `0.5 − alpha` within a pixel of it.

use onepaint_doc::BlendMode;
use onepaint_doc::styles::{Contour, Effect, EffectKind, LayerStyles, shadow_offset};
use onepaint_filters::Frame;
use onepaint_raster::{Rect, TILE};

type Buf = Vec<f32>;
type Px = [f32; 4];

/// One pattern tile's side, doc px (`PATTERN_TILE`).
const PATTERN_TILE: f32 = 32.0;

/// The node's alpha over a region and its signed distance field.
struct Field {
    region: Rect,
    alpha: Vec<f32>,
    dist: Vec<f32>,
    /// Off the canvas, and as far as the field looks (`reach + 4`).
    cap: f32,
    /// The document: positions are clamped to it, as a texture's edge.
    doc: [i32; 2],
    /// No distance effect: the field is flat (happypaint builds none).
    flat: bool,
}

/// The squared distance from each cell to the nearest seed along a line
/// (Felzenszwalb and Huttenlocher's lower envelope of parabolas). Cells
/// with no seed hold `far`, a finite stand-in for infinity, so the maths
/// stays exact in f64.
fn edt_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        let mut s;
        loop {
            let p = v[k];
            s = ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
            if s <= z[k] && k > 0 {
                k -= 1;
            } else {
                break;
            }
        }
        if s <= z[k] {
            // k == 0: q's parabola is lower everywhere.
            v[0] = q;
            z[0] = f64::NEG_INFINITY;
            z[1] = f64::INFINITY;
            continue;
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    let mut k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let d = q as f64 - v[k] as f64;
        *o = d * d + f[v[k]];
    }
}

/// Squared distance to the nearest seed over a `w × h` grid; infinite where
/// there is none.
fn edt(seed: &[bool], w: usize, h: usize) -> Vec<f32> {
    let n = w.max(h);
    let far = ((w + h) * (w + h) * 4) as f64;
    let (mut f, mut out, mut v, mut z) = (vec![0.0f64; n], vec![0.0f64; n], vec![0usize; n], vec![0.0f64; n + 1]);
    let mut g: Vec<f64> = seed.iter().map(|&s| if s { 0.0 } else { far }).collect();
    for x in 0..w {
        for y in 0..h {
            f[y] = g[y * w + x];
        }
        edt_1d(&f[..h], &mut out[..h], &mut v, &mut z);
        for y in 0..h {
            g[y * w + x] = out[y];
        }
    }
    for y in 0..h {
        f[..w].copy_from_slice(&g[y * w..(y + 1) * w]);
        edt_1d(&f[..w], &mut out[..w], &mut v, &mut z);
        g[y * w..(y + 1) * w].copy_from_slice(&out[..w]);
    }
    g.into_iter().map(|d| if d >= far { f32::INFINITY } else { d as f32 }).collect()
}

impl Field {
    /// The field over `region` (inside the document) from the node's alpha:
    /// inside where alpha ≥ ½; + outside, − inside, ±½ at the edge, clamped
    /// at `cap`, stored as half floats (as happypaint's R16F texture).
    fn new(region: Rect, alpha: Vec<f32>, cap: f32, doc: [i32; 2], flat: bool) -> Self {
        let (w, h) = (region.w as usize, region.h as usize);
        let mut dist = Vec::new();
        if !flat {
            let inside: Vec<bool> = alpha.iter().map(|a| *a >= 0.5).collect();
            let outside: Vec<bool> = inside.iter().map(|i| !i).collect();
            let (to_in, to_out) = (edt(&inside, w, h), edt(&outside, w, h));
            dist = (0..w * h)
                .map(|i| {
                    let s = if inside[i] { -(to_out[i].sqrt() - 0.5) } else { to_in[i].sqrt() - 0.5 };
                    let s = if s.is_finite() {
                        s
                    } else if inside[i] {
                        -cap
                    } else {
                        cap
                    };
                    half::f16::from_f32(s.clamp(-cap, cap)).to_f32()
                })
                .collect();
        }
        Self { region, alpha, dist, cap, doc, flat }
    }

    /// Bilinear at doc px `q` (texel centres at +½), clamped to the document.
    fn bilinear(&self, data: &[f32], q: [f32; 2]) -> f32 {
        let tx = (q[0] - 0.5).clamp(0.0, (self.doc[0] - 1) as f32);
        let ty = (q[1] - 0.5).clamp(0.0, (self.doc[1] - 1) as f32);
        let (x0, y0) = (tx.floor() as i32, ty.floor() as i32);
        let (x1, y1) = ((x0 + 1).min(self.doc[0] - 1), (y0 + 1).min(self.doc[1] - 1));
        let (fx, fy) = (tx - x0 as f32, ty - y0 as f32);
        let at = |x: i32, y: i32| {
            let (lx, ly) = (x - self.region.x, y - self.region.y);
            if lx < 0 || ly < 0 || lx >= self.region.w || ly >= self.region.h {
                return 0.0;
            }
            data[(ly * self.region.w + lx) as usize]
        };
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bot = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        top + (bot - top) * fy
    }

    /// `sdf()`: the distance at doc px `q`; off the canvas is outside; within
    /// a pixel of the edge the alpha says where the edge runs.
    fn sdf(&self, q: [f32; 2]) -> f32 {
        if q[0] < 0.0 || q[1] < 0.0 || q[0] > self.doc[0] as f32 || q[1] > self.doc[1] as f32 {
            return self.cap;
        }
        let f = if self.flat { 0.0 } else { self.bilinear(&self.dist, q) };
        if f.abs() < 1.0 { (0.5 - self.bilinear(&self.alpha, q)).clamp(-0.5, 0.5) } else { f }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 1 up to `f.0`, fading to 0 at `f.0 + f.1` (a hard ½-px edge when `f.1` is ~0).
fn fall(x: f32, f: (f32, f32)) -> f32 {
    if f.1 < 0.5 { (f.0 + 0.5 - x).clamp(0.0, 1.0) } else { 1.0 - smoothstep(f.0, f.0 + f.1, x) }
}

fn over(top: Px, bottom: Px) -> Px {
    [0, 1, 2, 3].map(|i| top[i] + bottom[i] * (1.0 - top[3]))
}

fn paint(rgb: [f32; 3], k: f32) -> Px {
    [rgb[0] * k, rgb[1] * k, rgb[2] * k, k]
}

/// A pattern tile's grey (white on black), bilinear and repeating at doc px
/// `q` with the tile `tile` px wide.
struct Pattern {
    w: usize,
    h: usize,
    grey: Vec<f32>,
}

impl Pattern {
    fn new(id: &str) -> Self {
        let t = onepaint_filters::pattern::pattern_pixels(id, [1.0; 3], [0.0; 3]);
        let grey = t.data.chunks_exact(4).map(|p| p[0] as f32 / 255.0).collect();
        Self { w: t.w, h: t.h, grey }
    }

    fn at(&self, q: [f32; 2], tile: f32) -> f32 {
        let (u, v) = (q[0] / tile * self.w as f32 - 0.5, q[1] / tile * self.h as f32 - 0.5);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let g = |x: f32, y: f32| {
            let (x, y) = ((x as i64).rem_euclid(self.w as i64) as usize, (y as i64).rem_euclid(self.h as i64) as usize);
            self.grey[y * self.w + x]
        };
        let top = g(x0, y0) + (g(x0 + 1.0, y0) - g(x0, y0)) * fx;
        let bot = g(x0, y0 + 1.0) + (g(x0 + 1.0, y0 + 1.0) - g(x0, y0 + 1.0)) * fx;
        top + (bot - top) * fy
    }
}

/// Where a gradient overlay runs, doc px (`overlayEnds`): across the canvas
/// along the angle, `scale` of the way; linear from edge to edge, the other
/// shapes from the centre.
pub fn overlay_ends(shape: &str, angle: f32, scale: f32, w: f32, h: f32) -> ([f32; 2], [f32; 2]) {
    let a = angle.to_radians();
    let (dx, dy) = (a.cos(), -a.sin());
    let half = (dx.abs() * w + dy.abs() * h) / 2.0 * scale;
    let (cx, cy) = (w / 2.0, h / 2.0);
    if shape == "linear" {
        ([cx - dx * half, cy - dy * half], [cx + dx * half, cy + dy * half])
    } else {
        ([cx, cy], [cx + dx * half, cy + dy * half])
    }
}

fn grad_shape(s: &str) -> onepaint_filters::gradient::Shape {
    use onepaint_filters::gradient::Shape;
    match s {
        "radial" => Shape::Radial,
        "angle" => Shape::Angle,
        "reflected" => Shape::Reflected,
        "diamond" => Shape::Diamond,
        _ => Shape::Linear,
    }
}

fn ramp_of(e: &Effect, space: onepaint_color::Space) -> onepaint_filters::gradient::Ramp {
    use onepaint_filters::gradient::{GradStop, Ramp};
    let mut stops: Vec<GradStop> = e
        .stops
        .iter()
        .filter_map(|s| {
            let pos = s.get("pos")?.as_f64()?.clamp(0.0, 1.0);
            let c = s.get("color")?.as_array()?;
            let color = [c.first()?.as_f64()?, c.get(1)?.as_f64()?, c.get(2)?.as_f64()?];
            let opacity = s.get("opacity").and_then(|o| o.as_f64()).unwrap_or(1.0).clamp(0.0, 1.0);
            Some(GradStop { pos, color, opacity, mid: s.get("mid").and_then(|m| m.as_f64()) })
        })
        .collect();
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    Ramp::new_in(&stops, space)
}

/// How many px around a tile the style pass reads: the field looks
/// `reach + 4` px for the edge, at points up to an effect's offset away.
pub fn margin(styles: &LayerStyles) -> i32 {
    let reach = styles.reach();
    if reach == 0 { 0 } else { 2 * reach + 8 }
}

/// The node's styled pixels on tile `(tx, ty)`. `content(x, y)` is the
/// node's own pixels on any tile (premultiplied; `None` when empty); the
/// tile's own may be passed as `here`. `backdrop` is what's under the node
/// on this tile, for effects in other blend modes; without it (a clip
/// family, a bake) every effect draws Normal, as happypaint does.
pub fn styled_tile(
    styles: &LayerStyles,
    (tx, ty): (i32, i32),
    frame: Frame,
    content: &dyn Fn(i32, i32) -> Option<Buf>,
    here: Option<&Buf>,
    backdrop: Option<&Buf>,
) -> Buf {
    let doc = [frame.doc_w as i32, frame.doc_h as i32];
    let reach = styles.reach();
    let tile = Rect::from_tile(tx, ty);
    let m = margin(styles);
    let bounds = Rect::new(0, 0, doc[0], doc[1]);
    let region = Rect::new(tile.x - m, tile.y - m, tile.w + 2 * m, tile.h + 2 * m).intersect(&bounds).unwrap_or(tile);
    let own = here.cloned().or_else(|| content(tx, ty)).unwrap_or_else(|| vec![0.0; (TILE * TILE * 4) as usize]);
    // The node's alpha over the region, from the tiles it touches.
    let mut alpha = vec![0.0f32; (region.w * region.h) as usize];
    for (x, y) in region.tiles() {
        let buf = if (x, y) == (tx, ty) { Some(own.clone()) } else { content(x, y) };
        let Some(buf) = buf else { continue };
        let t = Rect::from_tile(x, y);
        let Some(r) = t.intersect(&region) else { continue };
        for yy in r.y..r.y + r.h {
            for xx in r.x..r.x + r.w {
                let src = (((yy - t.y) * TILE + (xx - t.x)) * 4 + 3) as usize;
                alpha[((yy - region.y) * region.w + (xx - region.x)) as usize] = buf[src];
            }
        }
    }
    let field = Field::new(region, alpha, (reach + 4) as f32, doc, reach == 0);
    let space = frame.space;
    let linear = space == onepaint_color::Space::Linear;
    let rgb = |c: [f32; 3]| if linear { c.map(onepaint_color::srgb_to_linear) } else { c };
    let on: Vec<&Effect> = styles.effects.iter().filter(|e| e.active()).collect();
    let list = |k: EffectKind| on.iter().copied().filter(|e| e.kind == k).take(4).collect::<Vec<_>>();
    let first = |k: EffectKind| on.iter().copied().find(|e| e.kind == k);
    let (ds, is, strokes, cos, gos) = (
        list(EffectKind::DropShadow),
        list(EffectKind::InnerShadow),
        list(EffectKind::Stroke),
        list(EffectKind::ColorOverlay),
        list(EffectKind::GradientOverlay),
    );
    let (og, ig, sa, po, bv) = (
        first(EffectKind::OuterGlow),
        first(EffectKind::InnerGlow),
        first(EffectKind::Satin),
        first(EffectKind::PatternOverlay),
        first(EffectKind::Bevel),
    );
    let falloff = |e: &Effect| (e.size * e.spread, e.size * (1.0 - e.spread));
    let ramps: Vec<_> = gos.iter().map(|e| ramp_of(e, space)).collect();
    let ends: Vec<_> = gos.iter().map(|e| overlay_ends(&e.shape, e.angle, e.scale, doc[0] as f32, doc[1] as f32)).collect();
    let po_tile = po.map(|e| (Pattern::new(&e.pattern), PATTERN_TILE * e.scale));
    let bv_tex = bv.filter(|e| e.bevel.texture_on).map(|e| (Pattern::new(&e.bevel.texture_pattern), PATTERN_TILE * e.bevel.texture_scale));
    // The bevel's light: towards it, y down.
    let bv_light = bv.map(|e| {
        let (az, alt) = (e.angle.to_radians(), e.bevel.altitude.to_radians());
        [alt.cos() * az.cos(), -alt.cos() * az.sin(), alt.sin()]
    });
    let bv_contour = bv.map(|e| if e.bevel.contour_on { e.bevel.contour } else { Contour::HalfRound });
    let bevel_height = |q: [f32; 2]| -> f32 {
        let (Some(e), Some(c)) = (bv, bv_contour) else { return 0.0 };
        let sd = field.sdf(q);
        let s = e.size;
        let ct = |x: f32| c.at(x.clamp(0.0, 1.0));
        let mut h = match e.bevel.style.as_str() {
            "outer" => ct(1.0 - (sd / s).clamp(0.0, 1.0)),
            "emboss" => ct((0.5 - sd / s).clamp(0.0, 1.0)),
            "pillow" if sd <= 0.0 => ct((-sd / s).clamp(0.0, 1.0)),
            "pillow" => -ct(1.0 - (sd / s).clamp(0.0, 1.0)),
            _ => ct((-sd / s).clamp(0.0, 1.0)),
        };
        if let Some((p, tile)) = &bv_tex {
            let inv = if e.bevel.texture_invert { -1.0 } else { 1.0 };
            h += inv * (p.at(q, *tile) - 0.5) * e.bevel.texture_depth * (0.5 - sd).clamp(0.0, 1.0) / s.max(1.0) * 4.0;
        }
        h
    };
    // An effect's colour after its blend mode against what's under it (`eff`).
    let eff = |m: BlendMode, c: [f32; 3], stack: Px, bd: Px| -> [f32; 3] {
        if m == BlendMode::Normal || backdrop.is_none() {
            return c;
        }
        let u = over(stack, bd);
        if u[3] <= 0.0 {
            return c;
        }
        let mut b = [u[0] / u[3], u[1] / u[3], u[2] / u[3]];
        let mut sc = c;
        if linear {
            b = b.map(onepaint_color::linear_to_srgb);
            sc = sc.map(onepaint_color::linear_to_srgb);
        }
        let mut big = crate::blend::blend_color(m, b, sc);
        if linear {
            big = big.map(onepaint_color::srgb_to_linear);
        }
        [0, 1, 2].map(|i| c[i] + (big[i] - c[i]) * u[3])
    };
    let mut out = vec![0.0f32; (TILE * TILE * 4) as usize];
    for ly in 0..TILE {
        for lx in 0..TILE {
            let o = ((ly * TILE + lx) * 4) as usize;
            let p = [(tile.x + lx) as f32 + 0.5, (tile.y + ly) as f32 + 0.5];
            let c = [own[o], own[o + 1], own[o + 2], own[o + 3]];
            let a = c[3];
            let s = field.sdf(p);
            let bd = backdrop.map_or([0.0; 4], |b| [b[o], b[o + 1], b[o + 2], b[o + 3]]);
            let mut r = [0.0f32; 4];
            let outside = (s + 1.5).clamp(0.0, 1.0);
            let mut body = c.map(|v| v * styles.fill_opacity);
            let known = (1.5 - s).clamp(0.0, 1.0);
            let satin = sa.map_or(0.0, |e| {
                let size = e.size.max(0.5);
                let off = shadow_offset(e.angle, e.distance);
                let m1 = 1.0 - smoothstep(-size, size, field.sdf([p[0] + off[0], p[1] + off[1]]));
                let m2 = 1.0 - smoothstep(-size, size, field.sdf([p[0] - off[0], p[1] - off[1]]));
                let v = e.contour.at((m1 - m2).abs().clamp(0.0, 1.0));
                if e.invert { 1.0 - v } else { v }
            });
            let (mut lit, mut cover) = (0.0, 0.0);
            if let (Some(e), Some(light)) = (bv, bv_light) {
                let d = 1.0 + e.bevel.soften;
                let hq = |dx: f32, dy: f32| bevel_height([p[0] + dx, p[1] + dy]);
                let (h00, h10, h20) = (hq(-d, -d), hq(0.0, -d), hq(d, -d));
                let (h01, h21) = (hq(-d, 0.0), hq(d, 0.0));
                let (h02, h12, h22) = (hq(-d, d), hq(0.0, d), hq(d, d));
                let gx = ((h20 + 2.0 * h21 + h22) - (h00 + 2.0 * h01 + h02)) / (8.0 * d);
                let gy = ((h02 + 2.0 * h12 + h22) - (h00 + 2.0 * h10 + h20)) / (8.0 * d);
                let slope = e.size * e.bevel.depth * if e.bevel.up { 1.0 } else { -1.0 };
                let n = [-gx * slope, -gy * slope, 1.0];
                let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                let dot = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]) / len;
                lit = ((dot - light[2]) * 2.5).clamp(-1.0, 1.0);
                let inside = if e.bevel.style == "outer" { 0.0 } else { a * known };
                let ring = if e.bevel.style == "inner" { 0.0 } else { (1.0 - a) * (e.size + 0.5 - s).clamp(0.0, 1.0) * outside };
                cover = (inside + ring) * e.opacity;
            }
            // Under the pixels: into r.
            let under = |r: &mut Px, m: BlendMode, col: [f32; 3], k: f32| {
                let e = paint(eff(m, col, *r, bd), k);
                *r = over(e, *r);
            };
            for e in ds.iter().rev() {
                let off = shadow_offset(e.angle, e.distance);
                let k = fall(field.sdf([p[0] - off[0], p[1] - off[1]]), falloff(e)) * e.opacity * (1.0 - a) * outside;
                under(&mut r, e.blend_mode, rgb(e.color), k);
            }
            if let Some(e) = og {
                under(&mut r, e.blend_mode, rgb(e.color), fall(s, falloff(e)) * e.opacity * (1.0 - a) * outside);
            }
            for e in strokes.iter().rev().filter(|e| e.position == "outside") {
                let k = (e.size + 0.5 - s).clamp(0.0, 1.0) * (s + 1.5).clamp(0.0, 1.0) * e.opacity;
                under(&mut r, e.blend_mode, rgb(e.color), k);
            }
            // Inside the pixels: into the body.
            let inside = |body: &mut Px, r: Px, m: BlendMode, col: [f32; 3], k: f32| {
                let e = paint(eff(m, col, over(*body, r), bd), k);
                *body = over(e, *body);
            };
            if let (Some(e), Some((pat, tile))) = (po, &po_tile) {
                let cov = pat.at(p, *tile);
                let (ca, cb) = (rgb(e.color), rgb(e.color_b));
                let col = [0, 1, 2].map(|i| cb[i] + (ca[i] - cb[i]) * cov);
                inside(&mut body, r, e.blend_mode, col, a * e.opacity);
            }
            for (i, e) in gos.iter().enumerate().rev() {
                let (ga, gb) = ends[i];
                let t = onepaint_filters::gradient::shape_t(
                    grad_shape(&e.shape),
                    [p[0] as f64, p[1] as f64],
                    [ga[0] as f64, ga[1] as f64],
                    [gb[0] as f64, gb[1] as f64],
                );
                let g = ramps[i].sample(if e.reverse { 1.0 - t } else { t });
                inside(&mut body, r, e.blend_mode, [g[0], g[1], g[2]], g[3] * a * e.opacity);
            }
            for e in cos.iter().rev() {
                inside(&mut body, r, e.blend_mode, rgb(e.color), a * e.opacity);
            }
            if let Some(e) = sa {
                inside(&mut body, r, e.blend_mode, rgb(e.color), satin * a * known * e.opacity);
            }
            if let Some(e) = ig {
                inside(&mut body, r, e.blend_mode, rgb(e.color), fall(-s, falloff(e)) * a * known * e.opacity);
            }
            for e in is.iter().rev() {
                let off = shadow_offset(e.angle, e.distance);
                let k = fall(-field.sdf([p[0] - off[0], p[1] - off[1]]), falloff(e)) * a * known * e.opacity;
                inside(&mut body, r, e.blend_mode, rgb(e.color), k);
            }
            for e in strokes.iter().rev().filter(|e| e.position == "inside") {
                inside(&mut body, r, e.blend_mode, rgb(e.color), (e.size + 0.5 + s).clamp(0.0, 1.0) * a * known * e.opacity);
            }
            r = over(body, r);
            // Over the pixels: the bevel, then centre strokes.
            if let Some(e) = bv {
                let b = &e.bevel;
                under(&mut r, b.shadow_mode, rgb(b.shadow_color), (-lit).max(0.0) * b.shadow_opacity * cover);
                under(&mut r, b.highlight_mode, rgb(b.highlight_color), lit.max(0.0) * b.highlight_opacity * cover);
            }
            for e in strokes.iter().rev().filter(|e| e.position == "center") {
                let k = (e.size * 0.5 + 0.5 - s.abs()).clamp(0.0, 1.0) * e.opacity;
                under(&mut r, e.blend_mode, rgb(e.color), k);
            }
            out[o..o + 4].copy_from_slice(&r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_distance_transform_is_exact() {
        // One seed in a 7 × 5 grid: squared distances are dx² + dy².
        let (w, h) = (7, 5);
        let mut seed = vec![false; w * h];
        seed[2 * w + 3] = true;
        let d = edt(&seed, w, h);
        for y in 0..h {
            for x in 0..w {
                let want = (x as f32 - 3.0).powi(2) + (y as f32 - 2.0).powi(2);
                assert_eq!(d[y * w + x], want, "{x},{y}");
            }
        }
        assert!(edt(&[false; 6], 3, 2).iter().all(|v| v.is_infinite()), "no seed: infinitely far");
    }
}
