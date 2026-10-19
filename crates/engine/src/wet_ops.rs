//! Live wet media (spec 35 B): fluid strokes feed the drying simulation,
//! which keeps running after the pen lifts and settles paint into the layer
//! as it dries. The whole wet session, from the first deposit to dry, is one
//! undo step (happypaint `paint-engine.ts:3071-3460`).

use super::*;
use onepaint_brush::fluid::{self, ClockState, Fluid, FluidSim, WetClock, WetDab};
use onepaint_brush::height::HeightDab;

/// A fluid stroke in progress (pointer down → up).
pub(crate) struct WetStroke {
    size: f64,
    fluid: Fluid,
    /// Relief per dab (live oils build thickness).
    impasto: f64,
    color: [f32; 3],
    sampler: onepaint_stroke::Sampler,
    smoother: onepaint_stroke::Smoother,
    transforms: Vec<onepaint_brush::symmetry::SymTransform>,
    /// Previous stamp: the per-dab shove direction.
    last: Option<(f64, f64)>,
    /// Path length so far; the reservoir empties with it.
    travel: f64,
}

/// Wet paint on one layer, not yet committed to history.
pub(crate) struct WetSession {
    layer: NodeId,
    sim: FluidSim,
    clock: WetClock,
    /// The layer before the first deposit.
    before: Surface,
    height: Option<height_ops::HeightEdit>,
    /// Doc-space bounds the steps work in (grows 4 px per step).
    bounds: [f64; 4],
    time_acc: f64,
    last_now: Option<f64>,
    /// What the canvas shows over the layer: suspended paint and the damp veil.
    preview: Surface,
}

impl WetSession {
    fn rect(&self, doc: Rect) -> Option<Rect> {
        let [x0, y0, x1, y1] = self.bounds;
        Rect::covering(x0 as f32, y0 as f32, x1 as f32, y1 as f32).intersect(&doc)
    }

    fn grow(&mut self, x: f64, y: f64, r: f64) {
        let b = &mut self.bounds;
        *b = [b[0].min(x - r - 2.0), b[1].min(y - r - 2.0), b[2].max(x + r + 2.0), b[3].max(y + r + 2.0)];
    }
}

impl Session {
    /// Whether the active brush paints through the wet simulation.
    pub(crate) fn wet_brush(&self) -> bool {
        self.tool == Tool::Brush && !self.mask_target() && self.brush.fluid().is_some()
    }

    /// Anything still wet (simulating).
    pub fn wet_active(&self) -> bool {
        self.wet.is_some()
    }

    pub fn wet_tilt(&self) -> [f32; 2] {
        self.wet_tilt
    }

    pub(crate) fn set_wet_tilt(&mut self, x: f32, y: f32) {
        self.wet_tilt = [x, y];
        if let Some(s) = self.wet.as_mut() {
            s.sim.set_tilt(x, y);
        }
    }

    /// Change the document paper (no undo step, as in happypaint): new
    /// strokes and the live wet paint use it, and the canvas shows it.
    pub(crate) fn set_paper(&mut self, paper: Option<onepaint_doc::Paper>) {
        if let Some(s) = self.wet.as_mut() {
            s.sim.set_paper(paper.as_ref(), self.doc.dpi);
        }
        self.doc.set_paper(paper);
        self.doc.touch();
        self.mark_all_dirty();
    }

    /// The wet preview as a live-stroke overlay on its layer (display only).
    pub(crate) fn wet_overlay(&self) -> Option<StrokeOverlay<'_>> {
        let s = self.wet.as_ref()?;
        Some(StrokeOverlay {
            layer_id: &s.layer,
            stroke: &s.preview,
            opacity: 1.0,
            mode: StrokeMode::Paint(BlendMode::Normal),
            mask: false,
        })
    }

    pub(crate) fn wet_begin(&mut self, first: StrokePoint) {
        let Some(mut fl) = self.brush.fluid() else { return };
        let Some(layer) = self.doc.layer(&self.active) else { return };
        if !layer.visible || layer.locked || layer.alpha_lock || layer.adjustment.is_some() {
            return;
        }
        // Opacity scales the pigment load and the re-wet scrub (PC:5689).
        let b = &self.brush;
        fl.pigment *= b.opacity;
        fl.rewet *= b.opacity;
        let (size, impasto) = (self.prefs.stroke_brush(b).size, b.impasto_depth());
        // Blow only pushes paint that's already wet on this layer, and adopts
        // its medium; anything else dries a session on another layer or medium.
        let here = self.wet.as_ref().is_some_and(|s| s.layer == self.active);
        if fl.pure_blow() && !here {
            return;
        }
        if self.wet.as_ref().is_some_and(|s| s.layer != self.active || (!fl.pure_blow() && s.sim.oil != fl.oil)) {
            self.dry_wet();
        }
        let doc = self.doc.bounds();
        if self.wet.is_none() {
            let r = size / 2.0 + 8.0;
            let seed = Rect::covering((first.x - r) as f32, (first.y - r) as f32, (first.x + r) as f32, (first.y + r) as f32);
            let mut sim = FluidSim::new(doc, seed, fl.oil);
            sim.space = self.doc.space();
            sim.set_tilt(self.wet_tilt[0], self.wet_tilt[1]);
            sim.set_paper(self.doc.paper().as_ref(), self.doc.dpi);
            let before = self.doc.layer(&self.active).map(|l| l.pixels.clone()).unwrap_or_default();
            self.wet = Some(WetSession {
                layer: self.active.clone(),
                sim,
                clock: WetClock::default(),
                before,
                height: None,
                bounds: [first.x, first.y, first.x, first.y],
                time_acc: 0.0,
                last_now: None,
                preview: Surface::new(),
            });
        }
        if impasto > 0.0
            && let Some(s) = self.wet.as_mut()
            && s.height.is_none()
        {
            let layer = s.layer.clone();
            let edit = self.height_begin(&layer);
            if let Some(s) = self.wet.as_mut() {
                s.height = edit;
            }
        }
        let lin = self.doc.space().pixel(self.color);
        let mut smoother = onepaint_stroke::Smoother::new(0.35);
        let mut sampler = onepaint_stroke::Sampler::new();
        let gap = (size * 0.06).max(1.5);
        let first = sampler.add(smoother.next(first), &mut |_, _| gap);
        self.wet_stroke = Some(WetStroke {
            size,
            fluid: fl,
            impasto,
            color: [lin[0], lin[1], lin[2]],
            sampler,
            smoother,
            transforms: self.symmetry_transforms(),
            last: None,
            travel: 0.0,
        });
        self.wet_deposit(&first);
    }

    pub(crate) fn wet_extend(&mut self, pts: &[StrokePoint]) {
        let Some(ws) = self.wet_stroke.as_mut() else { return };
        let gap = (ws.size * 0.06).max(1.5);
        let mut stamps = Vec::new();
        for p in pts {
            stamps.extend(ws.sampler.add(ws.smoother.next(*p), &mut |_, _| gap));
        }
        self.wet_deposit(&stamps);
    }

    /// No history at pointer-up: the session commits when it dries.
    pub(crate) fn wet_end(&mut self) {
        self.wet_stroke = None;
    }

    /// Stamps → water/pigment dabs (with symmetry), deposited into the sim.
    fn wet_deposit(&mut self, stamps: &[StrokePoint]) {
        let (Some(ws), Some(s)) = (self.wet_stroke.as_mut(), self.wet.as_mut()) else { return };
        if stamps.is_empty() {
            return;
        }
        let radius = ws.size / 2.0;
        let start = ws.last.unwrap_or((stamps[0].x, stamps[0].y));
        let mut batch_len = 0.0;
        let mut prev = start;
        for p in stamps {
            batch_len += (p.x - prev.0).hypot(p.y - prev.1);
            prev = (p.x, p.y);
        }
        // Blow amplifies the shove and lets it coast further.
        let blow = ws.fluid.blow;
        let speed = (fluid::DRAG_MAX * (1.0 + 2.0 * blow)).min(batch_len * fluid::DRAG_FRAC * (1.0 + 4.0 * blow));
        let (mut px, mut py) = start;
        let (mut dir_x, mut dir_y) = (0.0, 0.0);
        let mut dabs = Vec::with_capacity(stamps.len() * ws.transforms.len().max(1));
        let mut relief = Vec::new();
        let mut max_water: f64 = 0.0;
        for p in stamps {
            let pressure = p.pressure.clamp(0.0, 1.0);
            let (dx, dy) = (p.x - px, p.y - py);
            let dl = dx.hypot(dy);
            ws.travel += dl;
            let res = if ws.fluid.oil { 1.0 } else { fluid::reservoir(ws.travel, ws.size, ws.fluid.water) };
            let a = fluid::dab_amounts(&ws.fluid, pressure, res);
            let (dry, grain_thr) = (fluid::dry_brush(res), fluid::dry_grain_threshold(pressure, ws.fluid.water));
            max_water = max_water.max(a.water);
            if dl > 1e-3 {
                (dir_x, dir_y) = (dx / dl, dy / dl);
            }
            (px, py) = (p.x, p.y);
            let (vx, vy) = (dir_x * speed, dir_y * speed);
            let mut push = |x: f64, y: f64, vx: f64, vy: f64| {
                dabs.push(WetDab {
                    x,
                    y,
                    radius,
                    water: a.water,
                    pigment: a.pigment,
                    color: ws.color,
                    vx,
                    vy,
                    rewet: a.rewet,
                    dry,
                    grain_thr,
                    halo: ws.fluid.halo(),
                });
                if ws.impasto > 0.0 {
                    relief.push(HeightDab { x, y, radius, amount: ws.impasto * 0.12 * (0.4 + 0.6 * pressure) });
                }
                s.grow(x, y, radius * ws.fluid.halo());
            };
            if ws.transforms.is_empty() {
                push(p.x, p.y, vx, vy);
            } else {
                // Mirrored strokes shove their paint the mirrored way.
                for tf in &ws.transforms {
                    let (tx, ty) = tf.apply(p.x, p.y);
                    let (tx2, ty2) = tf.apply(p.x + vx, p.y + vy);
                    push(tx, ty, tx2 - tx, ty2 - ty);
                }
            }
        }
        ws.last = Some((px, py));
        let rewet = ws.fluid.rewet > 0.0;
        let layer_id = s.layer.clone();
        let layer = if rewet { self.doc.layer_mut(&layer_id).map(|l| &mut l.pixels) } else { None };
        let r = s.sim.deposit(&dabs, layer);
        s.clock.deposit_water(max_water);
        s.preview = s.sim.preview();
        let sim_rect = s.sim.rect();
        self.mark_dirty(r.union(&sim_rect));
        if !relief.is_empty() {
            self.height_stamp(&layer_id, &relief);
        }
    }

    /// Advance the simulation to `now_ms` in fixed 16 ms steps (at most 4
    /// per call; a stall drops the backlog). Returns whether it stepped.
    pub fn wet_tick(&mut self, now_ms: f64) -> bool {
        if self.stroke.is_some() || self.smudge.is_some() || self.liquify.is_some() || self.transform.is_some() {
            if let Some(s) = self.wet.as_mut() {
                s.last_now = Some(now_ms);
            }
            return false;
        }
        let Some(s) = self.wet.as_mut() else { return false };
        let Some(last) = s.last_now.replace(now_ms) else { return false };
        s.time_acc += now_ms - last;
        let steps = ((s.time_acc / fluid::SIM_DT_MS).floor() as i64).clamp(0, 4) as usize;
        s.time_acc = (s.time_acc - steps as f64 * fluid::SIM_DT_MS).min(fluid::SIM_DT_MS);
        if steps == 0 {
            return false;
        }
        self.rec(|t| recording::RecEvent::WetSteps { t, steps });
        self.wet_steps(steps);
        true
    }

    /// Run `n` fixed steps (live ticks and replay).
    pub(crate) fn wet_steps(&mut self, n: usize) {
        if n > 0 && self.wet_steps_on_backend(n) {
            return;
        }
        let doc = self.doc.bounds();
        for _ in 0..n {
            let Some(mut s) = self.wet.take() else { return };
            let Some(layer) = self.doc.layer_mut(&s.layer) else {
                return; // the layer is gone: nothing to settle into
            };
            // Diffusion reaches 2 px per step and flow up to V_MAX more.
            let b = &mut s.bounds;
            *b = [b[0] - 4.0, b[1] - 4.0, b[2] + 4.0, b[3] + 4.0];
            let mut dirty = Rect::default();
            if let Some(rect) = s.rect(doc) {
                dirty = s.sim.step(&mut layer.pixels, rect, false);
            }
            s.preview = s.sim.preview();
            let done = s.clock.step() == ClockState::Dry && self.wet_stroke.is_none() && !s.sim.oil;
            let sim_rect = s.sim.rect();
            self.wet = Some(s);
            self.mark_dirty(dirty.union(&sim_rect));
            // Live oils never dry on their own: only the Dry action ends them.
            if done {
                self.finish_wet();
                return;
            }
        }
    }

    /// Run a tick's steps as one batch on the installed backend (the GPU),
    /// stopping where the CPU loop would dry. `false` = no backend, or it
    /// declined: nothing changed, step on the CPU.
    fn wet_steps_on_backend(&mut self, n: usize) -> bool {
        let Some(mut be) = self.wet_backend.take() else { return false };
        let ran = self.wet_batch(be.as_mut(), n);
        self.wet_backend = Some(be);
        ran
    }

    fn wet_batch(&mut self, be: &mut dyn fluid::StepBackend, n: usize) -> bool {
        let doc = self.doc.bounds();
        let Some(s) = self.wet.as_ref() else { return false };
        let (mut clock, mut k, mut done) = (s.clock, 0, false);
        while k < n {
            k += 1;
            if clock.step() == ClockState::Dry && self.wet_stroke.is_none() && !s.sim.oil {
                done = true;
                break;
            }
        }
        let Some(mut s) = self.wet.take() else { return false };
        let grow = 4.0 * k as f64;
        let b = s.bounds;
        let bounds = [b[0] - grow, b[1] - grow, b[2] + grow, b[3] + grow];
        let rect = Rect::covering(bounds[0] as f32, bounds[1] as f32, bounds[2] as f32, bounds[3] as f32).intersect(&doc);
        let (Some(rect), Some(layer)) = (rect, self.doc.layer_mut(&s.layer)) else {
            self.wet = Some(s);
            return false;
        };
        let Some(dirty) = s.sim.step_with(be, &mut layer.pixels, rect, k) else {
            self.wet = Some(s);
            return false;
        };
        (s.bounds, s.clock) = (bounds, clock);
        s.preview = s.sim.preview();
        let sim_rect = s.sim.rect();
        self.wet = Some(s);
        self.mark_dirty(dirty.union(&sim_rect));
        if done {
            self.finish_wet();
        }
        true
    }

    /// Run the wet steps on `backend` (the GPU) when it can; `None` = CPU only.
    pub fn set_wet_backend(&mut self, backend: Option<Box<dyn fluid::StepBackend>>) {
        self.wet_backend = backend;
    }

    pub fn has_wet_backend(&self) -> bool {
        self.wet_backend.is_some()
    }

    /// Settle everything now and commit the session as one undo step.
    pub(crate) fn dry_wet(&mut self) {
        if self.wet.is_none() {
            return;
        }
        self.wet_stroke = None;
        self.finish_wet();
    }

    fn finish_wet(&mut self) {
        let Some(mut s) = self.wet.take() else { return };
        self.wet_stroke = None;
        let doc = self.doc.bounds();
        if let Some(layer) = self.doc.layer_mut(&s.layer) {
            if let Some(rect) = s.rect(doc) {
                s.sim.dry_now(&mut layer.pixels, rect);
            }
            layer.pixels.prune_empty();
            let mut keys: BTreeSet<(i32, i32)> = s.before.tile_keys().into_iter().collect();
            keys.extend(layer.pixels.tile_keys());
            let (tiles, bytes) = tile_diff(&s.before, &layer.pixels, keys);
            let paint = (!tiles.is_empty()).then(|| (Change::Tiles { layer: s.layer.clone(), plane: Plane::Pixels, tiles }, bytes));
            let relief = s.height.take().and_then(|h| self.height_end(h));
            self.push_with_height(paint, relief);
        }
        self.mark_dirty(s.sim.rect());
    }
}
