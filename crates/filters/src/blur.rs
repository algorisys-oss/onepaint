//! Blur kinds with a centre or a geometry: radial blur (AR:1175), the Blur
//! Gallery (tilt-shift / iris / field, AR:1448 and AR:2411; Light Bokeh
//! AR:1503) and spin blur (AR:1535).

use crate::kind::field_blur_max;
use crate::params::Params;
use crate::tex::{LUMA, Src, Tex, blur_to, dot2, dot3, gauss_to, ign, mix4, render, smoothstep, unpremul};
use crate::{Kind, MAX_BLUR_PINS};

/// RADIAL_BLUR_SRC: spin around the centre or zoom toward it, 96 jittered taps.
pub(crate) fn radial(p: &Params, s: Src) -> Vec<f32> {
    let size = s.f.size();
    let c = [p.center_x.unwrap_or(0.5) * size[0], p.center_y.unwrap_or(0.5) * size[1]];
    let amount = p.amount.unwrap_or(0.0).clamp(0.0, 1.0);
    let zoom = p.radial_mode.as_deref() != Some("spin");
    const TAPS: usize = 96;
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let d = [q[0] - c[0], q[1] - c[1]];
        let jit = ign(q);
        let mut sum = [0.0f32; 4];
        for k in 0..TAPS {
            let t = (k as f32 + jit) / TAPS as f32;
            let pt = if zoom {
                let f = 1.0 - t * amount * 0.5;
                [c[0] + d[0] * f, c[1] + d[1] * f]
            } else {
                let a = (t - 0.5) * amount * std::f32::consts::FRAC_PI_3;
                let (co, sn) = (a.cos(), a.sin());
                [c[0] + co * d[0] - sn * d[1], c[1] + sn * d[0] + co * d[1]]
            };
            let v = s.px(pt);
            for ch in 0..4 {
                sum[ch] += v[ch];
            }
        }
        sum.map(|v| v / TAPS as f32)
    })
}

/// BOKEH_SRC: a 64-tap golden-angle disc weighted toward highlights, into `ow × oh`.
fn bokeh_disc(src: Tex, ow: usize, oh: usize, size: [f32; 2], radius: f32, bokeh: f32, range: f32) -> Vec<f32> {
    const TAPS: usize = 64;
    let radius = radius.max(1.0);
    render(ow, oh, |i, j| {
        let uv = [(i as f32 + 0.5) / ow as f32, (j as f32 + 0.5) / oh as f32];
        let px = [uv[0] * size[0], uv[1] * size[1]];
        let rot = ign([i as f32 + 0.5, j as f32 + 0.5]) * std::f32::consts::TAU;
        let (mut sum, mut wsum) = ([0.0f32; 4], 0.0);
        for k in 0..TAPS {
            let r = ((k as f32 + 0.5) / TAPS as f32).sqrt() * radius;
            let a = k as f32 * 2.399_963_2 + rot;
            let q = [px[0] + a.cos() * r, px[1] + a.sin() * r];
            let c = src.uv(q[0] / size[0], q[1] / size[1]);
            let lum = dot3(unpremul(c), LUMA);
            let w = 1.0 + bokeh * 12.0 * smoothstep(range, 1.0, lum);
            for ch in 0..4 {
                sum[ch] += c[ch] * w;
            }
            wsum += w;
        }
        sum.map(|v| v / wsum)
    })
}

/// Tilt-Shift / Iris / Field blur: three half-resolution blur levels blended by
/// a per-pixel amount. `s` must be the whole document.
pub(crate) fn gallery(kind: Kind, p: &Params, s: Src) -> Vec<f32> {
    let (w, h) = (s.t.w, s.t.h);
    let size = s.f.size();
    let r_max = if kind == Kind::FieldBlur { field_blur_max(p) } else { p.radius.unwrap_or(0.0).max(0.0) };
    let (hw, hh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
    // Source texels per doc px of the buffer extent (1 for a whole-document buffer).
    let per = [1.0, 1.0];
    let half_per = [hw as f32 / w as f32, hh as f32 / h as f32];
    let bokeh = p.bokeh.unwrap_or(0.0).clamp(0.0, 1.0);
    let range = p.bokeh_range.unwrap_or(0.75).clamp(0.0, 0.99);
    let ext = [w as f32, h as f32];
    let levels: Vec<Vec<f32>> = [r_max / 4.0, r_max / 2.0, r_max]
        .iter()
        .map(|&r| {
            if bokeh > 0.0 {
                let a = blur_to(s.t, hw, hh, [1.0, 0.0], r / 8.0, per);
                let b = blur_to(Tex::new(&a, hw, hh), hw, hh, [0.0, 1.0], r / 8.0, half_per);
                bokeh_disc(Tex::new(&b, hw, hh), hw, hh, ext, r, bokeh, range)
            } else {
                gauss_to(s.t, hw, hh, r, per)
            }
        })
        .collect();
    let a = p.angle.unwrap_or(0.0).to_radians();
    let min_side = size[0].min(size[1]);
    let c = [p.center_x.unwrap_or(0.5) * size[0], p.center_y.unwrap_or(0.5) * size[1]];
    let u = [a.cos(), -a.sin()];
    let n = [a.sin(), a.cos()];
    let focus = p.focus_size.unwrap_or(0.1).max(0.0) * min_side;
    let falloff = p.falloff_size.unwrap_or(0.2).max(0.0) * min_side;
    let rx = (p.extent.unwrap_or(0.6) * min_side * 0.5).max(1.0);
    let ry = (rx * p.aspect.unwrap_or(0.7)).max(1.0);
    let inner = p.focus.unwrap_or(0.5).clamp(0.0, 0.95);
    let pins: Vec<[f32; 3]> =
        p.pins.as_deref().unwrap_or(&[]).iter().take(MAX_BLUR_PINS).map(|q| [q.x * size[0], q.y * size[1], q.blur.max(0.0)]).collect();
    let amount_at = |px: [f32; 2]| -> f32 {
        let d = [px[0] - c[0], px[1] - c[1]];
        match kind {
            Kind::TiltShift => smoothstep(focus, focus + falloff.max(0.5), dot2(d, n).abs()),
            Kind::IrisBlur => {
                let e = (dot2(d, u) / rx).hypot(dot2(d, n) / ry);
                smoothstep(inner, 1.0, e)
            }
            _ => {
                let (mut num, mut den) = (0.0, 0.0);
                for q in &pins {
                    let dq = [q[0] - px[0], q[1] - px[1]];
                    let wt = 1.0 / (dot2(dq, dq) + 1.0);
                    num += wt * q[2];
                    den += wt;
                }
                if den > 0.0 { num / den / r_max.max(1e-4) } else { 0.0 }
            }
        }
    };
    let (l1, l2, l3) = (Tex::new(&levels[0], hw, hh), Tex::new(&levels[1], hw, hh), Tex::new(&levels[2], hw, hh));
    render(w, h, |i, j| {
        let src = s.t.get(i, j);
        let q = s.centre(i, j);
        let r = amount_at(q).clamp(0.0, 1.0);
        let uv = [(i as f32 + 0.5) / w as f32, (j as f32 + 0.5) / h as f32];
        let (a1, a2, a3) = (l1.uv(uv[0], uv[1]), l2.uv(uv[0], uv[1]), l3.uv(uv[0], uv[1]));
        if r < 0.25 {
            mix4(src, a1, r * 4.0)
        } else if r < 0.5 {
            mix4(a1, a2, (r - 0.25) * 4.0)
        } else {
            mix4(a2, a3, (r - 0.5) * 2.0)
        }
    })
}

/// SPIN_SRC: rotate inside the ellipse's own normalised space.
pub(crate) fn spin(p: &Params, s: Src) -> Vec<f32> {
    let size = s.f.size();
    let a = p.angle.unwrap_or(0.0).to_radians();
    let rx = (p.extent.unwrap_or(0.4) * size[0].min(size[1]) * 0.5).max(1.0);
    let ry = (rx * p.aspect.unwrap_or(1.0)).max(1.0);
    let c = [p.center_x.unwrap_or(0.5) * size[0], p.center_y.unwrap_or(0.5) * size[1]];
    let u = [a.cos(), -a.sin()];
    let n = [a.sin(), a.cos()];
    let inner = p.focus.unwrap_or(0.8).clamp(0.0, 0.95);
    let spin = p.spin.unwrap_or(0.0).to_radians();
    const TAPS: usize = 64;
    render(s.t.w, s.t.h, |i, j| {
        let src = s.t.get(i, j);
        let pq = s.centre(i, j);
        let d = [pq[0] - c[0], pq[1] - c[1]];
        let q = [dot2(d, u) / rx, dot2(d, n) / ry];
        let w = 1.0 - smoothstep(inner, 1.0, q[0].hypot(q[1]));
        if w <= 0.0 {
            return src;
        }
        let arc = spin * w;
        let jit = ign(pq);
        let mut sum = [0.0f32; 4];
        for k in 0..TAPS {
            let ang = ((k as f32 + jit) / TAPS as f32 - 0.5) * arc;
            let (co, sn) = (ang.cos(), ang.sin());
            let r = [co * q[0] - sn * q[1], sn * q[0] + co * q[1]];
            let pt = [c[0] + u[0] * r[0] * rx + n[0] * r[1] * ry, c[1] + u[1] * r[0] * rx + n[1] * r[1] * ry];
            let v = s.px(pt);
            for ch in 0..4 {
                sum[ch] += v[ch];
            }
        }
        sum.map(|v| v / TAPS as f32)
    })
}
