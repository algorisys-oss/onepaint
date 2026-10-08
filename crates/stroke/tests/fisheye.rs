#![allow(clippy::unwrap_used)]
use onepaint_stroke::fisheye::*;

/// The 5-point preset on a 1000×800 document.
const POLES: [[f64; 2]; 5] = [[-800.0, 400.0], [1800.0, 400.0], [500.0, -1120.0], [500.0, 1920.0], [500.0, 400.0]];

#[test]
fn circles_and_arcs() {
    let (cx, cy, r) = circle_through([0.0, 0.0], [2.0, 0.0], [1.0, 1.0]).unwrap();
    assert!((cx - 1.0).abs() < 1e-9 && cy.abs() < 1e-9 && (r - 1.0).abs() < 1e-9);
    assert!(circle_through([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]).is_none());
    let arc = arc_points_through([0.0, 0.0], [2.0, 0.0], [1.0, 1.0], 8);
    assert_eq!(arc.len(), 9);
    assert!(arc.iter().any(|p| (p[0] - 1.0).abs() < 1e-9 && (p[1] - 1.0).abs() < 1e-9), "passes through the thru point");
    assert_eq!(arc_points_through([0.0, 0.0], [2.0, 2.0], [1.0, 1.0], 8).len(), 2, "collinear → straight");
}

#[test]
fn curves_follow_the_drag_direction() {
    // From the centre, a horizontal drag picks the equator (a straight L–R line).
    let c = pick_curve(&POLES, [500.0, 400.0], [600.0, 405.0]).unwrap();
    assert!(matches!(c, Curve::Line { .. }));
    assert!((project(&c, [600.0, 450.0])[1] - 400.0).abs() < 1e-6);
    // Above the centre, a horizontal drag picks an arc through L and R.
    let c = pick_curve(&POLES, [500.0, 100.0], [600.0, 100.0]).unwrap();
    let Curve::Circle { cx, cy, r } = c else { panic!("arc") };
    for p in [[-800.0, 400.0], [1800.0, 400.0], [500.0, 100.0]] {
        assert!(((p[0] - cx).hypot(p[1] - cy) - r).abs() < 1e-6);
    }
    let on = project(&c, [600.0, 150.0]);
    assert!(((on[0] - cx).hypot(on[1] - cy) - r).abs() < 1e-6);
    let path = sample_curve(&c, [500.0, 100.0], on);
    assert!(path.len() > 2 && path.iter().all(|p| ((p[0] - cx).hypot(p[1] - cy) - r).abs() < 1e-6));
    // A drag toward the centre picks the radial line.
    let c = pick_curve(&POLES, [800.0, 700.0], [700.0, 600.0]).unwrap();
    assert!(matches!(c, Curve::Line { .. }));
}

#[test]
fn guides_have_families_spokes_and_emphasis() {
    let g = guide_lines(&POLES, 6, 1000.0, 800.0);
    assert_eq!(g.len(), 6 * 4 + 12 + 3);
    assert_eq!(g.iter().filter(|l| l.1).count(), 3);
}
