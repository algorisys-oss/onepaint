//! Shared shader plumbing: where a buffer sits in the document, GL-style
//! bilinear sampling, a parallel per-pixel driver, the separable blur and
//! the hash functions the shaders use (spec 18, "Conventions").

use rayon::prelude::*;

/// Where a buffer sits in the document.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Buffer origin in doc px.
    pub x: i32,
    pub y: i32,
    pub doc_w: u32,
    pub doc_h: u32,
    /// Document resolution (screentone cells are `dpi / lpi`).
    pub dpi: f32,
}

impl Default for Frame {
    fn default() -> Self {
        Frame { x: 0, y: 0, doc_w: 1, doc_h: 1, dpi: 300.0 }
    }
}

impl Frame {
    /// A buffer that is the whole `w × h` document.
    pub fn whole(w: usize, h: usize) -> Frame {
        Frame { x: 0, y: 0, doc_w: w.max(1) as u32, doc_h: h.max(1) as u32, dpi: 300.0 }
    }

    /// A region of a `doc_w × doc_h` document.
    pub fn region(x: i32, y: i32, doc_w: u32, doc_h: u32, dpi: f32) -> Frame {
        Frame { x, y, doc_w: doc_w.max(1), doc_h: doc_h.max(1), dpi }
    }

    pub fn size(&self) -> [f32; 2] {
        [self.doc_w as f32, self.doc_h as f32]
    }
}

pub type Px = [f32; 4];

/// A read-only RGBA f32 image sampled like a GL texture (bilinear, clamp-to-edge).
#[derive(Clone, Copy)]
pub(crate) struct Tex<'a> {
    pub d: &'a [f32],
    pub w: usize,
    pub h: usize,
}

impl<'a> Tex<'a> {
    pub fn new(d: &'a [f32], w: usize, h: usize) -> Self {
        Tex { d, w, h }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> Px {
        let i = (y * self.w + x) * 4;
        [self.d[i], self.d[i + 1], self.d[i + 2], self.d[i + 3]]
    }

    /// Bilinear at texel coordinates (texel centres at integers), clamp-to-edge.
    #[inline]
    pub fn texel(&self, x: f32, y: f32) -> Px {
        let x = x.clamp(0.0, (self.w - 1) as f32);
        let y = y.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let (a, b, c, d) = (self.get(x0, y0), self.get(x1, y0), self.get(x0, y1), self.get(x1, y1));
        [0, 1, 2, 3].map(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bot = c[k] + (d[k] - c[k]) * fx;
            top + (bot - top) * fy
        })
    }

    /// GL `texture(t, uv)` with this texture spanning uv 0..1.
    #[inline]
    pub fn uv(&self, u: f32, v: f32) -> Px {
        self.texel(u * self.w as f32 - 0.5, v * self.h as f32 - 0.5)
    }
}

/// A source buffer placed in the document: sample by doc px or doc uv.
#[derive(Clone, Copy)]
pub(crate) struct Src<'a> {
    pub t: Tex<'a>,
    pub f: Frame,
}

impl<'a> Src<'a> {
    /// `texture(u_src, q / u_size)` for a doc-px position `q`.
    #[inline]
    pub fn px(&self, q: [f32; 2]) -> Px {
        self.t.texel(q[0] - self.f.x as f32 - 0.5, q[1] - self.f.y as f32 - 0.5)
    }

    /// `texture(u_src, uv)` with uv over the whole document.
    #[inline]
    pub fn uv(&self, uv: [f32; 2]) -> Px {
        let s = self.f.size();
        self.px([uv[0] * s[0], uv[1] * s[1]])
    }

    /// Doc-px centre of local pixel (i, j) (= `gl_FragCoord.xy`).
    #[inline]
    pub fn centre(&self, i: usize, j: usize) -> [f32; 2] {
        [self.f.x as f32 + i as f32 + 0.5, self.f.y as f32 + j as f32 + 0.5]
    }
}

/// Run `f(i, j)` for every pixel of a `w × h` buffer in parallel rows.
pub(crate) fn render(w: usize, h: usize, f: impl Fn(usize, usize) -> Px + Sync) -> Vec<f32> {
    let mut out = vec![0.0; w * h * 4];
    if w == 0 {
        return out;
    }
    out.par_chunks_mut(w * 4).enumerate().for_each(|(j, row)| {
        for i in 0..w {
            row[i * 4..i * 4 + 4].copy_from_slice(&f(i, j));
        }
    });
    out
}

/// BLUR_SRC (AR:954) into an `ow × oh` target: 65 taps at `k·R/32` doc px along
/// `dir` (doc px), Gaussian σ = 16 taps. `per_px` is source texels per doc px.
pub(crate) fn blur_to(src: Tex, ow: usize, oh: usize, dir: [f32; 2], r: f32, per_px: [f32; 2]) -> Vec<f32> {
    const HALF: i32 = 32;
    let spread = r.max(1.0) / HALF as f32;
    let sigma = HALF as f32 * 0.5;
    let weights: Vec<f32> = (-HALF..=HALF).map(|i| (-0.5 * (i * i) as f32 / (sigma * sigma)).exp()).collect();
    let wsum: f32 = weights.iter().sum::<f32>().max(1e-4);
    render(ow, oh, |i, j| {
        let bx = (i as f32 + 0.5) / ow as f32 * src.w as f32 - 0.5;
        let by = (j as f32 + 0.5) / oh as f32 * src.h as f32 - 0.5;
        let mut acc = [0.0f32; 4];
        for (k, wt) in weights.iter().enumerate() {
            let o = (k as i32 - HALF) as f32 * spread;
            let s = src.texel(bx + dir[0] * o * per_px[0], by + dir[1] * o * per_px[1]);
            for c in 0..4 {
                acc[c] += s[c] * wt;
            }
        }
        acc.map(|v| v / wsum)
    })
}

/// A separable Gaussian of radius `r` (doc px) into `ow × oh`.
pub(crate) fn gauss_to(src: Tex, ow: usize, oh: usize, r: f32, per_px: [f32; 2]) -> Vec<f32> {
    let out_per_px = [per_px[0] * ow as f32 / src.w as f32, per_px[1] * oh as f32 / src.h as f32];
    let a = blur_to(src, ow, oh, [1.0, 0.0], r, per_px);
    blur_to(Tex::new(&a, ow, oh), ow, oh, [0.0, 1.0], r, out_per_px)
}

pub(crate) fn fract(x: f32) -> f32 {
    x - x.floor()
}

pub(crate) fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(crate) fn mix4(a: Px, b: Px, t: f32) -> Px {
    [0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * t)
}

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn len2(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

pub(crate) fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// Interleaved gradient noise (`ign`), the per-pixel tap jitter.
pub(crate) fn ign(p: [f32; 2]) -> f32 {
    fract(52.982_918 * fract(p[0] * 0.067_110_56 + p[1] * 0.005_837_15))
}

pub(crate) const LUMA: [f32; 3] = [0.299, 0.587, 0.114];

pub(crate) fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Straight colour of a premultiplied pixel (`a > 0 ? rgb / a : rgb`).
pub(crate) fn unpremul(s: Px) -> [f32; 3] {
    if s[3] > 0.0 { [s[0] / s[3], s[1] / s[3], s[2] / s[3]] } else { [s[0], s[1], s[2]] }
}

pub(crate) fn to_work(c: [f32; 3]) -> [f32; 3] {
    c.map(onepaint_color::fast_linear_to_srgb)
}

pub(crate) fn from_work(c: [f32; 3]) -> [f32; 3] {
    c.map(onepaint_color::fast_srgb_to_linear)
}

/// `vec4(fromWork(o) * a, a)`.
pub(crate) fn repremul(o: [f32; 3], a: f32) -> Px {
    let l = from_work(o);
    [l[0] * a, l[1] * a, l[2] * a, a]
}

/// Widen a param that came from a JSON decimal through its shortest decimal
/// form, so f64 maths ported from happypaint sees the same double it did
/// (0.1f32 → 0.1, not 0.100000001…).
pub(crate) fn widen(v: f32) -> f64 {
    v.to_string().parse::<f64>().unwrap_or(v as f64)
}
