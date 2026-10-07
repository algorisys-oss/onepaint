#![allow(clippy::unwrap_used)]
use onepaint_filters::gradient::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn shape_t_goldens() {
    let (a, b) = ([0.0, 0.0], [100.0, 0.0]);
    let t = |s: Shape, x: f64, y: f64| shape_t(s, [x, y], a, b);
    assert!(near(t(Shape::Linear, 25.0, 40.0), 0.25) && near(t(Shape::Linear, -10.0, 0.0), 0.0) && near(t(Shape::Linear, 150.0, 0.0), 1.0));
    assert!(near(t(Shape::Radial, 0.0, 50.0), 0.5) && near(t(Shape::Radial, -30.0, -40.0), 0.5) && near(t(Shape::Radial, 300.0, 0.0), 1.0));
    assert!(near(t(Shape::Reflected, 25.0, 0.0), 0.25) && near(t(Shape::Reflected, -25.0, 0.0), 0.25));
    assert!(
        near(t(Shape::Diamond, 100.0, 0.0), 1.0) && near(t(Shape::Diamond, 50.0, 50.0), 1.0) && near(t(Shape::Diamond, 25.0, 25.0), 0.5)
    );
    for (x, y, e) in [(50.0, 0.0, 0.0), (0.0, -50.0, 0.25), (-50.0, 0.0, 0.5), (0.0, 50.0, 0.75), (0.0, 0.0, 0.0)] {
        assert!(near(t(Shape::Angle, x, y), e), "angle ({x},{y})");
    }
    for s in Shape::ALL {
        assert!(shape_t(s, [3.0, 4.0], a, a).is_finite());
    }
}

fn stop(pos: f64, c: [f64; 3], o: f64, mid: Option<f64>) -> GradStop {
    GradStop { pos, color: c, opacity: o, mid }
}

#[test]
fn stops_and_midpoints() {
    let ramp = [stop(0.0, [1.0, 0.0, 0.0], 1.0, None), stop(0.5, [0.0, 1.0, 0.0], 0.5, None), stop(1.0, [0.0, 0.0, 1.0], 0.0, None)];
    assert_eq!(sample_stops(&ramp, 0.0), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(sample_stops(&ramp, 0.5), [0.0, 1.0, 0.0, 0.5]);
    assert_eq!(sample_stops(&ramp, 1.0), [0.0, 0.0, 1.0, 0.0]);
    let q = sample_stops(&ramp, 0.25);
    assert!(near(q[0], 0.5) && near(q[1], 0.5) && near(q[3], 0.75));
    let bw = |mid| [stop(0.0, [0.0; 3], 1.0, mid), stop(1.0, [1.0; 3], 1.0, None)];
    assert!(near(sample_stops(&bw(Some(0.25)), 0.25)[0], 0.5));
    assert!(near(sample_stops(&bw(Some(0.8)), 0.8)[0], 0.5));
    assert!(near(sample_stops(&bw(None), 0.3)[0], 0.3) && near(sample_stops(&bw(Some(0.5)), 0.3)[0], 0.3));
}

#[test]
fn reverse_moves_midpoints() {
    let s = [stop(0.0, [0.0; 3], 1.0, Some(0.2)), stop(0.6, [0.5; 3], 1.0, Some(0.9)), stop(1.0, [1.0; 3], 1.0, None)];
    let r = reverse_stops(&s);
    let pos: Vec<f64> = r.iter().map(|s| (s.pos * 1e9).round() / 1e9).collect();
    assert_eq!(pos, vec![0.0, 0.4, 1.0]);
    let mids: Vec<Option<f64>> = r.iter().map(|s| s.mid.map(|m| (m * 1e9).round() / 1e9)).collect();
    assert_eq!(mids, vec![Some(0.1), Some(0.8), None]);
}

#[test]
fn ramp_linearises_stops() {
    let r = Ramp::new(&[stop(0.0, [1.0, 0.0, 0.0], 1.0, None), stop(1.0, [1.0, 0.0, 0.0], 0.0, None)]);
    assert_eq!(r.sample(0.0), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(r.sample(1.0)[3], 0.0);
    let g = Ramp::new(&[stop(0.0, [0.5; 3], 1.0, None), stop(1.0, [0.5; 3], 1.0, None)]);
    assert!((g.sample(0.3)[0] - onepaint_color::srgb_to_linear(0.5)).abs() < 1e-6);
}
