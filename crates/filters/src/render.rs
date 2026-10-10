//! Render kinds: procedural clouds family and starfield (AR:623), lens flare
//! (AR:1604) and light rays (AR:1572, AR:2523).

use crate::params::Params;
use crate::stylize::{bloom_bright, bloom_combine};
use crate::tex::{Frame, Px, Src, Tex, fract, ign, len2, mix, render, smoothstep};

struct Gen {
    seed: f32,
}

impl Gen {
    /// CLOUDS_SRC `hash` (fract-based, no sine).
    fn hash(&self, p: [f32; 2]) -> f32 {
        let p = [p[0] + self.seed * 37.0, p[1] + self.seed * 37.0];
        let mut p3 = [fract(p[0] * 0.1031), fract(p[1] * 0.1031), fract(p[0] * 0.1031)];
        let d = p3[0] * (p3[1] + 33.33) + p3[1] * (p3[2] + 33.33) + p3[2] * (p3[0] + 33.33);
        p3 = p3.map(|v| v + d);
        fract((p3[0] + p3[1]) * p3[2])
    }

    fn vnoise(&self, p: [f32; 2]) -> f32 {
        let i = [p[0].floor(), p[1].floor()];
        let f = [fract(p[0]), fract(p[1])];
        let u = f.map(|v| v * v * (3.0 - 2.0 * v));
        let a = self.hash(i);
        let b = self.hash([i[0] + 1.0, i[1]]);
        let c = self.hash([i[0], i[1] + 1.0]);
        let d = self.hash([i[0] + 1.0, i[1] + 1.0]);
        mix(mix(a, b, u[0]), mix(c, d, u[0]), u[1])
    }

    fn fbm(&self, mut p: [f32; 2], octaves: f32) -> f32 {
        let (mut sum, mut amp, mut tot) = (0.0, 0.5, 0.0);
        let oct = octaves.clamp(1.0, 8.0) as i32;
        for _ in 0..oct {
            sum += amp * self.vnoise(p);
            tot += amp;
            p = [p[0] * 2.0, p[1] * 2.0];
            amp *= 0.5;
        }
        sum / f32::max(tot, 1e-4)
    }
}

/// CLOUDS_SRC at doc pixel (x, y): cloud field, or the starfield.
pub(crate) fn clouds_px(p: &Params, starfield: bool, _f: Frame, x: i32, y: i32) -> Px {
    let g = Gen { seed: p.gen_seed.unwrap_or(1.0) };
    let scale = p.gen_scale.unwrap_or(220.0).max(1.0);
    let density = p.gen_density.unwrap_or(0.5);
    let soft = p.gen_softness.unwrap_or(0.5);
    let ca = p.gen_color_a.unwrap_or([1.0; 3]);
    let cb = p.gen_color_b.unwrap_or([1.0; 3]);
    let fc = [x as f32 + 0.5, y as f32 + 0.5];
    if starfield {
        let q = [fc[0] / scale, fc[1] / scale];
        let cell = [q[0].floor(), q[1].floor()];
        let f = [fract(q[0]), fract(q[1])];
        let present = g.hash(cell);
        let mut cov = 0.0;
        if present > 1.0 - density * 0.6 {
            let jit = [g.hash([cell[0] + 3.1, cell[1] + 3.1]), g.hash([cell[0] + 7.7, cell[1] + 7.7])];
            let bright = 0.35 + 0.65 * g.hash([cell[0] + 5.5, cell[1] + 5.5]);
            let d = len2([f[0] - jit[0], f[1] - jit[1]]);
            let rad = mix(0.06, 0.42, soft);
            cov = smoothstep(rad, 0.0, d) * bright;
        }
        return [cb[0] * cov, cb[1] * cov, cb[2] * cov, cov];
    }
    let oct = p.gen_detail.unwrap_or(5.0);
    let warp = p.gen_warp.unwrap_or(0.25);
    let mut q = [fc[0] / scale, fc[1] / scale];
    if warp > 0.001 {
        let w = [g.fbm([q[0] + 11.3, q[1] + 11.3], oct) - 0.5, g.fbm([q[0] + 47.7, q[1] + 47.7], oct) - 0.5];
        q = [q[0] + w[0] * warp * 4.0, q[1] + w[1] * warp * 4.0];
    }
    let n = g.fbm(q, oct);
    let edge = mix(0.02, 0.5, soft);
    let cov = smoothstep(1.0 - density - edge, 1.0 - density + edge, n);
    let col = [0, 1, 2].map(|k| mix(ca[k], cb[k], n));
    [col[0] * cov, col[1] * cov, col[2] * cov, cov]
}

const POS: [f32; 6] = [0.35, 0.7, 1.1, 1.35, 1.7, 2.1];
const RAD: [f32; 6] = [0.05, 0.11, 0.03, 0.17, 0.07, 0.13];
const GHOST: [[f32; 3]; 6] = [[1.0, 0.6, 0.3], [0.4, 0.8, 1.0], [1.0, 1.0, 0.8], [0.5, 1.0, 0.6], [1.0, 0.5, 0.8], [0.6, 0.7, 1.0]];

fn ghost(p: [f32; 2], c: [f32; 2], r: f32) -> f32 {
    let d = len2([p[0] - c[0], p[1] - c[1]]) / r;
    smoothstep(1.0, 0.8, d) * (0.55 + 0.45 * smoothstep(0.3, 1.0, d))
}

/// FLARE_SRC at doc pixel (x, y); premultiplied, alpha = brightest channel.
pub(crate) fn flare_px(p: &Params, f: Frame, x: i32, y: i32) -> Px {
    let size = f.size();
    let c = [p.center_x.unwrap_or(0.3) * size[0], p.center_y.unwrap_or(0.3) * size[1]];
    let scale = (p.extent.unwrap_or(0.5) * size[0].min(size[1])).max(8.0);
    let amount = p.amount.unwrap_or(0.8).max(0.0);
    let tint = p.tint.unwrap_or([1.0, 0.9, 0.75]);
    let streak = if p.flare_streak.unwrap_or(false) { 1.0 } else { 0.0 };
    let px = [x as f32 + 0.5, y as f32 + 0.5];
    let d = [px[0] - c[0], px[1] - c[1]];
    let r = len2(d) / scale;
    let a = d[1].atan2(d[0]);
    let core = (-r * r * 60.0).exp() + 0.35 * (-r * 6.0).exp();
    // 0.5236 is the shader's literal, not exactly π/6.
    #[allow(clippy::approx_constant)]
    let star = ((a * 3.0).cos().abs().powf(60.0) + 0.6 * (a * 3.0 + 0.5236).cos().abs().powf(90.0)) * (-r * 4.0).exp() * 0.5;
    let ring_c = [0.62, 0.6, 0.58];
    let sk = [0.7, 0.85, 1.0];
    let streak_v = streak * (-d[1].abs() / (scale * 0.006)).exp() * (-d[0].abs() / (scale * 1.2)).exp() * 0.8;
    let axis = [size[0] * 0.5 - c[0], size[1] * 0.5 - c[1]];
    let mut col = [0.0f32; 3];
    for k in 0..3 {
        let ring = (r - ring_c[k]) / 0.018;
        col[k] = tint[k] * (core + star) + (-ring * ring).exp() * 0.18 + tint[k] * sk[k] * streak_v;
    }
    for i in 0..6 {
        let g = ghost(px, [c[0] + axis[0] * POS[i], c[1] + axis[1] * POS[i]], scale * RAD[i]) * 0.12;
        for k in 0..3 {
            col[k] += GHOST[i][k] * g;
        }
    }
    let col = col.map(|v| (v * amount).clamp(0.0, 1.0));
    let cov = col[0].max(col[1]).max(col[2]);
    [col[0], col[1], col[2], cov]
}

/// Light rays: bright-pass → half-resolution rays toward the source → Bloom's
/// screen combine (AR:2523). `src` must be the whole document.
pub(crate) fn light_rays(p: &Params, s: Src) -> Vec<f32> {
    let (w, h) = (s.t.w, s.t.h);
    let size = s.f.size();
    let bright = bloom_bright(s.t, p.threshold.unwrap_or(0.6));
    let (hw, hh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
    let c = [p.center_x.unwrap_or(0.5) * size[0], p.center_y.unwrap_or(0.3) * size[1]];
    let len = p.extent.unwrap_or(0.5).clamp(0.0, 1.0);
    let tint = p.tint.unwrap_or([1.0; 3]);
    let bt = Src { t: Tex::new(&bright, w, h), f: s.f };
    const TAPS: usize = 96;
    let rays = render(hw, hh, |i, j| {
        let uv = [(i as f32 + 0.5) / hw as f32, (j as f32 + 0.5) / hh as f32];
        let px = [s.f.x as f32 + uv[0] * w as f32, s.f.y as f32 + uv[1] * h as f32];
        let to_c = [c[0] - px[0], c[1] - px[1]];
        let jit = ign([i as f32 + 0.5, j as f32 + 0.5]);
        let (mut sum, mut wsum) = ([0.0f32; 3], 0.0);
        for k in 0..TAPS {
            let t = (k as f32 + jit) / TAPS as f32;
            let wt = 1.0 - t;
            let q = bt.px([px[0] + to_c[0] * t * len, px[1] + to_c[1] * t * len]);
            for ch in 0..3 {
                sum[ch] += q[ch] * wt;
            }
            wsum += wt;
        }
        let o = [0, 1, 2].map(|ch| sum[ch] / wsum * tint[ch] * 3.0);
        [o[0], o[1], o[2], 1.0]
    });
    bloom_combine(s.t, Tex::new(&rays, hw, hh), p.amount.unwrap_or(0.6).max(0.0), s.f.space)
}
