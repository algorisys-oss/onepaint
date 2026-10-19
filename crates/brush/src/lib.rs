//! Brush engine: dynamics, dab emission and CPU dab rasterisation.
//!
//! Ported from happypaint `engine/brush/brush-engine.ts` and
//! `stamp-renderer.ts`; see `docs/specs/03-brush-stroke.md`.

mod assets;
pub mod fluid;
pub mod height;
pub mod paper;
pub mod paper_field;
mod preset;
pub mod reservoir;
pub mod settle;
pub mod smudge;
pub mod studio;
pub mod symmetry;
pub mod tip_outline;
mod tips;
mod user;

pub use tips::{
    ColorTip, GrainSampler, Mask as TipMask, PaperLook, Tip, color_tip_for, grain_for, grain_names, grain_unit_mm, paper_look, tip_for,
    tip_ids,
};
pub use user::{
    CUSTOM_CATEGORY, CUSTOM_ID_PREFIX, IMPORTED_CATEGORY, USER_GRAIN_PREFIX, USER_TIP_PREFIX, UserGrain, UserLibrary, add_custom_brush,
    add_user_brushes, is_user_preset, remove_user_brush, rename_user_brush, set_user_library, update_user_brush, user_library,
    user_presets,
};

pub use preset::*;

use onepaint_raster::{Rect, Surface, TILE};
use onepaint_stroke::{Mulberry32, Sampler, SensorState, Smoother, Stabilizer, StrokePoint, eval_bindings};
use std::sync::Arc;

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

pub fn curved_pressure(b: &BrushPreset, p: f64) -> f64 {
    if b.pressure_curve == 1.0 { clamp01(p) } else { clamp01(p).powf(b.pressure_curve) }
}

/// Dab radius from curved pressure (legacy scalar path, 15% floor).
pub fn radius_for(b: &BrushPreset, cp: f64) -> f64 {
    (b.size * (1.0 - b.size_pressure + b.size_pressure * cp).max(0.15) / 2.0).max(0.5)
}

pub fn flow_for(b: &BrushPreset, cp: f64) -> f64 {
    clamp01(b.flow * (1.0 - b.flow_pressure + b.flow_pressure * cp))
}

fn size_bindings(b: &BrushPreset) -> Option<&onepaint_stroke::Binding> {
    b.dynamics.as_ref().and_then(|d| d.size.first())
}

fn base_radius(b: &BrushPreset, state: &SensorState, cp: f64) -> f64 {
    match &b.dynamics {
        Some(d) if !d.size.is_empty() => b.size * eval_bindings(&d.size, state, d.size_combine).max(0.05) / 2.0,
        _ => radius_for(b, cp),
    }
}

fn tip_roundness(b: &BrushPreset) -> f64 {
    match b.tip_roundness {
        Some(r) if r.is_finite() => r.clamp(0.05, 1.0),
        _ => 1.0,
    }
}

/// Distance to the next stamp: spacing × the dab's diameter (`brush-engine.ts:221-227, 758-779`).
pub fn spacing_gap(b: &BrushPreset, p: &StrokePoint) -> f64 {
    let cp = curved_pressure(b, p.pressure);
    let state = SensorState { pressure: clamp01(p.pressure), angle: 0.5, random: 0.5, ..Default::default() };
    let r = if size_bindings(b).is_some() { base_radius(b, &state, cp) } else { radius_for(b, cp) };
    b.spacing * 2.0 * r.max(0.5) * tip_roundness(b).sqrt()
}

/// Opacity linearisation so dense spacing doesn't pile up (`brush-engine.ts:787-800`).
pub fn linearize_alpha(a: f64, spacing: f64, lin: f64) -> f64 {
    if lin <= 0.0 || a >= 0.999 {
        return a;
    }
    let n = (1.0 / spacing.max(0.02)).max(1.0);
    let l = 1.0 - (1.0 - a).powf(1.0 / n);
    a + (l - a) * lin
}

/// One brush stamp in document pixels; colour is linear (not premultiplied).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dab {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub alpha: f64,
    pub rotation: f64,
    pub aspect: f64,
    pub hardness: f64,
    pub color: [f32; 3],
    /// Grain offset (doc px) when the grain moves with the brush (`grain.move`).
    pub grain_off: [f64; 2],
}

const SPEED_REF: f64 = 5.0;
const DIR_MIN_DIST: f64 = 0.25;
const DIR_FOLLOW: f64 = 0.5;

/// Turns input points into dabs for one stroke.
pub struct StrokeEngine {
    brush: BrushPreset,
    color: [f32; 3],
    rng: Mulberry32,
    smoother: Smoother,
    stabilizer: Option<Stabilizer>,
    sampler: Sampler,
    prev_pos: Option<(f64, f64)>,
    dir: (f64, f64),
    cum_dist: f64,
    speed_smooth: f64,
    last_t: Option<f64>,
    index: u32,
    pending: Vec<(f64, Dab)>,
    tip: Tip,
    grain: Option<GrainSampler>,
    /// Last raw input point and airbrush clock (ms, carried fraction).
    last_input: Option<StrokePoint>,
    air: Option<(f64, f64)>,
    /// Symmetry copies every dab is replicated through (empty = none).
    transforms: Vec<symmetry::SymTransform>,
    /// Grain movement (0..1) and the first dab's position it moves from.
    grain_move: f64,
    grain_origin: Option<(f64, f64)>,
    /// Dual brush and colour-tip rendering.
    mask_tip: Option<(Arc<TipMask>, f32)>,
    color_tip: Option<ColorTipMode>,
    /// The document's space (spec 50): colour dynamics and colour tips work
    /// on sRGB values, which 8-bit documents already hold.
    space: onepaint_color::Space,
}

/// How a colour (RGBA) tip colours its dabs (`stamp-renderer.ts:138-148, 247-268`).
pub struct ColorTipMode {
    pub tip: Arc<ColorTip>,
    /// Paint the tip's own colours (`useTipColor`), else tint the brush colour.
    pub baked: bool,
    /// HSV recolour: hue shift (turns), saturation and value gains.
    pub recolor: Option<[f32; 3]>,
}

/// Extra per-dab shaping beyond tip, grain and paper.
#[derive(Default, Clone, Copy)]
pub struct TipExtras<'a> {
    pub mask: Option<(&'a TipMask, f32)>,
    pub color: Option<&'a ColorTipMode>,
    /// The document's space: colour tips mix in sRGB (spec 50).
    pub space: onepaint_color::Space,
}

impl TipExtras<'_> {
    fn any(&self) -> bool {
        self.mask.is_some() || self.color.is_some()
    }
}

/// Spacing jitter: `gap · max(0.15, 1 + (r·2 − 1)·sj)`, one draw per call
/// (`brush-engine.ts:220-227`).
fn jittered_gap(b: &BrushPreset, s: &StrokePoint, rng: &mut Mulberry32) -> f64 {
    let gap = spacing_gap(b, s);
    match b.spacing_jitter.filter(|sj| *sj > 0.0) {
        Some(sj) => gap * (1.0 + (rng.next_f64() * 2.0 - 1.0) * sj.min(1.0)).max(0.15),
        None => gap,
    }
}

impl StrokeEngine {
    /// Paint in a document `space`: the colour given to `new` is a pixel
    /// colour in that space (spec 50).
    pub fn set_space(&mut self, space: onepaint_color::Space) {
        self.space = space;
    }

    pub fn new(brush: BrushPreset, color_linear: [f32; 3], seed: u32) -> Self {
        let stabilizer = brush.stabilizer.as_ref().filter(|s| s.enabled && s.strength > 0.0).map(|s| Stabilizer::new(s.strength));
        Self {
            smoother: Smoother::new(brush.smoothing),
            stabilizer,
            brush,
            color: color_linear,
            space: onepaint_color::Space::Linear,
            rng: Mulberry32::new(seed),
            sampler: Sampler::new(),
            prev_pos: None,
            dir: (1.0, 0.0),
            cum_dist: 0.0,
            speed_smooth: 0.0,
            last_t: None,
            index: 0,
            pending: Vec::new(),
            tip: Tip::Circle,
            grain: None,
            last_input: None,
            air: None,
            transforms: Vec::new(),
            grain_move: 0.0,
            grain_origin: None,
            mask_tip: None,
            color_tip: None,
        }
        .with_tip_and_grain()
        .with_extras()
    }

    /// The dual-brush mask and colour-tip mode for this brush's tip.
    fn with_extras(mut self) -> Self {
        if let Some((id, strength)) = self.brush.mask_tip()
            && let Tip::Image(m) = tip_for(&id)
        {
            self.mask_tip = Some((m, strength as f32));
        }
        if matches!(self.tip, Tip::Image(_))
            && let Some(tip) = color_tip_for(&self.brush.tip_id)
        {
            let recolor = self.brush.tip_recolor().map(|r| r.map(|v| v as f32));
            self.color_tip = Some(ColorTipMode { tip, baked: self.brush.use_tip_color(), recolor });
        }
        self
    }

    /// Resolve the tip and grain; grain-random draws come first, like happypaint.
    fn with_tip_and_grain(mut self) -> Self {
        self.tip = tip_for(&self.brush.tip_id);
        if let Some(g) = self.brush.grain.clone() {
            let rnd = g.random.unwrap_or(0.0);
            let (off, rot) = if rnd > 0.0 {
                let (x, y, r) = (self.rng.next_f64(), self.rng.next_f64(), self.rng.next_f64());
                ([x * g.scale * rnd, y * g.scale * rnd], r * std::f64::consts::TAU * rnd)
            } else {
                ([0.0, 0.0], 0.0)
            };
            self.grain = GrainSampler::new(&g, off, rot);
            if self.grain.is_some() && g.strength > 0.0 {
                self.grain_move = g.moves.unwrap_or(0.0).clamp(0.0, 1.0);
            }
        }
        self
    }

    /// Replicate every dab through these transforms (identity included).
    pub fn set_symmetry(&mut self, transforms: Vec<symmetry::SymTransform>) {
        self.transforms = if transforms.len() > 1 { transforms } else { Vec::new() };
    }

    fn replicate(&self, dabs: Vec<Dab>) -> Vec<Dab> {
        if self.transforms.is_empty() {
            return dabs;
        }
        let mut out = Vec::with_capacity(dabs.len() * self.transforms.len());
        for d in dabs {
            for tf in &self.transforms {
                let (x, y) = tf.apply(d.x, d.y);
                out.push(Dab { x, y, rotation: tf.map_angle(d.rotation), ..d });
            }
        }
        out
    }

    pub fn tip(&self) -> &Tip {
        &self.tip
    }

    pub fn grain(&self) -> Option<&GrainSampler> {
        self.grain.as_ref()
    }

    /// Rasterise one of this stroke's dabs with its tip and grain.
    pub fn rasterize(&self, s: &mut Surface, d: &Dab, clip: Option<&onepaint_selection::Mask>) -> Rect {
        rasterize_dab_full(s, d, &self.tip, clip, self.brush.wash, self.grain.as_ref(), None, self.extras())
    }

    fn extras(&self) -> TipExtras<'_> {
        TipExtras { mask: self.mask_tip.as_ref().map(|(m, k)| (&**m, *k)), color: self.color_tip.as_ref(), space: self.space }
    }

    /// [`StrokeEngine::rasterize`] on the document paper.
    pub fn rasterize_on_paper(
        &self,
        s: &mut Surface,
        d: &Dab,
        clip: Option<&onepaint_selection::Mask>,
        paper: Option<&paper::PaperTooth>,
    ) -> Rect {
        rasterize_dab_full(s, d, &self.tip, clip, self.brush.wash, self.grain.as_ref(), paper, self.extras())
    }

    /// Airbrush: spray `rate` dabs per second at the last input point while held.
    pub fn airbrush_tick(&mut self, now_ms: f64) -> Vec<Dab> {
        let Some(rate) = self.brush.airbrush.as_ref().map(|a| a.rate).filter(|r| *r > 0.0) else { return Vec::new() };
        let Some(p) = self.last_input else { return Vec::new() };
        let Some((last, acc)) = self.air else {
            self.air = Some((now_ms, 0.0));
            return Vec::new();
        };
        let acc = acc + (now_ms - last).max(0.0) / 1000.0 * rate;
        let n = acc.floor().min(64.0);
        self.air = Some((now_ms, acc - n));
        let pts = vec![StrokePoint { t: Some(now_ms), ..p }; n as usize];
        let d = self.emit(&pts);
        self.replicate(d)
    }

    pub fn brush(&self) -> &BrushPreset {
        &self.brush
    }

    pub fn begin(&mut self, p: StrokePoint) -> Vec<Dab> {
        self.add(p)
    }

    pub fn add(&mut self, p: StrokePoint) -> Vec<Dab> {
        self.last_input = Some(p);
        let pts = match &mut self.stabilizer {
            Some(s) => s.next(p),
            None => Some(p),
        };
        let Some(pt) = pts else { return Vec::new() };
        let sp = self.smoother.next(pt);
        let b = self.brush.clone();
        let rng = &mut self.rng;
        let stamps = self.sampler.add(sp, &mut |s, _| jittered_gap(&b, s, rng));
        let d = self.emit(&stamps);
        self.replicate(d)
    }

    pub fn end(&mut self) -> Vec<Dab> {
        let mut out = Vec::new();
        if let Some(fin) = self.stabilizer.as_mut().and_then(Stabilizer::finish) {
            let b = self.brush.clone();
            let rng = &mut self.rng;
            let stamps = self.sampler.add(fin, &mut |s, _| jittered_gap(&b, s, rng));
            out.extend(self.emit(&stamps));
        }
        if !self.pending.is_empty() {
            let total = self.cum_dist;
            let degenerate = total <= 1.0;
            for (cd, mut d) in std::mem::take(&mut self.pending) {
                let f = if degenerate { 1.0 } else { self.entry_factor(cd) * self.exit_factor(cd, total) };
                d.radius *= f;
                out.push(d);
            }
        }
        self.replicate(out)
    }

    fn taper(&self) -> (f64, f64) {
        self.brush.taper.as_ref().map_or((0.0, 0.0), |t| (t.entry.max(0.0), t.exit.max(0.0)))
    }

    fn entry_factor(&self, cd: f64) -> f64 {
        let (e, _) = self.taper();
        if e > 0.0 { clamp01(cd / e) } else { 1.0 }
    }

    fn exit_factor(&self, cd: f64, total: f64) -> f64 {
        let (_, x) = self.taper();
        if x > 0.0 { clamp01((total - cd) / x) } else { 1.0 }
    }

    fn emit(&mut self, stamps: &[StrokePoint]) -> Vec<Dab> {
        let b = &self.brush;
        let mut made = Vec::with_capacity(stamps.len());
        for s in stamps {
            let dlen = self.prev_pos.map_or(0.0, |(px, py)| (s.x - px).hypot(s.y - py));
            if let Some((px, py)) = self.prev_pos
                && dlen > DIR_MIN_DIST
            {
                let (ux, uy) = ((s.x - px) / dlen, (s.y - py) / dlen);
                let (nx, ny) = (self.dir.0 + (ux - self.dir.0) * DIR_FOLLOW, self.dir.1 + (uy - self.dir.1) * DIR_FOLLOW);
                let n = nx.hypot(ny);
                if n > 0.0 {
                    self.dir = (nx / n, ny / n);
                }
            }
            self.prev_pos = Some((s.x, s.y));
            self.cum_dist += dlen;
            if let (Some(t), Some(lt)) = (s.t, self.last_t) {
                let dt = t - lt;
                if dt > 0.0 {
                    self.speed_smooth += (dlen / dt - self.speed_smooth) * 0.3;
                }
            }
            if s.t.is_some() {
                self.last_t = s.t;
            }
            let speed = (self.speed_smooth / SPEED_REF).min(1.0);
            let tilt = s.tilt_x.hypot(s.tilt_y).min(1.0);
            let state = SensorState {
                pressure: clamp01(s.pressure),
                speed,
                tilt,
                twist: s.twist.rem_euclid(360.0) / 360.0,
                angle: self.dir.1.atan2(self.dir.0) / std::f64::consts::TAU + 0.5,
                distance: (self.cum_dist / (b.size * 4.0)).fract(),
                fade: (self.index as f64 / 50.0).min(1.0),
                random: self.rng.next_f64(),
            };
            self.index += 1;
            let cp = curved_pressure(b, s.pressure);
            let radius = (base_radius(b, &state, cp) * (1.0 - b.speed_size * speed)).max(0.5);
            let base_alpha = match &b.dynamics {
                Some(d) if !d.flow.is_empty() => b.flow * eval_bindings(&d.flow, &state, d.flow_combine),
                _ => flow_for(b, cp),
            };
            let target = clamp01(base_alpha * (1.0 - b.speed_flow * speed));
            let alpha = if b.wash { target } else { linearize_alpha(target, b.spacing, b.opaque_linearize.unwrap_or(0.9)) };
            let hardness = match &b.dynamics {
                Some(d) if !d.hardness.is_empty() => (b.hardness * eval_bindings(&d.hardness, &state, d.hardness_combine)).clamp(0.0, 0.98),
                _ => b.hardness.min(0.98),
            };
            let mag = s.tilt_x.hypot(s.tilt_y);
            let (mut rotation, mut aspect) = if b.tilt_strength <= 0.0 || mag < 1e-4 {
                (0.0, 1.0)
            } else {
                (s.tilt_y.atan2(s.tilt_x), 1.0 - mag.min(1.0) * b.tilt_strength * (1.0 - 0.3))
            };
            let dynamics = b.dynamics.as_ref();
            if let Some(d) = dynamics {
                if !d.ratio.is_empty() {
                    aspect = (aspect * eval_bindings(&d.ratio, &state, d.ratio_combine)).clamp(0.15, 1.0);
                }
                if let Some(r) = &d.rotation {
                    rotation = match r.source.as_str() {
                        "angle" => self.dir.1.atan2(self.dir.0),
                        "twist" => state.twist * std::f64::consts::TAU,
                        "none" => 0.0,
                        _ => rotation,
                    } + r.offset.unwrap_or(0.0).to_radians()
                        + (state.random * 2.0 - 1.0) * r.jitter.unwrap_or(0.0) * std::f64::consts::PI;
                }
                if !d.spin.is_empty() {
                    rotation += eval_bindings(&d.spin, &state, d.spin_combine) * std::f64::consts::TAU;
                }
            }
            let k = tip_roundness(b);
            let radius = radius * k.sqrt();
            aspect *= k;
            rotation += -b.tip_angle_deg.unwrap_or(0.0).to_radians();
            // Scatter (2 draws per axis), then colour jitter, in happypaint's draw order.
            let (mut x, mut y) = (s.x, s.y);
            if let Some(sc) = dynamics.and_then(|d| d.scatter).filter(|v| *v > 0.0) {
                let (sigma, lim) = (sc * b.size * 0.41, sc * b.size * 2.0);
                x += (onepaint_stroke::gaussian(&mut self.rng) * sigma).clamp(-lim, lim);
                y += (onepaint_stroke::gaussian(&mut self.rng) * sigma).clamp(-lim, lim);
            }
            let mut color = self.color;
            if let Some(cd) = dynamics.and_then(|d| d.color.as_ref()) {
                let space = self.space;
                let mut srgb = color.map(|v| space.to_srgb(v));
                // Two-colour load and hue shift follow fade (dab count) or pressure.
                let drive = if cd.hue_shift_source.as_deref() == Some("pressure") { state.pressure } else { state.fade };
                if let Some(to) = cd.to_color {
                    srgb = std::array::from_fn(|k| srgb[k] + (to[k] as f32 - srgb[k]) * drive as f32);
                }
                let (mut h, mut sat, mut val) = onepaint_color::Rgba::new(srgb[0], srgb[1], srgb[2], 1.0).to_hsv();
                h += ((self.rng.next_f64() * 2.0 - 1.0) * cd.hue.unwrap_or(0.0) * 360.0) as f32;
                if let Some(shift) = cd.hue_shift.filter(|v| *v != 0.0) {
                    h += (shift * 360.0 * drive) as f32;
                }
                sat = (sat + ((self.rng.next_f64() * 2.0 - 1.0) * cd.sat.unwrap_or(0.0)) as f32).clamp(0.0, 1.0);
                val = (val + ((self.rng.next_f64() * 2.0 - 1.0) * cd.val.unwrap_or(0.0)) as f32).clamp(0.0, 1.0);
                val *= (1.0 - cd.pressure_darken.unwrap_or(0.0) * state.pressure) as f32;
                let c = onepaint_color::Rgba::from_hsv(h, sat, val, 1.0);
                color = [c.r, c.g, c.b].map(|v| space.from_srgb(v));
            }
            // The grain rolls with the brush from the first dab (pre-scatter).
            let grain_off = if self.grain_move > 0.0 {
                let o = *self.grain_origin.get_or_insert((s.x, s.y));
                [(s.x - o.0) * self.grain_move, (s.y - o.1) * self.grain_move]
            } else {
                [0.0; 2]
            };
            made.push((self.cum_dist, Dab { x, y, radius, alpha, rotation, aspect, hardness, color, grain_off }));
        }
        let (entry, exit) = self.taper();
        if exit > 0.0 {
            self.pending.extend(made);
            let threshold = self.cum_dist - exit;
            let n = self.pending.iter().take_while(|(cd, _)| *cd <= threshold).count();
            let ready: Vec<_> = self.pending.drain(..n).collect();
            return ready
                .into_iter()
                .map(|(cd, mut d)| {
                    d.radius *= self.entry_factor(cd);
                    d
                })
                .collect();
        }
        made.into_iter()
            .map(|(cd, mut d)| {
                if entry > 0.0 {
                    d.radius *= self.entry_factor(cd);
                }
                d
            })
            .collect()
    }
}

#[inline]
fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bounding box of a dab in document px (square/image tips reach √2 further).
pub fn dab_bounds(d: &Dab) -> Rect {
    let r = d.radius / d.aspect.max(1e-3).sqrt() * std::f64::consts::SQRT_2 + 1.0;
    Rect::covering((d.x - r) as f32, (d.y - r) as f32, (d.x + r) as f32, (d.y + r) as f32)
}

/// Rasterise a round dab into the stroke buffer: premultiplied source-over,
/// or per-channel max when `wash`. Coverage is multiplied by `clip` (the
/// selection) when given. Returns the touched rect.
pub fn rasterize_dab(s: &mut Surface, d: &Dab, wash: bool, clip: Option<&onepaint_selection::Mask>) -> Rect {
    rasterize_dab_tip(s, d, &Tip::Circle, clip, wash, None)
}

/// Rasterise a dab with any tip (stamp-renderer.ts:130-165) and optional grain.
pub fn rasterize_dab_tip(
    s: &mut Surface,
    d: &Dab,
    tip: &Tip,
    clip: Option<&onepaint_selection::Mask>,
    wash: bool,
    grain: Option<&GrainSampler>,
) -> Rect {
    rasterize_dab_paper(s, d, tip, clip, wash, grain, None)
}

/// [`rasterize_dab_tip`] on the document paper: the tooth bites after the grain.
pub fn rasterize_dab_paper(
    s: &mut Surface,
    d: &Dab,
    tip: &Tip,
    clip: Option<&onepaint_selection::Mask>,
    wash: bool,
    grain: Option<&GrainSampler>,
    paper: Option<&paper::PaperTooth>,
) -> Rect {
    rasterize_dab_full(s, d, tip, clip, wash, grain, paper, TipExtras::default())
}

/// The full stamp: tip, dual-brush mask, colour tip, grain, paper and selection.
#[allow(clippy::too_many_arguments)]
pub fn rasterize_dab_full(
    s: &mut Surface,
    d: &Dab,
    tip: &Tip,
    clip: Option<&onepaint_selection::Mask>,
    wash: bool,
    grain: Option<&GrainSampler>,
    paper: Option<&paper::PaperTooth>,
    extras: TipExtras,
) -> Rect {
    // Monomorphised so the common path carries no tooth or extras checks.
    match (paper.is_some(), extras.any()) {
        (false, false) => rasterize_impl::<false, false>(s, d, tip, clip, wash, grain, None, extras),
        (true, false) => rasterize_impl::<true, false>(s, d, tip, clip, wash, grain, paper, extras),
        (false, true) => rasterize_impl::<false, true>(s, d, tip, clip, wash, grain, None, extras),
        (true, true) => rasterize_impl::<true, true>(s, d, tip, clip, wash, grain, paper, extras),
    }
}

/// A colour tip's colour for one pixel (sRGB in, linear out): baked tips
/// paint their own colour, others tint the brush colour by the tip's
/// relative luma and chroma (`stamp-renderer.ts:247-268`).
#[inline]
fn color_tip_rgb(mode: &ColorTipMode, u: f64, v: f64, brush_srgb: [f32; 3], brush_sat: f32, space: onepaint_color::Space) -> [f32; 3] {
    let mut t = mode.tip.sample(u, v);
    if let Some([hue, sat, val]) = mode.recolor {
        let (h, s, vv) = onepaint_color::Rgba::new(t[0], t[1], t[2], 1.0).to_hsv();
        let c = onepaint_color::Rgba::from_hsv(h + hue * 360.0, (s * sat).clamp(0.0, 1.0), (vv * val).clamp(0.0, 1.0), 1.0);
        t = [c.r, c.g, c.b];
    }
    const VARIATION: f32 = 0.85;
    let base = if mode.baked {
        t
    } else {
        let l = 0.299 * t[0] + 0.587 * t[1] + 0.114 * t[2];
        let rel: f32 = (l / mode.tip.lum.max(0.05)).clamp(0.0, 2.0);
        let chroma = VARIATION * 0.5 * (0.25 + 0.75 * brush_sat);
        std::array::from_fn(|k| (brush_srgb[k] * (1.0 + (rel - 1.0) * VARIATION) + (t[k] - l) * chroma).clamp(0.0, 1.0))
    };
    base.map(|c| space.from_work(c))
}

#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn rasterize_impl<const PAPER: bool, const EXTRA: bool>(
    s: &mut Surface,
    d: &Dab,
    tip: &Tip,
    clip: Option<&onepaint_selection::Mask>,
    wash: bool,
    grain: Option<&GrainSampler>,
    paper: Option<&paper::PaperTooth>,
    extras: TipExtras,
) -> Rect {
    let bounds = dab_bounds(d);
    if d.alpha <= 0.0 || d.radius <= 0.0 {
        return bounds;
    }
    let sc = d.aspect.max(1e-3).sqrt();
    let (ax, ay) = (d.radius / sc, d.radius * sc);
    let aa = (1.2 / (d.radius * sc).max(1.0)).clamp(0.0, 0.5);
    let inner = d.hardness.min(1.0 - aa);
    let (c, sn) = (d.rotation.cos(), d.rotation.sin());
    // The brush colour as the colour-tip formula sees it (sRGB, and its saturation).
    let (brush_srgb, brush_sat) = if EXTRA && extras.color.is_some() {
        let v = d.color.map(|c| extras.space.to_work(c));
        let (mx, mn) = (v[0].max(v[1]).max(v[2]), v[0].min(v[1]).min(v[2]));
        (v, (mx - mn) / mx.max(1e-4))
    } else {
        ([0.0; 3], 0.0)
    };
    for (tx, ty) in bounds.tiles() {
        let tr = Rect::from_tile(tx, ty);
        let Some(ix) = bounds.intersect(&tr) else {
            continue;
        };
        let tile = s.tile_mut(tx, ty);
        let tooth = if PAPER { paper.map(|p| p.tile(tx, ty)) } else { None };
        for y in ix.y..ix.bottom() {
            let py = y as f64 + 0.5 - d.y;
            for x in ix.x..ix.right() {
                let px = x as f64 + 0.5 - d.x;
                // Undo the rotation, then normalise by the ellipse axes.
                let lx = (px * c + py * sn) / ax;
                let ly = (-px * sn + py * c) / ay;
                let m = match tip {
                    Tip::Circle => {
                        let dist = (lx * lx + ly * ly).sqrt();
                        if dist >= 1.0 { 0.0 } else { 1.0 - smoothstep(inner, 1.0, dist) }
                    }
                    Tip::Gauss => {
                        let d2 = lx * lx + ly * ly;
                        if d2 >= 1.0 { 0.0 } else { (-d2 * (2.0 + (10.0 - 2.0) * d.hardness)).exp() }
                    }
                    Tip::Square => {
                        let e = lx.abs().max(ly.abs());
                        if e >= 1.0 { 0.0 } else { 1.0 - smoothstep(inner, 1.0, e) }
                    }
                    Tip::Image(mask) => {
                        if lx.abs() >= 1.0 || ly.abs() >= 1.0 {
                            0.0
                        } else {
                            mask.sample(lx * 0.5 + 0.5, ly * 0.5 + 0.5) as f64
                        }
                    }
                };
                if m <= 0.0 {
                    continue;
                }
                let mut a = (m * d.alpha) as f32;
                // Dual brush: the second tip carves the dab in the same frame.
                if EXTRA && let Some((mt, k)) = extras.mask {
                    let md = if lx.abs() >= 1.0 || ly.abs() >= 1.0 { 0.0 } else { mt.sample(lx * 0.5 + 0.5, ly * 0.5 + 0.5) };
                    a *= 1.0 + (md - 1.0) * k;
                }
                if let Some(g) = grain {
                    a *= g.factor(x as f64 + 0.5 - d.grain_off[0], y as f64 + 0.5 - d.grain_off[1]);
                }
                if PAPER && let (Some(p), Some(t)) = (paper, &tooth) {
                    a *= p.factor(t, x, y, (x - tx * TILE) as usize, (y - ty * TILE) as usize);
                }
                if let Some(m) = clip {
                    a *= m.coverage(x, y);
                }
                if a <= 0.0 {
                    continue;
                }
                let (lxi, lyi) = ((x - tx * TILE) as usize, (y - ty * TILE) as usize);
                let dst = tile.get(lxi, lyi);
                let col = match extras.color.filter(|_| EXTRA) {
                    Some(mode) => color_tip_rgb(mode, lx * 0.5 + 0.5, ly * 0.5 + 0.5, brush_srgb, brush_sat, extras.space),
                    None => d.color,
                };
                let src = [col[0] * a, col[1] * a, col[2] * a, a];
                let out = if wash {
                    [dst[0].max(src[0]), dst[1].max(src[1]), dst[2].max(src[2]), dst[3].max(src[3])]
                } else {
                    let k = 1.0 - a;
                    [src[0] + dst[0] * k, src[1] + dst[1] * k, src[2] + dst[2] * k, src[3] + dst[3] * k]
                };
                tile.set(lxi, lyi, out);
            }
        }
    }
    bounds
}
