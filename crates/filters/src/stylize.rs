//! Stylize and Pixelate kinds: bloom, chromatic aberration, halftone, glitch,
//! mosaic, crystallize, emboss, oil paint, wind and line art (AR:997-1445,
//! AR:1710-1830). Each returns the effect for every pixel of `s`; the caller
//! mixes it in by the selection.

use crate::params::Params;
use crate::tex::{LUMA, Px, Src, Tex, dot3, fract, gauss_to, ign, len2, mix, render, repremul, smoothstep, to_work, unpremul};

/// BLOOM_BRIGHT_SRC (AR:1710): straight light map of what clears the threshold.
/// Reads the linear storage value (no `toWork`), as the shader does.
pub(crate) fn bloom_bright(t: Tex, thr: f32) -> Vec<f32> {
    render(t.w, t.h, |i, j| {
        let s = t.get(i, j);
        let rgb = unpremul(s).map(|v| v.clamp(0.0, 1.0));
        let lum = dot3(rgb, LUMA);
        let k = smoothstep(thr, (thr + 0.25).min(1.0), lum) * if s[3] < 0.0001 { 0.0 } else { 1.0 };
        [rgb[0] * k, rgb[1] * k, rgb[2] * k, 1.0]
    })
}

/// BLOOM_COMBINE_SRC (AR:1726): screen `glow` (any size spanning the same
/// extent) over the source in the work space; alpha is kept.
pub(crate) fn bloom_combine(src: Tex, glow: Tex, intensity: f32) -> Vec<f32> {
    render(src.w, src.h, |i, j| {
        let s = src.get(i, j);
        let uv = [(i as f32 + 0.5) / src.w as f32, (j as f32 + 0.5) / src.h as f32];
        let g = glow.uv(uv[0], uv[1]);
        let base = to_work(unpremul(s));
        let gl = to_work([g[0], g[1], g[2]].map(|v| v.clamp(0.0, 1.0))).map(|v| v * intensity);
        repremul([0, 1, 2].map(|k| 1.0 - (1.0 - base[k]) * (1.0 - gl[k])), s[3])
    })
}

pub(crate) fn bloom(p: &Params, s: Src) -> Vec<f32> {
    let (w, h) = (s.t.w, s.t.h);
    let bright = bloom_bright(s.t, p.threshold.unwrap_or(0.6));
    let glow = gauss_to(Tex::new(&bright, w, h), w, h, p.radius.unwrap_or(15.0), [1.0, 1.0]);
    bloom_combine(s.t, Tex::new(&glow, w, h), p.amount.unwrap_or(0.5))
}

fn straight_work(t: Px) -> [f32; 3] {
    to_work(unpremul(t))
}

/// CHROMATIC_SRC (AR:997).
pub(crate) fn chromatic(p: &Params, s: Src) -> Vec<f32> {
    let amount = p.amount.unwrap_or(0.0);
    let size = s.f.size();
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let uv = [q[0] / size[0], q[1] / size[1]];
        let sh = [(uv[0] - 0.5) * amount * 0.04, (uv[1] - 0.5) * amount * 0.04];
        let sr = s.uv([uv[0] + sh[0], uv[1] + sh[1]]);
        let sg = s.t.get(i, j);
        let sb = s.uv([uv[0] - sh[0], uv[1] - sh[1]]);
        repremul([straight_work(sr)[0], straight_work(sg)[1], straight_work(sb)[2]], sg[3])
    })
}

/// HALFTONE_SRC (AR:1016).
pub(crate) fn halftone(p: &Params, s: Src) -> Vec<f32> {
    let scale = p.cell_size.unwrap_or(6.0).max(2.0);
    let ang = p.angle.unwrap_or(15.0).to_radians();
    let keep_colour = !p.mono.unwrap_or(false);
    let (c, sn) = (ang.cos(), ang.sin());
    render(s.t.w, s.t.h, |i, j| {
        let a = s.t.get(i, j)[3];
        let px = s.centre(i, j);
        let rp = [c * px[0] - sn * px[1], sn * px[0] + c * px[1]];
        let ccr = [((rp[0] / scale).floor() + 0.5) * scale, ((rp[1] / scale).floor() + 0.5) * scale];
        let cell_px = [c * ccr[0] + sn * ccr[1], -sn * ccr[0] + c * ccr[1]];
        let cell_rgb = straight_work(s.px(cell_px)).map(|v| v.clamp(0.0, 1.0));
        let l = dot3(cell_rgb, LUMA);
        let radius = (1.0 - l) * (scale * 0.72);
        let dist = len2([rp[0] - ccr[0], rp[1] - ccr[1]]);
        let ink = 1.0 - smoothstep(radius - 1.0, radius + 1.0, dist);
        let dot = if keep_colour { cell_rgb } else { [0.0; 3] };
        repremul(dot.map(|d| mix(1.0, d, ink)), a)
    })
}

/// GLITCH_SRC (AR:1145), seed 23.
pub(crate) fn glitch(p: &Params, s: Src) -> Vec<f32> {
    let amount = p.amount.unwrap_or(0.0);
    let seed = 23.0f32;
    let rand = |x: f32| fract((x * 12.9898 + seed).sin() * 43_758.547);
    let size = s.f.size();
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let v_uv = [q[0] / size[0], q[1] / size[1]];
        let band = (v_uv[1] * 24.0).floor();
        let r = rand(band);
        let shift = if r > 0.7 { rand(band + 1.0) - 0.5 } else { 0.0 } * amount * 0.12;
        let uv = [fract(v_uv[0] + shift), v_uv[1]];
        let ca = amount * 0.012 * rand(band + 5.0);
        let sr = s.uv([uv[0] + ca, uv[1]]);
        let sg = s.uv(uv);
        let sb = s.uv([uv[0] - ca, uv[1]]);
        repremul([straight_work(sr)[0], straight_work(sg)[1], straight_work(sb)[2]], sg[3])
    })
}

/// MOSAIC_SRC (AR:1292): each tile the mean of an 8×8 tap lattice.
pub(crate) fn mosaic(p: &Params, s: Src) -> Vec<f32> {
    let cell = p.cell_size.unwrap_or(16.0).max(2.0);
    let size = s.f.size();
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let origin = [(q[0] / cell).floor() * cell, (q[1] / cell).floor() * cell];
        let mut sum = [0.0f32; 4];
        for y in 0..8 {
            for x in 0..8 {
                let t = [origin[0] + (x as f32 + 0.5) * (cell / 8.0), origin[1] + (y as f32 + 0.5) * (cell / 8.0)];
                let v = s.px([t[0].min(size[0] - 0.5), t[1].min(size[1] - 0.5)]);
                for k in 0..4 {
                    sum[k] += v[k];
                }
            }
        }
        sum.map(|v| v / 64.0)
    })
}

/// CRYSTALLIZE_SRC `hash2`.
fn hash2(p: [f32; 2], seed: f32) -> [f32; 2] {
    let p = [p[0] + seed * 17.0, p[1] + seed * 17.0];
    let mut p3 = [fract(p[0] * 0.1031), fract(p[1] * 0.1030), fract(p[0] * 0.0973)];
    let d = p3[0] * (p3[1] + 33.33) + p3[1] * (p3[2] + 33.33) + p3[2] * (p3[0] + 33.33);
    p3 = p3.map(|v| v + d);
    [fract((p3[0] + p3[1]) * p3[2]), fract((p3[0] + p3[2]) * p3[1])]
}

/// CRYSTALLIZE_SRC (AR:1311): Voronoi facets, seed 1.
pub(crate) fn crystallize(p: &Params, s: Src) -> Vec<f32> {
    let cell = p.cell_size.unwrap_or(16.0).max(2.0);
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let g = [q[0] / cell, q[1] / cell];
        let c0 = [g[0].floor(), g[1].floor()];
        let mut best = 1e9f32;
        let mut site = g;
        for dj in -1..=1 {
            for di in -1..=1 {
                let c = [c0[0] + di as f32, c0[1] + dj as f32];
                let h = hash2(c, 1.0);
                let pt = [c[0] + h[0], c[1] + h[1]];
                let d = (pt[0] - g[0]).powi(2) + (pt[1] - g[1]).powi(2);
                if d < best {
                    best = d;
                    site = pt;
                }
            }
        }
        let sp = [site[0] * cell, site[1] * cell];
        let r = cell * 0.2;
        let at = |o: [f32; 2]| s.px([sp[0] + o[0], sp[1] + o[1]]);
        let (m, a, b, c, d) = (at([0.0, 0.0]), at([r, 0.0]), at([-r, 0.0]), at([0.0, r]), at([0.0, -r]));
        [0, 1, 2, 3].map(|k| m[k] * 0.4 + (a[k] + b[k] + c[k] + d[k]) * 0.15)
    })
}

/// EMBOSS_SRC (AR:1348).
pub(crate) fn emboss(p: &Params, s: Src) -> Vec<f32> {
    let a = p.angle.unwrap_or(135.0).to_radians();
    let h = p.distance.unwrap_or(3.0).max(0.5);
    let dir = [a.cos() * h, -a.sin() * h];
    let amount = p.amount.unwrap_or(0.5).max(0.0);
    let mono = p.mono.unwrap_or(true);
    let height = |q: [f32; 2]| {
        let t = s.px(q);
        dot3(to_work([t[0] + 1.0 - t[3], t[1] + 1.0 - t[3], t[2] + 1.0 - t[3]]), LUMA)
    };
    render(s.t.w, s.t.h, |i, j| {
        let src = s.t.get(i, j);
        let q = s.centre(i, j);
        let rel = (height([q[0] - dir[0], q[1] - dir[1]]) - height([q[0] + dir[0], q[1] + dir[1]])) * amount * 3.0;
        let c = straight_work(src);
        let o = if mono { [0.5 + rel; 3] } else { c.map(|v| v + rel) };
        repremul(o.map(|v| v.clamp(0.0, 1.0)), src[3])
    })
}

/// OIL_PAINT_SRC (AR:1373): weighted Kuwahara over four quadrants.
pub(crate) fn oil_paint(p: &Params, s: Src) -> Vec<f32> {
    let radius = p.radius.unwrap_or(8.0).max(1.0);
    let sharp = 1.0 + p.amount.unwrap_or(0.5).clamp(0.0, 1.0) * 5.0;
    let st = (radius / 6.0).max(1.0);
    let n = (radius / st).ceil().min(6.0) as i32;
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let jit = if st > 1.0 { ign(q) * (st - 1.0) } else { 0.0 };
        let mut acc = [0.0f32; 4];
        let mut wsum = 0.0;
        for k in 0..4 {
            let sg = [if k == 1 || k == 3 { 1.0 } else { -1.0 }, if k >= 2 { 1.0 } else { -1.0 }];
            let (mut m, mut m2, mut cnt) = ([0.0f32; 4], [0.0f32; 4], 0.0f32);
            for jj in 0..=n {
                for ii in 0..=n {
                    let t = s.px([q[0] + sg[0] * (ii as f32 * st + jit), q[1] + sg[1] * (jj as f32 * st + jit)]);
                    for c in 0..4 {
                        m[c] += t[c];
                        m2[c] += t[c] * t[c];
                    }
                    cnt += 1.0;
                }
            }
            let m = m.map(|v| v / cnt);
            let var: f32 = (0..4).map(|c| (m2[c] / cnt - m[c] * m[c]).max(0.0)).sum();
            let w = (var + 1e-4).powf(-sharp);
            for c in 0..4 {
                acc[c] += m[c] * w;
            }
            wsum += w;
        }
        acc.map(|v| v / wsum)
    })
}

/// WIND_SRC (AR:1416).
pub(crate) fn wind(p: &Params, s: Src) -> Vec<f32> {
    let a = p.angle.unwrap_or(0.0).to_radians();
    let dir = [a.cos(), -a.sin()];
    let len = p.distance.unwrap_or(40.0).max(0.0);
    let amount = p.amount.unwrap_or(0.6).clamp(0.0, 1.0);
    let lane_hash = |x: f32| fract((x * 91.3458).sin() * 47_453.547);
    const TAPS: usize = 48;
    render(s.t.w, s.t.h, |i, j| {
        let src = s.t.get(i, j);
        let q = s.centre(i, j);
        let lane = (q[0] * -dir[1] + q[1] * dir[0]).floor();
        let l = len * (0.35 + 0.65 * lane_hash(lane));
        let mut best = src;
        let mut best_l = dot3([src[0], src[1], src[2]], LUMA);
        for k in 1..=TAPS {
            let t = k as f32 / TAPS as f32;
            let c = s.px([q[0] - dir[0] * t * l, q[1] - dir[1] * t * l]).map(|v| v * (1.0 - t));
            let lum = dot3([c[0], c[1], c[2]], LUMA);
            if lum > best_l {
                best = c;
                best_l = lum;
            }
        }
        [0, 1, 2, 3].map(|k| mix(src[k], best[k], amount))
    })
}

/// LUM_SRC / Line Art's `lum`: linear luma, transparent = white.
fn line_lum(t: Px) -> f32 {
    let l = dot3(unpremul(t).map(|v| v.clamp(0.0, 1.0)), LUMA);
    if t[3] < 0.0001 { 1.0 } else { l }
}

/// Line art: Sobel (AR:1741) or XDoG (AR:1797); lines on a transparent layer.
pub(crate) fn line_art(p: &Params, s: Src) -> Vec<f32> {
    let line = if p.mono.unwrap_or(false) { 1.0 } else { 0.0 };
    let amount = p.amount.unwrap_or(0.5);
    let (w, h) = (s.t.w, s.t.h);
    if p.line_mode.as_deref() == Some("xdog") {
        let lum = render(w, h, |i, j| {
            let l = line_lum(s.t.get(i, j));
            [l, l, l, 1.0]
        });
        let sigma = p.radius.unwrap_or(2.0).max(0.8);
        let lt = Tex::new(&lum, w, h);
        let g1 = gauss_to(lt, w, h, sigma, [1.0, 1.0]);
        let g2 = gauss_to(lt, w, h, sigma * 1.6, [1.0, 1.0]);
        let eps = (p.threshold.unwrap_or(0.5) - 0.5) * 0.2;
        let phi = 5.0 + amount * 60.0;
        return render(w, h, |i, j| {
            let k = (j * w + i) * 4;
            let d = g1[k] - 0.985 * g2[k];
            let e = if d >= eps { 1.0 } else { 1.0 + (phi * (d - eps)).tanh() };
            let ink = 1.0 - e.clamp(0.0, 1.0);
            [line * ink, line * ink, line * ink, ink]
        });
    }
    let o = p.radius.unwrap_or(1.0).max(1.0);
    let thr = p.threshold.unwrap_or(0.12);
    let strength = 1.0 + amount * 4.0;
    render(w, h, |i, j| {
        let q = s.centre(i, j);
        let l = |dx: f32, dy: f32| line_lum(s.px([q[0] + dx * o, q[1] + dy * o]));
        let (tl, t, tr) = (l(-1.0, 1.0), l(0.0, 1.0), l(1.0, 1.0));
        let (lf, rt) = (l(-1.0, 0.0), l(1.0, 0.0));
        let (bl, b, br) = (l(-1.0, -1.0), l(0.0, -1.0), l(1.0, -1.0));
        let gx = (tr + 2.0 * rt + br) - (tl + 2.0 * lf + bl);
        let gy = (tl + 2.0 * t + tr) - (bl + 2.0 * b + br);
        let mag = len2([gx, gy]) * strength;
        let a = smoothstep(thr, thr + 0.15, mag).clamp(0.0, 1.0);
        [line * a, line * a, line * a, a]
    })
}
