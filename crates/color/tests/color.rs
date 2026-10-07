#![allow(clippy::unwrap_used)]
use onepaint_color::*;

fn close(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

#[test]
fn srgb_known_points() {
    assert_eq!(srgb_to_linear(0.0), 0.0);
    assert!(close(srgb_to_linear(1.0), 1.0, 1e-6));
    assert!(close(srgb_to_linear(0.5), 0.214_041_14, 1e-6));
    assert!(close(linear_to_srgb(1.0), 1.0, 1e-6));
    assert_eq!(linear_to_srgb(-0.5), 0.0);
}

#[test]
fn srgb_round_trip() {
    for i in 0..=1000 {
        let x = i as f32 / 1000.0;
        assert!(close(srgb_to_linear(linear_to_srgb(x)), x, 1e-5), "x={x}");
    }
}

#[test]
fn premultiply_round_trip_and_zero_alpha() {
    let c = [0.2, 0.4, 0.6, 0.5];
    let p = premultiply(c);
    assert_eq!(p, [0.1, 0.2, 0.3, 0.5]);
    let u = unpremultiply(p);
    for i in 0..4 {
        assert!(close(u[i], c[i], 1e-6));
    }
    assert_eq!(unpremultiply([0.3, 0.3, 0.3, 0.0]), [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn rgba_to_linear_premul() {
    let red_half = Rgba::new(1.0, 0.0, 0.0, 0.5);
    assert_eq!(red_half.to_linear_premul(), [0.5, 0.0, 0.0, 0.5]);
    let grey = Rgba::new(0.5, 0.5, 0.5, 1.0).to_linear_premul();
    assert!(close(grey[0], 0.214_041_14, 1e-6));
}

#[test]
fn hex_parse_and_format() {
    let c = Rgba::from_hex("#ff8000").unwrap();
    assert_eq!(c.to_hex(), "#ff8000");
    assert!(close(c.g, 128.0 / 255.0, 1e-6));
    assert!(Rgba::from_hex("zz").is_none());
    assert_eq!(Rgba::from_hex("4c8dff").unwrap().to_hex(), "#4c8dff");
}

#[test]
fn hsv_round_trip() {
    for &(h, s, v) in &[(0.0, 1.0, 1.0), (120.0, 0.5, 0.75), (300.0, 0.25, 0.4), (0.0, 0.0, 0.3)] {
        let c = Rgba::from_hsv(h, s, v, 1.0);
        let (h2, s2, v2) = c.to_hsv();
        assert!(close(s2, s, 1e-5) && close(v2, v, 1e-5), "{h} {s} {v}");
        if s > 0.0 {
            assert!(close(h2, h, 1e-3), "{h} vs {h2}");
        }
    }
    let red = Rgba::from_hsv(0.0, 1.0, 1.0, 1.0);
    assert_eq!((red.r, red.g, red.b), (1.0, 0.0, 0.0));
}

#[test]
fn fast_transfer_curves_match_exact() {
    let mut worst = (0.0f32, 0.0f32);
    for i in 0..=100_000 {
        let x = i as f32 / 100_000.0;
        worst.0 = worst.0.max((fast_srgb_to_linear(x) - srgb_to_linear(x)).abs());
        worst.1 = worst.1.max((fast_linear_to_srgb(x) - linear_to_srgb(x)).abs());
    }
    assert!(worst.0 < 2e-5 && worst.1 < 1e-4, "{worst:?}");
    // Out-of-range inputs fall back to the exact curves.
    assert_eq!(fast_linear_to_srgb(-1.0), 0.0);
    assert_eq!(fast_srgb_to_linear(1.5), srgb_to_linear(1.5));
}
