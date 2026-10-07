#![allow(clippy::unwrap_used)]
//! Cases ported from happypaint `state/guide-lines.test.ts`.
use onepaint_stroke::guide_lines::*;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, SQRT_2};

fn h(y: f64) -> GuideLine {
    GuideLine::new("h", 0.0, y, 0.0)
}
fn v(x: f64) -> GuideLine {
    GuideLine::new("v", x, 0.0, FRAC_PI_2)
}
fn diag(x: f64, y: f64) -> GuideLine {
    GuideLine::new("d", x, y, FRAC_PI_4)
}
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn distance_and_projection() {
    assert!(near(h(100.0).signed_distance([999.0, 130.0]), 30.0));
    assert!(near(h(100.0).signed_distance([-50.0, 70.0]), -30.0));
    assert!(near(v(200.0).signed_distance([260.0, 5.0]).abs(), 60.0));
    assert!(near(diag(10.0, 10.0).signed_distance([60.0, 60.0]), 0.0));
    assert!(near(diag(10.0, 0.0).signed_distance([0.0, 0.0]).abs(), SQRT_2 * 5.0));
    let p = h(100.0).project([42.0, 7.0]);
    assert!(near(p[0], 42.0) && near(p[1], 100.0));
    let p = diag(0.0, 0.0).project([10.0, 0.0]);
    assert!(near(p[0], 5.0) && near(p[1], 5.0));
}

#[test]
fn nearest_skips_locked_and_respects_tolerance() {
    let lines = vec![h(100.0), v(200.0)];
    assert_eq!(nearest_guide(&lines, [10.0, 104.0], 8.0), Some(0));
    assert_eq!(nearest_guide(&lines, [195.0, 900.0], 8.0), Some(1));
    assert_eq!(nearest_guide(&lines, [10.0, 140.0], 8.0), None);
    let mut locked = h(101.0);
    locked.locked = Some(true);
    assert_eq!(nearest_guide(&[locked.clone(), h(105.0)], [0.0, 101.0], 8.0), Some(1));
    assert!(near(snap_point_to_guides(&[locked], [0.0, 97.0], 8.0)[1], 101.0), "locked guides still snap");
    assert_eq!(nearest_guide(&[], [0.0, 0.0], 8.0), None);
}

#[test]
fn delete_test_is_the_document_crossing() {
    assert!(h(100.0).intersects_doc(800.0, 600.0));
    assert!(!h(-5.0).intersects_doc(800.0, 600.0));
    assert!(!h(605.0).intersects_doc(800.0, 600.0));
    assert!(!v(-1.0).intersects_doc(800.0, 600.0));
    assert!(h(0.0).intersects_doc(800.0, 600.0) && h(600.0).intersects_doc(800.0, 600.0));
    assert!(!diag(0.0, 2000.0).intersects_doc(800.0, 600.0));
    assert!(diag(400.0, 300.0).intersects_doc(800.0, 600.0));
}

#[test]
fn units_and_measure() {
    assert_eq!(unit_size("px", 300.0), 1.0);
    assert_eq!(format_length(128.4, "px", 300.0), "128 px");
    assert_eq!(format_length(300.0, "in", 300.0), "1.00 in");
    assert_eq!(format_length(300.0, "cm", 300.0), "2.54 cm");
    assert_eq!(unit_size("in", 0.0), 300.0);
    assert!(near(measure_between([0.0, 0.0], [3.0, 4.0], "px", 300.0).1, 5.0));
    assert!(near(measure_between([0.0, 100.0], [0.0, 0.0], "px", 300.0).2, 90.0));
    assert!(near(measure_between([0.0, 0.0], [0.0, 100.0], "px", 300.0).2, -90.0));
    assert!(near(measure_between([10.0, 0.0], [0.0, 0.0], "px", 300.0).2.abs(), 180.0));
    let z = measure_between([5.0, 5.0], [5.0, 5.0], "px", 300.0);
    assert!(z.1 == 0.0 && z.2.is_sign_positive());
    assert_eq!(measure_between([0.0, 0.0], [300.0, 0.0], "in", 300.0).0, "1.00 in");
}

#[test]
fn nice_steps_and_ticks() {
    for (a, b) in [(1.0, 1.0), (1.5, 2.0), (3.0, 5.0), (7.0, 10.0), (80.0, 100.0), (230.0, 500.0)] {
        assert_eq!(nice_step(a), b);
    }
    assert!(near(nice_step(0.03), 0.05) && near(nice_step(0.4), 0.5));
    assert_eq!(nice_step(0.0), 1.0);
    assert_eq!(nice_step(f64::NAN), 1.0);
    assert_eq!(tick_steps(1.0, "px", 300.0), (20.0, 100.0));
    assert!(tick_steps(4.0, "px", 300.0).1 < 100.0 && tick_steps(0.1, "px", 300.0).1 > 100.0);
    assert!(near(tick_steps(1.0, "in", 300.0).1, 150.0));
    let t = ruler_ticks_span(0.0, 400.0, 1.0, "px", 300.0, 2000);
    assert_eq!(t[0], RulerTick { pos: 0.0, major: true, label: Some("0".into()) });
    let labels: Vec<_> = t.iter().filter_map(|t| t.label.clone()).collect();
    assert_eq!(labels, ["0", "100", "200", "300", "400"]);
    assert_eq!(t.len(), 21);
    let neg = ruler_ticks_span(-200.0, 0.0, 1.0, "px", 300.0, 2000);
    assert_eq!(neg.first().unwrap().label.as_deref(), Some("-200"));
    assert!(ruler_ticks_span(0.0, 100_000.0, 8.0, "px", 300.0, 50).len() <= 51);
}

#[test]
fn angles_are_canonical() {
    assert_eq!(normalize_angle(PI), 0.0);
    assert!(near(normalize_angle(-FRAC_PI_2), FRAC_PI_2));
    assert!(near(normalize_angle(3.0 * PI / 2.0), FRAC_PI_2));
    assert_eq!(normalize_angle(f64::NAN), 0.0);
    assert!(near(snap_angle(0.27, 15.0), 15f64.to_radians()));
    assert!(near(angle_through([0.0, 0.0], [10.0, 10.0]), FRAC_PI_4));
    assert!(near(angle_deg(0.0), 0.0));
    assert!(near(angle_deg(FRAC_PI_2), 90.0));
    assert!(near(angle_deg(FRAC_PI_4), 135.0));
    assert!(rulers_pinned(0.0) && rulers_pinned(2.0 * PI) && !rulers_pinned(0.3));
}
