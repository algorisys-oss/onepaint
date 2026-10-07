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
    assert_eq!(drag_shape(ShapeKind::Rect, A, [300.0, 200.0], o), Some(Shape::Rect { cx: 200.0, cy: 150.0, hw: 100.0, hh: 50.0 }));
    assert_eq!(drag_shape(ShapeKind::Rect, [300.0, 200.0], A, o), Some(Shape::Rect { cx: 200.0, cy: 150.0, hw: 100.0, hh: 50.0 }));
    let c = DragOpts { constrain: true, ..o };
    assert_eq!(drag_shape(ShapeKind::Rect, A, [300.0, 200.0], c), Some(Shape::Rect { cx: 200.0, cy: 200.0, hw: 100.0, hh: 100.0 }));
    assert_eq!(drag_shape(ShapeKind::Rect, A, [-100.0, 40.0], c), Some(Shape::Rect { cx: 0.0, cy: 0.0, hw: 100.0, hh: 100.0 }));
    let f = DragOpts { from_center: true, ..o };
    assert_eq!(drag_shape(ShapeKind::Rect, A, [160.0, 130.0], f), Some(Shape::Rect { cx: 100.0, cy: 100.0, hw: 60.0, hh: 30.0 }));
    assert_eq!(drag_shape(ShapeKind::Ellipse, A, [300.0, 200.0], o), Some(Shape::Ellipse { cx: 200.0, cy: 150.0, rx: 100.0, ry: 50.0 }));
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
    let p = sample_path(&Shape::Rect { cx: 0.0, cy: 0.0, hw: 10.0, hh: 10.0 }, 2.0);
    assert_eq!(p.first(), p.last(), "closed");
    assert!(p.windows(2).all(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]) <= 2.0 + 1e-9));
    let e = sample_path(&Shape::Ellipse { cx: 0.0, cy: 0.0, rx: 50.0, ry: 50.0 }, 2.0);
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
