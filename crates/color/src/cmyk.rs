//! The built-in coated-press model (happypaint `engine/color/cmyk.ts`,
//! `press-profile.ts`): RGB → CMYK separation by inverting a print model,
//! the print simulation for the soft-proof, and their baked 33³ LUTs.

use std::sync::OnceLock;

pub type Cmyk = [f64; 4];

const GCR_START: f64 = 0.35;
const K_MAX: f64 = 0.9;
const TOTAL_INK_LIMIT: f64 = 3.0;
const DOT_GAIN: f64 = 0.13;
const INKS: [[f64; 3]; 4] = [[0.0, 0.62, 0.878], [0.878, 0.0, 0.478], [1.0, 0.91, 0.0], [0.173, 0.173, 0.18]];
const PAPER: [f64; 3] = [0.968, 0.965, 0.945];
pub const GRID: usize = 33;
pub const DELTA_E_SCALE: f64 = 8.0;

fn s2l(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn l2s(c: f64) -> f64 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

struct Lin {
    inks: [[f64; 3]; 4],
    paper: [f64; 3],
}

fn lin() -> &'static Lin {
    static L: OnceLock<Lin> = OnceLock::new();
    L.get_or_init(|| Lin { inks: INKS.map(|i| i.map(s2l)), paper: PAPER.map(s2l) })
}

fn dot_gain(a: f64) -> f64 {
    a + 4.0 * DOT_GAIN * a * (1.0 - a)
}

fn dot_gain_slope(a: f64) -> f64 {
    1.0 + 4.0 * DOT_GAIN * (1.0 - 2.0 * a)
}

fn ink_factor(ink: f64, a: f64) -> f64 {
    (1.0 + dot_gain(a) * (ink - 1.0)).max(1e-4)
}

fn print_linear(c: &Cmyk) -> [f64; 3] {
    let l = lin();
    std::array::from_fn(|ch| (0..4).fold(l.paper[ch], |v, i| v * ink_factor(l.inks[i][ch], c[i])))
}

fn black_for(r: f64, g: f64, b: f64) -> f64 {
    let grey = (1.0 - clamp01(r)).min(1.0 - clamp01(g)).min(1.0 - clamp01(b));
    if grey <= GCR_START { 0.0 } else { K_MAX * (grey - GCR_START) / (1.0 - GCR_START) }
}

/// Separate gamma sRGB (0..1) into ink coverages that print it (Newton on
/// the print model; skeleton black; 300% ink limit).
pub fn separate_rgb(r: f64, g: f64, b: f64, seed: Option<Cmyk>) -> Cmyk {
    let l = lin();
    let k = black_for(r, g, b);
    let t = [(s2l(clamp01(r)) * l.paper[0]).max(1e-4), (s2l(clamp01(g)) * l.paper[1]).max(1e-4), (s2l(clamp01(b)) * l.paper[2]).max(1e-4)];
    let inv = if 1.0 - k == 0.0 { 1.0 } else { 1.0 - k };
    let (mut c, mut m, mut y) = match seed {
        Some(s) => (s[0], s[1], s[2]),
        None => (clamp01((1.0 - clamp01(r) - k) / inv), clamp01((1.0 - clamp01(g) - k) / inv), clamp01((1.0 - clamp01(b) - k) / inv)),
    };
    for _ in 0..24 {
        let (gc, gm, gy, gk) = (dot_gain(c), dot_gain(m), dot_gain(y), dot_gain(k));
        let (sc, sm, sy) = (dot_gain_slope(c), dot_gain_slope(m), dot_gain_slope(y));
        let mut jm = [0.0; 9];
        let mut res = [0.0; 3];
        let mut worst: f64 = 0.0;
        for ch in 0..3 {
            let (ic, im, iy) = (l.inks[0][ch] - 1.0, l.inks[1][ch] - 1.0, l.inks[2][ch] - 1.0);
            let fc = (1.0 + gc * ic).max(1e-4);
            let fm = (1.0 + gm * im).max(1e-4);
            let fy = (1.0 + gy * iy).max(1e-4);
            let fk = (1.0 + gk * (l.inks[3][ch] - 1.0)).max(1e-4);
            let ratio = l.paper[ch] * fc * fm * fy * fk / t[ch];
            jm[ch * 3] = ratio * ic * sc / fc;
            jm[ch * 3 + 1] = ratio * im * sm / fm;
            jm[ch * 3 + 2] = ratio * iy * sy / fy;
            res[ch] = ratio - 1.0;
            worst = worst.max(res[ch].abs());
        }
        if worst < 1e-4 {
            break;
        }
        let Some(s) = solve3(&jm, &res) else { break };
        let (nc, nm, ny) = (clamp01(c - s[0]), clamp01(m - s[1]), clamp01(y - s[2]));
        let moved = (nc - c).abs().max((nm - m).abs()).max((ny - y).abs());
        (c, m, y) = (nc, nm, ny);
        if moved < 1e-5 {
            break;
        }
    }
    if c + m + y + k > TOTAL_INK_LIMIT {
        let scale = (TOTAL_INK_LIMIT - k) / (c + m + y);
        (c, m, y) = (c * scale, m * scale, y * scale);
    }
    [c, m, y, k]
}

fn solve3(j: &[f64; 9], r: &[f64; 3]) -> Option<[f64; 3]> {
    let [a, b, c, d, e, f, g, h, i] = *j;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 {
        return None;
    }
    let [r0, r1, r2] = *r;
    Some([
        (r0 * (e * i - f * h) - b * (r1 * i - f * r2) + c * (r1 * h - e * r2)) / det,
        (a * (r1 * i - f * r2) - r0 * (d * i - f * g) + c * (d * r2 - r1 * g)) / det,
        (a * (e * r2 - r1 * h) - b * (d * r2 - r1 * g) + r0 * (d * h - e * g)) / det,
    ])
}

/// Print a separation: absolute gamma sRGB, paper colour included.
pub fn cmyk_to_proof_rgb(c: &Cmyk) -> [f64; 3] {
    print_linear(c).map(|v| l2s(clamp01(v)))
}

pub fn srgb_to_lab(r: f64, g: f64, b: f64) -> [f64; 3] {
    const M: [f64; 9] = [0.4360747, 0.3850649, 0.1430804, 0.2225045, 0.7168786, 0.0606169, 0.0139322, 0.0971045, 0.7141733];
    const W: [f64; 3] = [0.9642, 1.0, 0.8249];
    let (lr, lg, lb) = (s2l(r), s2l(g), s2l(b));
    let f = |t: f64| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let x = f((M[0] * lr + M[1] * lg + M[2] * lb) / W[0]);
    let y = f((M[3] * lr + M[4] * lg + M[5] * lb) / W[1]);
    let z = f((M[6] * lr + M[7] * lg + M[8] * lb) / W[2]);
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}

fn normalize_proof(rgb: [f64; 3], white: [f64; 3], black: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|ch| {
        let (lo, hi) = (s2l(black[ch]), s2l(white[ch]));
        l2s(((s2l(rgb[ch]) - lo) / (hi - lo).max(1e-4)).clamp(0.0, 1.0))
    })
}

/// The baked press: separation and proof LUTs (r fastest), and the printed
/// white and black.
pub struct Press {
    pub sep_lut: Vec<u8>,
    pub proof_lut: Vec<u8>,
    pub white: [f64; 3],
    pub black: [f64; 3],
}

/// The built-in press, baked once (~36k warm-started Newton solves).
pub fn builtin_press() -> &'static Press {
    static P: OnceLock<Press> = OnceLock::new();
    P.get_or_init(|| {
        let sep = |r, g, b, s| separate_rgb(r, g, b, s);
        let white = cmyk_to_proof_rgb(&sep(1.0, 1.0, 1.0, None));
        let black = cmyk_to_proof_rgb(&sep(0.0, 0.0, 0.0, None));
        let n = GRID;
        let mut sep_lut = Vec::with_capacity(n * n * n * 4);
        let mut proof_lut = Vec::with_capacity(n * n * n * 4);
        let mut prev = None;
        for bi in 0..n {
            for gi in 0..n {
                for ri in 0..n {
                    let src = [ri as f64 / (n - 1) as f64, gi as f64 / (n - 1) as f64, bi as f64 / (n - 1) as f64];
                    let c = sep(src[0], src[1], src[2], prev);
                    prev = Some(c);
                    sep_lut.extend(c.map(|v| (v * 255.0).round() as u8));
                    let p = cmyk_to_proof_rgb(&c);
                    proof_lut.extend(p.map(|v| (v * 255.0).round() as u8));
                    let a = srgb_to_lab(src[0], src[1], src[2]);
                    let q = normalize_proof(p, white, black);
                    let bq = srgb_to_lab(q[0], q[1], q[2]);
                    let de = ((a[0] - bq[0]).powi(2) + (a[1] - bq[1]).powi(2) + (a[2] - bq[2]).powi(2)).sqrt();
                    proof_lut.push((de * DELTA_E_SCALE).round().min(255.0) as u8);
                }
            }
        }
        Press { sep_lut, proof_lut, white, black }
    })
}

fn trilinear(lut: &[u8], r: f64, g: f64, b: f64) -> [f64; 4] {
    let n = GRID;
    let g1 = (n - 1) as f64;
    let (r, gg, b) = (r.clamp(0.0, 1.0) * g1, g.clamp(0.0, 1.0) * g1, b.clamp(0.0, 1.0) * g1);
    let (r0, g0, b0) = ((r.floor() as usize).min(n - 2), (gg.floor() as usize).min(n - 2), (b.floor() as usize).min(n - 2));
    let (fr, fg, fb) = (r - r0 as f64, gg - g0 as f64, b - b0 as f64);
    let mut out = [0.0; 4];
    for c in 0..8 {
        let (dr, dg, db) = (c & 1, (c >> 1) & 1, (c >> 2) & 1);
        let w = (if dr == 1 { fr } else { 1.0 - fr }) * (if dg == 1 { fg } else { 1.0 - fg }) * (if db == 1 { fb } else { 1.0 - fb });
        if w == 0.0 {
            continue;
        }
        let idx = (((b0 + db) * n + (g0 + dg)) * n + (r0 + dr)) * 4;
        for (ch, o) in out.iter_mut().enumerate() {
            *o += w * lut[idx + ch] as f64;
        }
    }
    out
}

/// Straight RGBA8 (already on white) → CMYK bytes through the separation LUT.
pub fn separate_rgba8(rgba: &[u8]) -> Vec<u8> {
    let p = builtin_press();
    rgba.chunks_exact(4)
        .flat_map(|px| trilinear(&p.sep_lut, px[0] as f64 / 255.0, px[1] as f64 / 255.0, px[2] as f64 / 255.0).map(|v| v.round() as u8))
        .collect()
}

/// Soft-proof one gamma sRGB colour (already on white): the media-relative
/// print (or absolute with `paper_white`) and the gamut error ΔE.
pub fn proof(rgb: [f64; 3], paper_white: bool) -> ([f64; 3], f64) {
    let p = builtin_press();
    let s = trilinear(&p.proof_lut, rgb[0], rgb[1], rgb[2]);
    let printed = [s[0] / 255.0, s[1] / 255.0, s[2] / 255.0];
    let out = if paper_white { printed } else { normalize_proof(printed, p.white, p.black) };
    (out, s[3] / DELTA_E_SCALE)
}
