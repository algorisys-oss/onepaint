//! Lightning (happypaint `engine/adjustments/lightning.ts`): a seeded
//! midpoint-displacement bolt with branches. The path is ported exactly; the
//! Canvas2D stroke-plus-`shadowBlur` raster is replaced by round-capped
//! antialiased polylines and a Gaussian shadow (spec 18).

use crate::params::Params;
use crate::tex::widen;
use rayon::prelude::*;

/// mulberry32, as in the brush engine.
pub struct Mulberry32(u32);

impl Mulberry32 {
    pub fn new(seed: u32) -> Self {
        Mulberry32(seed)
    }

    pub fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }
}

pub type Pt = [f64; 2];

fn bolt_path(a: Pt, b: Pt, jag: f64, iters: usize, rnd: &mut Mulberry32) -> Vec<Pt> {
    let mut pts = vec![a, b];
    for _ in 0..iters {
        let mut next = vec![pts[0]];
        for w in pts.windows(2) {
            let (p, q) = (w[0], w[1]);
            let (mx, my) = ((p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0);
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let len = dx.hypot(dy);
            let off = (rnd.next_f64() - 0.5) * len * jag * 0.5;
            let l = if len == 0.0 { 1.0 } else { len };
            let (nx, ny) = (-dy / l, dx / l);
            next.push([mx + nx * off, my + ny * off]);
            next.push(q);
        }
        pts = next;
    }
    pts
}

/// One polyline the bolt strokes, with its Canvas2D state.
#[derive(Clone, Debug, PartialEq)]
pub struct BoltStroke {
    pub pts: Vec<Pt>,
    pub width: f64,
    pub alpha: f64,
    /// `shadowBlur` in px.
    pub blur: f64,
    /// Straight sRGB stroke colour; the shadow is always the bolt colour.
    pub color: [f64; 3],
}

/// The strokes `renderLightning` issues, in order (glow pass, then core pass).
pub fn lightning_strokes(width: u32, height: u32, p: &Params) -> Vec<BoltStroke> {
    let (w, h) = (width as f64, height as f64);
    let seed = (widen(p.gen_seed.unwrap_or(1.0)) * 2_654_435_761.0).round().rem_euclid(4_294_967_296.0) as u32;
    let mut rnd = Mulberry32::new(seed);
    let jag = widen(p.lgt_jag.unwrap_or(0.5));
    let branchiness = widen(p.lgt_branch.unwrap_or(0.5));
    let glow = widen(p.lgt_glow.unwrap_or(0.5));
    let core_w = widen(p.lgt_width.unwrap_or(3.0)).max(1.0);
    let color = p.gen_color_b.unwrap_or([0.7, 0.85, 1.0]).map(widen);
    let start = [widen(p.lgt_start_x.unwrap_or(0.5)) * w, 0.0];
    let end = [widen(p.lgt_end_x.unwrap_or(0.5)) * w, h];
    let main = bolt_path(start, end, jag, 7, &mut rnd);
    let n_branch = (branchiness * 8.0).round() as usize;
    let mut branches = Vec::with_capacity(n_branch);
    for _ in 0..n_branch {
        let at = main[1 + (rnd.next_f64() * (main.len() - 2) as f64).floor() as usize];
        let dir = rnd.next_f64() * std::f64::consts::PI * 2.0;
        let len = h * (0.12 + rnd.next_f64() * 0.22);
        let tip = [at[0] + dir.cos() * len, at[1] + dir.sin().abs() * len];
        branches.push(bolt_path(at, tip, jag * 1.2, 5, &mut rnd));
    }
    let mut out = Vec::new();
    let glow_blur = core_w * (4.0 + glow * 20.0);
    out.push(BoltStroke { pts: main.clone(), width: core_w * (1.5 + glow * 2.0), alpha: 0.5, blur: glow_blur, color });
    for b in &branches {
        out.push(BoltStroke { pts: b.clone(), width: core_w * (1.0 + glow), alpha: 0.5, blur: glow_blur, color });
    }
    let core_blur = core_w * (1.0 + glow * 3.0);
    out.push(BoltStroke { pts: main, width: core_w, alpha: 1.0, blur: core_blur, color: [1.0; 3] });
    for b in branches {
        out.push(BoltStroke { pts: b, width: (core_w * 0.6).max(1.0), alpha: 0.85, blur: core_blur, color: [1.0; 3] });
    }
    out
}

/// Antialiased round-capped coverage of a polyline (max over its segments).
fn stroke_coverage(s: &BoltStroke, w: usize, h: usize) -> Vec<f32> {
    let mut cov = vec![0.0f32; w * h];
    let r = s.width / 2.0;
    for seg in s.pts.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let x0 = ((a[0].min(b[0]) - r - 1.0).floor().max(0.0)) as usize;
        let x1 = ((a[0].max(b[0]) + r + 1.0).ceil().min(w as f64)).max(0.0) as usize;
        let y0 = ((a[1].min(b[1]) - r - 1.0).floor().max(0.0)) as usize;
        let y1 = ((a[1].max(b[1]) + r + 1.0).ceil().min(h as f64)).max(0.0) as usize;
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = (dx * dx + dy * dy).max(1e-12);
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let t = (((px - a[0]) * dx + (py - a[1]) * dy) / l2).clamp(0.0, 1.0);
                let d = (px - a[0] - dx * t).hypot(py - a[1] - dy * t);
                let c = (r + 0.5 - d).clamp(0.0, 1.0) as f32;
                let k = y * w + x;
                if c > cov[k] {
                    cov[k] = c;
                }
            }
        }
    }
    cov
}

/// Separable Gaussian of a coverage map (Canvas2D `shadowBlur` ≈ σ = blur/2).
fn blur_cov(cov: &[f32], w: usize, h: usize, sigma: f64) -> Vec<f32> {
    if sigma < 0.5 {
        return cov.to_vec();
    }
    let rad = (sigma * 3.0).ceil() as i64;
    let k: Vec<f32> = (-rad..=rad).map(|i| (-0.5 * (i * i) as f64 / (sigma * sigma)).exp() as f32).collect();
    let ks: f32 = k.iter().sum();
    let pass = |src: &[f32], horiz: bool| -> Vec<f32> {
        let mut out = vec![0.0f32; w * h];
        out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, o) in row.iter_mut().enumerate() {
                let mut acc = 0.0;
                for (i, kv) in k.iter().enumerate() {
                    let off = i as i64 - rad;
                    let (sx, sy) = if horiz { (x as i64 + off, y as i64) } else { (x as i64, y as i64 + off) };
                    if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                        acc += src[sy as usize * w + sx as usize] * kv;
                    }
                }
                *o = acc / ks;
            }
        });
        out
    };
    pass(&pass(cov, true), false)
}

/// Render the bolt into a premultiplied linear `w × h` buffer.
pub fn render_lightning(w: usize, h: usize, p: &Params) -> Vec<f32> {
    render_lightning_in(w, h, p, onepaint_color::Space::Linear)
}

/// [`render_lightning`] for a document in `space` (8-bit documents keep sRGB, spec 50).
pub fn render_lightning_in(w: usize, h: usize, p: &Params, space: onepaint_color::Space) -> Vec<f32> {
    let mut out = vec![0.0f32; w * h * 4];
    if w == 0 || h == 0 {
        return out;
    }
    let shadow = p.gen_color_b.unwrap_or([0.7, 0.85, 1.0]);
    let over = |out: &mut [f32], cov: &[f32], col: [f32; 3], alpha: f32| {
        let lin = col.map(|c| space.from_work(c));
        out.par_chunks_mut(4).zip(cov.par_iter()).for_each(|(o, &c)| {
            let a = c * alpha;
            if a > 0.0 {
                for k in 0..3 {
                    o[k] = lin[k] * a + o[k] * (1.0 - a);
                }
                o[3] = a + o[3] * (1.0 - a);
            }
        });
    };
    for s in lightning_strokes(w as u32, h as u32, p) {
        let cov = stroke_coverage(&s, w, h);
        let sh = blur_cov(&cov, w, h, s.blur / 2.0);
        over(&mut out, &sh, shadow, s.alpha as f32);
        over(&mut out, &cov, s.color.map(|c| c as f32), s.alpha as f32);
    }
    out
}
