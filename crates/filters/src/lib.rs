//! Adjustments and filters: CPU reference implementations of happypaint's
//! `engine/adjustments/adjustment-renderer.ts` shaders (spec 10).
//!
//! Pixels are premultiplied linear RGBA (f32). Colour kinds work on
//! unpremultiplied sRGB, like happypaint's float documents; spatial kinds work
//! on premultiplied storage values.

pub mod gradient;

use onepaint_color::{linear_to_srgb, srgb_to_linear};
use onepaint_stroke::{CurvePoint, Interp, eval_curve};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    BrightnessContrast,
    HueSaturation,
    BlackWhite,
    Invert,
    Threshold,
    Posterize,
    Levels,
    Curves,
    ColorBalance,
    GradientMap,
    Noise,
    GaussianBlur,
    MotionBlur,
    Sharpen,
}

impl Kind {
    pub const ALL: [Kind; 14] = [
        Kind::BrightnessContrast,
        Kind::HueSaturation,
        Kind::BlackWhite,
        Kind::Invert,
        Kind::Threshold,
        Kind::Posterize,
        Kind::Levels,
        Kind::Curves,
        Kind::ColorBalance,
        Kind::GradientMap,
        Kind::Noise,
        Kind::GaussianBlur,
        Kind::MotionBlur,
        Kind::Sharpen,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::BrightnessContrast => "brightness-contrast",
            Kind::HueSaturation => "hue-saturation",
            Kind::BlackWhite => "black-white",
            Kind::Invert => "invert",
            Kind::Threshold => "threshold",
            Kind::Posterize => "posterize",
            Kind::Levels => "levels",
            Kind::Curves => "curves",
            Kind::ColorBalance => "color-balance",
            Kind::GradientMap => "gradient-map",
            Kind::Noise => "noise",
            Kind::GaussianBlur => "gaussian-blur",
            Kind::MotionBlur => "motion-blur",
            Kind::Sharpen => "sharpen",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::BrightnessContrast => "Brightness / Contrast",
            Kind::HueSaturation => "Hue / Saturation",
            Kind::BlackWhite => "Black & White",
            Kind::Invert => "Invert",
            Kind::Threshold => "Threshold",
            Kind::Posterize => "Posterize",
            Kind::Levels => "Levels",
            Kind::Curves => "Curves",
            Kind::ColorBalance => "Color Balance",
            Kind::GradientMap => "Gradient Map",
            Kind::Noise => "Noise",
            Kind::GaussianBlur => "Gaussian Blur",
            Kind::MotionBlur => "Motion Blur",
            Kind::Sharpen => "Sharpen",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Per-pixel colour kinds (usable as live adjustment layers per tile).
    pub fn is_color(self) -> bool {
        !matches!(self, Kind::GaussianBlur | Kind::MotionBlur | Kind::Sharpen)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub pos: f32,
    pub color: [f32; 3],
}

/// happypaint's flat `AdjustmentParams` bag (camelCase JSON); unknown fields are kept.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Params {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hue: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_black: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_white: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gamma: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_black: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub out_white: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Vec<CurvePoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "curveR")]
    pub curve_r: Option<Vec<CurvePoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "curveG")]
    pub curve_g: Option<Vec<CurvePoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "curveB")]
    pub curve_b: Option<Vec<CurvePoint>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cyan_red: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub magenta_green: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yellow_blue: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient_stops: Option<Vec<Stop>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mono: Option<bool>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// happypaint `defaultParams` (AR:151-301).
pub fn default_params(k: Kind) -> Params {
    let mut p = Params::default();
    match k {
        Kind::BrightnessContrast => (p.brightness, p.contrast) = (Some(0.0), Some(0.0)),
        Kind::HueSaturation => (p.hue, p.saturation, p.brightness) = (Some(0.0), Some(0.0), Some(0.0)),
        Kind::BlackWhite => p.amount = Some(1.0),
        Kind::Invert => {}
        Kind::Threshold => p.threshold = Some(0.5),
        Kind::Posterize => p.levels = Some(4.0),
        Kind::Levels => {
            (p.in_black, p.in_white, p.gamma, p.out_black, p.out_white) = (Some(0.0), Some(1.0), Some(1.0), Some(0.0), Some(1.0))
        }
        Kind::Curves => {
            p.curve = Some(vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)]);
            p.curve_mode = Some("smooth".into());
        }
        Kind::ColorBalance => (p.cyan_red, p.magenta_green, p.yellow_blue) = (Some(0.0), Some(0.0), Some(0.0)),
        Kind::GradientMap => p.gradient_stops = Some(vec![Stop { pos: 0.0, color: [0.0; 3] }, Stop { pos: 1.0, color: [1.0; 3] }]),
        Kind::Noise => (p.amount, p.mono) = (Some(0.0), Some(true)),
        Kind::GaussianBlur => p.radius = Some(0.0),
        Kind::MotionBlur => (p.angle, p.distance) = (Some(0.0), Some(0.0)),
        Kind::Sharpen => p.amount = Some(0.0),
    }
    p
}

fn curve_is_identity(c: &Option<Vec<CurvePoint>>) -> bool {
    c.as_ref().is_none_or(|c| c.len() < 2 || c.iter().all(|p| (p.y - p.x).abs() < 1e-3))
}

/// The destructive preview skips identity params (AR `isIdentity`).
pub fn is_identity(k: Kind, p: &Params) -> bool {
    let z = |v: Option<f32>| v.unwrap_or(0.0) == 0.0;
    match k {
        Kind::BrightnessContrast => z(p.brightness) && z(p.contrast),
        Kind::HueSaturation => z(p.hue) && z(p.saturation) && z(p.brightness),
        Kind::BlackWhite => p.amount.unwrap_or(1.0) <= 0.0,
        Kind::Invert | Kind::Threshold | Kind::Posterize | Kind::GradientMap => false,
        Kind::Levels => {
            z(p.in_black)
                && p.in_white.unwrap_or(1.0) == 1.0
                && p.gamma.unwrap_or(1.0) == 1.0
                && z(p.out_black)
                && p.out_white.unwrap_or(1.0) == 1.0
        }
        Kind::Curves => [&p.curve, &p.curve_r, &p.curve_g, &p.curve_b].into_iter().all(curve_is_identity),
        Kind::ColorBalance => z(p.cyan_red) && z(p.magenta_green) && z(p.yellow_blue),
        Kind::GaussianBlur => p.radius.unwrap_or(0.0) <= 0.0,
        Kind::MotionBlur => p.distance.unwrap_or(0.0) <= 0.0,
        Kind::Sharpen | Kind::Noise => p.amount.unwrap_or(0.0) <= 0.0,
    }
}

pub type Lut = [[u8; 4]; 256];

fn interp(p: &Params) -> Interp {
    if p.curve_mode.as_deref() == Some("linear") { Interp::Linear } else { Interp::Smooth }
}

/// Curves LUT (AR `uploadLut`): master curve, then the channel curve on its (unclamped) output.
pub fn build_curves_lut(p: &Params) -> Lut {
    let identity = vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)];
    let master = p.curve.clone().unwrap_or(identity);
    let mode = interp(p);
    let chans = [&p.curve_r, &p.curve_g, &p.curve_b];
    let mut lut = [[0u8; 4]; 256];
    for (i, e) in lut.iter_mut().enumerate() {
        let m = eval_curve(&master, i as f64 / 255.0, mode);
        for c in 0..3 {
            let v = match chans[c] {
                Some(ch) if ch.len() >= 2 => eval_curve(ch, m, mode),
                _ => m,
            };
            e[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        e[3] = 255;
    }
    lut
}

fn gradient_stops(p: &Params) -> Vec<Stop> {
    match &p.gradient_stops {
        Some(s) if s.len() >= 2 => {
            let mut s = s.clone();
            s.sort_by(|a, b| a.pos.total_cmp(&b.pos));
            s
        }
        _ => vec![Stop { pos: 0.0, color: p.shadow.unwrap_or([0.0; 3]) }, Stop { pos: 1.0, color: p.highlight.unwrap_or([1.0; 3]) }],
    }
}

pub fn build_gradient_lut(p: &Params) -> Lut {
    let stops = gradient_stops(p);
    let mut lut = [[0u8; 4]; 256];
    for (i, e) in lut.iter_mut().enumerate() {
        let t = i as f32 / 255.0;
        let c = if t <= stops[0].pos {
            stops[0].color
        } else if t >= stops[stops.len() - 1].pos {
            stops[stops.len() - 1].color
        } else {
            let mut out = stops[0].color;
            for w in stops.windows(2) {
                let (a, b) = (&w[0], &w[1]);
                if a.pos <= t && t <= b.pos {
                    let f = if b.pos == a.pos { 0.0 } else { (t - a.pos) / (b.pos - a.pos) };
                    out = [0, 1, 2].map(|k| a.color[k] + (b.color[k] - a.color[k]) * f);
                    break;
                }
            }
            out
        };
        for k in 0..3 {
            e[k] = (c[k] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        e[3] = 255;
    }
    lut
}

/// Bilinear 256×1 texture lookup at `v` (texel centres at (i+0.5)/256, clamp-to-edge).
pub fn lut_sample(lut: &Lut, v: f32, ch: usize) -> f32 {
    let t = (v * 256.0 - 0.5).clamp(0.0, 255.0);
    let i0 = t.floor() as usize;
    let i1 = (i0 + 1).min(255);
    let f = t - i0 as f32;
    (lut[i0][ch] as f32 * (1.0 - f) + lut[i1][ch] as f32 * f) / 255.0
}

fn rgb2hsv(c: [f32; 3]) -> [f32; 3] {
    // GLSL rgb2hsv as in AR (branchless form expanded).
    let k = [0.0f32, -1.0 / 3.0, 2.0 / 3.0, -1.0];
    let step = |edge: f32, x: f32| if x < edge { 0.0 } else { 1.0 };
    let mixv = |a: [f32; 4], b: [f32; 4], t: f32| [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t);
    let p = mixv([c[2], c[1], k[3], k[2]], [c[1], c[2], k[0], k[1]], step(c[2], c[1]));
    let q = mixv([p[0], p[1], p[3], c[0]], [c[0], p[1], p[2], p[0]], step(p[0], c[0]));
    let d = q[0] - q[3].min(q[1]);
    [(q[2] + (q[3] - q[1]) / (6.0 * d + 1e-10)).abs(), d / (q[0] + 1e-10), q[0]]
}

fn hsv2rgb(c: [f32; 3]) -> [f32; 3] {
    let k = [1.0f32, 2.0 / 3.0, 1.0 / 3.0, 3.0];
    [0, 1, 2].map(|i| {
        let p = ((c[0] + k[i]).fract() * 6.0 - k[3]).abs();
        c[2] * (k[0] + ((p - k[0]).clamp(0.0, 1.0) - k[0]) * c[1])
    })
}

fn pcg(v: u32) -> u32 {
    let s = v.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
    (w >> 22) ^ w
}

fn rnd(x: u32, y: u32, k: u32) -> f32 {
    pcg(x ^ pcg(y ^ pcg(k))) as f32 * (1.0 / 4_294_967_296.0)
}

/// Resolved parameters plus any LUT, ready to run per pixel.
#[derive(Clone)]
pub struct Prepared {
    pub kind: Kind,
    p: Params,
    lut: Option<Box<Lut>>,
}

const LUMA: [f32; 3] = [0.299, 0.587, 0.114];

fn luma(c: [f32; 3]) -> f32 {
    c[0].clamp(0.0, 1.0) * LUMA[0] + c[1].clamp(0.0, 1.0) * LUMA[1] + c[2].clamp(0.0, 1.0) * LUMA[2]
}

pub fn prepare(kind: Kind, p: &Params) -> Prepared {
    let lut = match kind {
        Kind::Curves => Some(Box::new(build_curves_lut(p))),
        Kind::GradientMap => Some(Box::new(build_gradient_lut(p))),
        _ => None,
    };
    Prepared { kind, p: p.clone(), lut }
}

impl Prepared {
    /// Adjust one premultiplied linear pixel at doc (x, y). Spatial kinds pass through.
    pub fn color_px(&self, s: [f32; 4], x: i32, y: i32) -> [f32; 4] {
        if !self.kind.is_color() {
            return s;
        }
        let a = s[3];
        let un = if a > 0.0 { [s[0] / a, s[1] / a, s[2] / a] } else { [s[0], s[1], s[2]] };
        let w = un.map(linear_to_srgb);
        let p = &self.p;
        let c01 = |v: f32| v.clamp(0.0, 1.0);
        let o: [f32; 3] = match self.kind {
            Kind::BrightnessContrast => {
                let (b, c) = (p.brightness.unwrap_or(0.0), p.contrast.unwrap_or(0.0));
                let k = if c <= 0.0 { 1.0 + c } else { 1.0 / (1.0 - c).max(1e-3) };
                w.map(|v| c01((v + b - 0.5) * k + 0.5))
            }
            Kind::HueSaturation => {
                let mut h = rgb2hsv(w.map(c01));
                h[0] = (h[0] + p.hue.unwrap_or(0.0) / 360.0).rem_euclid(1.0);
                h[1] = c01(h[1] * (1.0 + p.saturation.unwrap_or(0.0)));
                h[2] = c01(h[2] * (1.0 + p.brightness.unwrap_or(0.0)));
                hsv2rgb(h)
            }
            Kind::BlackWhite => {
                let l = luma(w);
                let t = p.amount.unwrap_or(1.0);
                w.map(|v| v + (l - v) * t)
            }
            Kind::Invert => w.map(|v| c01(1.0 - v)),
            Kind::Threshold => {
                let l = luma(w);
                [if l >= p.threshold.unwrap_or(0.5) { 1.0 } else { 0.0 }; 3]
            }
            Kind::Posterize => {
                let n = p.levels.unwrap_or(4.0).max(2.0) - 1.0;
                w.map(|v| (c01(v) * n + 0.5).floor() / n)
            }
            Kind::Levels => {
                let (ib, iw) = (p.in_black.unwrap_or(0.0), p.in_white.unwrap_or(1.0));
                let g = p.gamma.unwrap_or(1.0).max(1e-3);
                let (ob, ow) = (p.out_black.unwrap_or(0.0), p.out_white.unwrap_or(1.0));
                w.map(|v| {
                    let n = c01((c01(v) - ib) / (iw - ib).max(1e-4)).powf(1.0 / g);
                    c01(ob + n * (ow - ob))
                })
            }
            Kind::Curves => {
                let lut = self.lut.as_deref().map_or_else(|| build_curves_lut(p), |l| *l);
                [0, 1, 2].map(|i| lut_sample(&lut, c01(w[i]), i))
            }
            Kind::ColorBalance => {
                let l = luma(w);
                let k = 1.0 - (2.0 * l - 1.0).abs();
                let d = [p.cyan_red.unwrap_or(0.0), p.magenta_green.unwrap_or(0.0), p.yellow_blue.unwrap_or(0.0)];
                [0, 1, 2].map(|i| c01(w[i] + d[i] * 0.5 * k))
            }
            Kind::GradientMap => {
                let l = luma(w.map(c01));
                let lut = self.lut.as_deref().map_or_else(|| build_gradient_lut(p), |l| *l);
                [0, 1, 2].map(|i| lut_sample(&lut, l, i))
            }
            Kind::Noise => {
                let amt = p.amount.unwrap_or(0.0);
                let seed = 17u32 * 3;
                let (px, py) = (x.max(0) as u32, y.max(0) as u32);
                let n = if p.mono.unwrap_or(true) {
                    [rnd(px, py, seed) - 0.5; 3]
                } else {
                    [rnd(px, py, seed) - 0.5, rnd(px, py, seed + 1) - 0.5, rnd(px, py, seed + 2) - 0.5]
                };
                [0, 1, 2].map(|i| c01(w[i] + n[i] * amt))
            }
            _ => w,
        };
        let l = o.map(srgb_to_linear);
        [l[0] * a, l[1] * a, l[2] * a, a]
    }
}

fn sample(buf: &[f32], w: usize, h: usize, x: f32, y: f32) -> [f32; 4] {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |xx: usize, yy: usize| {
        let i = (yy * w + xx) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    [0, 1, 2, 3].map(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        top + (bot - top) * fy
    })
}

/// One pass of happypaint's 65-tap Gaussian along `dir` (px), radius `r`.
fn blur_pass(buf: &[f32], w: usize, h: usize, dir: (f32, f32), r: f32) -> Vec<f32> {
    const HALF: i32 = 32;
    let spread = r.max(1.0) / HALF as f32;
    let sigma = HALF as f32 * 0.5;
    let weights: Vec<f32> = (-HALF..=HALF).map(|i| (-0.5 * (i * i) as f32 / (sigma * sigma)).exp()).collect();
    let wsum: f32 = weights.iter().sum();
    let mut out = vec![0.0; buf.len()];
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let mut acc = [0.0f32; 4];
            for (k, wt) in weights.iter().enumerate() {
                let o = (k as i32 - HALF) as f32 * spread;
                let s = sample(buf, w, h, x as f32 + dir.0 * o, y as f32 + dir.1 * o);
                for c in 0..4 {
                    acc[c] += s[c] * wt;
                }
            }
            for c in 0..4 {
                row[x * 4 + c] = acc[c] / wsum.max(1e-4);
            }
        }
    });
    out
}

/// Spatial kinds on a dense premultiplied buffer (`w × h × 4`); colour kinds return a copy.
pub fn apply_spatial(kind: Kind, p: &Params, buf: &[f32], w: usize, h: usize) -> Vec<f32> {
    if is_identity(kind, p) || w == 0 || h == 0 {
        return buf.to_vec();
    }
    match kind {
        Kind::GaussianBlur => {
            let r = p.radius.unwrap_or(0.0);
            let a = blur_pass(buf, w, h, (1.0, 0.0), r);
            blur_pass(&a, w, h, (0.0, 1.0), r)
        }
        Kind::MotionBlur => {
            let t = p.angle.unwrap_or(0.0).to_radians();
            blur_pass(buf, w, h, (t.cos(), t.sin()), p.distance.unwrap_or(0.0))
        }
        Kind::Sharpen => {
            let a = blur_pass(buf, w, h, (1.0, 0.0), 2.0);
            let b = blur_pass(&a, w, h, (0.0, 1.0), 2.0);
            let k = p.amount.unwrap_or(0.0) * 3.0;
            buf.iter().zip(&b).map(|(s, bl)| (s + (s - bl) * k).clamp(0.0, 1.0)).collect()
        }
        _ => buf.to_vec(),
    }
}

/// How far a spatial kind reads beyond a pixel (px), for tiling margins.
pub fn reach(kind: Kind, p: &Params) -> i32 {
    match kind {
        Kind::GaussianBlur => p.radius.unwrap_or(0.0).max(1.0).ceil() as i32 + 2,
        Kind::MotionBlur => p.distance.unwrap_or(0.0).max(1.0).ceil() as i32 + 2,
        Kind::Sharpen => 4,
        _ => 0,
    }
}
