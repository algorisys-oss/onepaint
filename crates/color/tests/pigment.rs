#![allow(clippy::unwrap_used)]
use onepaint_color::pigment::pigment_mix;

#[derive(serde::Deserialize)]
struct Case {
    a: [f32; 3],
    b: [f32; 3],
    t: f32,
    o: [f32; 3],
}

#[test]
fn matches_happypaint_goldens() {
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/pigment.json")).unwrap();
    assert!(cases.len() > 500);
    let mut worst = 0.0f32;
    for c in &cases {
        let o = pigment_mix(c.a, c.b, c.t);
        for (x, y) in o.iter().zip(c.o) {
            worst = worst.max((x - y).abs());
        }
    }
    assert!(worst < 1e-5, "max error {worst}");
}

#[test]
fn yellow_and_blue_make_green() {
    let o = pigment_mix([1.0, 1.0, 0.0], [0.0, 0.0, 1.0], 0.5);
    assert!(o[1] > o[0] && o[1] > o[2], "{o:?}");
}

#[test]
fn same_colour_round_trips() {
    for c in [[0.9f32, 0.3, 0.1], [0.2, 0.6, 0.8]] {
        let o = pigment_mix(c, c, 0.37);
        for (x, y) in o.iter().zip(c) {
            assert!((x - y).abs() < 1e-4);
        }
    }
}
