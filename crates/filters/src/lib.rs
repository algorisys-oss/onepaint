//! Adjustments and filters: CPU reference implementations of happypaint's
//! `engine/adjustments/adjustment-renderer.ts` shaders (specs 10 and 18).
//!
//! Pixels are premultiplied linear RGBA (f32). Colour kinds work on
//! unpremultiplied sRGB, like happypaint's float documents; spatial kinds work
//! on premultiplied storage values.

mod blur;
mod colour;
mod distort;
pub mod gpu;
pub mod gradient;
mod kind;
pub mod lightning;
pub mod liquify;
mod params;
pub mod pattern;
mod render;
mod stylize;
mod tex;
pub mod ui;

pub use colour::{MAX_COLOR_SAMPLES, fill_stops, rgb01_to_hsv, selective_color_rgb};
pub use kind::{Kind, SECTIONS, default_params, field_blur_max, is_identity};
pub use params::{BlurPin, ColorSample, FillStop, Params, Stop};
pub use tex::Frame;

use colour::{hsv2rgb, rgb2hsv};
use onepaint_color::{fast_linear_to_srgb as linear_to_srgb, fast_srgb_to_linear as srgb_to_linear};
use onepaint_stroke::{CurvePoint, Interp, eval_curve};
use rayon::prelude::*;

/// Field Blur takes at most this many pins (AR `MAX_BLUR_PINS`).
pub const MAX_BLUR_PINS: usize = 8;

/// `reach` for kinds whose result depends on the whole canvas.
pub const GLOBAL_REACH: i32 = 1 << 24;

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
    ex: colour::Extra,
}

const LUMA: [f32; 3] = [0.299, 0.587, 0.114];

fn luma(c: [f32; 3]) -> f32 {
    c[0].clamp(0.0, 1.0) * LUMA[0] + c[1].clamp(0.0, 1.0) * LUMA[1] + c[2].clamp(0.0, 1.0) * LUMA[2]
}

/// Prepare a per-pixel kind for a default (1×1, 300 dpi) document; kinds that
/// need the document's size (tone, fills, generators) should use `prepare_in`.
pub fn prepare(kind: Kind, p: &Params) -> Prepared {
    prepare_in(kind, p, Frame::default())
}

/// Prepare a per-pixel kind for the document described by `frame`.
pub fn prepare_in(kind: Kind, p: &Params, frame: Frame) -> Prepared {
    let lut = match kind {
        Kind::Curves => Some(Box::new(build_curves_lut(p))),
        Kind::GradientMap => Some(Box::new(build_gradient_lut(p))),
        _ => None,
    };
    Prepared { kind, p: p.clone(), lut, ex: colour::extra_for(kind, p, frame) }
}

impl Prepared {
    /// Adjust one premultiplied linear pixel at doc (x, y). Kinds that are not
    /// pixelwise pass through.
    pub fn color_px(&self, s: [f32; 4], x: i32, y: i32) -> [f32; 4] {
        if !self.kind.is_pixelwise() {
            return s;
        }
        if let Some(o) = colour::px(self.kind, &self.p, &self.ex, s, x, y) {
            return o;
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

/// Spatial kinds on a dense premultiplied buffer (`w × h × 4`) that is the
/// whole document; colour kinds return a copy.
pub fn apply_spatial(kind: Kind, p: &Params, buf: &[f32], w: usize, h: usize) -> Vec<f32> {
    apply_spatial_in(kind, p, buf, w, h, Frame::whole(w, h))
}

/// Spatial kinds on a `w × h` buffer placed in the document by `frame`. Kinds
/// with `GLOBAL_REACH` expect the buffer to be the whole document. Generators
/// return their own layer content (line art reads the buffer; lightning does
/// not); pixelwise kinds are run through `Prepared::color_px`.
pub fn apply_spatial_in(kind: Kind, p: &Params, buf: &[f32], w: usize, h: usize, frame: Frame) -> Vec<f32> {
    if w == 0 || h == 0 {
        return buf.to_vec();
    }
    if kind.is_pixelwise() {
        let pr = prepare_in(kind, p, frame);
        return tex::render(w, h, |i, j| {
            let k = (j * w + i) * 4;
            pr.color_px([buf[k], buf[k + 1], buf[k + 2], buf[k + 3]], frame.x + i as i32, frame.y + j as i32)
        });
    }
    if kind == Kind::Lightning {
        return lightning::render_lightning(w, h, p);
    }
    if is_identity(kind, p) {
        return buf.to_vec();
    }
    let s = tex::Src { t: tex::Tex::new(buf, w, h), f: frame };
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
        Kind::RadialBlur => blur::radial(p, s),
        Kind::TiltShift | Kind::IrisBlur | Kind::FieldBlur => blur::gallery(kind, p, s),
        Kind::SpinBlur => blur::spin(p, s),
        Kind::Twirl => distort::twirl(p, s),
        Kind::Pinch => distort::pinch(p, s),
        Kind::Ripple => distort::ripple(p, s),
        Kind::PolarCoordinates => distort::polar(p, s),
        Kind::Bloom => stylize::bloom(p, s),
        Kind::ChromaticAberration => stylize::chromatic(p, s),
        Kind::Halftone => stylize::halftone(p, s),
        Kind::Glitch => stylize::glitch(p, s),
        Kind::Mosaic => stylize::mosaic(p, s),
        Kind::Crystallize => stylize::crystallize(p, s),
        Kind::Emboss => stylize::emboss(p, s),
        Kind::OilPaint => stylize::oil_paint(p, s),
        Kind::Wind => stylize::wind(p, s),
        Kind::LineArt => stylize::line_art(p, s),
        Kind::LightRays => render::light_rays(p, s),
        _ => buf.to_vec(),
    }
}

/// How far a spatial kind reads beyond a pixel (px), for tiling margins.
/// `GLOBAL_REACH` means the whole canvas; per-pixel kinds read nothing else.
pub fn reach(kind: Kind, p: &Params) -> i32 {
    let ceil = |v: f32| v.max(0.0).ceil() as i32;
    match kind {
        Kind::GaussianBlur => p.radius.unwrap_or(0.0).max(1.0).ceil() as i32 + 2,
        Kind::MotionBlur => p.distance.unwrap_or(0.0).max(1.0).ceil() as i32 + 2,
        Kind::Sharpen => 4,
        Kind::Bloom => p.radius.unwrap_or(15.0).max(1.0).ceil() as i32 + 2,
        Kind::Ripple => ceil(p.amplitude.unwrap_or(0.0) * 1.35) + 2,
        Kind::Halftone => ceil(p.cell_size.unwrap_or(6.0).max(2.0)) + 2,
        Kind::Mosaic => ceil(p.cell_size.unwrap_or(16.0).max(2.0)) + 2,
        Kind::Crystallize => ceil(p.cell_size.unwrap_or(16.0).max(2.0) * 3.0) + 2,
        Kind::Emboss => ceil(p.distance.unwrap_or(3.0).max(0.5)) + 2,
        Kind::OilPaint => {
            let r = p.radius.unwrap_or(8.0).max(1.0);
            ceil(r + (r / 6.0).max(1.0)) + 2
        }
        Kind::Wind => ceil(p.distance.unwrap_or(40.0)) + 2,
        Kind::LineArt => {
            let r = p.radius.unwrap_or(2.0).max(1.0);
            ceil(r * 1.6 * 2.0) + 4
        }
        Kind::RadialBlur
        | Kind::TiltShift
        | Kind::IrisBlur
        | Kind::FieldBlur
        | Kind::SpinBlur
        | Kind::Twirl
        | Kind::Pinch
        | Kind::PolarCoordinates
        | Kind::ChromaticAberration
        | Kind::Glitch
        | Kind::LightRays
        | Kind::Lightning => GLOBAL_REACH,
        _ => 0,
    }
}
