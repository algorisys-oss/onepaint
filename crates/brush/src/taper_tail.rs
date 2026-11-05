//! Exit-taper tail: the dabs a tapered stroke holds back until its end is
//! known (happypaint `engine/brush/taper-tail.ts`, v0.204.15).
//!
//! With exit taper on, the last `exit` px of a stroke can't be drawn for good
//! while the pen still moves: the taper ramps them to a point at the END, and
//! the end keeps moving. [`crate::StrokeEngine`] keeps them deferred; the
//! session draws them live into a display copy of the stroke, tapered toward
//! the current end, and for real on lift. Both draws go through
//! [`tail_factor`] and [`build_tail`], so what lands on lift is what was last
//! on screen.

use crate::Dab;

#[inline]
fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

/// Radius factor for a deferred dab at `cum_dist` when the stroke is `total`
/// px long: entry ramp × exit ramp. A degenerate stroke (a tap, no travel) has
/// no direction to taper along, so it is drawn full size: a dot still marks.
pub fn tail_factor(entry: f64, exit: f64, cum_dist: f64, total: f64) -> f64 {
    if total <= 1.0 {
        return 1.0;
    }
    let e = if entry > 0.0 { clamp01(cum_dist / entry) } else { 1.0 };
    let x = if exit > 0.0 { clamp01((total - cum_dist) / exit) } else { 1.0 };
    e * x
}

/// The deferred dabs, each scaled by its factor. The inputs are left as they
/// were: the live tail rebuilds from them on every move.
pub fn build_tail(items: &[(f64, Dab)], factor: impl Fn(f64) -> f64) -> Vec<Dab> {
    items
        .iter()
        .map(|&(cd, d)| {
            let f = factor(cd);
            Dab { radius: d.radius * f, ..d }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dab(x: f64, radius: f64) -> Dab {
        Dab { x, y: 50.0, radius, alpha: 1.0, rotation: 0.0, aspect: 1.0, hardness: 1.0, color: [0.0; 3], grain_off: [0.0; 2] }
    }

    #[test]
    fn ramps_from_full_size_exit_px_before_the_end_to_a_point() {
        assert_eq!(tail_factor(0.0, 20.0, 50.0, 100.0), 1.0);
        assert_eq!(tail_factor(0.0, 20.0, 80.0, 100.0), 1.0);
        assert!((tail_factor(0.0, 20.0, 90.0, 100.0) - 0.5).abs() < 1e-12);
        assert_eq!(tail_factor(0.0, 20.0, 100.0, 100.0), 0.0);
    }

    #[test]
    fn entry_taper_multiplies_in() {
        assert!((tail_factor(10.0, 20.0, 5.0, 100.0) - 0.5).abs() < 1e-12);
        assert!((tail_factor(10.0, 20.0, 5.0, 10.0) - 0.5 * 0.25).abs() < 1e-12);
    }

    #[test]
    fn a_tap_is_drawn_full_size() {
        assert_eq!(tail_factor(10.0, 20.0, 0.0, 0.0), 1.0);
        assert_eq!(tail_factor(10.0, 20.0, 0.5, 1.0), 1.0);
    }

    #[test]
    fn build_tail_scales_each_dab_and_leaves_inputs() {
        let items = vec![(80.0, dab(10.0, 4.0)), (90.0, dab(20.0, 4.0))];
        let out = build_tail(&items, |d| if d == 80.0 { 1.0 } else { 0.5 });
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].radius, 4.0);
        assert_eq!(out[1].radius, 2.0);
        assert_eq!(items[1].1.radius, 4.0);
    }

    #[test]
    fn nothing_deferred_nothing_to_draw() {
        assert!(build_tail(&[], |_| 1.0).is_empty());
    }
}
