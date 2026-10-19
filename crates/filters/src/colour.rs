//! Per-pixel kinds added in spec 18: color-to-alpha, replace-color,
//! selective-color, screentone and the three fill layers.

use crate::gradient::{GradStop, Ramp, Shape, reverse_stops, shape_t};
use crate::params::Params;
use crate::pattern::PatternTile;
use crate::tex::{Frame, LUMA, Px, dot3, fract, from_work, mix, repremul, smoothstep, to_work, unpremul};
use onepaint_color::Space;

/// Selective Colour+ cap (SC `MAX_COLOR_SAMPLES`).
pub const MAX_COLOR_SAMPLES: usize = 8;

/// GLSL `rgb2hsv` (hue in turns), as in AR's colour shaders.
pub(crate) fn rgb2hsv(c: [f32; 3]) -> [f32; 3] {
    let k = [0.0f32, -1.0 / 3.0, 2.0 / 3.0, -1.0];
    let step = |edge: f32, x: f32| if x < edge { 0.0 } else { 1.0 };
    let mixv = |a: [f32; 4], b: [f32; 4], t: f32| [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t);
    let p = mixv([c[2], c[1], k[3], k[2]], [c[1], c[2], k[0], k[1]], step(c[2], c[1]));
    let q = mixv([p[0], p[1], p[3], c[0]], [c[0], p[1], p[2], p[0]], step(p[0], c[0]));
    let d = q[0] - q[3].min(q[1]);
    [(q[2] + (q[3] - q[1]) / (6.0 * d + 1e-10)).abs(), d / (q[0] + 1e-10), q[0]]
}

/// GLSL `hsv2rgb` (hue in turns).
pub(crate) fn hsv2rgb(c: [f32; 3]) -> [f32; 3] {
    let k = [1.0f32, 2.0 / 3.0, 1.0 / 3.0, 3.0];
    [0, 1, 2].map(|i| {
        let p = (fract(c[0] + k[i]) * 6.0 - k[3]).abs();
        c[2] * (k[0] + ((p - k[0]).clamp(0.0, 1.0) - k[0]) * c[1])
    })
}

/// CPU `rgb01ToHsv` (color.ts:87): hue in degrees.
pub fn rgb01_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let d = max - min;
    let mut h = 0.0;
    if d > 0.0 {
        h = if max == c[0] {
            60.0 * ((c[1] - c[2]) / d % 6.0)
        } else if max == c[1] {
            60.0 * ((c[2] - c[0]) / d + 2.0)
        } else {
            60.0 * ((c[0] - c[1]) / d + 4.0)
        };
    }
    if h < 0.0 {
        h += 360.0;
    }
    [h, if max == 0.0 { 0.0 } else { d / max }, max]
}

const W_HUE: f32 = 1.0;
const W_SAT: f32 = 0.7;
const W_VAL: f32 = 0.45;

/// One enabled sample as the shader receives it: HSV target (hue in turns),
/// deltas (hue in turns) and (reach, falloff).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScSample {
    target: [f32; 3],
    delta: [f32; 3],
    range: [f32; 2],
}

impl ScSample {
    /// Target, delta and (reach, falloff), for the GPU packing (spec 42).
    pub(crate) fn parts(&self) -> ([f32; 3], [f32; 3], [f32; 2]) {
        (self.target, self.delta, self.range)
    }
}

pub(crate) fn sc_samples(p: &Params) -> Vec<ScSample> {
    p.color_samples
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter(|c| c.enabled != Some(false))
        .take(MAX_COLOR_SAMPLES)
        .map(|c| {
            let hsv = rgb01_to_hsv(c.target);
            ScSample { target: [hsv[0] / 360.0, hsv[1], hsv[2]], delta: [c.dh / 360.0, c.ds, c.dv], range: [c.reach, c.falloff] }
        })
        .collect()
}

fn sc_weight(px: [f32; 3], t: [f32; 3], reach: f32, falloff: f32) -> f32 {
    let chroma = px[1].min(t[1]);
    let d = (px[0] - t[0]).abs();
    let dh = W_HUE * chroma * d.min(1.0 - d) * 2.0;
    let ds = W_SAT * (px[1] - t[1]);
    let dv = W_VAL * (px[2] - t[2]);
    let norm = (W_HUE * W_HUE + W_SAT * W_SAT + W_VAL * W_VAL).sqrt();
    let dist = (dh * dh + ds * ds + dv * dv).sqrt() / norm;
    let edge = reach + falloff;
    if dist <= reach {
        1.0
    } else if dist >= edge {
        0.0
    } else {
        1.0 - smoothstep(reach, edge, dist)
    }
}

/// `scCoverage`: accumulated selection weight, clamped.
pub(crate) fn sc_coverage(rgb: [f32; 3], samples: &[ScSample]) -> f32 {
    let px = rgb2hsv(rgb.map(|v| v.clamp(0.0, 1.0)));
    samples.iter().map(|s| sc_weight(px, s.target, s.range[0], s.range[1])).sum::<f32>().clamp(0.0, 1.0)
}

/// `scApply`: the weighted H/S/V shift for one straight sRGB colour.
pub(crate) fn sc_apply(rgb: [f32; 3], samples: &[ScSample]) -> [f32; 3] {
    let px = rgb2hsv(rgb.map(|v| v.clamp(0.0, 1.0)));
    let mut w_sum = 0.0;
    let mut delta = [0.0f32; 3];
    for s in samples {
        let w = sc_weight(px, s.target, s.range[0], s.range[1]);
        if w <= 0.0 {
            continue;
        }
        w_sum += w;
        for (d, sd) in delta.iter_mut().zip(s.delta) {
            *d += w * sd;
        }
    }
    if w_sum <= 0.0 {
        return rgb;
    }
    let scale = if w_sum > 1.0 { 1.0 / w_sum } else { 1.0 };
    let d = delta.map(|v| v * scale);
    hsv2rgb([fract(px[0] + d[0]), (px[1] * (1.0 + d[1])).clamp(0.0, 1.0), (px[2] * (1.0 + d[2])).clamp(0.0, 1.0)])
}

/// The Selective Colour+ result for one straight sRGB pixel (the CPU reference
/// `selectiveColorRgb`, but with the shader's HSV maths).
pub fn selective_color_rgb(rgb: [f32; 3], p: &Params) -> [f32; 3] {
    sc_apply(rgb, &sc_samples(p))
}

/// Everything a per-pixel kind needs beyond its params.
#[derive(Clone, Default)]
pub(crate) struct Extra {
    pub frame: Frame,
    pub samples: Vec<ScSample>,
    pub ramp: Option<std::sync::Arc<Ramp>>,
    pub pattern: Option<std::sync::Arc<PatternTile>>,
}

/// A gradient fill's shape as the GPU numbers it (spec 46).
pub(crate) fn grad_shape_code(s: Option<&str>) -> u32 {
    match grad_shape(s) {
        Shape::Linear => 0,
        Shape::Radial => 1,
        Shape::Angle => 2,
        Shape::Reflected => 3,
        Shape::Diamond => 4,
    }
}

fn grad_shape(s: Option<&str>) -> Shape {
    match s {
        Some("radial") => Shape::Radial,
        Some("angle") => Shape::Angle,
        Some("reflected") => Shape::Reflected,
        Some("diamond") => Shape::Diamond,
        _ => Shape::Linear,
    }
}

/// The gradient fill's stops (reversed when asked), sorted.
pub fn fill_stops(p: &Params) -> Vec<GradStop> {
    let raw = match p.fill_stops.as_deref() {
        Some(s) if !s.is_empty() => s.to_vec(),
        _ => crate::default_params(crate::Kind::GradientFill).fill_stops.unwrap_or_default(),
    };
    let mut stops: Vec<GradStop> = raw
        .iter()
        .map(|s| GradStop {
            pos: s.pos.clamp(0.0, 1.0) as f64,
            color: s.color.map(|c| c as f64),
            opacity: s.opacity.clamp(0.0, 1.0) as f64,
            mid: s.mid.map(|m| m as f64),
        })
        .collect();
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    if p.grad_reverse.unwrap_or(false) { reverse_stops(&stops) } else { stops }
}

pub(crate) fn extra_for(kind: crate::Kind, p: &Params, frame: Frame) -> Extra {
    use crate::Kind;
    let mut e = Extra { frame, ..Default::default() };
    match kind {
        Kind::SelectiveColor => e.samples = sc_samples(p),
        Kind::GradientFill => e.ramp = Some(std::sync::Arc::new(Ramp::new_in(&fill_stops(p), frame.space))),
        Kind::PatternFill => {
            e.pattern = match p.pattern_image.as_deref().filter(|u| !u.is_empty()) {
                Some(url) => crate::pattern::imported_tile(url),
                None => {
                    let id = p.pattern_id.as_deref().unwrap_or("checker");
                    let tile =
                        crate::pattern::pattern_pixels(id, p.pattern_color_a.unwrap_or([0.0; 3]), p.pattern_color_b.unwrap_or([1.0; 3]));
                    Some(std::sync::Arc::new(tile))
                }
            };
        }
        _ => {}
    }
    e
}

/// COLOR_TO_ALPHA_SRC (AR:784).
fn color_to_alpha(s: Px, p: &Params, sp: Space) -> Px {
    let a = s[3];
    let rgb = to_work(unpremul(s), sp);
    let t = p.target.unwrap_or([1.0; 3]);
    let thr = p.threshold.unwrap_or(0.0);
    let d = [0, 1, 2].map(|c| if rgb[c] > t[c] { (rgb[c] - t[c]) / (1.0 - t[c]).max(1e-4) } else { (t[c] - rgb[c]) / t[c].max(1e-4) });
    let mut alpha = d[0].max(d[1]).max(d[2]).clamp(0.0, 1.0);
    alpha = ((alpha - thr) / (1.0 - thr).max(1e-4)).clamp(0.0, 1.0);
    let out = if alpha > 1e-4 { [0, 1, 2].map(|c| (t[c] + (rgb[c] - t[c]) / alpha).clamp(0.0, 1.0)) } else { rgb };
    repremul(out, a * alpha, sp)
}

/// REPLACE_COLOR_SRC (AR:808).
fn replace_color(s: Px, p: &Params, sp: Space) -> Px {
    let rgb = to_work(unpremul(s), sp).map(|v| v.clamp(0.0, 1.0));
    let t = p.target.unwrap_or([1.0; 3]).map(|v| v.clamp(0.0, 1.0));
    let rep = p.replace.unwrap_or([0.2, 0.45, 1.0]).map(|v| v.clamp(0.0, 1.0));
    let thr = p.threshold.unwrap_or(0.3);
    let dist = ((rgb[0] - t[0]).powi(2) + (rgb[1] - t[1]).powi(2) + (rgb[2] - t[2]).powi(2)).sqrt() / 1.732;
    let w = 1.0 - smoothstep(thr, thr + 0.12, dist);
    let ph = rgb2hsv(rgb);
    let rh = rgb2hsv(rep);
    let recol = hsv2rgb([rh[0], rh[1], ph[2]]);
    repremul([0, 1, 2].map(|c| mix(rgb[c], recol[c], w)), s[3], sp)
}

/// SELECTIVE_COLOR_SRC (AR:840).
fn selective_color(s: Px, p: &Params, e: &Extra) -> Px {
    let rgb = to_work(unpremul(s), e.frame.space).map(|v| v.clamp(0.0, 1.0));
    let mut o = sc_apply(rgb, &e.samples);
    if p.show_range.unwrap_or(false) {
        let cov = sc_coverage(rgb, &e.samples);
        let grey = dot3(rgb, LUMA) * 0.55;
        let red = [1.0, 0.15, 0.15];
        o = [0, 1, 2].map(|c| mix(grey * 0.5, mix(grey, red[c], 0.75), cov));
    }
    repremul(o, s[3], e.frame.space)
}

/// Tone's `rand` (sin hash), ported in f32.
fn tone_rand(p: [f32; 2]) -> f32 {
    fract((p[0] * 127.1 + p[1] * 311.7).sin() * 43_758.547)
}

/// TONE_SRC (AR:1061) with `u_viewScale = 0` (real output).
fn tone(s: Px, p: &Params, e: &Extra, x: i32, y: i32) -> Px {
    let f = e.frame;
    let size = f.size();
    let px = [x as f32 + 0.5, y as f32 + 0.5];
    let density = p.tone_density.as_deref().unwrap_or("flat");
    let lvl = p.tone_level.unwrap_or(0.5).clamp(0.0, 1.0);
    let level = match density {
        "luminance" => {
            let rgb = to_work(unpremul(s), f.space).map(|v| v.clamp(0.0, 1.0));
            (1.0 - dot3(rgb, LUMA).clamp(0.0, 1.0)) * s[3]
        }
        "gradient" => {
            let g = p.tone_gradient_angle.unwrap_or(0.0).to_radians();
            let dir = [g.sin(), g.cos()];
            let t = (px[0] * dir[0] + px[1] * dir[1]) / (dir[0].abs() * size[0] + dir[1].abs() * size[1]).max(1.0);
            mix(lvl, p.tone_level_end.unwrap_or(0.0).clamp(0.0, 1.0), t.clamp(0.0, 1.0))
        }
        _ => lvl,
    };
    let dpi = if f.dpi > 0.0 { f.dpi } else { 300.0 };
    let cell = (dpi / p.lpi.unwrap_or(60.0).max(5.0)).max(1.0);
    let ang = p.angle.unwrap_or(15.0).to_radians();
    let (c, sn) = (ang.cos(), ang.sin());
    let shape = p.tone_shape.as_deref().unwrap_or("dot");
    let dist_at = |q: [f32; 2]| {
        let rp = [c * q[0] - sn * q[1], sn * q[0] + c * q[1]];
        let cc = [((rp[0] / cell).floor() + 0.5) * cell, ((rp[1] / cell).floor() + 0.5) * cell];
        let d = [rp[0] - cc[0], rp[1] - cc[1]];
        match shape {
            "line" => d[1].abs() - level * cell * 0.5,
            "cross" => d[0].abs().min(d[1].abs()) - level * cell * 0.35,
            "sand" => {
                let g = (cell * 0.25).max(1.0);
                tone_rand([(rp[0] / g).floor(), (rp[1] / g).floor()]) - level
            }
            _ => (d[0] * d[0] + d[1] * d[1]).sqrt() - level * cell * 0.72,
        }
    };
    let dist = dist_at(px);
    let mut cov = if p.hard_dots.unwrap_or(false) {
        if dist <= 0.0 { 1.0 } else { 0.0 }
    } else {
        let fw = (dist_at([px[0] + 1.0, px[1]]) - dist).abs() + (dist_at([px[0], px[1] + 1.0]) - dist).abs();
        let w = fw.max(1e-5);
        1.0 - smoothstep(-w, w, dist)
    };
    if level < 0.0005 {
        cov = 0.0;
    }
    let ink = to_work(p.target.unwrap_or([0.0; 3]), f.space).map(|v| v.clamp(0.0, 1.0));
    let l = from_work(ink, f.space);
    [l[0] * cov, l[1] * cov, l[2] * cov, cov]
}

/// GRADIENT_FILL_SRC (AR:1650).
fn gradient_fill(p: &Params, e: &Extra, x: i32, y: i32) -> Px {
    let size = e.frame.size();
    let a = [p.grad_ax.unwrap_or(0.5) as f64 * size[0] as f64, p.grad_ay.unwrap_or(0.0) as f64 * size[1] as f64];
    let b = [p.grad_bx.unwrap_or(0.5) as f64 * size[0] as f64, p.grad_by.unwrap_or(1.0) as f64 * size[1] as f64];
    let t = shape_t(grad_shape(p.grad_shape.as_deref()), [x as f64 + 0.5, y as f64 + 0.5], a, b);
    let c = match &e.ramp {
        Some(r) => r.sample(t),
        None => [t as f32, t as f32, t as f32, 1.0],
    };
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// PATTERN_FILL_SRC (AR:1685): the tile repeated, scaled, rotated, offset.
fn pattern_fill(p: &Params, e: &Extra, x: i32, y: i32) -> Px {
    let Some(tile) = &e.pattern else { return [0.0; 4] };
    let size = e.frame.size();
    let scale = p.pattern_scale.unwrap_or(1.0).max(0.05);
    let d = [x as f32 + 0.5 - p.pattern_off_x.unwrap_or(0.0) * size[0], y as f32 + 0.5 - p.pattern_off_y.unwrap_or(0.0) * size[1]];
    let a = p.pattern_angle.unwrap_or(0.0).to_radians();
    let (c, s) = (a.cos(), a.sin());
    let r = [c * d[0] - s * d[1], s * d[0] + c * d[1]];
    let t = tile.sample_repeat(r[0] / (tile.w as f32 * scale), r[1] / (tile.h as f32 * scale));
    let l = from_work([t[0], t[1], t[2]], e.frame.space);
    [l[0] * t[3], l[1] * t[3], l[2] * t[3], t[3]]
}

/// SOLID_FILL_SRC (AR:1670).
fn solid_fill(p: &Params, sp: Space) -> Px {
    let l = from_work(p.fill_color.unwrap_or([0.0; 3]), sp);
    [l[0], l[1], l[2], 1.0]
}

/// The spec-18 per-pixel kinds; `None` for kinds handled elsewhere.
pub(crate) fn px(kind: crate::Kind, p: &Params, e: &Extra, s: Px, x: i32, y: i32) -> Option<Px> {
    use crate::Kind;
    Some(match kind {
        Kind::ColorToAlpha => color_to_alpha(s, p, e.frame.space),
        Kind::ReplaceColor => replace_color(s, p, e.frame.space),
        Kind::SelectiveColor => selective_color(s, p, e),
        Kind::Tone => tone(s, p, e, x, y),
        Kind::GradientFill => gradient_fill(p, e, x, y),
        Kind::SolidFill => solid_fill(p, e.frame.space),
        Kind::PatternFill => pattern_fill(p, e, x, y),
        Kind::Clouds | Kind::Fog | Kind::Smoke | Kind::Nebula | Kind::Starfield => {
            crate::render::clouds_px(p, kind == Kind::Starfield, e.frame, x, y)
        }
        Kind::LensFlare => crate::render::flare_px(p, e.frame, x, y),
        _ => return None,
    })
}

/// The hue weights in shader order: primaries (reds, greens, blues) and
/// secondaries (yellows, cyans, magentas).
pub(crate) fn bw_weights(p: &Params) -> ([f32; 3], [f32; 3]) {
    let [r, y, g, c, b, m] = crate::BW_DEFAULT_WEIGHTS;
    let w = |v: Option<f32>, d: f32| v.unwrap_or(d);
    ([w(p.bw_reds, r), w(p.bw_greens, g), w(p.bw_blues, b)], [w(p.bw_yellows, y), w(p.bw_cyans, c), w(p.bw_magentas, m)])
}

/// BLACK_WHITE_SRC (happypaint v0.201.3, AR:1012): grey = min + (mid − min)
/// · w(secondary hue) + (max − mid) · w(primary hue), the primary being the
/// largest channel's hue and the secondary the two largest's; the tint
/// colours the grey at its own luminosity (Color blend); Amount mixes.
/// `rgb` is straight work values in 0..1.
pub(crate) fn black_white(rgb: [f32; 3], p: &Params) -> [f32; 3] {
    let (prim, sec) = bw_weights(p);
    let [r, g, b] = rgb;
    let (mn, mx) = (r.min(g).min(b), r.max(g).max(b));
    let md = r + g + b - mn - mx;
    let (wp, ws) = if r >= g && r >= b {
        (prim[0], if g >= b { sec[0] } else { sec[2] })
    } else if g >= b {
        (prim[1], if r >= b { sec[0] } else { sec[1] })
    } else {
        (prim[2], if r >= g { sec[2] } else { sec[1] })
    };
    let grey = (mn + (md - mn) * ws + (mx - md) * wp).clamp(0.0, 1.0);
    let o = if p.bw_tint.unwrap_or(false) {
        set_lum(p.bw_tint_color.unwrap_or(crate::BW_DEFAULT_TINT), grey).map(|v| v.clamp(0.0, 1.0))
    } else {
        [grey; 3]
    };
    let t = p.amount.unwrap_or(1.0);
    [0, 1, 2].map(|i| rgb[i] + (o[i] - rgb[i]) * t)
}

/// Color Balance's tone-range weights at HSL lightness `l` (GIMP's
/// published masks, as happypaint uses): shadows, midtones, highlights.
pub(crate) fn balance_weights(l: f32) -> [f32; 3] {
    const A: f32 = 0.25;
    const B: f32 = 0.333;
    const SCALE: f32 = 0.7;
    let c = |v: f32| v.clamp(0.0, 1.0);
    [c((l - B) / -A + 0.5) * SCALE, c((l - B) / A + 0.5) * c((l + B - 1.0) / -A + 0.5) * SCALE, c((l + B - 1.0) / A + 0.5) * SCALE]
}

/// COLOR_BALANCE_SRC (happypaint v0.201.3, AR:846): a (cyan–red,
/// magenta–green, yellow–blue) shift per tone range, weighted by the
/// pixel's HSL lightness; Preserve Luminosity (on unless false) puts the
/// original luminosity back, the Luminosity blend's. `rgb` is straight
/// work values in 0..1.
pub(crate) fn color_balance(rgb: [f32; 3], p: &Params) -> [f32; 3] {
    let v = |x: Option<f32>| x.unwrap_or(0.0);
    let sh = [v(p.cb_shadow_cr), v(p.cb_shadow_mg), v(p.cb_shadow_yb)];
    let mid = [v(p.cyan_red), v(p.magenta_green), v(p.yellow_blue)];
    let hi = [v(p.cb_high_cr), v(p.cb_high_mg), v(p.cb_high_yb)];
    let l = (rgb[0].max(rgb[1]).max(rgb[2]) + rgb[0].min(rgb[1]).min(rgb[2])) * 0.5;
    let [ws, wm, wh] = balance_weights(l);
    let o = [0, 1, 2].map(|i| (rgb[i] + sh[i] * ws + mid[i] * wm + hi[i] * wh).clamp(0.0, 1.0));
    if p.preserve_lum == Some(false) {
        return o;
    }
    set_lum(o, lum_w3c(rgb)).map(|v| v.clamp(0.0, 1.0))
}

/// The W3C / PDF blend luminosity (0.3, 0.59, 0.11).
pub(crate) fn lum_w3c(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// W3C SetLum with ClipColor: `c` moved to luminosity `l`, kept in gamut
/// (happypaint `setLum`, PHOTO_FILTER_SRC).
pub(crate) fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum_w3c(c);
    let mut c = c.map(|v| v + d);
    let ll = lum_w3c(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 {
        c = c.map(|v| ll + (v - ll) * ll / (ll - n).max(1e-6));
    }
    if x > 1.0 {
        c = c.map(|v| ll + (v - ll) * (1.0 - ll) / (x - ll).max(1e-6));
    }
    c
}
