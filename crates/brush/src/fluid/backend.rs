//! Running the wet steps elsewhere (the GPU): the same maths as
//! [`FluidSim::step`], checked against it, behind a trait so this crate
//! stays free of wgpu.

use super::{BLOCK, FluidSim, blocks};

use crate::TipMask;
use onepaint_raster::{Rect, Surface, TILE};

/// A batch of steps for a backend: the whole allocation, in and out.
pub struct StepJob<'a> {
    /// The allocation in doc px (sim-local (0, 0) is `rect.x, rect.y`).
    pub rect: Rect,
    /// Water, vorticity, velocity per pixel; updated in place.
    pub sim: &'a mut [[f32; 4]],
    /// Premultiplied linear suspended pigment; updated in place.
    pub col: &'a mut [[f32; 4]],
    /// The fingering factor per pixel.
    pub finger: &'a [f32],
    pub oil: bool,
    /// Pigment mixing runs on sRGB values: 8-bit documents hold them (spec 50).
    pub space: onepaint_color::Space,
    pub tilt: [f32; 2],
    /// Paper: the grain tile, doc px per tile, strength (none = smooth).
    /// The grain's handle (a backend may keep it to cache the upload), doc
    /// px per tile, and strength.
    pub paper: Option<(&'a std::sync::Arc<TipMask>, f64, f32)>,
    pub steps: usize,
    /// The state's generation: a backend still holding this generation from
    /// its last run may skip the upload.
    pub generation: u64,
    /// The generation the result will have (the backend keeps it on success).
    pub next_generation: u64,
    /// Out: the paint settled over the steps, source-over composited in
    /// order (premultiplied, one per allocation pixel, zeros in).
    pub settled: &'a mut [[f32; 4]],
}

/// Runs [`StepJob`]s; `false` means "can't", and the CPU steps instead.
pub trait StepBackend: Send {
    fn run(&mut self, job: StepJob<'_>) -> bool;
}

impl FluidSim {
    /// `steps` steps on `backend` over `rect` (grown to cover it). Returns
    /// the doc rect whose layer pixels changed, or `None` when the backend
    /// declined (nothing was touched).
    pub fn step_with(&mut self, backend: &mut dyn StepBackend, layer: &mut Surface, rect: Rect, steps: usize) -> Option<Rect> {
        self.ensure_rect(rect);
        let r = self.rect();
        let mut settled = vec![[0.0f32; 4]; (r.w * r.h) as usize];
        let tilt = self.tilt();
        let next = super::next_generation();
        let paper = self.paper.as_ref().map(|(t, s)| (t.mask_arc(), t.tile_px(), *s));
        let job = StepJob {
            rect: r,
            sim: &mut self.sim,
            col: &mut self.col,
            finger: &self.finger,
            oil: self.oil,
            space: self.space,
            tilt,
            paper,
            steps,
            generation: self.generation,
            next_generation: next,
            settled: &mut settled,
        };
        if !backend.run(job) {
            return None;
        }
        self.generation = next;
        // Settled paint goes onto the layer in one source-over (it is associative).
        let mut dirty = Rect::default();
        for (tx, ty) in r.tiles() {
            let Some(ix) = r.intersect(&Rect::from_tile(tx, ty)) else { continue };
            let mut touched = false;
            for y in ix.y..ix.bottom() {
                for x in ix.x..ix.right() {
                    let s = settled[((y - r.y) * r.w + (x - r.x)) as usize];
                    if s[3] <= 0.0 && s[0] <= 0.0 && s[1] <= 0.0 && s[2] <= 0.0 {
                        continue;
                    }
                    let t = layer.tile_mut(tx, ty);
                    let (lx, ly) = ((x - tx * TILE) as usize, (y - ty * TILE) as usize);
                    let d = t.get(lx, ly);
                    t.set(lx, ly, std::array::from_fn(|c| s[c] + d[c] * (1.0 - s[3])));
                    touched = true;
                }
            }
            if touched {
                dirty = dirty.union(&ix);
                layer.prune_tile(tx, ty);
            }
        }
        // Occupancy from the new state; the spare buffers hold nothing live.
        let (bw, bh) = blocks(r);
        let mut occ = vec![false; bw * bh];
        for y in 0..r.h {
            for x in 0..r.w {
                let i = (y * r.w + x) as usize;
                let (sv, cv) = (self.sim[i], self.col[i]);
                if sv[0] != 0.0 || sv[2] != 0.0 || sv[3] != 0.0 || cv[3] != 0.0 {
                    occ[(y / BLOCK) as usize * bw + (x / BLOCK) as usize] = true;
                }
            }
        }
        self.occ = occ;
        self.occ_next = vec![true; bw * bh];
        Some(dirty)
    }
}
