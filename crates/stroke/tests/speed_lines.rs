#![allow(clippy::unwrap_used)]
use onepaint_stroke::speed_lines::*;

#[derive(serde::Deserialize)]
struct Golden {
    focus: Vec<Vec<[f64; 2]>>,
    speed: Vec<Vec<[f64; 2]>>,
}

fn pts(s: &[onepaint_stroke::bezier::Subpath]) -> Vec<Vec<[f64; 2]>> {
    s.iter().map(|p| p.anchors.iter().map(|a| [a.x, a.y]).collect()).collect()
}

fn same(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>]) -> bool {
    a.len() == b.len() && a.iter().flatten().zip(b.iter().flatten()).all(|(p, q)| (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9)
}

#[test]
fn matches_happypaint() {
    let g: Golden = serde_json::from_str(include_str!("fixtures/speed_lines.json")).unwrap();
    let f = focus_lines(&FocusLines {
        center: [400.0, 300.0],
        count: 24,
        inner_radius: 90.0,
        outer_radius: 500.0,
        jitter: 0.35,
        angle_jitter: 0.7,
        start_angle: 0.0,
        sweep: std::f64::consts::TAU,
        seed: 1234,
    });
    assert!(same(&pts(&f), &g.focus));
    let s = speed_lines(&SpeedLines { angle: 0.3, count: 16, width: 800.0, height: 600.0, jitter: 0.35, spacing_jitter: 0.6, seed: 99 });
    assert!(same(&pts(&s), &g.speed));
}

#[test]
fn degenerate_inputs_give_nothing() {
    assert!(
        speed_lines(&SpeedLines { angle: 0.0, count: 0, width: 10.0, height: 10.0, jitter: 0.0, spacing_jitter: 0.0, seed: 1 }).is_empty()
    );
    let f = FocusLines {
        center: [0.0, 0.0],
        count: 5,
        inner_radius: 1.0,
        outer_radius: 0.0,
        jitter: 0.0,
        angle_jitter: 0.0,
        start_angle: 0.0,
        sweep: 1.0,
        seed: 1,
    };
    assert!(focus_lines(&f).is_empty());
}
