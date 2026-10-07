#![allow(clippy::unwrap_used)]
use onepaint_brush::*;
use onepaint_raster::Surface;
use onepaint_stroke::{Sampler, StrokePoint};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 2e-6
}

#[test]
fn presets_are_valid_and_complete() {
    let ps = presets();
    for id in ["hard-round", "soft-round", "soft-airbrush", "pencil", "ink", "comic-inker", "studio-pen", "eraser-hard", "eraser-soft"] {
        assert!(ps.iter().any(|p| p.id == id), "missing {id}");
    }
    for p in &ps {
        assert!(p.size > 0.0 && p.spacing > 0.0);
        assert!((0.0..=1.0).contains(&p.hardness) && p.flow > 0.0 && p.flow <= 1.0 && p.opacity > 0.0 && p.opacity <= 1.0);
        assert!((0.3..=3.0).contains(&p.pressure_curve));
    }
    let hr = preset("hard-round").unwrap();
    assert_eq!((hr.size, hr.spacing, hr.hardness, hr.size_pressure), (24.0, 0.1, 0.95, 0.85));
}

#[test]
fn hand_checked_radii() {
    let hr = preset("hard-round").unwrap();
    assert!(near(radius_for(&hr, 1.0), 12.0));
    assert!(near(radius_for(&hr, 0.5), 6.9));
    assert!(near(radius_for(&hr, 0.0), 1.8));
}

#[test]
fn g2_hard_round_gap() {
    let hr = preset("hard-round").unwrap();
    let mut s = Sampler::new();
    let mut gap = |p: &StrokePoint, _: Option<(f64, f64)>| spacing_gap(&hr, p);
    let mut xs: Vec<f64> = s.add(StrokePoint::at(0.0, 0.0, 0.0, 0.0), &mut gap).iter().map(|p| p.x).collect();
    let tail = s.add(StrokePoint::at(20.0, 0.0, 1.0, 10.0), &mut gap);
    xs.extend(tail.iter().map(|p| p.x));
    let exp = [
        0.0, 0.5, 1.0, 1.5, 2.013, 2.578326, 3.201315, 3.887849, 4.64441, 5.47814, 6.39691, 7.409395, 8.525153, 9.754719, 11.1097,
        12.60289, 14.248384, 16.06172, 18.060015,
    ];
    assert_eq!(xs.len(), exp.len(), "{xs:?}");
    for (a, e) in xs.iter().zip(exp) {
        assert!((a - e).abs() < 2e-5, "{a} vs {e}");
    }
    assert!(near(tail.last().unwrap().pressure, 0.903001));
}

#[test]
fn g9_linearize_alpha() {
    for (a, sp, e) in [(0.6, 0.06, 0.108144), (0.35, 0.08, 0.065488), (0.85, 0.15, 0.307895), (0.5, 0.1, 0.11027), (0.5, 1.5, 0.5)] {
        assert!(near(linearize_alpha(a, sp, 0.9), e), "{a} {sp}");
    }
}

#[test]
fn hard_dab_covers_center_not_outside() {
    let mut s = Surface::new();
    let d = Dab { x: 50.0, y: 50.0, radius: 10.0, alpha: 1.0, rotation: 0.0, aspect: 1.0, hardness: 0.95, color: [1.0, 0.0, 0.0] };
    let r = rasterize_dab(&mut s, &d, false, None);
    assert!(r.x <= 40 && r.right() >= 60);
    assert!((s.get(50, 50)[3] - 1.0).abs() < 1e-3);
    assert!((s.get(50, 50)[0] - 1.0).abs() < 1e-3);
    assert_eq!(s.get(50, 62)[3], 0.0);
    assert_eq!(s.get(62, 50)[3], 0.0);
    // Soft dab falls off.
    let mut t = Surface::new();
    let soft = Dab { hardness: 0.05, ..d };
    rasterize_dab(&mut t, &soft, false, None);
    let mid = t.get(55, 50)[3];
    assert!(mid > 0.05 && mid < 0.95, "{mid}");
}

#[test]
fn dabs_accumulate_over_and_wash_caps() {
    let d = Dab { x: 10.0, y: 10.0, radius: 6.0, alpha: 0.5, rotation: 0.0, aspect: 1.0, hardness: 0.95, color: [0.0, 0.0, 1.0] };
    let mut over = Surface::new();
    rasterize_dab(&mut over, &d, false, None);
    rasterize_dab(&mut over, &d, false, None);
    assert!((over.get(10, 10)[3] - 0.75).abs() < 1e-3);
    let mut wash = Surface::new();
    rasterize_dab(&mut wash, &d, true, None);
    rasterize_dab(&mut wash, &d, true, None);
    assert!((wash.get(10, 10)[3] - 0.5).abs() < 1e-3);
}

#[test]
fn stroke_engine_draws_a_line_and_tap_draws_a_dot() {
    let b = preset("hard-round").unwrap();
    let mut e = StrokeEngine::new(b.clone(), [1.0, 1.0, 1.0], 7);
    let mut s = Surface::new();
    let mut dabs = e.begin(StrokePoint::at(10.0, 10.0, 1.0, 0.0));
    for i in 1..=10 {
        dabs.extend(e.add(StrokePoint::at(10.0 + i as f64 * 10.0, 10.0, 1.0, i as f64 * 8.0)));
    }
    dabs.extend(e.end());
    for d in &dabs {
        rasterize_dab(&mut s, d, b.wash, None);
    }
    assert!(s.get(60, 10)[3] > 0.5, "line body");
    assert_eq!(s.get(60, 40)[3], 0.0);

    let mut e = StrokeEngine::new(b.clone(), [1.0, 1.0, 1.0], 7);
    let mut dabs = e.begin(StrokePoint::at(5.0, 5.0, 1.0, 0.0));
    dabs.extend(e.end());
    assert_eq!(dabs.len(), 1);
}

#[test]
fn taper_thins_the_ends() {
    let b = preset("comic-inker").unwrap();
    let mut e = StrokeEngine::new(b, [0.0; 3], 1);
    let mut dabs = e.begin(StrokePoint::at(0.0, 0.0, 1.0, 0.0));
    for i in 1..=40 {
        dabs.extend(e.add(StrokePoint::at(i as f64 * 5.0, 0.0, 1.0, i as f64 * 8.0)));
    }
    dabs.extend(e.end());
    let mid = dabs[dabs.len() / 2].radius;
    assert!(dabs[0].radius < mid * 0.2, "entry {} vs {mid}", dabs[0].radius);
    assert!(dabs.last().unwrap().radius < mid * 0.2, "exit");
    // Dabs come out in stroke order.
    assert!(dabs.windows(2).all(|w| w[0].x <= w[1].x + 1e-9));
}
