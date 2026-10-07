//! Brush engine: dynamics, dab emission and CPU dab rasterisation.
//!
//! Ported from happypaint `engine/brush/brush-engine.ts` and
//! `stamp-renderer.ts`; see `docs/specs/03-brush-stroke.md`.

mod preset;

pub use preset::*;

use onepaint_raster::{Rect, Surface, TILE};
use onepaint_stroke::{
    Mulberry32, Sampler, SensorState, Smoother, Stabilizer, StrokePoint, eval_bindings,
};

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

pub fn curved_pressure(b: &BrushPreset, p: f64) -> f64 {
    if b.pressure_curve == 1.0 {
        clamp01(p)
    } else {
        clamp01(p).powf(b.pressure_curve)
    }
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
        Some(d) if !d.size.is_empty() => {
            b.size * eval_bindings(&d.size, state, d.size_combine).max(0.05) / 2.0
        }
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
    let state = SensorState {
        pressure: clamp01(p.pressure),
        angle: 0.5,
        random: 0.5,
        ..Default::default()
    };
    let r = if size_bindings(b).is_some() {
        base_radius(b, &state, cp)
    } else {
        radius_for(b, cp)
    };
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
}

impl StrokeEngine {
    pub fn new(brush: BrushPreset, color_linear: [f32; 3], seed: u32) -> Self {
        let stabilizer = brush
            .stabilizer
            .as_ref()
            .filter(|s| s.enabled && s.strength > 0.0)
            .map(|s| Stabilizer::new(s.strength));
        Self {
            smoother: Smoother::new(brush.smoothing),
            stabilizer,
            brush,
            color: color_linear,
            rng: Mulberry32::new(seed),
            sampler: Sampler::new(),
            prev_pos: None,
            dir: (1.0, 0.0),
            cum_dist: 0.0,
            speed_smooth: 0.0,
            last_t: None,
            index: 0,
            pending: Vec::new(),
        }
    }

    pub fn brush(&self) -> &BrushPreset {
        &self.brush
    }

    pub fn begin(&mut self, p: StrokePoint) -> Vec<Dab> {
        self.add(p)
    }

    pub fn add(&mut self, p: StrokePoint) -> Vec<Dab> {
        let pts = match &mut self.stabilizer {
            Some(s) => s.next(p),
            None => Some(p),
        };
        let Some(pt) = pts else { return Vec::new() };
        let sp = self.smoother.next(pt);
        let b = self.brush.clone();
        let stamps = self.sampler.add(sp, &mut |s, _| spacing_gap(&b, s));
        self.emit(&stamps)
    }

    pub fn end(&mut self) -> Vec<Dab> {
        let mut out = Vec::new();
        if let Some(fin) = self.stabilizer.as_mut().and_then(Stabilizer::finish) {
            let b = self.brush.clone();
            let stamps = self.sampler.add(fin, &mut |s, _| spacing_gap(&b, s));
            out.extend(self.emit(&stamps));
        }
        if !self.pending.is_empty() {
            let total = self.cum_dist;
            let degenerate = total <= 1.0;
            for (cd, mut d) in std::mem::take(&mut self.pending) {
                let f = if degenerate {
                    1.0
                } else {
                    self.entry_factor(cd) * self.exit_factor(cd, total)
                };
                d.radius *= f;
                out.push(d);
            }
        }
        out
    }

    fn taper(&self) -> (f64, f64) {
        self.brush
            .taper
            .as_ref()
            .map_or((0.0, 0.0), |t| (t.entry.max(0.0), t.exit.max(0.0)))
    }

    fn entry_factor(&self, cd: f64) -> f64 {
        let (e, _) = self.taper();
        if e > 0.0 { clamp01(cd / e) } else { 1.0 }
    }

    fn exit_factor(&self, cd: f64, total: f64) -> f64 {
        let (_, x) = self.taper();
        if x > 0.0 {
            clamp01((total - cd) / x)
        } else {
            1.0
        }
    }

    fn emit(&mut self, stamps: &[StrokePoint]) -> Vec<Dab> {
        let b = &self.brush;
        let mut made = Vec::with_capacity(stamps.len());
        for s in stamps {
            let dlen = self
                .prev_pos
                .map_or(0.0, |(px, py)| (s.x - px).hypot(s.y - py));
            if let Some((px, py)) = self.prev_pos
                && dlen > DIR_MIN_DIST
            {
                let (ux, uy) = ((s.x - px) / dlen, (s.y - py) / dlen);
                let (nx, ny) = (
                    self.dir.0 + (ux - self.dir.0) * DIR_FOLLOW,
                    self.dir.1 + (uy - self.dir.1) * DIR_FOLLOW,
                );
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
                Some(d) if !d.flow.is_empty() => {
                    b.flow * eval_bindings(&d.flow, &state, d.flow_combine)
                }
                _ => flow_for(b, cp),
            };
            let target = clamp01(base_alpha * (1.0 - b.speed_flow * speed));
            let alpha = if b.wash {
                target
            } else {
                linearize_alpha(target, b.spacing, b.opaque_linearize.unwrap_or(0.9))
            };
            let hardness = match &b.dynamics {
                Some(d) if !d.hardness.is_empty() => (b.hardness
                    * eval_bindings(&d.hardness, &state, d.hardness_combine))
                .clamp(0.0, 0.98),
                _ => b.hardness.min(0.98),
            };
            let mag = s.tilt_x.hypot(s.tilt_y);
            let (mut rotation, mut aspect) = if b.tilt_strength <= 0.0 || mag < 1e-4 {
                (0.0, 1.0)
            } else {
                (
                    s.tilt_y.atan2(s.tilt_x),
                    1.0 - mag.min(1.0) * b.tilt_strength * (1.0 - 0.3),
                )
            };
            let k = tip_roundness(b);
            let radius = radius * k.sqrt();
            aspect *= k;
            rotation += -b.tip_angle_deg.unwrap_or(0.0).to_radians();
            made.push((
                self.cum_dist,
                Dab {
                    x: s.x,
                    y: s.y,
                    radius,
                    alpha,
                    rotation,
                    aspect,
                    hardness,
                    color: self.color,
                },
            ));
        }
        let (entry, exit) = self.taper();
        if exit > 0.0 {
            self.pending.extend(made);
            let threshold = self.cum_dist - exit;
            let n = self
                .pending
                .iter()
                .take_while(|(cd, _)| *cd <= threshold)
                .count();
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

/// Bounding box of a dab in document px.
pub fn dab_bounds(d: &Dab) -> Rect {
    let r = d.radius / d.aspect.max(1e-3).sqrt() + 1.0;
    Rect::covering(
        (d.x - r) as f32,
        (d.y - r) as f32,
        (d.x + r) as f32,
        (d.y + r) as f32,
    )
}

/// Rasterise a round dab into the stroke buffer: premultiplied source-over,
/// or per-channel max when `wash`. Returns the touched rect.
pub fn rasterize_dab(s: &mut Surface, d: &Dab, wash: bool) -> Rect {
    let bounds = dab_bounds(d);
    if d.alpha <= 0.0 || d.radius <= 0.0 {
        return bounds;
    }
    let sc = d.aspect.max(1e-3).sqrt();
    let (ax, ay) = (d.radius / sc, d.radius * sc);
    let aa = (1.2 / (d.radius * sc).max(1.0)).clamp(0.0, 0.5);
    let inner = d.hardness.min(1.0 - aa);
    let (c, sn) = (d.rotation.cos(), d.rotation.sin());
    for (tx, ty) in bounds.tiles() {
        let tr = Rect::from_tile(tx, ty);
        let Some(ix) = bounds.intersect(&tr) else {
            continue;
        };
        let tile = s.tile_mut(tx, ty);
        for y in ix.y..ix.bottom() {
            let py = y as f64 + 0.5 - d.y;
            for x in ix.x..ix.right() {
                let px = x as f64 + 0.5 - d.x;
                // Undo the rotation, then normalise by the ellipse axes.
                let lx = (px * c + py * sn) / ax;
                let ly = (-px * sn + py * c) / ay;
                let dist = (lx * lx + ly * ly).sqrt();
                if dist >= 1.0 {
                    continue;
                }
                let a = ((1.0 - smoothstep(inner, 1.0, dist)) * d.alpha) as f32;
                if a <= 0.0 {
                    continue;
                }
                let (lxi, lyi) = ((x - tx * TILE) as usize, (y - ty * TILE) as usize);
                let dst = tile.get(lxi, lyi);
                let src = [d.color[0] * a, d.color[1] * a, d.color[2] * a, a];
                let out = if wash {
                    [
                        dst[0].max(src[0]),
                        dst[1].max(src[1]),
                        dst[2].max(src[2]),
                        dst[3].max(src[3]),
                    ]
                } else {
                    let k = 1.0 - a;
                    [
                        src[0] + dst[0] * k,
                        src[1] + dst[1] * k,
                        src[2] + dst[2] * k,
                        src[3] + dst[3] * k,
                    ]
                };
                tile.set(lxi, lyi, out);
            }
        }
    }
    bounds
}
