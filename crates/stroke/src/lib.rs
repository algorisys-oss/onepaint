//! Stroke geometry: input points, EMA smoothing, the pulled-string stabilizer,
//! the spacing sampler, response curves and the seeded RNG.
//!
//! Ported from happypaint `engine/stroke/{smoothing,stabilizer}.ts`,
//! `engine/brush/dynamics.ts` and `engine/util/rng.ts`; see `docs/specs/03-brush-stroke.md`.

pub mod bezier;
pub mod fisheye;
pub mod guide_lines;
pub mod shape;
pub mod snap;
pub mod speed_lines;

use serde::{Deserialize, Serialize};

/// One input sample in document pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StrokePoint {
    pub x: f64,
    pub y: f64,
    pub pressure: f64,
    pub tilt_x: f64,
    pub tilt_y: f64,
    /// Degrees, 0..360.
    pub twist: f64,
    /// Timestamp in ms, when known.
    pub t: Option<f64>,
}

impl StrokePoint {
    pub fn at(x: f64, y: f64, pressure: f64, t: f64) -> Self {
        Self { x, y, pressure, t: Some(t), ..Default::default() }
    }
}

fn lerp_angle_deg(a: f64, b: f64, t: f64) -> f64 {
    let d = ((b - a) % 360.0 + 540.0) % 360.0 - 180.0;
    ((a + d * t) % 360.0 + 360.0) % 360.0
}

const MAX_STAMPS_PER_SEGMENT: usize = 4096;

/// Gap to the next stamp, given the stamp and the unit travel direction (absent for the first).
pub type GapFn<'a> = dyn FnMut(&StrokePoint, Option<(f64, f64)>) -> f64 + 'a;

/// Places stamps at a (possibly varying) gap along the polyline, carrying the
/// remainder across segments. `gap(stamp, dir)` returns the distance to the next stamp.
#[derive(Default)]
pub struct Sampler {
    prev: Option<StrokePoint>,
    dist_to_next: f64,
}

impl Sampler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, p: StrokePoint, gap: &mut GapFn) -> Vec<StrokePoint> {
        let Some(prev) = self.prev else {
            self.prev = Some(p);
            self.dist_to_next = gap(&p, None).max(0.5);
            return vec![p];
        };
        let (dx, dy) = (p.x - prev.x, p.y - prev.y);
        let dist = dx.hypot(dy);
        if dist == 0.0 {
            return Vec::new();
        }
        let dir = (dx / dist, dy / dist);
        let (pt, td) = (prev.t.unwrap_or(0.0), p.t.unwrap_or(0.0) - prev.t.unwrap_or(0.0));
        let mut out = Vec::new();
        let mut acc = 0.0;
        let mut remaining = dist;
        while remaining >= self.dist_to_next && out.len() < MAX_STAMPS_PER_SEGMENT {
            acc += self.dist_to_next;
            remaining -= self.dist_to_next;
            let t = acc / dist;
            let s = StrokePoint {
                x: prev.x + dx * t,
                y: prev.y + dy * t,
                pressure: prev.pressure + (p.pressure - prev.pressure) * t,
                tilt_x: prev.tilt_x + (p.tilt_x - prev.tilt_x) * t,
                tilt_y: prev.tilt_y + (p.tilt_y - prev.tilt_y) * t,
                twist: lerp_angle_deg(prev.twist, p.twist, t),
                t: Some(pt + td * t),
            };
            out.push(s);
            self.dist_to_next = gap(&s, Some(dir)).max(0.5);
        }
        // Carry the leftover unfloored: the 0.5 px floor belongs on the gap
        // (above). Floored here, moves shorter than 0.5 px never ran it down,
        // so a slow stroke zoomed in drew dashes (happypaint v0.211.8). The
        // loop leaves `remaining < dist_to_next` unless the stamp cap cut it
        // short; then start the spacing afresh.
        self.dist_to_next = if remaining < self.dist_to_next { self.dist_to_next - remaining } else { gap(&p, Some(dir)).max(0.5) };
        self.prev = Some(p);
        out
    }
}

/// Time-based exponential smoothing (`smoothing.ts:19-58`).
pub struct Smoother {
    tau: f64,
    current: Option<StrokePoint>,
    last_t: Option<f64>,
}

impl Smoother {
    pub fn new(strength: f64) -> Self {
        Self { tau: strength.clamp(0.0, 1.0) * 60.0, current: None, last_t: None }
    }

    pub fn next(&mut self, p: StrokePoint) -> StrokePoint {
        let Some(c) = self.current else {
            self.current = Some(p);
            self.last_t = p.t;
            return p;
        };
        let mut dt = match (p.t, self.last_t) {
            (Some(t), Some(l)) => t - l,
            _ => 16.0,
        };
        if dt <= 0.0 {
            dt = 4.0;
        }
        self.last_t = p.t;
        let follow = if self.tau <= 0.0 { 1.0 } else { 1.0 - (-dt / self.tau).exp() };
        let tf = (follow * 2.0).min(1.0);
        let n = StrokePoint {
            x: c.x + (p.x - c.x) * follow,
            y: c.y + (p.y - c.y) * follow,
            pressure: c.pressure + (p.pressure - c.pressure) * tf,
            tilt_x: c.tilt_x + (p.tilt_x - c.tilt_x) * tf,
            tilt_y: c.tilt_y + (p.tilt_y - c.tilt_y) * tf,
            twist: 0.0,
            t: p.t,
        };
        self.current = Some(n);
        n
    }
}

/// Pulled-string stabilizer (`stabilizer.ts:33-79`).
pub struct Stabilizer {
    len: f64,
    anchor: Option<StrokePoint>,
    last: Option<StrokePoint>,
}

/// Max string length in SCREEN px, at full strength. Measured on screen, as
/// in Krita and Procreate: the leash is felt by the hand, so it must not grow
/// with zoom (happypaint v0.204.17).
pub const MAX_STRING_PX: f64 = 120.0;

impl Stabilizer {
    /// `view_scale` is the zoom (screen px per doc px) the stroke is drawn
    /// at; a bad one falls back to 100% rather than an endless string.
    pub fn new(strength: f64, view_scale: f64) -> Self {
        let z = if view_scale > 0.0 && view_scale.is_finite() { view_scale } else { 1.0 };
        Self { len: strength.clamp(0.0, 1.0) * MAX_STRING_PX / z, anchor: None, last: None }
    }

    pub fn next(&mut self, p: StrokePoint) -> Option<StrokePoint> {
        self.last = Some(p);
        let Some(a) = self.anchor else {
            self.anchor = Some(p);
            return Some(p);
        };
        let (dx, dy) = (p.x - a.x, p.y - a.y);
        let d = dx.hypot(dy);
        if d <= self.len || d == 0.0 {
            self.anchor = Some(StrokePoint { pressure: p.pressure, tilt_x: p.tilt_x, tilt_y: p.tilt_y, t: p.t, ..a });
            return None;
        }
        let m = (d - self.len) / d;
        let n =
            StrokePoint { x: a.x + dx * m, y: a.y + dy * m, pressure: p.pressure, tilt_x: p.tilt_x, tilt_y: p.tilt_y, twist: 0.0, t: p.t };
        self.anchor = Some(n);
        Some(n)
    }

    pub fn finish(&mut self) -> Option<StrokePoint> {
        let (a, l) = (self.anchor?, self.last?);
        if l.x == a.x && l.y == a.y {
            return None;
        }
        self.anchor = Some(l);
        Some(l)
    }
}

/// Mulberry32 (`engine/util/rng.ts`), bit-exact with the JS version.
#[derive(Clone, Debug)]
pub struct Mulberry32(u32);

impl Mulberry32 {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    pub fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let a = self.0;
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = (t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t))) ^ t;
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }
}

/// Box–Muller standard normal.
pub fn gaussian(r: &mut Mulberry32) -> f64 {
    let u1 = r.next_f64().max(1e-9);
    let u2 = r.next_f64();
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f64,
    pub y: f64,
}

impl CurvePoint {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Interp {
    #[default]
    Linear,
    Smooth,
}

fn monotone_tangents(p: &[CurvePoint]) -> Vec<f64> {
    let n = p.len();
    let sec: Vec<f64> = (0..n - 1)
        .map(|i| {
            let dx = p[i + 1].x - p[i].x;
            (p[i + 1].y - p[i].y) / if dx == 0.0 { 1.0 } else { dx }
        })
        .collect();
    let mut m = vec![0.0; n];
    m[0] = sec[0];
    m[n - 1] = sec[n - 2];
    for i in 1..n - 1 {
        m[i] = if sec[i - 1] * sec[i] <= 0.0 { 0.0 } else { (sec[i - 1] + sec[i]) / 2.0 };
    }
    for i in 0..n - 1 {
        let s = sec[i];
        if s == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
            continue;
        }
        let (a, b) = (m[i] / s, m[i + 1] / s);
        let h = a * a + b * b;
        if h > 9.0 {
            let t = 3.0 / h.sqrt();
            m[i] = t * a * s;
            m[i + 1] = t * b * s;
        }
    }
    m
}

const IDENTITY: [CurvePoint; 2] = [CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)];

/// Curve lookup, flat beyond the ends (`dynamics.ts:192-216`).
pub fn eval_curve(curve: &[CurvePoint], x: f64, interp: Interp) -> f64 {
    let pts = if curve.len() >= 2 { curve } else { &IDENTITY[..] };
    if x <= pts[0].x {
        return pts[0].y;
    }
    let last = pts[pts.len() - 1];
    if x >= last.x {
        return last.y;
    }
    for i in 1..pts.len() {
        if x <= pts[i].x {
            let (a, b) = (pts[i - 1], pts[i]);
            let span = if b.x - a.x == 0.0 { 1.0 } else { b.x - a.x };
            let t = (x - a.x) / span;
            if interp != Interp::Smooth {
                return a.y + (b.y - a.y) * t;
            }
            let m = monotone_tangents(pts);
            let (t2, t3) = (t * t, t * t * t);
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            return h00 * a.y + h10 * span * m[i - 1] + h01 * b.y + h11 * span * m[i];
        }
    }
    last.y
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sensor {
    #[default]
    Pressure,
    Speed,
    Tilt,
    Twist,
    Angle,
    Distance,
    Fade,
    Random,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SensorState {
    pub pressure: f64,
    pub speed: f64,
    pub tilt: f64,
    pub twist: f64,
    pub angle: f64,
    pub distance: f64,
    pub fade: f64,
    pub random: f64,
}

impl SensorState {
    pub fn value(&self, s: Sensor) -> f64 {
        match s {
            Sensor::Pressure => self.pressure,
            Sensor::Speed => self.speed,
            Sensor::Tilt => self.tilt,
            Sensor::Twist => self.twist,
            Sensor::Angle => self.angle,
            Sensor::Distance => self.distance,
            Sensor::Fade => self.fade,
            Sensor::Random => self.random,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    pub sensor: Sensor,
    pub curve: Vec<CurvePoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interp: Option<Interp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Combine {
    #[default]
    Multiply,
    Add,
    Max,
    Min,
}

fn binding_value(b: &Binding, s: &SensorState) -> f64 {
    let c = eval_curve(&b.curve, s.value(b.sensor), b.interp.unwrap_or_default());
    let (lo, hi) = (b.min.unwrap_or(0.0), b.max.unwrap_or(1.0));
    lo + (hi - lo) * c
}

/// Combined multiplier for a set of bindings; empty → 1 (`dynamics.ts:248-277`).
pub fn eval_bindings(bs: &[Binding], s: &SensorState, mode: Combine) -> f64 {
    match bs {
        [] => 1.0,
        [b] => binding_value(b, s),
        _ => {
            let it = bs.iter().map(|b| binding_value(b, s));
            match mode {
                Combine::Add => it.sum(),
                Combine::Max => it.fold(f64::NEG_INFINITY, f64::max),
                Combine::Min => it.fold(f64::INFINITY, f64::min),
                Combine::Multiply => it.product(),
            }
        }
    }
}
