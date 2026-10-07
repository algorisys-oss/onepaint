//! Guide-line, ruler and measure geometry (happypaint `state/guide-lines.ts`):
//! pure, in document space. A guide is a point plus an undirected angle.

use serde::{Deserialize, Serialize};

pub type Pt = [f64; 2];

/// Grab radius for a guide, in screen px.
pub const GUIDE_HIT_PX: f64 = 7.0;
/// Ruler bar thickness, screen px.
pub const RULER_PX: f64 = 18.0;
/// Snapped guide rotation step, degrees.
pub const ANGLE_SNAP_DEG: f64 = 15.0;
const MAJOR_TARGET_PX: f64 = 80.0;

/// A user-placed guide: an infinite line through (x, y) at `angle` radians.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GuideLine {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

impl GuideLine {
    pub fn new(id: impl Into<String>, x: f64, y: f64, angle: f64) -> Self {
        Self { id: id.into(), x, y, angle: normalize_angle(angle), locked: None }
    }

    pub fn is_locked(&self) -> bool {
        self.locked == Some(true)
    }

    pub fn direction(&self) -> Pt {
        [self.angle.cos(), self.angle.sin()]
    }

    /// Signed perpendicular distance (the sign is the side of the line).
    pub fn signed_distance(&self, p: Pt) -> f64 {
        let d = self.direction();
        d[0] * (p[1] - self.y) - d[1] * (p[0] - self.x)
    }

    pub fn project(&self, p: Pt) -> Pt {
        let d = self.direction();
        let t = (p[0] - self.x) * d[0] + (p[1] - self.y) * d[1];
        [self.x + d[0] * t, self.y + d[1] * t]
    }

    /// Does the infinite line cross the document? (The delete test.)
    pub fn intersects_doc(&self, width: f64, height: f64) -> bool {
        let (mut pos, mut neg) = (false, false);
        for c in [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]] {
            let s = self.signed_distance(c);
            if s > 0.0 {
                pos = true;
            } else if s < 0.0 {
                neg = true;
            } else {
                return true;
            }
        }
        pos && neg
    }
}

/// The unlocked guide nearest `p` within `tol` doc px.
pub fn nearest_guide(lines: &[GuideLine], p: Pt, tol: f64) -> Option<usize> {
    let mut best = None;
    let mut best_d = tol;
    for (i, g) in lines.iter().enumerate() {
        if g.is_locked() {
            continue;
        }
        let d = g.signed_distance(p).abs();
        if d <= best_d {
            best_d = d;
            best = Some(i);
        }
    }
    best
}

/// Pull `p` onto the nearest guide within `tol` (locked guides still snap).
pub fn snap_point_to_guides(lines: &[GuideLine], p: Pt, tol: f64) -> Pt {
    let mut best: Option<&GuideLine> = None;
    let mut best_d = tol;
    for g in lines {
        let d = g.signed_distance(p).abs();
        if d <= best_d {
            best_d = d;
            best = Some(g);
        }
    }
    best.map_or(p, |g| g.project(p))
}

// ── Units ──

/// Document px per display unit (`px`, `cm` or `in`).
pub fn unit_size(units: &str, dpi: f64) -> f64 {
    let d = if dpi > 0.0 { dpi } else { 300.0 };
    match units {
        "cm" => d / 2.54,
        "in" => d,
        _ => 1.0,
    }
}

pub fn format_length(px: f64, units: &str, dpi: f64) -> String {
    let v = px / unit_size(units, dpi);
    match units {
        "cm" | "in" => format!("{v:.2} {units}"),
        _ => format!("{} px", v.round()),
    }
}

/// Length (formatted and px) and angle (degrees, counter-clockwise, 0° right).
pub fn measure_between(a: Pt, b: Pt, units: &str, dpi: f64) -> (String, f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    let mut deg = -dy.atan2(dx).to_degrees();
    if deg == 0.0 {
        deg = 0.0;
    }
    (format_length(len, units, dpi), len, deg)
}

// ── Ruler ticks ──

#[derive(Clone, Debug, PartialEq)]
pub struct RulerTick {
    pub pos: f64,
    pub major: bool,
    pub label: Option<String>,
}

/// Round up to 1 / 2 / 5 × 10ⁿ.
pub fn nice_step(v: f64) -> f64 {
    if v.is_nan() || v <= 0.0 || v.is_infinite() {
        return 1.0;
    }
    let pow = 10f64.powf(v.log10().floor());
    let m = v / pow;
    let nice = if m <= 1.0 {
        1.0
    } else if m <= 2.0 {
        2.0
    } else if m <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * pow
}

/// (minor, major) tick spacing in doc px for the zoom `scale`.
pub fn tick_steps(scale: f64, units: &str, dpi: f64) -> (f64, f64) {
    let u = unit_size(units, dpi);
    let s = if scale > 0.0 { scale } else { 1.0 };
    let major = nice_step(MAJOR_TARGET_PX / (s * u)) * u;
    (major / 5.0, major)
}

/// Ticks over the doc-px span `[from, to]`, at most `max_ticks + 1`.
pub fn ruler_ticks_span(from: f64, to: f64, scale: f64, units: &str, dpi: f64, max_ticks: usize) -> Vec<RulerTick> {
    let length = to - from;
    if length.is_nan() || length < 0.0 || length.is_infinite() {
        return Vec::new();
    }
    let u = unit_size(units, dpi);
    let cap = max_ticks.max(1) as f64;
    let (mut minor, mut major) = tick_steps(scale, units, dpi);
    if length / major > cap {
        major = nice_step(length / cap);
        minor = major;
    }
    let step = if length / minor > cap { major } else { minor };
    let first = (from / step - 1e-6).ceil() as i64;
    let last = (to / step + 1e-6).floor() as i64;
    (first..=last)
        .map(|i| {
            let pos = i as f64 * step;
            let m = pos.rem_euclid(major);
            let is_major = m < major * 1e-6 || major - m < major * 1e-6;
            let label = is_major.then(|| {
                let v = pos / u;
                let r = if units == "cm" || units == "in" { (v * 100.0).round() / 100.0 } else { v.round() };
                let r = if r == 0.0 { 0.0 } else { r };
                format!("{r}")
            });
            RulerTick { pos, major: is_major, label }
        })
        .collect()
}

/// Rulers pin to the view edges while the canvas is unrotated.
pub fn rulers_pinned(rotation: f64) -> bool {
    rotation.sin().atan2(rotation.cos()).abs() < 1e-4
}

// ── Angles ──

/// Canonical guide angle in `[0, π)`.
pub fn normalize_angle(rad: f64) -> f64 {
    if !rad.is_finite() {
        return 0.0;
    }
    let pi = std::f64::consts::PI;
    let mut a = rad % pi;
    if a < 0.0 {
        a += pi;
    }
    if a >= pi - 1e-9 { 0.0 } else { a }
}

pub fn snap_angle(rad: f64, step_deg: f64) -> f64 {
    let step = step_deg.max(1e-6).to_radians();
    normalize_angle((rad / step).round() * step)
}

pub fn angle_through(a: Pt, b: Pt) -> f64 {
    normalize_angle((b[1] - a[1]).atan2(b[0] - a[0]))
}

/// Degrees for display (counter-clockwise, like the measure readout).
pub fn angle_deg(rad: f64) -> f64 {
    let d = (-normalize_angle(rad).to_degrees() + 180.0) % 180.0;
    if d == 0.0 { 0.0 } else { d }
}
