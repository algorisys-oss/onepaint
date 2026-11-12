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

    /// A Gaussian feather whose `sigma` is in doc px: Photoshop's meaning of
    /// a feather radius (v0.209.0, S:446-505). Three box passes per axis
    /// ([`gauss_box_radii`]), clamp-to-edge, over the selection's bounds grown
    /// by the blur's reach only; outside that every window is all zero.
    pub fn feathered(&self, sigma: f32) -> Mask {
        let mut out = Mask::new(self.w, self.h);
        let Some((bx, by, bw, bh)) = self.bounds() else {
            return out;
        };
        let radii = gauss_box_radii(f64::from(sigma.max(0.5)));
        let reach = (radii[0] + radii[1] + radii[2]) as u32;
        let x0 = bx.saturating_sub(reach);
        let y0 = by.saturating_sub(reach);
        let cw = ((bx + bw - 1 + reach).min(self.w - 1) - x0 + 1) as usize;
        let ch = ((by + bh - 1 + reach).min(self.h - 1) - y0 + 1) as usize;
        let mut a = vec![0f32; cw * ch];
        for (y, row) in a.chunks_exact_mut(cw).enumerate() {
            let src = (y0 as usize + y) * self.w as usize + x0 as usize;
            for (d, &v) in row.iter_mut().zip(&self.data[src..src + cw]) {
                *d = f32::from(v) / 255.0;
            }
        }
        let line = |src: &[f32], dst: &mut [f32]| {
            let mut tmp = vec![0f32; src.len()];
            box_line(src, dst, radii[0]);
            box_line(dst, &mut tmp, radii[1]);
            box_line(&tmp, dst, radii[2]);
        };
        let blurred = separable(&a, cw, ch, line);
        for (y, row) in blurred.chunks_exact(cw).enumerate() {
            let dst = (y0 as usize + y) * self.w as usize + x0 as usize;
            for (d, &v) in out.data[dst..dst + cw].iter_mut().zip(row) {
                *d = if v <= 0.0 {
                    0
                } else if v >= 1.0 {
                    255
                } else {
                    (v * 255.0).round() as u8
                };
            }
        }
        out
    }

    /// Feather then threshold at 128 → a hard mask (S:560-569). Smooth by `r`
    /// keeps the strength of the single box blur it always used: the Gaussian
    /// gets that box's variance, r(r+1)/3.
    pub fn smoothed(&self, px: f32) -> Mask {
        let r = px.round().clamp(1.0, 100.0);
        let f = self.feathered((r * (r + 1.0) / 3.0).sqrt());
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

/// Radii of three successive box blurs whose combined variance matches a
/// Gaussian of standard deviation `sigma` (a box of radius r has variance
/// r(r+1)/3, and variances add). Mixes two neighbouring radii so the match
/// holds at small sigmas too (S:386-400).
pub fn gauss_box_radii(sigma: f64) -> [usize; 3] {
    let target = sigma * sigma;
    let lo = ((target + 0.25).sqrt() - 0.5).floor().max(0.0);
    let v_lo = lo * (lo + 1.0) / 3.0;
    let v_hi = (lo + 1.0) * (lo + 2.0) / 3.0;
    // m passes at `lo`, the rest at `lo + 1`: m·vLo + (3 − m)·vHi ≈ target.
    let m = ((3.0 * v_hi - target) / (v_hi - v_lo)).round().clamp(0.0, 3.0) as usize;
    let lo = lo as usize;
    [0, 1, 2].map(|i| if i < m { lo } else { lo + 1 })
}

/// A running-sum box blur of radius `r` along one line, edge samples extended.
fn box_line(src: &[f32], dst: &mut [f32], r: usize) {
    if r == 0 {
        dst.copy_from_slice(src);
        return;
    }
    let last = src.len() as i64 - 1;
    let at = |i: i64| f64::from(src[i.clamp(0, last) as usize]);
    let win = (2 * r + 1) as f64;
    let r = r as i64;
    let mut sum: f64 = (-r..=r).map(at).sum();
    for (i, d) in dst.iter_mut().enumerate() {
        *d = (sum / win) as f32;
        let i = i as i64;
        sum += at(i + r + 1) - at(i - r);
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

/// Ellipse inscribed in the box from `a` to `b` (S:362-383). The segment
/// count follows the size so each flat strays at most ~0.1 px from the true
/// curve: a chord of angle 2π/n on radius r bows by r(1 − cos π/n) ≈ rπ²/2n².
pub fn ellipse_polygon(a: Pt, b: Pt) -> Vec<Pt> {
    let (rx, ry) = ((b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0);
    let n = (std::f64::consts::PI * (rx.max(ry) / 0.2).sqrt()).ceil().clamp(64.0, 2048.0) as usize;
    ellipse_polygon_n(a, b, n)
}

/// [`ellipse_polygon`] with an explicit segment count.
pub fn ellipse_polygon_n(a: Pt, b: Pt, segments: usize) -> Vec<Pt> {
    let (cx, cy) = ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0);
    let (rx, ry) = ((b[0] - a[0]).abs() / 2.0, (b[1] - a[1]).abs() / 2.0);
    (0..segments)
        .map(|i| {
            let t = i as f64 / segments as f64 * std::f64::consts::TAU;
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
    let test = |x: u32, y: u32| {
        inside(x, y) && {
            let p = get(x, y);
            (0..4).all(|c| (p[c] - seed[c]).abs() <= tol)
        }
    };
    if contiguous {
        // Only the blocks the region reaches are ever tested.
        return Some(flood(&mut LazyHits::new(w, h, &test), sx, sy));
    }
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
    Some(hits)
}

/// Hit tests in 64² blocks, each run the first time the flood reads it;
/// once a big share is needed (a region spanning much of the canvas), the
/// rest run at once, in parallel.
struct LazyHits<'a> {
    w: usize,
    h: usize,
    cols: usize,
    blocks: Vec<Option<Box<[bool]>>>,
    made: usize,
    test: &'a (dyn Fn(u32, u32) -> bool + Sync),
    /// Every hit, row-major, once all blocks are in (read directly).
    flat: Option<Vec<bool>>,
}

const BLOCK: usize = 64;

impl<'a> LazyHits<'a> {
    fn new(w: u32, h: u32, test: &'a (dyn Fn(u32, u32) -> bool + Sync)) -> Self {
        let (w, h) = (w as usize, h as usize);
        let cols = w.div_ceil(BLOCK);
        Self { w, h, cols, blocks: vec![None; cols * h.div_ceil(BLOCK)], made: 0, test, flat: None }
    }

    fn block(&self, bx: usize, by: usize) -> Box<[bool]> {
        let mut v = vec![false; BLOCK * BLOCK].into_boxed_slice();
        for j in 0..BLOCK.min(self.h - by * BLOCK) {
            for i in 0..BLOCK.min(self.w - bx * BLOCK) {
                v[j * BLOCK + i] = (self.test)((bx * BLOCK + i) as u32, (by * BLOCK + j) as u32);
            }
        }
        v
    }

    #[inline]
    fn hit(&mut self, x: usize, y: usize) -> bool {
        if let Some(f) = &self.flat {
            return f[y * self.w + x];
        }
        let (bx, by) = (x / BLOCK, y / BLOCK);
        let at = by * self.cols + bx;
        if self.blocks[at].is_none() {
            if self.made * 8 > self.blocks.len() {
                self.fill_all();
                return self.hit(x, y);
            } else {
                self.blocks[at] = Some(self.block(bx, by));
                self.made += 1;
            }
        }
        self.blocks[at].as_ref().is_some_and(|b| b[(y % BLOCK) * BLOCK + x % BLOCK])
    }

    /// Every block not yet tested, in parallel.
    fn fill_all(&mut self) {
        use rayon::prelude::*;
        let cols = self.cols;
        let made: Vec<(usize, Box<[bool]>)> = (0..self.blocks.len())
            .into_par_iter()
            .filter(|&i| self.blocks[i].is_none())
            .map(|i| (i, self.block(i % cols, i / cols)))
            .collect();
        self.made += made.len();
        for (i, b) in made {
            self.blocks[i] = Some(b);
        }
        // All in: lay them out flat for the rest of the flood.
        let (w, h) = (self.w, self.h);
        let mut flat = vec![false; w * h];
        for (i, b) in self.blocks.iter().enumerate() {
            let Some(b) = b else { continue };
            let (x0, y0) = ((i % cols) * BLOCK, (i / cols) * BLOCK);
            let bw = BLOCK.min(w - x0);
            for j in 0..BLOCK.min(h - y0) {
                flat[(y0 + j) * w + x0..(y0 + j) * w + x0 + bw].copy_from_slice(&b[j * BLOCK..j * BLOCK + bw]);
            }
        }
        self.blocks = Vec::new();
        self.flat = Some(flat);
    }
}

/// The 4-connected region of hits holding (sx, sy): scanline spans, each
/// pixel visited once.
fn flood(hits: &mut LazyHits, sx: u32, sy: u32) -> Mask {
    let (w, h) = (hits.w, hits.h);
    let mut hit = |x: usize, y: usize| hits.hit(x, y);
    let mut m = Mask::new(w as u32, h as u32);
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
