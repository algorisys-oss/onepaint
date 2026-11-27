#![allow(clippy::unwrap_used)]
//! happypaint `input/pen-pressure-curve.test.ts` (v0.212.0).
use onepaint_input::pen_curve::*;
use onepaint_input::{PointerKind, PressureProvider};

fn close(a: f64, b: f64, digits: i32) -> bool {
    (a - b).abs() < 0.5 * 10f64.powi(-digits)
}

#[test]
fn the_default_curve_is_the_one_every_pen_had_before() {
    for raw in [0.0, 0.01, 0.08, 0.3, 0.5, 0.77, 1.0] {
        let before = f64::clamp(raw, 0.02, 1.0).powf(0.7);
        assert!(close(PenCurve::default().apply(raw), before, 10));
    }
}

#[test]
fn full_pressure_at_reaches_1_there_and_stays() {
    let c = PenCurve { gamma: 1.0, full: 0.6 };
    assert!(close(c.apply(0.6), 1.0, 10));
    assert_eq!(c.apply(0.9), 1.0);
    assert!(close(c.apply(0.3), 0.5, 10));
}

#[test]
fn a_softer_exponent_lifts_the_light_end() {
    assert!(PenCurve { gamma: 0.5, full: 1.0 }.apply(0.2) > 0.2);
    assert!(PenCurve { gamma: 2.0, full: 1.0 }.apply(0.2) < 0.2);
}

#[test]
fn stays_in_range_for_junk_input() {
    for raw in [-1.0, f64::NAN, 5.0] {
        let v = PenCurve::default().apply(raw);
        assert!((0.0..=1.0).contains(&v));
    }
}

#[test]
fn broken_values_fall_back_and_out_of_range_ones_clamp() {
    let d = PenCurve::default();
    assert_eq!(PenCurve { gamma: f64::NAN, full: f64::INFINITY }.coerced(), d);
    let c: PenCurve = serde_json::from_str("{}").unwrap();
    assert_eq!(c, d, "missing fields take the default");
    assert_eq!(PenCurve { gamma: 99.0, full: 0.01 }.coerced(), PenCurve { gamma: PEN_GAMMA_MAX, full: PEN_FULL_MIN });
    assert_eq!(PenCurve { gamma: 0.001, full: 3.0 }.coerced(), PenCurve { gamma: PEN_GAMMA_MIN, full: 1.0 });
}

fn spread(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    (0..n).map(|i| lo + (hi - lo) * i as f64 / (n - 1) as f64).collect()
}

#[test]
fn calibration_needs_enough_samples() {
    assert!(PenCurve::calibrate(&spread(0.1, 0.4, MIN_CALIBRATION_SAMPLES - 1)).is_none());
    assert!(PenCurve::calibrate(&spread(0.1, 0.4, MIN_CALIBRATION_SAMPLES)).is_some());
}

#[test]
fn a_light_pen_reaches_full_pressure_near_its_own_top() {
    let c = PenCurve::calibrate(&spread(0.05, 0.4, 200)).unwrap();
    assert!(c.full > 0.35 && c.full < 0.41, "{c:?}");
    assert!(close(c.apply(0.225), 0.5, 1), "its typical pressure lands mid-range");
    assert_eq!(c.apply(0.4), 1.0);
}

#[test]
fn a_pen_using_the_whole_range_keeps_full_at_1() {
    let c = PenCurve::calibrate(&spread(0.02, 1.0, 200)).unwrap();
    assert!(c.full > 0.94);
    assert!(close(c.apply(0.5), 0.5, 1));
}

#[test]
fn calibration_ignores_zero_samples_and_stays_in_range() {
    let mut s = spread(0.3, 0.32, 200);
    s.extend(std::iter::repeat_n(0.0, 500));
    let c = PenCurve::calibrate(&s).unwrap();
    assert!(c.full >= PEN_FULL_MIN);
    assert!((PEN_GAMMA_MIN..=PEN_GAMMA_MAX).contains(&c.gamma));
}

#[test]
fn hardware_pen_pressure_goes_through_the_curve() {
    let mut p = PressureProvider::with_curve(PointerKind::Pen, PenCurve { gamma: 1.0, full: 0.5 });
    assert!(close(p.pressure(0.0, 0.0, 0.0, Some(0.25)), 0.5, 10));
    assert!(close(p.pressure(1.0, 0.0, 8.0, Some(0.5)), 1.0, 10));
    let mut p = PressureProvider::new(PointerKind::Pen);
    assert!(close(p.pressure(0.0, 0.0, 0.0, Some(0.3)), 0.3f64.powf(0.7), 10));
}

#[test]
fn the_mouse_ignores_the_pen_curve() {
    let mut a = PressureProvider::with_curve(PointerKind::Mouse, PenCurve { gamma: 3.0, full: 0.2 });
    let mut b = PressureProvider::new(PointerKind::Mouse);
    assert_eq!(a.pressure(0.0, 0.0, 0.0, Some(0.5)), b.pressure(0.0, 0.0, 0.0, Some(0.5)));
}
