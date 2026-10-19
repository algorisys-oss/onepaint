//! Selection masks: combine modes, polygon rasterisation, feather, grow/shrink,
//! smooth and colour matching (magic wand / flood fill).
//!
//! Ported from happypaint `engine/selection/selection.ts` and the flood fill in
//! `engine/paint-engine.ts`; see `docs/specs/08-selection-fill.md`.

pub mod saved;

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Per-pixel coverage, 0 (out) ..= 255 (selected), one byte per document pixel.
#[derive(Clone, PartialEq, Eq)]
pub struct Mask {
    w: u32,
    h: u32,
    data: Vec<u8>,
}

impl std::fmt::Debug for Mask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Mask({}x{})", self.w, self.h)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

impl Mode {
    /// Shift+Alt intersect, Alt subtract, Shift add, otherwise the sticky mode (S:27-32).
    pub fn from_modifiers(shift: bool, alt: bool, sticky: Mode) -> Mode {
        match (shift, alt) {
            (true, true) => Mode::Intersect,
            (false, true) => Mode::Subtract,
            (true, false) => Mode::Add,
            _ => sticky,
        }
    }
}

impl Mask {
    pub fn new(w: u32, h: u32) -> Self {
        Self { w, h, data: vec![0; (w * h) as usize] }
    }

    pub fn full(w: u32, h: u32) -> Self {
        Self { w, h, data: vec![255; (w * h) as usize] }
    }

    pub fn from_vec(w: u32, h: u32, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), (w * h) as usize, "mask size");
        Self { w, h, data }
    }

    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        self.data[(y * self.w + x) as usize]
    }

    /// Coverage 0..1 at a document pixel; outside the canvas is 0.
    #[inline]
    pub fn coverage(&self, x: i32, y: i32) -> f32 {
        if x < 0 || y < 0 || x as u32 >= self.w || y as u32 >= self.h {
            return 0.0;
        }
        self.get(x as u32, y as u32) as f32 / 255.0
    }

    pub fn has_content(&self) -> bool {
        self.data.iter().any(|&v| v != 0)
    }

    pub fn is_full(&self) -> bool {
        self.data.iter().all(|&v| v == 255)
    }

    pub fn inverted(&self) -> Mask {
        Mask { w: self.w, h: self.h, data: self.data.iter().map(|v| 255 - v).collect() }
    }

    /// Bounding box of non-zero coverage as (x, y, w, h).
    pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..self.h {
            let row = &self.data[(y * self.w) as usize..((y + 1) * self.w) as usize];
            if let Some(a) = row.iter().position(|&v| v != 0) {
                let b = row.iter().rposition(|&v| v != 0).unwrap_or(a);
                x0 = x0.min(a as u32);
                x1 = x1.max(b as u32);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
        (x0 != u32::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    }

    fn sliding(&self, r: u32, max: bool) -> Mask {
        // One line's running max (or min) over ±r, with a monotonic deque.
        let line = |src: &[u8], dst: &mut [u8]| {
            let len = src.len();
            let mut q: VecDeque<usize> = VecDeque::new();
            let r = r as usize;
            let mut next = 0;
            for (i, d) in dst.iter_mut().enumerate() {
                let hi = (i + r).min(len - 1);
                while next <= hi {
                    let v = src[next];
                    while let Some(&b) = q.back() {
                        let bv = src[b];
                        if (max && bv <= v) || (!max && bv >= v) {
                            q.pop_back();
                        } else {
                            break;
                        }
                    }
                    q.push_back(next);
                    next += 1;
                }
                while q.front().is_some_and(|&f| f + r < i) {
                    q.pop_front();
                }
                *d = q.front().map_or(0, |&f| src[f]);
            }
        };
        let (w, h) = (self.w as usize, self.h as usize);
        Mask { w: self.w, h: self.h, data: separable(&self.data, w, h, line) }
    }

    /// Square max filter; the window is clipped at the canvas edge (S:453-462).
    pub fn dilated(&self, r: u32) -> Mask {
        if r == 0 { self.clone() } else { self.sliding(r.min(200), true) }
    }

    /// Square min filter; clipped windows mean the border is not eaten.
    pub fn eroded(&self, r: u32) -> Mask {
        if r == 0 { self.clone() } else { self.sliding(r.min(200), false) }
    }

    /// Separable box blur, applied once, clamp-to-edge (S:377-411).
    pub fn feathered(&self, radius: f32) -> Mask {
        let r = (radius.round() as i64).max(1) as usize;
        let (w, h) = (self.w as usize, self.h as usize);
        let win = (2 * r + 1) as f32;
        let a: Vec<f32> = self.data.iter().map(|&v| v as f32 / 255.0).collect();
        let line = |src: &[f32], dst: &mut [f32]| {
            let len = src.len();
            let at = |i: i64| src[i.clamp(0, len as i64 - 1) as usize];
            let mut sum: f32 = (-(r as i64)..=r as i64).map(at).sum();
            for (i, d) in dst.iter_mut().enumerate() {
                *d = sum / win;
                sum += at(i as i64 + r as i64 + 1) - at(i as i64 - r as i64);
            }
        };
        let out = separable(&a, w, h, line);
        Mask { w: self.w, h: self.h, data: out.iter().map(|v| (v.min(1.0) * 255.0).round() as u8).collect() }
    }

    /// Feather then threshold at 128 → a hard mask (S:467-472).
    pub fn smoothed(&self, px: f32) -> Mask {
        let r = px.round().clamp(0.0, 100.0);
        let f = self.feathered(r);
        Mask { w: self.w, h: self.h, data: f.data.iter().map(|&v| if v >= 128 { 255 } else { 0 }).collect() }
    }

    /// `n` passes of 4-neighbour dilation of non-zero pixels, confined to `within > 0`.
    pub fn grown4(&self, n: u32, within: Option<&Mask>) -> Mask {
        let mut cur = self.clone();
        let (w, h) = (self.w as usize, self.h as usize);
        for _ in 0..n {
            let mut next = cur.clone();
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    if cur.data[i] != 0 || within.is_some_and(|m| m.data[i] == 0) {
                        continue;
                    }
                    let hit = (x > 0 && cur.data[i - 1] != 0)
                        || (x + 1 < w && cur.data[i + 1] != 0)
                        || (y > 0 && cur.data[i - w] != 0)
                        || (y + 1 < h && cur.data[i + w] != 0);
                    if hit {
                        next.data[i] = 255;
                    }
                }
            }
            cur = next;
        }
        cur
    }
}

/// A separable filter: `line` along every row, then every column (rows in
/// parallel; the columns as rows of the transposed image, which keeps them
/// in cache). Each line runs in the same order as a strided pass would.
fn separable<T: Copy + Default + Send + Sync>(src: &[T], w: usize, h: usize, line: impl Fn(&[T], &mut [T]) + Sync) -> Vec<T> {
    use rayon::prelude::*;
    let rows = |src: &[T], len: usize| {
        let mut out = vec![T::default(); src.len()];
        out.par_chunks_mut(len).zip(src.par_chunks(len)).for_each(|(d, s)| line(s, d));
        out
    };
    let across = rows(src, w);
    let down = rows(&transpose(&across, w, h), h);
    transpose(&down, h, w)
}

/// `src` (`w` × `h`, row-major) transposed, in 64² blocks, rows of blocks in parallel.
fn transpose<T: Copy + Default + Send + Sync>(src: &[T], w: usize, h: usize) -> Vec<T> {
    use rayon::prelude::*;
    const B: usize = 64;
    let mut out = vec![T::default(); src.len()];
    // Output row x holds input column x: a band of B output rows per task.
    out.par_chunks_mut(B * h).enumerate().for_each(|(bx, band)| {
        let x0 = bx * B;
        let xs = band.len() / h;
        for y0 in (0..h).step_by(B) {
            for y in y0..(y0 + B).min(h) {
                for dx in 0..xs {
                    band[dx * h + y] = src[y * w + x0 + dx];
                }
            }
        }
    });
    out
}

/// Combine a new region into the existing selection (S:46-62).
pub fn combine(base: Option<&Mask>, src: &Mask, mode: Mode) -> Mask {
    let mut out = src.clone();
    match (mode, base) {
        (Mode::Replace, _) | (Mode::Add, None) => {}
        (Mode::Subtract | Mode::Intersect, None) => out.data.fill(0),
        (Mode::Add, Some(b)) => out.data.iter_mut().zip(&b.data).for_each(|(o, &b)| *o = (*o).max(b)),
        (Mode::Intersect, Some(b)) => out.data.iter_mut().zip(&b.data).for_each(|(o, &b)| *o = (*o).min(b)),
        (Mode::Subtract, Some(b)) => out.data.iter_mut().zip(&b.data).for_each(|(o, &b)| {
            *o = match *o {
                0 => b,
                255 => 0,
                s => ((b as f32) * (255 - s) as f32 / 255.0).round() as u8,
            }
        }),
    }
    out
}

pub type Pt = [f64; 2];

pub fn rect_polygon(a: Pt, b: Pt) -> Vec<Pt> {
    vec![a, [b[0], a[1]], b, [a[0], b[1]]]
}

/// Ellipse inscribed in the box from `a` to `b`, 64 segments (S:359-370).
pub fn ellipse_polygon(a: Pt, b: Pt) -> Vec<Pt> {
    let (cx, cy) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let (rx, ry) = ((b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0);
    (0..64)
        .map(|i| {
            let t = i as f64 / 64.0 * std::f64::consts::TAU;
            [cx + rx * t.cos(), cy + ry * t.sin()]
        })
        .collect()
}

/// Square/circle constraint: side = max(|dx|, |dy|), anchored at `start`.
pub fn constrain_square(start: Pt, end: Pt) -> Pt {
    let (dx, dy) = (end[0] - start[0], end[1] - start[1]);
    let s = dx.abs().max(dy.abs());
    [start[0] + s * dx.signum(), start[1] + s * dy.signum()]
}

/// Snap the segment `from → to` to the nearest 45°, keeping its length.
pub fn constrain_45(from: Pt, to: Pt) -> Pt {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let len = dx.hypot(dy);
    let a = (dy.atan2(dx) / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
    [from[0] + len * a.cos(), from[1] + len * a.sin()]
}

/// Hard, even-odd scanline fill of one or more closed contours (S:97-132).
pub fn rasterize_polygons(w: u32, h: u32, contours: &[Vec<Pt>]) -> Mask {
    let mut m = Mask::new(w, h);
    let pts = contours.iter().flatten();
    let (mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in pts {
        min_y = min_y.min(p[1]);
        max_y = max_y.max(p[1]);
    }
    if !min_y.is_finite() {
        return m;
    }
    let y0 = (min_y.floor() as i64).max(0);
    let y1 = (max_y.ceil() as i64).min(h as i64 - 1);
    let mut xs = Vec::new();
    for y in y0..=y1 {
        let cy = y as f64 + 0.5;
        xs.clear();
        for c in contours.iter().filter(|c| c.len() >= 3) {
            for i in 0..c.len() {
                let (a, b) = (c[i], c[(i + 1) % c.len()]);
                if (a[1] <= cy) != (b[1] <= cy) {
                    xs.push(a[0] + (cy - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        for pair in xs.chunks_exact(2) {
            let s = (pair[0].round() as i64).max(0);
            let e = (pair[1].round() as i64 - 1).min(w as i64 - 1);
            for x in s..=e {
                m.data[(y as u32 * w + x as u32) as usize] = 255;
            }
        }
    }
    m
}

/// Pixels whose premultiplied RGBA is within `tol` (Chebyshev, 0..1) of the seed.
/// Contiguous = 4-connected flood from the seed. `within` confines the search
/// to coverage > 0. Returns `None` when the seed is outside the canvas or excluded.
///
/// Every pixel is tested once, rows in parallel; the flood then runs over
/// that bitmap span by span.
#[allow(clippy::too_many_arguments)]
pub fn color_match(
    w: u32,
    h: u32,
    get: &(dyn Fn(u32, u32) -> [f32; 4] + Sync),
    sx: i32,
    sy: i32,
    tol: f32,
    contiguous: bool,
    within: Option<&Mask>,
) -> Option<Mask> {
    use rayon::prelude::*;
    if sx < 0 || sy < 0 || sx as u32 >= w || sy as u32 >= h {
        return None;
    }
    let (sx, sy) = (sx as u32, sy as u32);
    let inside = |x: u32, y: u32| within.is_none_or(|m| m.get(x, y) != 0);
    if !inside(sx, sy) {
        return None;
    }
    let seed = get(sx, sy);
    let tol = tol.clamp(0.0, 1.0) + 1e-6;
    let mut hits = Mask::new(w, h);
    hits.data.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
        let y = y as u32;
        for (x, v) in row.iter_mut().enumerate() {
            let x = x as u32;
            if inside(x, y) {
                let p = get(x, y);
                if (0..4).all(|c| (p[c] - seed[c]).abs() <= tol) {
                    *v = 255;
                }
            }
        }
    });
    if !contiguous {
        return Some(hits);
    }
    Some(flood(&hits, sx, sy))
}

/// The 4-connected region of `hits` holding (sx, sy): scanline spans, each
/// pixel visited once.
fn flood(hits: &Mask, sx: u32, sy: u32) -> Mask {
    let (w, h) = (hits.w as usize, hits.h as usize);
    let hit = |x: usize, y: usize| hits.data[y * w + x] != 0;
    let mut m = Mask::new(hits.w, hits.h);
    let mut stack = vec![(sx as usize, sy as usize)];
    while let Some((x, y)) = stack.pop() {
        if m.data[y * w + x] != 0 || !hit(x, y) {
            continue;
        }
        let (mut a, mut b) = (x, x);
        while a > 0 && hit(a - 1, y) && m.data[y * w + a - 1] == 0 {
            a -= 1;
        }
        while b + 1 < w && hit(b + 1, y) && m.data[y * w + b + 1] == 0 {
            b += 1;
        }
        m.data[y * w + a..=y * w + b].fill(255);
        // One seed per run of hits above and below the span.
        for ny in [y.wrapping_sub(1), y + 1] {
            if ny >= h {
                continue;
            }
            let mut in_run = false;
            for nx in a..=b {
                let open = hit(nx, ny) && m.data[ny * w + nx] == 0;
                if open && !in_run {
                    stack.push((nx, ny));
                }
                in_run = open;
            }
        }
    }
    m
}
