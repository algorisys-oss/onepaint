//! Gradient maths (happypaint `utils/gradient.ts`, `engine/renderer/gradient-ramp.ts`).

use onepaint_color::srgb_to_linear;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Shape {
    #[default]
    Linear,
    Radial,
    Angle,
    Reflected,
    Diamond,
}

impl Shape {
    pub const ALL: [Shape; 5] = [Shape::Linear, Shape::Radial, Shape::Angle, Shape::Reflected, Shape::Diamond];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Linear => "Linear",
            Shape::Radial => "Radial",
            Shape::Angle => "Angle",
            Shape::Reflected => "Reflected",
            Shape::Diamond => "Diamond",
        }
    }
}

/// Position along the gradient for point `p` with the drag from `a` to `b` (`shapeT`).
pub fn shape_t(shape: Shape, p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (p[0] - a[0], p[1] - a[1]);
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let len2 = (abx * abx + aby * aby).max(1e-4);
    let along = (dx * abx + dy * aby) / len2;
    let across = (dx * aby - dy * abx) / len2;
    let t = match shape {
        Shape::Radial => dx.hypot(dy) / len2.sqrt(),
        Shape::Reflected => along.abs(),
        Shape::Diamond => along.abs() + across.abs(),
        Shape::Angle => {
            if dx * dx + dy * dy < 1e-8 {
                return 0.0;
            }
            let turn = (aby.atan2(abx) - dy.atan2(dx)) / std::f64::consts::TAU;
            return turn - turn.floor();
        }
        Shape::Linear => along,
    };
    t.clamp(0.0, 1.0)
}

/// A resolved colour stop: straight sRGB colour, opacity, optional midpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradStop {
    pub pos: f64,
    pub color: [f64; 3],
    pub opacity: f64,
    pub mid: Option<f64>,
}

const MID_MIN: f64 = 0.05;
const MID_MAX: f64 = 0.95;

fn bend(f: f64, mid: Option<f64>) -> f64 {
    match mid {
        None => f,
        Some(m) if m == 0.5 || f <= 0.0 || f >= 1.0 => {
            let _ = m;
            f
        }
        Some(m) => {
            let m = m.clamp(MID_MIN, MID_MAX);
            if f <= m { 0.5 * f / m } else { 0.5 + 0.5 * (f - m) / (1.0 - m) }
        }
    }
}

/// Straight RGBA at `t` (stops sorted by `pos`).
pub fn sample_stops(stops: &[GradStop], t: f64) -> [f64; 4] {
    let (Some(first), Some(last)) = (stops.first(), stops.last()) else { return [t, t, t, 1.0] };
    let at = |s: &GradStop| [s.color[0], s.color[1], s.color[2], s.opacity];
    if t <= first.pos {
        return at(first);
    }
    if t >= last.pos {
        return at(last);
    }
    for w in stops.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if a.pos <= t && t <= b.pos {
            let f = bend(if b.pos == a.pos { 0.0 } else { (t - a.pos) / (b.pos - a.pos) }, a.mid);
            let (ca, cb) = (at(a), at(b));
            return [0, 1, 2, 3].map(|i| ca[i] + (cb[i] - ca[i]) * f);
        }
    }
    at(last)
}

/// Mirror the stops; each midpoint moves to the stop that now starts its segment.
pub fn reverse_stops(stops: &[GradStop]) -> Vec<GradStop> {
    let n = stops.len();
    (0..n)
        .map(|k| {
            let s = stops[n - 1 - k];
            let mid = if k + 1 < n { stops[n - 2 - k].mid.map(|m| 1.0 - m) } else { None };
            GradStop { pos: 1.0 - s.pos, mid, ..s }
        })
        .collect()
}

/// Foreground → transparent (the default preset).
pub fn fg_transparent(fg: [f64; 3]) -> Vec<GradStop> {
    vec![GradStop { pos: 0.0, color: fg, opacity: 1.0, mid: None }, GradStop { pos: 1.0, color: fg, opacity: 0.0, mid: None }]
}

pub fn fg_bg(fg: [f64; 3], bg: [f64; 3]) -> Vec<GradStop> {
    vec![GradStop { pos: 0.0, color: fg, opacity: 1.0, mid: None }, GradStop { pos: 1.0, color: bg, opacity: 1.0, mid: None }]
}

pub const RAMP_SIZE: usize = 1024;

/// 1024-entry ramp of straight linear RGB + opacity; stops are linearised first.
pub struct Ramp(Vec<[f32; 4]>);

impl Ramp {
    pub fn new(stops: &[GradStop]) -> Self {
        let lin: Vec<GradStop> = stops.iter().map(|s| GradStop { color: s.color.map(|c| srgb_to_linear(c as f32) as f64), ..*s }).collect();
        Ramp((0..RAMP_SIZE).map(|i| sample_stops(&lin, i as f64 / (RAMP_SIZE - 1) as f64).map(|v| v as f32)).collect())
    }

    /// The ramp's entries (for the GPU, spec 46).
    pub fn entries(&self) -> &[[f32; 4]] {
        &self.0
    }

    /// Manual lerp between entries (RGBA32F isn't filterable everywhere).
    pub fn sample(&self, t: f64) -> [f32; 4] {
        let x = t.clamp(0.0, 1.0) * (RAMP_SIZE - 1) as f64;
        let i0 = x.floor() as usize;
        let i1 = (i0 + 1).min(RAMP_SIZE - 1);
        let f = (x - i0 as f64) as f32;
        let (a, b) = (self.0[i0], self.0[i1]);
        [0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * f)
    }
}
