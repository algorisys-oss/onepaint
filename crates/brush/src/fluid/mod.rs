//! Live wet media (spec 35 B): a CPU port of happypaint's wet simulation
//! (`engine/wet/wet-renderer.ts`). Fluid brushes deposit water and
//! suspended pigment; fixed 16 ms steps diffuse the water, bloom pigment
//! through it, evaporate, and settle pigment into the layer as it dries.
//!
//! State covers only the wet region (padded, 64-px quantised, clamped to the
//! document), with a doc-space origin:
//! - `sim`: water, vorticity ω, velocity (px/step).
//! - `col`: premultiplied linear suspended pigment.
//!
//! Deterministic: no RNG, no clock; the engine records step counts. The
//! document paper (spec 36) drives the dry-brush gate, absorption, flow
//! resistance and granulation.

mod backend;
pub mod dab;
mod step;

pub use backend::{StepBackend, StepJob};
pub use dab::{ClockState, DabAmounts, Fluid, WetClock, dab_amounts, dry_brush, dry_grain_threshold, reservoir};

use onepaint_color::pigment::pigment_mix;
use onepaint_raster::{Rect, Surface, TILE, Tile};

/// Fixed simulation timestep (ms).
pub const SIM_DT_MS: f64 = 16.0;
/// Velocity clamp (px/step).
pub const V_MAX: f32 = 2.0;
/// Brush drag per batch: path length × this, clamped to `DRAG_MAX` px.
pub const DRAG_FRAC: f64 = 0.6;
pub const DRAG_MAX: f64 = 10.0;
const IMPULSE_K: f32 = 0.5;
const RECT_PAD: i32 = 96;
const RECT_QUANTUM: i32 = 64;

/// One fluid dab (doc px; straight linear colour).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetDab {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub water: f64,
    pub pigment: f64,
    pub color: [f32; 3],
    /// Brush drag at this dab (px per batch).
    pub vx: f64,
    pub vy: f64,
    /// Re-wet lift fraction (0 = plain dab).
    pub rewet: f64,
    /// Dry-brush strength (0 = off) and the paper height it must reach.
    pub dry: f64,
    pub grain_thr: f64,
}

/// Colours this close mix to themselves within 8-bit precision: skip KM.
#[inline]
pub(crate) fn same_color(a: [f32; 3], b: [f32; 3]) -> bool {
    (a[0] - b[0]).abs() < 1.0 / 512.0 && (a[1] - b[1]).abs() < 1.0 / 512.0 && (a[2] - b[2]).abs() < 1.0 / 512.0
}

pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A clamp-to-edge, bilinear view of one RGBA field (sim-local pixels).
/// `data` may hold just a window `[x0, x0 + w) × [y0, y0 + h)` of the
/// `aw × ah` allocation (a snapshot); reads clamp to the allocation first.
#[derive(Clone, Copy)]
pub(crate) struct Field<'a> {
    pub data: &'a [[f32; 4]],
    pub x0: i32,
    pub y0: i32,
    pub w: i32,
    pub h: i32,
    pub aw: i32,
    pub ah: i32,
}

impl<'a> Field<'a> {
    pub fn full(data: &'a [[f32; 4]], w: i32, h: i32) -> Self {
        Self { data, x0: 0, y0: 0, w, h, aw: w, ah: h }
    }

    #[inline]
    pub fn texel(&self, x: i32, y: i32) -> [f32; 4] {
        if self.w == self.aw && self.h == self.ah {
            return self.data[(y.clamp(0, self.ah - 1) * self.w + x.clamp(0, self.aw - 1)) as usize];
        }
        let x = x.clamp(0, self.aw - 1).clamp(self.x0, self.x0 + self.w - 1) - self.x0;
        let y = y.clamp(0, self.ah - 1).clamp(self.y0, self.y0 + self.h - 1) - self.y0;
        self.data[(y * self.w + x) as usize]
    }

    /// LINEAR sample at (u, v) in pixel units (pixel centres at i + 0.5).
    #[inline]
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (x, y) = (u - 0.5, v - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        if fx == 0.0 && fy == 0.0 {
            return self.texel(x0, y0);
        }
        let (a, b, c, d) = (self.texel(x0, y0), self.texel(x0 + 1, y0), self.texel(x0, y0 + 1), self.texel(x0 + 1, y0 + 1));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bot = c[k] + (d[k] - c[k]) * fx;
            top + (bot - top) * fy
        })
    }
}

/// Activity block size (px). A step only touches blocks holding state and
/// their neighbours: everything else is zero and stays zero (the state
/// reaches at most 2 px by diffusion plus 2 px by flow per step).
pub(crate) const BLOCK: i32 = 16;

/// The wet layer: suspended paint and the water it moves in.
pub struct FluidSim {
    /// Allocation in doc px.
    rect: Rect,
    doc: Rect,
    sim: Vec<[f32; 4]>,
    col: Vec<[f32; 4]>,
    /// Ping-pong targets, reused across steps.
    sim_next: Vec<[f32; 4]>,
    col_next: Vec<[f32; 4]>,
    /// Blocks of `sim`/`col` that may be non-zero, and the same for the
    /// `*_next` buffers (so stale blocks there can be cleared).
    pub(crate) occ: Vec<bool>,
    pub(crate) occ_next: Vec<bool>,
    /// The fingering noise factor per pixel (a fixed function of doc position).
    pub(crate) finger: Vec<f32>,
    /// Live oils: viscous, non-drying.
    pub oil: bool,
    /// Canvas tilt (direction × strength, |t| ≤ 1): wet paint runs downhill.
    tilt: [f32; 2],
    /// The document paper and its strength (none = smooth).
    pub(crate) paper: Option<(crate::paper::PaperTexture, f32)>,
    /// Which state this is: changes whenever the state does, so a backend
    /// holding a copy knows when it is stale.
    pub(crate) generation: u64,
    /// The document's space: pigment mixing runs on sRGB values (spec 50).
    pub space: onepaint_color::Space,
}

/// Fresh state generations, unique across every sim (backends are shared).
pub(crate) fn next_generation() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl FluidSim {
    /// A fresh session covering `seed` (doc px).
    pub fn new(doc: Rect, seed: Rect, oil: bool) -> Self {
        let rect = pad_rect(seed, doc);
        let n = (rect.w * rect.h) as usize;
        let nb = blocks(rect).0 * blocks(rect).1;
        Self {
            rect,
            doc,
            sim: vec![[0.0; 4]; n],
            col: vec![[0.0; 4]; n],
            sim_next: vec![[0.0; 4]; n],
            col_next: vec![[0.0; 4]; n],
            occ: vec![false; nb],
            occ_next: vec![false; nb],
            finger: step::finger_field(rect),
            oil,
            tilt: [0.0; 2],
            paper: None,
            generation: next_generation(),
            space: onepaint_color::Space::Linear,
        }
    }

    /// The doc-space rect the state covers.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn set_tilt(&mut self, x: f32, y: f32) {
        let m = x.hypot(y);
        let k = if m > 1.0 { 1.0 / m } else { 1.0 };
        self.tilt = [x * k, y * k];
    }

    /// Textured paper: absorption, flow resistance, granulation, and the
    /// dry-brush gate (`wet-renderer.ts` PAPER_*).
    pub fn set_paper(&mut self, paper: Option<&onepaint_doc::Paper>) {
        self.paper = paper.and_then(|p| Some((crate::paper::PaperTexture::new(p)?, p.strength() as f32))).filter(|(_, s)| *s > 0.0);
    }

    /// Paper height at sim-local position (u, v), or 0.5 on smooth paper.
    #[inline]
    pub(crate) fn paper_at(&self, u: f32, v: f32) -> f32 {
        self.paper.as_ref().map_or(0.5, |(t, _)| t.height((u + self.rect.x as f32) as f64, (v + self.rect.y as f32) as f64))
    }

    pub(crate) fn paper_strength(&self) -> f32 {
        self.paper.as_ref().map_or(0.0, |p| p.1)
    }

    /// Dry brush: an emptying brush only touches the paper's ridges.
    #[inline]
    fn dry_gate(&self, x: i32, y: i32, d: &WetDab) -> f32 {
        let s = self.paper_strength();
        if d.dry <= 0.0 || s <= 0.0 {
            return 1.0;
        }
        let h = self.paper_at(x as f32 + 0.5, y as f32 + 0.5);
        let thr = d.grain_thr as f32 - 0.6 * (1.0 - d.dry as f32 * s);
        smoothstep(thr - 0.08, thr + 0.08, h)
    }

    pub(crate) fn tilt(&self) -> [f32; 2] {
        self.tilt
    }

    pub(crate) fn field(&self) -> (Field<'_>, Field<'_>) {
        let (w, h) = (self.rect.w, self.rect.h);
        (Field::full(&self.sim, w, h), Field::full(&self.col, w, h))
    }

    /// Mark the blocks a sim-local rect touches as holding state.
    fn occupy(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        let bw = blocks(self.rect).0 as i32;
        for by in (y0 / BLOCK)..=((y1 - 1).max(y0) / BLOCK) {
            for bx in (x0 / BLOCK)..=((x1 - 1).max(x0) / BLOCK) {
                if let Some(o) = self.occ.get_mut((by * bw + bx) as usize) {
                    *o = true;
                }
            }
        }
    }

    /// Grow the allocation to cover `needed` (doc px), keeping the state.
    pub fn ensure_rect(&mut self, needed: Rect) {
        let Some(n) = needed.intersect(&self.doc) else { return };
        let r = self.rect;
        if n.x >= r.x && n.y >= r.y && n.right() <= r.right() && n.bottom() <= r.bottom() {
            return;
        }
        // Pad only what's new (happypaint re-pads the whole union, which
        // compounds; the state outside is zero either way).
        let nr = pad_rect(n, self.doc).union(&r);
        let len = (nr.w * nr.h) as usize;
        let (mut sim, mut col) = (vec![[0.0; 4]; len], vec![[0.0; 4]; len]);
        let (dx, dy) = (r.x - nr.x, r.y - nr.y);
        for y in 0..r.h {
            let src = (y * r.w) as usize;
            let dst = ((y + dy) * nr.w + dx) as usize;
            sim[dst..dst + r.w as usize].copy_from_slice(&self.sim[src..src + r.w as usize]);
            col[dst..dst + r.w as usize].copy_from_slice(&self.col[src..src + r.w as usize]);
        }
        let (obw, _) = blocks(r);
        let old_occ = std::mem::take(&mut self.occ);
        self.finger = step::finger_field(nr);
        self.generation = next_generation();
        self.rect = nr;
        self.sim = sim;
        self.col = col;
        self.sim_next = vec![[0.0; 4]; len];
        self.col_next = vec![[0.0; 4]; len];
        let nb = blocks(nr).0 * blocks(nr).1;
        self.occ = vec![false; nb];
        self.occ_next = vec![false; nb];
        for (i, _) in old_occ.iter().enumerate().filter(|(_, o)| **o) {
            let (bx, by) = ((i % obw) as i32 * BLOCK, (i / obw) as i32 * BLOCK);
            self.occupy(bx + dx, by + dy, bx + dx + BLOCK, by + dy + BLOCK);
        }
    }

    /// Pixels (sim-local) whose centres lie in a dab's quad.
    fn quad(&self, cx: f64, cy: f64, r: f64) -> Option<(i32, i32, i32, i32)> {
        let (cx, cy) = (cx - self.rect.x as f64, cy - self.rect.y as f64);
        let x0 = ((cx - r - 0.5).ceil() as i32).max(0);
        let y0 = ((cy - r - 0.5).ceil() as i32).max(0);
        let x1 = ((cx + r - 0.5).ceil() as i32).min(self.rect.w);
        let y1 = ((cy + r - 0.5).ceil() as i32).min(self.rect.h);
        (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
    }

    /// A copy of the sim-local window `r` of `buf` (a shader's snapshot).
    fn snapshot(&self, buf: &[[f32; 4]], r: Rect) -> Vec<[f32; 4]> {
        let mut out = Vec::with_capacity((r.w * r.h) as usize);
        for y in r.y..r.bottom() {
            let i = (y * self.rect.w + r.x) as usize;
            out.extend_from_slice(&buf[i..i + r.w as usize]);
        }
        out
    }

    /// Lay down a batch of dabs: drag the wet state along the brush, lift
    /// re-wetted paint off `layer`, then water (MAX) and pigment (source-over,
    /// KM pre-mixed with the paint under it). Returns the doc rect touched.
    pub fn deposit(&mut self, dabs: &[WetDab], layer: Option<&mut Surface>) -> Rect {
        if dabs.is_empty() {
            return Rect::default();
        }
        self.generation = next_generation();
        let mut bb = Rect::default();
        let mut reach: f64 = 0.0;
        for d in dabs {
            let r = d.radius + 2.0;
            bb = bb.union(&Rect::covering((d.x - r) as f32, (d.y - r) as f32, (d.x + r) as f32, (d.y + r) as f32));
            reach = reach.max(d.vx.abs()).max(d.vy.abs());
        }
        self.ensure_rect(bb);
        let (w, h) = (self.rect.w, self.rect.h);
        let alloc = Rect::new(0, 0, w, h);
        let local_bb = Rect::new(bb.x - self.rect.x, bb.y - self.rect.y, bb.w, bb.h);
        let local = |d: &WetDab, s: &Self| (d.x - s.rect.x as f64, d.y - s.rect.y as f64);
        for d in dabs {
            if let Some((x0, y0, x1, y1)) = self.quad(d.x, d.y, d.radius) {
                self.occupy(x0, y0, x1, y1);
            }
        }

        // 1. Drag: shove the existing wet state along the brush (from a
        // snapshot; overlapping dabs rewrite whole texels, last one wins).
        if dabs.iter().any(|d| d.vx.abs() + d.vy.abs() > 0.01)
            && let Some(win) = local_bb.inflate(reach.ceil() as i32 + 2).intersect(&alloc)
        {
            let (sim0, col0) = (self.snapshot(&self.sim, win), self.snapshot(&self.col, win));
            let view = |data| Field { data, x0: win.x, y0: win.y, w: win.w, h: win.h, aw: w, ah: h };
            let (fs, fc) = (view(&sim0), view(&col0));
            for d in dabs {
                let Some((x0, y0, x1, y1)) = self.quad(d.x, d.y, d.radius) else { continue };
                let (cx, cy) = local(d, self);
                let vel = [(d.vx as f32).clamp(-V_MAX, V_MAX), (d.vy as f32).clamp(-V_MAX, V_MAX)];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let (u, v) = (x as f32 + 0.5, y as f32 + 0.5);
                        let (lx, ly) = ((u as f64 - cx) / d.radius, (v as f64 - cy) / d.radius);
                        let cov = 1.0 - smoothstep(0.55, 1.0, lx.hypot(ly) as f32);
                        let (su, sv) = (u - d.vx as f32 * cov, v - d.vy as f32 * cov);
                        let shifted = fs.sample(su, sv);
                        let under = fs.texel(x, y);
                        let k = IMPULSE_K * cov;
                        let i = (y * w + x) as usize;
                        self.sim[i] = [shifted[0], shifted[1], under[2] + (vel[0] - under[2]) * k, under[3] + (vel[1] - under[3]) * k];
                        self.col[i] = fc.sample(su, sv);
                    }
                }
            }
        }

        // 2. Re-wet: settled paint under lifting dabs goes back into suspension.
        if let Some(layer) = layer.filter(|_| dabs.iter().any(|d| d.rewet > 0.0005)) {
            for d in dabs.iter().filter(|d| d.rewet > 0.0005) {
                let Some((x0, y0, x1, y1)) = self.quad(d.x, d.y, d.radius) else { continue };
                let (cx, cy) = local(d, self);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let (lx, ly) = ((x as f64 + 0.5 - cx) / d.radius, (y as f64 + 0.5 - cy) / d.radius);
                        let cov = (1.0 - smoothstep(0.45, 1.0, lx.hypot(ly) as f32)) * d.rewet as f32;
                        let p = layer.get(self.rect.x + x, self.rect.y + y);
                        let c = &mut self.col[(y * w + x) as usize];
                        for k in 0..4 {
                            c[k] += p[k] * cov;
                        }
                    }
                }
            }
            for d in dabs.iter().filter(|d| d.rewet > 0.0005) {
                let r = d.radius;
                let q = Rect::covering((d.x - r) as f32, (d.y - r) as f32, (d.x + r) as f32, (d.y + r) as f32);
                let Some(q) = q.intersect(&self.doc) else { continue };
                for y in q.y..q.bottom() {
                    for x in q.x..q.right() {
                        let (lx, ly) = ((x as f64 + 0.5 - d.x) / r, (y as f64 + 0.5 - d.y) / r);
                        let cov = (1.0 - smoothstep(0.45, 1.0, lx.hypot(ly) as f32)) * d.rewet as f32;
                        if cov > 0.0 {
                            let p = layer.get(x, y);
                            layer.set(x, y, p.map(|v| v * (1.0 - cov)));
                        }
                    }
                }
            }
        }

        // 3. Water sets the paper to its level (MAX).
        for d in dabs {
            let Some((x0, y0, x1, y1)) = self.quad(d.x, d.y, d.radius) else { continue };
            let (cx, cy) = local(d, self);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (lx, ly) = ((x as f64 + 0.5 - cx) / d.radius, (y as f64 + 0.5 - cy) / d.radius);
                    let cov = (1.0 - smoothstep(0.55, 1.0, lx.hypot(ly) as f32)) * d.water as f32 * self.dry_gate(x, y, d);
                    let s = &mut self.sim[(y * w + x) as usize];
                    s[0] = s[0].max(cov);
                }
            }
        }

        // 4. Pigment: KM pre-mix with the wet paint under the batch, then source-over.
        let win = local_bb.intersect(&alloc).unwrap_or(alloc);
        let under_buf = self.snapshot(&self.col, win);
        let under = Field { data: &under_buf, x0: win.x, y0: win.y, w: win.w, h: win.h, aw: w, ah: h };
        for d in dabs {
            if d.pigment <= 0.0 {
                continue;
            }
            let Some((x0, y0, x1, y1)) = self.quad(d.x, d.y, d.radius) else { continue };
            let (cx, cy) = local(d, self);
            let sp = self.space;
            let bw = d.color.map(|v| sp.to_work(v));
            for y in y0..y1 {
                for x in x0..x1 {
                    let (lx, ly) = ((x as f64 + 0.5 - cx) / d.radius, (y as f64 + 0.5 - cy) / d.radius);
                    let cov = (1.0 - smoothstep(0.45, 1.0, lx.hypot(ly) as f32)) * d.pigment as f32 * self.dry_gate(x, y, d);
                    if cov <= 0.0 {
                        continue;
                    }
                    let i = (y * w + x) as usize;
                    let un = under.texel(x, y);
                    let mut brush = d.color;
                    if un[3] > 0.001 && cov > 0.0005 {
                        let uc = [un[0] / un[3], un[1] / un[3], un[2] / un[3]].map(|v| sp.to_work(v));
                        let t = (un[3] / (un[3] + cov * 2.0)).clamp(0.0, 0.9);
                        if !same_color(uc, bw) {
                            brush = pigment_mix(bw, uc, t).map(|v| sp.from_work(v));
                        }
                    }
                    let c = &mut self.col[i];
                    let k = 1.0 - cov;
                    *c = [brush[0] * cov + c[0] * k, brush[1] * cov + c[1] * k, brush[2] * cov + c[2] * k, cov + c[3] * k];
                }
            }
        }
        bb.intersect(&self.doc).unwrap_or_default()
    }

    /// The live preview over [`FluidSim::rect`]: suspended paint over a faint
    /// cool veil wherever the paper is wet (display only).
    pub fn preview(&self) -> Surface {
        use rayon::prelude::*;
        let veil = if self.oil { 0.0 } else { 0.24 };
        let r = self.rect;
        let bw = blocks(r).0;
        let mut per_tile: std::collections::BTreeMap<(i32, i32), Vec<Rect>> = Default::default();
        for (b, _) in self.occ.iter().enumerate().filter(|(_, o)| **o) {
            let block = Rect::new(r.x + (b % bw) as i32 * BLOCK, r.y + (b / bw) as i32 * BLOCK, BLOCK, BLOCK);
            let Some(ix) = block.intersect(&r) else { continue };
            for (tx, ty) in ix.tiles() {
                if let Some(part) = ix.intersect(&Rect::from_tile(tx, ty)) {
                    per_tile.entry((tx, ty)).or_default().push(part);
                }
            }
        }
        let tiles: Vec<_> = per_tile
            .into_par_iter()
            .filter_map(|((tx, ty), parts)| {
                let mut buf = vec![0.0f32; (TILE * TILE * 4) as usize];
                let mut any = false;
                for p in parts {
                    for y in p.y..p.bottom() {
                        for x in p.x..p.right() {
                            let i = ((y - r.y) * r.w + (x - r.x)) as usize;
                            let c = self.col[i];
                            let a = veil * smoothstep(0.02, 0.5, self.sim[i][0]);
                            if c[3] <= 0.0 && a <= 0.0 {
                                continue;
                            }
                            let k = 1.0 - c[3];
                            let o = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
                            buf[o..o + 4].copy_from_slice(&[c[0], c[1] + 0.08 * a * k, c[2] + 0.33 * a * k, c[3] + a * k]);
                            any = true;
                        }
                    }
                }
                any.then(|| ((tx, ty), Tile::from_f32(&buf)))
            })
            .collect();
        let mut s = Surface::new();
        for ((tx, ty), t) in tiles {
            s.set_tile(tx, ty, Some(std::sync::Arc::new(t)));
        }
        s
    }

    /// Total suspended pigment mass (tests and diagnostics).
    pub fn suspended_mass(&self) -> f64 {
        self.col.iter().map(|c| c[3] as f64).sum()
    }

    /// Total water (tests and diagnostics).
    pub fn water(&self) -> f64 {
        self.sim.iter().map(|s| s[0] as f64).sum()
    }

    /// The wettest pixel's water.
    pub fn peak_water(&self) -> f32 {
        self.sim.iter().map(|s| s[0]).fold(0.0, f32::max)
    }
}

/// Block grid size of an allocation.
pub(crate) fn blocks(r: Rect) -> (usize, usize) {
    (((r.w + BLOCK - 1) / BLOCK) as usize, ((r.h + BLOCK - 1) / BLOCK) as usize)
}

/// Pad and quantise a needed rect, clamped to the document.
fn pad_rect(n: Rect, doc: Rect) -> Rect {
    let q = RECT_QUANTUM;
    let x0 = ((n.x - RECT_PAD).div_euclid(q) * q).max(doc.x);
    let y0 = ((n.y - RECT_PAD).div_euclid(q) * q).max(doc.y);
    let x1 = ((n.right() + RECT_PAD + q - 1).div_euclid(q) * q).min(doc.right());
    let y1 = ((n.bottom() + RECT_PAD + q - 1).div_euclid(q) * q).min(doc.bottom());
    Rect::new(x0, y0, (x1 - x0).max(1), (y1 - y0).max(1))
}
