//! Per-dab amounts for the live wet brushes and the CPU dryness clock
//! (happypaint `engine/wet/wet-dab.ts`, `WetClock` in `wet-renderer.ts`).
//!
//! A stroke starts fully loaded and empties exponentially with the distance
//! travelled, down to a floor. Water runs out first; then the brush turns dry
//! and, on textured paper, only touches the ridges.

use serde::{Deserialize, Serialize};

/// Travel, in brush diameters per unit of `0.5 + water`, to ~1/e of the load.
const RESERVOIR_DIAMETERS: f64 = 20.0;
/// A brush never runs completely dry.
const RESERVOIR_FLOOR: f64 = 0.25;
/// The water level a full dab sets the paper to (MAX-deposited, so a level).
const WATER_LEVEL: f64 = 3.0;
/// Water clamp, shared with the sim.
pub const WATER_MAX: f64 = 2.5;
/// Water lost per step.
pub const EVAP: f64 = 0.003;
/// Steps of pure settling after the water bound reaches zero.
pub const TAIL_STEPS: u32 = 90;

/// A brush's `fluid` block (`brush-presets.ts:217-234`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Fluid {
    /// Water load, 0..2.
    pub water: f64,
    /// Pigment load, 0..1.
    pub pigment: f64,
    /// Re-wet: lift dried paint back into suspension, 0..1.
    pub rewet: f64,
    /// Air: push wet paint without adding any, 0..1.
    pub blow: f64,
    /// Live oils: viscous, non-drying, no blooms.
    pub oil: bool,
    /// Water reaches this many radii (1..3; absent = 1): a wet halo
    /// around the pigment (happypaint v0.199.2).
    #[serde(skip_serializing_if = "is_no_halo")]
    pub halo: f64,
}

fn is_no_halo(h: &f64) -> bool {
    *h <= 1.0
}

impl Fluid {
    /// The halo's reach in radii, 1..3 (`clamp(halo ?? 1, 1, 3)`).
    pub fn halo(&self) -> f64 {
        if self.halo.is_finite() { self.halo.clamp(1.0, 3.0) } else { 1.0 }
    }

    /// Blow alone carries nothing: it only pushes paint that is already wet.
    pub fn pure_blow(&self) -> bool {
        self.blow > 0.0 && self.water <= 0.0 && self.pigment <= 0.0 && self.rewet <= 0.0
    }
}

/// How full the brush still is (FLOOR..1) after `travel` px of stroke.
pub fn reservoir(travel: f64, size: f64, water: f64) -> f64 {
    let tau = size.max(1.0) * RESERVOIR_DIAMETERS * (0.5 + water.max(0.0));
    RESERVOIR_FLOOR + (1.0 - RESERVOIR_FLOOR) * (-travel.max(0.0) / tau).exp()
}

/// Dry-brush strength: off above 0.55 of the load, full at 0.35.
pub fn dry_brush(reservoir: f64) -> f64 {
    ((0.55 - reservoir) / 0.2).clamp(0.0, 1.0)
}

/// Paper height a drying dab must reach (firm pressure reaches lower).
pub fn dry_grain_threshold(pressure: f64, water: f64) -> f64 {
    0.8 - 0.3 * pressure.clamp(0.0, 1.0) - 0.05 * water.clamp(0.0, 1.0)
}

/// Water level, pigment load and re-wet lift for one dab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DabAmounts {
    pub water: f64,
    pub pigment: f64,
    pub rewet: f64,
}

pub fn dab_amounts(f: &Fluid, pressure: f64, reservoir: f64) -> DabAmounts {
    let p = pressure.clamp(0.0, 1.0);
    let r = if f.oil { 1.0 } else { reservoir };
    // Water stays full while the brush is ≥ 3/4 loaded, gone at 0.45.
    let wet = ((r - 0.45) / 0.3).clamp(0.0, 1.0);
    DabAmounts {
        water: (f.water * (0.45 + 0.55 * p) * WATER_LEVEL * wet).min(WATER_MAX),
        pigment: f.pigment * (0.3 + 0.7 * p) * 0.55 * r,
        rewet: f.rewet * (0.35 + 0.65 * p) * 0.08,
    }
}

/// Dryness bookkeeping from counts, never readbacks: deterministic.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WetClock {
    /// Upper bound of per-pixel water.
    pub level: f64,
    /// Post-water settling countdown.
    pub tail: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockState {
    Wet,
    Tail,
    Dry,
}

impl WetClock {
    /// Dabs SET water levels (MAX), so the bound is the largest deposited.
    pub fn deposit_water(&mut self, amount: f64) {
        self.level = self.level.max(amount).min(WATER_MAX);
        self.tail = TAIL_STEPS;
    }

    pub fn step(&mut self) -> ClockState {
        if self.level > 0.0 {
            self.level = (self.level - EVAP).max(0.0);
            return ClockState::Wet;
        }
        if self.tail > 0 {
            self.tail -= 1;
            return if self.tail == 0 { ClockState::Dry } else { ClockState::Tail };
        }
        ClockState::Dry
    }

    pub fn wet(&self) -> bool {
        self.level > 0.0 || self.tail > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn reservoir_empties_with_travel() {
        assert!(close(reservoir(0.0, 44.0, 1.0), 1.0, 1e-6));
        let (a, b) = (reservoir(500.0, 44.0, 1.0), reservoir(2000.0, 44.0, 1.0));
        assert!(a < 1.0 && b < a);
        assert!(close(reservoir(1e9, 44.0, 1.0), 0.25, 1e-6));
        assert!(close(reservoir(440.0, 44.0, 1.0), reservoir(880.0, 88.0, 1.0), 1e-9));
        assert!(reservoir(800.0, 44.0, 1.6) > reservoir(800.0, 44.0, 1.0));
        assert!(close(reservoir(30.0 * 44.0, 44.0, 1.0), 0.25 + 0.75 / std::f64::consts::E, 1e-6));
        for v in [reservoir(-5.0, 0.0, -1.0), reservoir(100.0, 0.0, 0.0)] {
            assert!(v.is_finite() && (0.25..=1.0).contains(&v));
        }
    }

    #[test]
    fn dry_brush_and_threshold() {
        assert_eq!(dry_brush(1.0), 0.0);
        assert_eq!(dry_brush(0.55), 0.0);
        assert!(close(dry_brush(0.45), 0.5, 1e-6));
        assert!(close(dry_brush(0.35), 1.0, 1e-6));
        assert_eq!(dry_brush(0.25), 1.0);
        let live = Fluid { water: 1.0, pigment: 1.0, ..Default::default() };
        assert_eq!(dab_amounts(&live, 0.5, 0.45).water, 0.0);
        assert!(dry_grain_threshold(1.0, 1.0) < dry_grain_threshold(0.0, 1.0));
        assert!(close(dry_grain_threshold(0.0, 0.0), 0.8, 1e-6));
        assert!(close(dry_grain_threshold(2.0, 5.0), 0.45, 1e-6));
        assert!(dry_grain_threshold(1.0, 1.0) > 0.4);
    }

    #[test]
    fn dab_amounts_match_happypaint() {
        let live = Fluid { water: 1.0, pigment: 0.75, ..Default::default() };
        let a = dab_amounts(&live, 0.5, 1.0);
        assert!(a.water > 1.5 && a.water <= 2.5);
        assert!(close(a.pigment, 0.75 * 0.65 * 0.55, 1e-6));
        assert_eq!(dab_amounts(&Fluid { water: 1.6, ..Default::default() }, 1.0, 1.0).water, 2.5);
        let (full, half, low) = (a, dab_amounts(&live, 0.5, 0.5), dab_amounts(&live, 0.5, 0.25));
        assert!(close(dab_amounts(&live, 0.5, 0.75).water, full.water, 1e-9));
        assert!(half.water > 0.0 && half.water < full.water / 2.0);
        assert_eq!(low.water, 0.0);
        assert!(close(low.pigment, full.pigment * 0.25, 1e-6));
        let tail = dab_amounts(&live, 0.5, reservoir(60.0 * 44.0, 44.0, 1.0));
        assert!(tail.water < 0.35 && tail.pigment > 0.0);
        let oil = Fluid { water: 0.9, pigment: 0.85, oil: true, ..Default::default() };
        assert_eq!(dab_amounts(&oil, 0.5, 0.25), dab_amounts(&oil, 0.5, 1.0));
        let water = Fluid { water: 1.6, rewet: 0.9, ..Default::default() };
        assert!(close(dab_amounts(&water, 0.7, 0.3).rewet, 0.9 * (0.35 + 0.65 * 0.7) * 0.08, 1e-9));
    }

    #[test]
    fn wet_clock_levels_and_dries() {
        let mut c = WetClock::default();
        for _ in 0..20 {
            c.deposit_water(0.6);
        }
        assert!(close(c.level, 0.6, 1e-9));
        c.deposit_water(1.2);
        assert!(close(c.level, 1.2, 1e-9));
        let mut c = WetClock::default();
        c.deposit_water(9.0);
        assert_eq!(c.level, 2.5);
        let mut c = WetClock::default();
        c.deposit_water(0.03);
        let mut steps = 0;
        while c.step() != ClockState::Dry {
            steps += 1;
        }
        assert!(steps > 90 && !c.wet());
    }
}
