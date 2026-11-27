//! The app-wide pen pressure response (happypaint `input/pen-pressure-curve.ts`,
//! v0.212.0), applied once at the input boundary before any brush sees the
//! value. Each brush's own pressure curve still shapes the result after this.
//!
//! Two numbers, as in Krita and Clip Studio:
//! - `full`: the raw pen pressure that already counts as full pressure. A pen
//!   that reports only ~0.4 under a normal hand sets this to ~0.4, so the
//!   artist no longer has to bear down to get a full-width line.
//! - `gamma`: the exponent on the rescaled value. Below 1 is soft, above 1 firm.
//!
//! The default (`gamma` 0.7, `full` 1) is the fixed curve every pen had
//! before the setting existed.

use serde::{Deserialize, Serialize};

pub const PEN_GAMMA_MIN: f64 = 1.0 / 3.0;
pub const PEN_GAMMA_MAX: f64 = 3.0;
pub const PEN_FULL_MIN: f64 = 0.2;
/// Raw samples the calibration pad needs before it can suggest a curve.
pub const MIN_CALIBRATION_SAMPLES: usize = 60;
/// Lightest output: keeps a feather-light touch from rendering sub-pixel.
const RAW_FLOOR: f64 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PenCurve {
    pub gamma: f64,
    pub full: f64,
}

impl Default for PenCurve {
    fn default() -> Self {
        Self { gamma: 0.7, full: 1.0 }
    }
}

impl PenCurve {
    /// Raw pen pressure (0..1) → normalised pressure (0..1).
    pub fn apply(&self, raw: f64) -> f64 {
        let c = self.coerced();
        let r = if raw.is_finite() { raw } else { 0.0 };
        (r / c.full).clamp(RAW_FLOOR, 1.0).powf(c.gamma)
    }

    /// A stored value made usable: non-finite fields take the default, the
    /// rest are clamped to the allowed range (`coercePenCurve`).
    pub fn coerced(&self) -> Self {
        let d = Self::default();
        Self {
            gamma: if self.gamma.is_finite() { self.gamma.clamp(PEN_GAMMA_MIN, PEN_GAMMA_MAX) } else { d.gamma },
            full: if self.full.is_finite() { self.full.clamp(PEN_FULL_MIN, 1.0) } else { d.full },
        }
    }

    /// Raw pen samples drawn "at your normal pressure" → a curve fitted to
    /// them: the 95th percentile becomes full pressure (an occasional hard
    /// press still saturates; one spike does not set the scale), and the
    /// exponent puts the median at 0.5 output. Zero samples (hover, lift) are
    /// dropped. `None` until there are enough samples.
    pub fn calibrate(samples: &[f64]) -> Option<Self> {
        let mut s: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite() && *v > 0.005).collect();
        if s.len() < MIN_CALIBRATION_SAMPLES {
            return None;
        }
        s.sort_by(f64::total_cmp);
        let at = |q: f64| s[(q * (s.len() - 1) as f64).floor() as usize];
        let full = at(0.95).clamp(PEN_FULL_MIN, 1.0);
        let median = (at(0.5) / full).clamp(RAW_FLOOR, 0.999);
        let gamma = (0.5f64.ln() / median.ln()).clamp(PEN_GAMMA_MIN, PEN_GAMMA_MAX);
        Some(Self { gamma: round2(gamma), full: round2(full) })
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
