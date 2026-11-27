#![allow(clippy::unwrap_used)]
use onepaint_stroke::shape::*;

const A: [f64; 2] = [100.0, 100.0];

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn drags() {
    let o = DragOpts::default();
    assert!(drag_shape(ShapeKind::Rect, A, [100.5, 100.5], o).is_none());
    assert_eq!(
        drag_shape(ShapeKind::Rect, A, [300.0, 200.0], o),
        Some(Shape::Rect { cx: 200.0, cy: 150.0, hw: 100.0, hh: 50.0, rot: 0.0 })
    );
    assert_eq!(
        drag_shape(ShapeKind::Rect, [300.0, 200.0], A, o),
        Some(Shape::Rect { cx: 200.0, cy: 150.0, hw: 100.0, hh: 50.0, rot: 0.0 })
    );
    let c = DragOpts { constrain: true, ..o };
    assert_eq!(
        drag_shape(ShapeKind::Rect, A, [300.0, 200.0], c),
        Some(Shape::Rect { cx: 200.0, cy: 200.0, hw: 100.0, hh: 100.0, rot: 0.0 })
    );
    assert_eq!(drag_shape(ShapeKind::Rect, A, [-100.0, 40.0], c), Some(Shape::Rect { cx: 0.0, cy: 0.0, hw: 100.0, hh: 100.0, rot: 0.0 }));
    let f = DragOpts { from_center: true, ..o };
    assert_eq!(drag_shape(ShapeKind::Rect, A, [160.0, 130.0], f), Some(Shape::Rect { cx: 100.0, cy: 100.0, hw: 60.0, hh: 30.0, rot: 0.0 }));
    assert_eq!(
        drag_shape(ShapeKind::Ellipse, A, [300.0, 200.0], o),
        Some(Shape::Ellipse { cx: 200.0, cy: 150.0, rx: 100.0, ry: 50.0, rot: 0.0 })
    );
    let Some(Shape::Line { a, b }) = drag_shape(ShapeKind::Line, A, [200.0, 110.0], c) else { panic!() };
    assert_eq!(a, A);
    assert!(near(b[0], 100.0 + (100.0f64).hypot(10.0)) && near(b[1], 100.0));
    let Some(Shape::Line { a, .. }) = drag_shape(ShapeKind::Line, A, [160.0, 100.0], f) else { panic!() };
    assert_eq!(a, [40.0, 100.0]);
    let Some(Shape::Polygon { pts }) = drag_shape(ShapeKind::Polygon, A, [100.0, 20.0], DragOpts { sides: 7, ..o }) else { panic!() };
    assert_eq!(pts.len(), 7);
    assert!(pts.iter().all(|p| near((p[0] - 100.0).hypot(p[1] - 100.0), 80.0)));
    let Some(Shape::Polygon { pts }) = drag_shape(ShapeKind::Polygon, A, [100.0, 180.0], o) else { panic!() };
    assert!(near(pts[0][0], 100.0) && near(pts[0][1], 180.0));
}

#[test]
fn sides_and_paths() {
    assert_eq!([6.0, 2.0, 99.0, 5.7, f64::NAN].map(clamp_sides), [6, 3, 12, 6, 3]);
    let p = sample_path(&Shape::Rect { cx: 0.0, cy: 0.0, hw: 10.0, hh: 10.0, rot: 0.0 }, 2.0);
    assert_eq!(p.first(), p.last(), "closed");
    assert!(p.windows(2).all(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]) <= 2.0 + 1e-9));
    let e = sample_path(&Shape::Ellipse { cx: 0.0, cy: 0.0, rx: 50.0, ry: 50.0, rot: 0.0 }, 2.0);
    assert!(e.len() >= 16 && e.iter().all(|q| near(q[0].hypot(q[1]), 50.0)));
}

#[test]
fn quick_shape_recognises_line_ellipse_and_rect() {
    let line: Vec<[f64; 2]> = (0..30).map(|i| [i as f64 * 5.0, (i as f64 * 0.7).sin()]).collect();
    assert!(matches!(recognize(&line, false), Some(Shape::Line { .. })));
    let circle: Vec<[f64; 2]> = (0..=60)
        .map(|i| {
            let t = i as f64 / 60.0 * std::f64::consts::TAU;
            [200.0 + 80.0 * t.cos(), 200.0 + 50.0 * t.sin()]
        })
        .collect();
    assert!(matches!(recognize(&circle, false), Some(Shape::Ellipse { .. })));
    let mut rect = Vec::new();
    for (a, b) in [([0.0, 0.0], [120.0, 0.0]), ([120.0, 0.0], [120.0, 80.0]), ([120.0, 80.0], [0.0, 80.0]), ([0.0, 80.0], [0.0, 1.0])] {
        for k in 0..15 {
            let f = k as f64 / 15.0;
            rect.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
        }
    }
    assert!(matches!(recognize(&rect, false), Some(Shape::Rect { .. })), "{:?}", recognize(&rect, false));
    let scribble: Vec<[f64; 2]> = (0..20).map(|i| [i as f64 * 7.0, ((i * 37) % 11) as f64 * 9.0]).collect();
    assert!(recognize(&scribble, false).is_none() || !matches!(recognize(&scribble, false), Some(Shape::Line { .. })));
    assert!(recognize(&line[..5], false).is_none());
}

// happypaint v0.211.5 `quick-shape.test.ts`: QuickShape keeps following the
// pen after the snap (`hold_drag_shape`).
mod hold_drag {
    use onepaint_stroke::shape::*;
    use std::f64::consts::PI;

    const LINE: Shape = Shape::Line { a: [100.0, 100.0], b: [200.0, 100.0] };
    const ELLIPSE: Shape = Shape::Ellipse { cx: 200.0, cy: 200.0, rx: 100.0, ry: 50.0, rot: 0.0 };
    const RECT: Shape = Shape::Rect { cx: 200.0, cy: 200.0, hw: 100.0, hh: 50.0, rot: 0.0 };
    fn square() -> Shape {
        Shape::Polygon { pts: vec![[300.0, 200.0], [200.0, 300.0], [100.0, 200.0], [200.0, 100.0]] }
    }
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn a_line_end_follows_the_pen_from_wherever_it_snapped() {
        assert_eq!(hold_drag_shape(&LINE, [200.0, 100.0], [320.0, 180.0], false), Shape::Line { a: [100.0, 100.0], b: [320.0, 180.0] });
        assert_eq!(
            hold_drag_shape(&LINE, [198.0, 101.0], [320.0, 180.0], false),
            hold_drag_shape(&LINE, [0.0, 0.0], [320.0, 180.0], false)
        );
    }

    #[test]
    fn constrain_locks_a_line_to_45_degrees_keeping_its_length() {
        let Shape::Line { a, b } = hold_drag_shape(&LINE, [200.0, 100.0], [300.0, 110.0], true) else { panic!() };
        assert_eq!(a, [100.0, 100.0]);
        assert!(near(b[0], 100.0 + 200f64.hypot(10.0)) && near(b[1], 100.0));
    }

    #[test]
    fn pulling_out_scales_without_turning() {
        let Shape::Ellipse { cx, cy, rx, ry, rot } = hold_drag_shape(&ELLIPSE, [300.0, 200.0], [350.0, 200.0], false) else { panic!() };
        assert_eq!((cx, cy), (200.0, 200.0));
        assert!(near(rx, 150.0) && near(ry, 75.0) && near(rot, 0.0));
    }

    #[test]
    fn swinging_round_turns_without_scaling() {
        let Shape::Rect { hw, hh, rot, .. } = hold_drag_shape(&RECT, [300.0, 200.0], [200.0, 300.0], false) else { panic!() };
        assert!(near(hw, 100.0) && near(hh, 50.0) && near(rot, PI / 2.0));
    }

    #[test]
    fn the_outline_point_under_the_pen_stays_under_it() {
        let to = [260.0, 330.0];
        let Shape::Polygon { pts } = hold_drag_shape(&square(), [300.0, 200.0], to, false) else { panic!() };
        assert!(near(pts[0][0], to[0]) && near(pts[0][1], to[1]));
        let r = (to[0] - 200.0).hypot(to[1] - 200.0);
        assert!(pts.iter().all(|p| near((p[0] - 200.0).hypot(p[1] - 200.0), r)));
        assert_eq!(pts.len(), 4);
    }

    #[test]
    fn no_travel_leaves_the_shape_as_it_snapped() {
        let at = [300.0, 200.0];
        assert_eq!(hold_drag_shape(&ELLIPSE, at, at, false), ELLIPSE);
        assert_eq!(hold_drag_shape(&RECT, at, at, false), RECT);
        let (Shape::Polygon { pts }, Shape::Polygon { pts: base }) = (hold_drag_shape(&square(), at, at, false), square()) else {
            panic!()
        };
        assert!(pts.iter().zip(&base).all(|(a, b)| near(a[0], b[0]) && near(a[1], b[1])));
    }

    #[test]
    fn constrain_snaps_the_turn_to_15_degrees_and_still_scales() {
        let a = 20f64.to_radians();
        let to = [200.0 + 150.0 * a.cos(), 200.0 + 150.0 * a.sin()];
        let Shape::Rect { hw, hh, rot, .. } = hold_drag_shape(&RECT, [300.0, 200.0], to, true) else { panic!() };
        assert!(near(rot, ANGLE_STEP) && near(hw, 150.0) && near(hh, 75.0));
        let to = [200.0 + 100.0 * a.cos(), 200.0 + 100.0 * a.sin()];
        let Shape::Polygon { pts } = hold_drag_shape(&square(), [300.0, 200.0], to, true) else { panic!() };
        assert!(near((pts[0][1] - 200.0).atan2(pts[0][0] - 200.0), ANGLE_STEP), "by its first vertex");
    }

    #[test]
    fn a_pen_on_the_centre_has_no_lever_arm() {
        assert_eq!(hold_drag_shape(&ELLIPSE, [200.0, 200.0], [400.0, 90.0], false), ELLIPSE);
        assert_eq!(hold_drag_shape(&square(), [200.0, 200.0], [400.0, 90.0], false), square());
    }

    #[test]
    fn dragging_onto_the_centre_never_collapses_the_shape() {
        let Shape::Ellipse { rx, ry, rot, .. } = hold_drag_shape(&ELLIPSE, [300.0, 200.0], [200.0, 200.0], false) else { panic!() };
        assert!(rx >= 1.0 && ry >= 1.0 && rot.is_finite());
        let Shape::Polygon { pts } = hold_drag_shape(&square(), [300.0, 200.0], [200.0, 200.0], false) else { panic!() };
        assert!(pts.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
        assert!((pts[0][0] - 200.0).hypot(pts[0][1] - 200.0) > 0.0);
    }

    #[test]
    fn a_turned_rect_samples_its_turned_corners() {
        let r = Shape::Rect { cx: 0.0, cy: 0.0, hw: 10.0, hh: 10.0, rot: PI / 4.0 };
        let p = sample_path(&r, 2.0);
        let d = 200f64.sqrt();
        assert!(near(p[0][0], 0.0) && near(p[0][1], -d), "{:?}", p[0]);
    }
}
