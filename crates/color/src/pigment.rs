//! Subtractive (Kubelka–Munk) colour mixing, ported from happypaint
//! `engine/color/pigment.ts`: 9-band overlapping Gaussian basis, two-constant
//! KM mix, pseudo-inverse back to RGB, then chroma/hue restoration.
//! Inputs and output are straight sRGB in 0..1; maths runs in f64 like the JS.

use std::sync::OnceLock;

const BINS: usize = 9;
const CENTERS: [f64; 3] = [8.2, 3.4, 2.0];
const SIGMA: [f64; 3] = [1.0, 0.9, 1.7];
const EPS: f64 = 1e-3;
const SCATTER_MIN: f64 = 1e-3;
const CHROMA_RESTORE: f64 = 0.55;
const HUE_TRUST_SAT: f64 = 0.22;
const ARC_SOFT: f64 = 0.12;

struct Basis {
    b: [[f64; 3]; BINS],
    t: [[f64; BINS]; 3],
}

fn basis() -> &'static Basis {
    static B: OnceLock<Basis> = OnceLock::new();
    B.get_or_init(|| {
        let mut b = [[0.0; 3]; BINS];
        for (i, row) in b.iter_mut().enumerate() {
            for c in 0..3 {
                let d = i as f64 - CENTERS[c];
                row[c] = (-(d * d) / (2.0 * SIGMA[c] * SIGMA[c])).exp();
            }
            let sum: f64 = row.iter().sum();
            row.iter_mut().for_each(|v| *v /= sum);
        }
        let mut btb = [[0.0; 3]; 3];
        for r in 0..3 {
            for c in 0..3 {
                btb[r][c] = (0..BINS).map(|k| b[k][r] * b[k][c]).sum();
            }
        }
        let inv = inv3(btb);
        let mut t = [[0.0; BINS]; 3];
        for r in 0..3 {
            for k in 0..BINS {
                t[r][k] = (0..3).map(|j| inv[r][j] * b[k][j]).sum();
            }
        }
        Basis { b, t }
    })
}

fn inv3(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let [a, b, c] = m[0];
    let [d, e, f] = m[1];
    let [g, h, i] = m[2];
    let ca = e * i - f * h;
    let cb = -(d * i - f * g);
    let cc = d * h - e * g;
    let id = 1.0 / (a * ca + b * cb + c * cc);
    [
        [ca * id, -(b * i - c * h) * id, (b * f - c * e) * id],
        [cb * id, (a * i - c * g) * id, -(a * f - c * d) * id],
        [cc * id, -(a * h - b * g) * id, (a * e - b * d) * id],
    ]
}

fn s2l(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn l2s(c: f64) -> f64 {
    let x = c.clamp(0.0, 1.0);
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

fn clamp_r(r: f64) -> f64 {
    r.clamp(EPS, 1.0 - EPS)
}

fn spectrum(lin: [f64; 3]) -> [f64; BINS] {
    let b = &basis().b;
    std::array::from_fn(|k| b[k][0] * lin[0] + b[k][1] * lin[1] + b[k][2] * lin[2])
}

fn scattering(s: &[f64; BINS]) -> f64 {
    let min = s.iter().fold(1.0f64, |m, &v| m.min(clamp_r(v)));
    (min * min).max(SCATTER_MIN)
}

/// Subtractive mix of two straight sRGB colours; `t` is `b`'s weight.
pub fn pigment_mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let (a, b, t) = (a.map(f64::from), b.map(f64::from), t as f64);
    let o = mix64(a, b, t);
    o.map(|v| v as f32)
}

fn mix64(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    let wa = 1.0 - t;
    let sa = spectrum(a.map(s2l));
    let sb = spectrum(b.map(s2l));
    let (sca, scb) = (scattering(&sa), scattering(&sb));
    let mixed: [f64; BINS] = std::array::from_fn(|k| {
        let (ra, rb) = (clamp_r(sa[k]), clamp_r(sb[k]));
        let ka = sca * (1.0 - ra) * (1.0 - ra) / (2.0 * ra);
        let kb = scb * (1.0 - rb) * (1.0 - rb) / (2.0 * rb);
        let v = (wa * ka + t * kb) / (wa * sca + t * scb);
        1.0 + v - (v * v + 2.0 * v).sqrt()
    });
    let tm = &basis().t;
    let out: [f64; 3] = std::array::from_fn(|c| (0..BINS).map(|k| tm[c][k] * mixed[k]).sum::<f64>());
    chroma_restore(a, b, t, out.map(l2s))
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sat3(c: [f64; 3]) -> f64 {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx > 1e-4 { (mx - mn) / mx } else { 0.0 }
}

fn hue_vec(c: [f64; 3]) -> [f64; 2] {
    let x = c[0] - 0.5 * (c[1] + c[2]);
    let y = 0.8660254 * (c[1] - c[2]);
    let m = x.hypot(y);
    if m > 1e-5 { [x / m, y / m] } else { [0.0, 0.0] }
}

fn hsv_vec_to_rgb(hx: f64, hy: f64, s: f64, v: f64) -> [f64; 3] {
    let mut h = hy.atan2(hx) * 3.0 / std::f64::consts::PI;
    if h < 0.0 {
        h += 6.0;
    }
    let f = |n: f64| {
        let k = (n + h) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

fn chroma_restore(a: [f64; 3], b: [f64; 3], t: f64, o: [f64; 3]) -> [f64; 3] {
    let target = (1.0 - t) * sat3(a) + t * sat3(b);
    let so = sat3(o);
    if target <= so {
        return o;
    }
    let (ua, ub, uo) = (hue_vec(a), hue_vec(b), hue_vec(o));
    let mx0 = (1.0 - t) * sat3(a) * ua[0] + t * sat3(b) * ub[0];
    let my0 = (1.0 - t) * sat3(a) * ua[1] + t * sat3(b) * ub[1];
    let mm = mx0.hypot(my0);
    let cab = ua[0] * ub[1] - ua[1] * ub[0];
    let side_a = (ua[0] * uo[1] - ua[1] * uo[0]) * sign(cab);
    let side_b = (uo[0] * ub[1] - uo[1] * ub[0]) * sign(cab);
    let arc_side = side_a.min(side_b);
    let sat_new = (so + (target - so) * CHROMA_RESTORE).min(1.0);
    let vo = o[0].max(o[1].max(o[2]));
    if mm < 1e-4 {
        return o;
    }
    let chroma_trust = smoothstep(0.0, HUE_TRUST_SAT, so);
    let arc_trust = if cab.abs() < 1e-4 { 1.0 } else { smoothstep(-ARC_SOFT, 0.0, arc_side) };
    let trust = chroma_trust * arc_trust;
    let (mux, muy) = (mx0 / mm, my0 / mm);
    let mut hx = uo[0] * trust + mux * (1.0 - trust);
    let mut hy = uo[1] * trust + muy * (1.0 - trust);
    let hm = hx.hypot(hy);
    if hm < 1e-5 {
        hx = mux;
        hy = muy;
    } else {
        hx /= hm;
        hy /= hm;
    }
    let f = if so > 1e-6 { sat_new / so } else { 1.0 };
    let scaled = o.map(|c| (vo - (vo - c) * f).clamp(0.0, 1.0));
    if trust >= 1.0 {
        return scaled;
    }
    let rebuilt = hsv_vec_to_rgb(hx, hy, sat_new, vo);
    std::array::from_fn(|i| scaled[i] * trust + rebuilt[i] * (1.0 - trust))
}
