//! Smudge and mixer (wet) strokes on the CPU: a port of happypaint's
//! `smudge-renderer.ts` shaders and `paint-engine.ts` smudge loop (spec 16).

use crate::height::{HeightDab, WET_RATE};
use crate::symmetry::SymTransform;
use crate::tips::Tip;
use onepaint_color::{Space, pigment::pigment_mix};
use onepaint_raster::{Rect, Surface, TILE};
use onepaint_selection::Mask;
use onepaint_stroke::{Sampler, Smoother, StrokePoint};
use std::collections::HashMap;

pub const SMUDGE_STRENGTH: f64 = 0.4;
const LOAD_CAPACITY: f64 = 120.0;
const LOAD_FADE: f64 = 24.0;
const CONTAM_RATE: f64 = 0.18;

/// Mixer-brush knobs (Photoshop's Wet / Load / Mix).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WetParams {
    /// 0 = all reservoir paint, 1 = all canvas pickup.
    pub mix: f64,
    pub load: f64,
    pub wetness: f64,
    pub pigment: bool,
}

#[derive(Clone, Debug)]
pub struct SmudgeParams {
    /// Diameter in document px.
    pub size: f64,
    /// Master coverage (smudge strength, or the brush opacity for the mixer).
    pub strength: f64,
    pub hardness: f64,
    pub tip: Tip,
    pub size_pressure: f64,
    pub flow_pressure: f64,
    pub spacing: Option<f64>,
    pub wet: Option<WetParams>,
    /// Relief per dab at full deposit (`pigmentBody`; 0 = flat).
    pub impasto: f64,
    /// Blender: level the relief under each dab instead (`wet.blend`).
    pub blend: f64,
}

/// Straight linear reservoir colour plus how much paint it carries.
pub type Reservoir = [f32; 4];

pub struct Smudge {
    p: SmudgeParams,
    reservoir: Reservoir,
    sampler: Sampler,
    smoother: Smoother,
    gap: f64,
    prev: StrokePoint,
    budget: f64,
    contam: f64,
    tip_angle: f64,
    transforms: Vec<SymTransform>,
    doc: Rect,
    alpha_lock: bool,
    /// Last pickup (for contamination): its rect and premultiplied pixels.
    last_pickup: Option<(f64, f64, f64, Rect, Vec<f32>)>,
    /// Height and blender dabs since the last [`Smudge::take_height`].
    height_dabs: Vec<HeightDab>,
    blend_dabs: Vec<HeightDab>,
    /// The document's space: the pigment mix runs on sRGB values (spec 50).
    space: Space,
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn depth(amount: f64, pressure: f64) -> f64 {
    (1.0 - amount + amount * pressure.clamp(0.0, 1.0)).max(0.05)
}

/// Bilinear premultiplied sample at document position (x, y) from a region
/// read out of the layer; outside the region (and the document) is clear.
fn bilinear(buf: &[f32], r: Rect, x: f64, y: f64) -> [f32; 4] {
    let (u, v) = (x - 0.5 - r.x as f64, y - 0.5 - r.y as f64);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = ((u - x0) as f32, (v - y0) as f32);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let px = |xi: i32, yi: i32| -> [f32; 4] {
        if xi < 0 || yi < 0 || xi >= r.w || yi >= r.h {
            return [0.0; 4];
        }
        let i = ((yi * r.w + xi) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
    std::array::from_fn(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        top + (bot - top) * fy
    })
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

impl Smudge {
    /// Start a stroke. `color` is the reservoir paint (straight linear), or
    /// `None` for the pure smudge tool; `held` carries a kept reservoir over.
    pub fn new(
        p: SmudgeParams,
        color: Option<[f32; 3]>,
        held: Option<Reservoir>,
        transforms: Vec<SymTransform>,
        doc: Rect,
        alpha_lock: bool,
        first: StrokePoint,
    ) -> Self {
        let frac = p.spacing.map_or(0.25, |s| s.clamp(0.02, 0.25));
        let gap = (p.size * frac).max(1.0);
        let mut smoother = Smoother::new(0.3);
        let mut sampler = Sampler::new();
        let first = smoother.next(first);
        sampler.add(first, &mut |_, _| gap);
        let reservoir = held.unwrap_or_else(|| {
            let c = color.unwrap_or([0.0; 3]);
            [c[0], c[1], c[2], 1.0]
        });
        let (budget, contam) = match p.wet {
            Some(w) => (w.load * LOAD_CAPACITY, CONTAM_RATE * w.wetness.clamp(0.0, 1.0) * w.mix),
            None => (0.0, 0.0),
        };
        Self {
            p,
            reservoir,
            sampler,
            smoother,
            gap,
            prev: first,
            budget,
            contam,
            tip_angle: 0.0,
            transforms,
            doc,
            alpha_lock,
            last_pickup: None,
            height_dabs: Vec::new(),
            blend_dabs: Vec::new(),
            space: Space::Linear,
        }
    }

    /// Smudge in a document `space` (spec 50); the reservoir colour is a
    /// pixel colour in that space.
    pub fn set_space(&mut self, space: Space) {
        self.space = space;
    }

    pub fn reservoir(&self) -> Reservoir {
        self.reservoir
    }

    /// Feed one input point; smears into `layer` and returns the touched rect.
    pub fn add(&mut self, layer: &mut Surface, input: StrokePoint, clip: Option<&Mask>) -> Rect {
        let gap = self.gap;
        let stamps = self.sampler.add(self.smoother.next(input), &mut |_, _| gap);
        let mut dirty = Rect::default();
        let max_r = self.p.size / 2.0;
        for s in stamps {
            let deposit = if self.p.wet.is_some() { (self.budget / LOAD_FADE).clamp(0.0, 1.0) } else { 0.0 };
            let radius = max_r * depth(self.p.size_pressure, s.pressure);
            let strength = self.p.strength * depth(self.p.flow_pressure, s.pressure);
            for i in 0..self.transforms.len() {
                let tf = self.transforms[i];
                let (a, b) = (tf.apply(self.prev.x, self.prev.y), tf.apply(s.x, s.y));
                dirty = dirty.union(&self.smear(layer, a, b, deposit, radius, strength, clip));
                self.relief_dab(b, radius, deposit, strength);
            }
            if self.transforms.is_empty() {
                dirty = dirty.union(&self.smear(layer, (self.prev.x, self.prev.y), (s.x, s.y), deposit, radius, strength, clip));
                self.relief_dab((s.x, s.y), radius, deposit, strength);
            }
            if self.contam > 0.0 {
                self.contaminate();
            }
            if let Some(w) = self.p.wet {
                self.budget -= deposit * (1.0 - w.mix);
            }
            self.prev = s;
        }
        dirty
    }

    /// Impasto builds `impasto · 0.14 · deposit` per dab; a blender levels
    /// instead, by the dab's strength (PE:3010-3031).
    fn relief_dab(&mut self, at: (f64, f64), radius: f64, deposit: f64, strength: f64) {
        let d = HeightDab { x: at.0, y: at.1, radius, amount: 0.0 };
        if self.p.impasto > 0.0 && self.p.blend <= 0.0 {
            self.height_dabs.push(HeightDab { amount: self.p.impasto * WET_RATE * deposit, ..d });
        } else if self.p.blend > 0.0 {
            self.blend_dabs.push(HeightDab { amount: strength, ..d });
        }
    }

    /// Relief dabs (to stamp) and blender dabs (to smooth) since the last call.
    pub fn take_height(&mut self) -> (Vec<HeightDab>, Vec<HeightDab>) {
        (std::mem::take(&mut self.height_dabs), std::mem::take(&mut self.blend_dabs))
    }

    /// How much of the relief a blender dab takes off at full coverage.
    pub fn blend_flatten(&self) -> f64 {
        self.p.blend * crate::height::BLEND_FLATTEN
    }

    fn mixer(&self, a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
        if self.p.wet.is_some_and(|w| w.pigment) && t > 0.0 { pigment_mix(a, b, t) } else { mix3(a, b, t) }
    }

    #[allow(clippy::too_many_arguments)]
    fn smear(
        &mut self,
        layer: &mut Surface,
        from: (f64, f64),
        to: (f64, f64),
        deposit: f64,
        radius: f64,
        strength: f64,
        clip: Option<&Mask>,
    ) -> Rect {
        let radius = radius.max(0.5);
        // Pickup: the layer under `from`, before this dab.
        let reach = radius + 2.0;
        let pick_rect = Rect::covering((from.0 - reach) as f32, (from.1 - reach) as f32, (from.0 + reach) as f32, (from.1 + reach) as f32)
            .intersect(&self.doc);
        let pick = pick_rect.map(|r| (r, layer.read_rect(r)));
        if let Some((r, buf)) = &pick {
            self.last_pickup = Some((from.0, from.1, radius, *r, buf.clone()));
        } else {
            self.last_pickup = None;
        }
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        if dx * dx + dy * dy > 0.0625 {
            self.tip_angle = dy.atan2(dx);
        }
        let image_tip = matches!(self.p.tip, Tip::Image(_));
        let aa = (1.2 / radius.max(1.0)).min(0.5);
        let hard = if image_tip { 1.0 } else { self.p.hardness.clamp(0.0, 0.98) };
        let h = hard.min(1.0 - aa);
        let strength = strength.min(0.98);
        let (wet, mix) = self.p.wet.map_or((1.0, 1.0), |w| (w.wetness.clamp(0.0, 1.0), w.mix));
        let res_w = ((1.0 - mix) * deposit * self.reservoir[3] as f64) as f32;
        let rc = [self.reservoir[0], self.reservoir[1], self.reservoir[2]];
        let sp = self.space;
        let rc_w = rc.map(|v| sp.to_work(v));
        let (tc, ts) = (self.tip_angle.cos(), self.tip_angle.sin());
        let bounds = Rect::covering(
            (to.0 - radius - 1.0) as f32,
            (to.1 - radius - 1.0) as f32,
            (to.0 + radius + 1.0) as f32,
            (to.1 + radius + 1.0) as f32,
        );
        let Some(bounds) = bounds.intersect(&self.doc) else { return Rect::default() };
        let mut memo: HashMap<u32, [f32; 3]> = HashMap::new();
        let pigment = self.p.wet.is_some_and(|w| w.pigment) && res_w > 0.0;
        for (tx, ty) in bounds.tiles() {
            let Some(ix) = bounds.intersect(&Rect::from_tile(tx, ty)) else { continue };
            let tile = layer.tile_mut(tx, ty);
            for y in ix.y..ix.bottom() {
                let ly = (y as f64 + 0.5 - to.1) / radius;
                for x in ix.x..ix.right() {
                    let lx = (x as f64 + 0.5 - to.0) / radius;
                    let d = (lx * lx + ly * ly).sqrt();
                    if d >= 1.0 {
                        continue;
                    }
                    let mut cov = (1.0 - smoothstep(h, 1.0, d)) * strength;
                    if let Tip::Image(m) = &self.p.tip {
                        let (ux, uy) = (lx * tc + ly * ts, -lx * ts + ly * tc);
                        cov *= if ux.abs() >= 1.0 || uy.abs() >= 1.0 { 0.0 } else { m.sample(ux * 0.5 + 0.5, uy * 0.5 + 0.5) as f64 };
                    }
                    let mut cov = cov as f32;
                    if let Some(m) = clip {
                        cov *= m.coverage(x, y);
                    }
                    if cov <= 0.0 {
                        continue;
                    }
                    let p = pick.as_ref().map_or([0.0; 4], |(r, b)| bilinear(b, *r, x as f64 + 0.5 - dx, y as f64 + 0.5 - dy));
                    let pconf = smoothstep(0.002, 0.05, p[3] as f64) as f32;
                    let pa = p[3].max(0.002);
                    let pc = mix3(rc, [0, 1, 2].map(|i| (p[i] / pa).clamp(0.0, 1.0)), pconf);
                    let col = if res_w <= 0.0 {
                        pc
                    } else {
                        let pc_w = pc.map(|v| sp.to_work(v));
                        let w = if pigment {
                            let key = pc_w.iter().fold(0u32, |k, v| (k << 8) | (v.clamp(0.0, 1.0) * 255.0).round() as u32);
                            *memo.entry(key).or_insert_with(|| pigment_mix(pc_w, rc_w, res_w))
                        } else {
                            mix3(pc_w, rc_w, res_w)
                        };
                        w.map(|v| sp.from_work(v))
                    };
                    let al = p[3] * wet as f32 + (1.0 - p[3] * wet as f32) * res_w;
                    let k = al * cov;
                    let src = [col[0] * k, col[1] * k, col[2] * k, k];
                    let (lxi, lyi) = ((x - tx * TILE) as usize, (y - ty * TILE) as usize);
                    let dst = tile.get(lxi, lyi);
                    let out = if self.alpha_lock {
                        let da = dst[3];
                        [src[0] * da + dst[0] * (1.0 - k), src[1] * da + dst[1] * (1.0 - k), src[2] * da + dst[2] * (1.0 - k), da]
                    } else {
                        std::array::from_fn(|i| src[i] + dst[i] * (1.0 - k))
                    };
                    tile.set(lxi, lyi, out);
                }
            }
        }
        bounds
    }

    /// Drift the reservoir toward the last pickup's average colour.
    fn contaminate(&mut self) {
        let Some((fx, fy, r, rect, buf)) = &self.last_pickup else { return };
        let mut pa = [0.0f32; 4];
        for j in 0..4 {
            for i in 0..4 {
                let lx = (i as f64 + 0.5) / 4.0 * 2.0 - 1.0;
                let ly = (j as f64 + 0.5) / 4.0 * 2.0 - 1.0;
                let s = bilinear(buf, *rect, fx + lx * r, fy + ly * r);
                for k in 0..4 {
                    pa[k] += s[k] / 16.0;
                }
            }
        }
        let rate = self.contam.min(1.0);
        let pv = self.reservoir;
        let prev = [pv[0], pv[1], pv[2]];
        let got = pa[3] > 0.001;
        let avg = if got { [pa[0] / pa[3], pa[1] / pa[3], pa[2] / pa[3]] } else { prev };
        let rate = (1.0 + (rate - 1.0) * smoothstep(0.0, 0.05, pv[3] as f64)) as f32;
        let sp = self.space;
        let col = self.mixer(prev.map(|v| sp.to_work(v)), avg.map(|v| sp.to_work(v)), rate).map(|v| sp.from_work(v));
        let a = if got { pv[3] + (1.0 - pv[3]) * (self.contam as f32 * pa[3].min(1.0)) } else { pv[3] };
        self.reservoir = [col[0], col[1], col[2], a];
    }
}
