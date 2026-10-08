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
    /// Shown in the View menu ("Generic coated (built-in)" or the profile's).
    pub name: String,
    /// The loaded profile's bytes (a CMYK TIFF embeds them); none built in.
    pub icc: Option<Vec<u8>>,
}

/// Bake both LUTs from a separation (gamma sRGB → CMYK 0..1, ink amounts)
/// and a proof (CMYK → gamma sRGB) over the whole grid.
fn bake(
    sep_grid: impl Fn(&[[f64; 3]]) -> Vec<[f64; 4]>,
    proof_all: impl Fn(&[[f64; 4]]) -> Vec<[f64; 3]>,
    name: String,
    icc: Option<Vec<u8>>,
) -> Press {
    let n = GRID;
    let mut grid = Vec::with_capacity(n * n * n);
    for bi in 0..n {
        for gi in 0..n {
            for ri in 0..n {
                grid.push([ri as f64 / (n - 1) as f64, gi as f64 / (n - 1) as f64, bi as f64 / (n - 1) as f64]);
            }
        }
    }
    let seps = sep_grid(&grid);
    let ends = sep_grid(&[[1.0; 3], [0.0; 3]]);
    let ends = proof_all(&ends);
    let (white, black) = (ends[0], ends[1]);
    let proofs = proof_all(&seps);
    let mut sep_lut = Vec::with_capacity(n * n * n * 4);
    let mut proof_lut = Vec::with_capacity(n * n * n * 4);
    for ((src, c), p) in grid.iter().zip(&seps).zip(&proofs) {
        sep_lut.extend(c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        proof_lut.extend(p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        let a = srgb_to_lab(src[0], src[1], src[2]);
        let q = normalize_proof(*p, white, black);
        let bq = srgb_to_lab(q[0], q[1], q[2]);
        let de = ((a[0] - bq[0]).powi(2) + (a[1] - bq[1]).powi(2) + (a[2] - bq[2]).powi(2)).sqrt();
        proof_lut.push((de * DELTA_E_SCALE).round().min(255.0) as u8);
    }
    Press { sep_lut, proof_lut, white, black, name, icc }
}

/// The built-in press, baked once (~36k warm-started Newton solves).
pub fn builtin_press() -> &'static Press {
    static P: OnceLock<Press> = OnceLock::new();
    P.get_or_init(|| {
        let sep = |grid: &[[f64; 3]]| {
            let mut prev = None;
            grid.iter()
                .map(|c| {
                    let s = separate_rgb(c[0], c[1], c[2], prev);
                    prev = Some(s);
                    s
                })
                .collect()
        };
        let proof = |cs: &[[f64; 4]]| cs.iter().map(cmyk_to_proof_rgb).collect();
        bake(sep, proof, "Generic coated (built-in)".into(), None)
    })
}

/// Why a press profile can't be used.
#[derive(Debug, Clone, PartialEq)]
pub struct PressError(pub String);

impl std::fmt::Display for PressError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A press from a CMYK output profile (`.icc` / `.icm`): separation and proof
/// through it at relative colorimetric, as happypaint does, with
/// `moxcms` as the colour engine (happypaint parses the LUTs itself).
pub fn press_from_icc(bytes: &[u8]) -> Result<Press, PressError> {
    use moxcms::{ColorProfile, DataColorSpace, Layout, ProfileText, RenderingIntent, TransformOptions};
    let err = |m: &str| PressError(m.into());
    let prof = ColorProfile::new_from_slice(bytes).map_err(|e| PressError(format!("Not a readable ICC profile ({e:?})")))?;
    if prof.color_space != DataColorSpace::Cmyk {
        return Err(err("Not a CMYK press profile"));
    }
    let name = match &prof.description {
        Some(ProfileText::PlainString(s)) => s.clone(),
        Some(ProfileText::Localizable(l)) => l.first().map(|v| v.value.clone()).unwrap_or_default(),
        Some(ProfileText::Description(d)) => d.ascii_string.clone(),
        None => String::new(),
    };
    let name = if name.trim().is_empty() { "Custom press".to_string() } else { name.trim().trim_end_matches('\0').to_string() };
    let srgb = ColorProfile::new_srgb();
    let opts = TransformOptions { rendering_intent: RenderingIntent::RelativeColorimetric, ..Default::default() };
    let to_cmyk = srgb
        .create_transform_f32(Layout::Rgb, &prof, Layout::Rgba, opts)
        .map_err(|e| PressError(format!("Can't separate through it ({e:?})")))?;
    let to_rgb = prof
        .create_transform_f32(Layout::Rgba, &srgb, Layout::Rgb, opts)
        .map_err(|e| PressError(format!("Can't proof through it ({e:?})")))?;
    let sep = |grid: &[[f64; 3]]| {
        let src: Vec<f32> = grid.iter().flat_map(|c| c.map(|v| v as f32)).collect();
        let mut dst = vec![0.0f32; grid.len() * 4];
        if to_cmyk.transform(&src, &mut dst).is_err() {
            return vec![[0.0; 4]; grid.len()];
        }
        dst.chunks_exact(4).map(|c| [c[0] as f64, c[1] as f64, c[2] as f64, c[3] as f64]).collect()
    };
    let proof = |cs: &[[f64; 4]]| {
        let src: Vec<f32> = cs.iter().flat_map(|c| c.map(|v| v as f32)).collect();
        let mut dst = vec![0.0f32; cs.len() * 3];
        if to_rgb.transform(&src, &mut dst).is_err() {
            return vec![[0.0; 3]; cs.len()];
        }
        dst.chunks_exact(3).map(|c| [c[0] as f64, c[1] as f64, c[2] as f64]).collect()
    };
    Ok(bake(sep, proof, name, Some(bytes.to_vec())))
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

/// Straight RGBA8 (already on white) → CMYK bytes through the built-in press.
pub fn separate_rgba8(rgba: &[u8]) -> Vec<u8> {
    separate_rgba8_with(builtin_press(), rgba)
}

/// Straight RGBA8 (already on white) → CMYK bytes through `p`'s separation LUT.
pub fn separate_rgba8_with(p: &Press, rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|px| trilinear(&p.sep_lut, px[0] as f64 / 255.0, px[1] as f64 / 255.0, px[2] as f64 / 255.0).map(|v| v.round() as u8))
        .collect()
}

/// Soft-proof one gamma sRGB colour on the built-in press.
pub fn proof(rgb: [f64; 3], paper_white: bool, black_ink: bool) -> ([f64; 3], f64) {
    proof_with(builtin_press(), rgb, paper_white, black_ink)
}

/// Soft-proof one gamma sRGB colour (already on white): the print, made
/// media-relative unless paper white / black ink are simulated, and the
/// gamut error ΔE.
pub fn proof_with(p: &Press, rgb: [f64; 3], paper_white: bool, black_ink: bool) -> ([f64; 3], f64) {
    let s = trilinear(&p.proof_lut, rgb[0], rgb[1], rgb[2]);
    let lo = if black_ink { [0.0; 3] } else { p.black.map(s2l) };
    let hi = if paper_white { [1.0; 3] } else { p.white.map(s2l) };
    let out = std::array::from_fn(|ch| l2s(((s2l(s[ch] / 255.0) - lo[ch]) / (hi[ch] - lo[ch]).max(1e-4)).clamp(0.0, 1.0)));
    (out, s[3] / DELTA_E_SCALE)
}

/// Present-only view modes (happypaint's View menu): value check, print
/// preview and the non-printable-colour warning.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewModes {
    pub grayscale: bool,
    pub proof: bool,
    pub paper_white: bool,
    pub black_ink: bool,
    /// Flag colours the press moved by more than this ΔE; 0 = off.
    pub gamut_warn: f32,
}

impl ViewModes {
    pub fn any(&self) -> bool {
        self.grayscale || self.proof || self.gamut_warn > 0.0
    }
}

const WARN: [f32; 3] = [0.78, 0.06, 0.10];

/// Apply the view modes to premultiplied-linear RGBA in place (the compositor
/// present shader: grayscale on the encoded colour, then the proof lookup).
pub fn apply_view(px: &mut [f32], v: &ViewModes) {
    apply_view_with(builtin_press(), px, v);
}

/// [`apply_view`] proofing on `press`.
pub fn apply_view_with(press: &Press, px: &mut [f32], v: &ViewModes) {
    if !v.any() {
        return;
    }
    let warn_lut = v.gamut_warn as f64 * DELTA_E_SCALE;
    for p in px.chunks_exact_mut(4) {
        let a = p[3];
        let enc = |c: f32| if a > 0.0 { crate::linear_to_srgb(c / a) * a } else { 0.0 };
        let mut c = [enc(p[0]), enc(p[1]), enc(p[2])];
        if v.grayscale {
            let lum = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            c = [lum; 3];
        }
        let mut out_a = a;
        if v.proof || v.gamut_warn > 0.0 {
            let on_white = c.map(|x| (x + 1.0 - a).clamp(0.0, 1.0) as f64);
            let s = trilinear(&press.proof_lut, on_white[0], on_white[1], on_white[2]);
            if v.proof {
                c = proof_with(press, on_white, v.paper_white, v.black_ink).0.map(|x| x as f32);
                out_a = 1.0;
            }
            if v.gamut_warn > 0.0 && s[3] >= warn_lut {
                c = WARN;
                out_a = 1.0;
            }
        }
        for ch in 0..3 {
            let straight = if out_a > 0.0 { c[ch] / out_a } else { 0.0 };
            p[ch] = crate::srgb_to_linear(straight) * out_a;
        }
        p[3] = out_a;
    }
}
