//! Symmetry transforms (happypaint `engine/brush/symmetry.ts`, spec 16).

use serde::{Deserialize, Serialize};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// A 2×2 linear map applied about a centre, in document px.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymTransform {
    pub m00: f64,
    pub m01: f64,
    pub m10: f64,
    pub m11: f64,
    pub cx: f64,
    pub cy: f64,
}

impl SymTransform {
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        (self.cx + self.m00 * dx + self.m01 * dy, self.cy + self.m10 * dx + self.m11 * dy)
    }

    /// Map a dab orientation through the linear part (handles mirroring).
    pub fn map_angle(&self, rot: f64) -> f64 {
        let (c, s) = (rot.cos(), rot.sin());
        (self.m10 * c + self.m11 * s).atan2(self.m00 * c + self.m01 * s)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymmetryMode {
    #[default]
    Off,
    Vertical,
    Horizontal,
    Both,
    Radial,
}

/// The guides-store symmetry settings: centre as document fractions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Symmetry {
    pub mode: SymmetryMode,
    pub cx: f64,
    pub cy: f64,
    pub radial_count: u32,
    /// Radians.
    pub angle: f64,
}

impl Default for Symmetry {
    fn default() -> Self {
        Self { mode: SymmetryMode::Off, cx: 0.5, cy: 0.5, radial_count: 6, angle: 0.0 }
    }
}

impl Symmetry {
    pub fn is_on(&self) -> bool {
        self.mode != SymmetryMode::Off
    }

    /// The transforms a stroke is replicated through, identity first.
    pub fn transforms(&self, width: u32, height: u32) -> Vec<SymTransform> {
        let (cx, cy) = (self.cx * width as f64, self.cy * height as f64);
        let rot = |a: f64| SymTransform { m00: a.cos(), m01: -a.sin(), m10: a.sin(), m11: a.cos(), cx, cy };
        let refl = |phi: f64| {
            let (c, s) = ((2.0 * phi).cos(), (2.0 * phi).sin());
            SymTransform { m00: c, m01: s, m10: s, m11: -c, cx, cy }
        };
        let id = SymTransform { m00: 1.0, m01: 0.0, m10: 0.0, m11: 1.0, cx, cy };
        let a = self.angle;
        match self.mode {
            SymmetryMode::Off => vec![id],
            SymmetryMode::Vertical => vec![id, refl(FRAC_PI_2 + a)],
            SymmetryMode::Horizontal => vec![id, refl(a)],
            SymmetryMode::Both => vec![id, refl(a), refl(FRAC_PI_2 + a), rot(PI)],
            SymmetryMode::Radial => {
                let n = self.radial_count.clamp(2, 24);
                (0..n).map(|i| rot(a + i as f64 / n as f64 * TAU)).collect()
            }
        }
    }
}
