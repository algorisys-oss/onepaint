#![allow(clippy::unwrap_used)]
//! Spec 18 colour and fill kinds: color-to-alpha, replace-color,
//! selective-color (bun goldens), screentone and the fill layers.

use onepaint_color::{linear_to_srgb, srgb_to_linear};
use onepaint_filters::pattern::{PATTERNS, pattern_pixels};
use onepaint_filters::*;

/// An opaque pixel from straight sRGB.
fn rgb(c: [f32; 3]) -> [f32; 4] {
    [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]), 1.0]
}

fn srgb(p: [f32; 4]) -> [f32; 3] {
    let a = p[3];
    [0, 1, 2].map(|i| linear_to_srgb(if a > 0.0 { p[i] / a } else { p[i] }))
}

fn near3(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= eps)
}

fn run(kind: Kind, p: &Params, px: [f32; 4]) -> [f32; 4] {
    prepare(kind, p).color_px(px, 3, 4)
}

#[test]
fn color_to_alpha_knocks_out_the_target() {
    let p = default_params(Kind::ColorToAlpha);
    let o = run(Kind::ColorToAlpha, &p, rgb([1.0; 3]));
    assert!(o[3] < 1e-5, "white over white target goes clear: {o:?}");
    // 50 % sRGB grey over a white target = black at half opacity.
    let o = run(Kind::ColorToAlpha, &p, rgb([0.5; 3]));
    assert!((o[3] - 0.5).abs() < 1e-3, "{o:?}");
    assert!(near3(srgb(o), [0.0; 3], 2e-3), "{:?}", srgb(o));
    let o = run(Kind::ColorToAlpha, &p, rgb([0.0; 3]));
    assert!((o[3] - 1.0).abs() < 1e-6 && near3(srgb(o), [0.0; 3], 1e-4));
    // Threshold widens the knock-out.
    let mut q = p.clone();
    q.threshold = Some(0.6);
    assert!(run(Kind::ColorToAlpha, &q, rgb([0.5; 3]))[3] < 1e-5);
}

#[test]
fn replace_color_recolours_the_target_and_keeps_value() {
    let p = Params { target: Some([1.0, 0.0, 0.0]), replace: Some([0.0, 0.0, 1.0]), threshold: Some(0.1), ..Default::default() };
    let o = srgb(run(Kind::ReplaceColor, &p, rgb([1.0, 0.0, 0.0])));
    assert!(near3(o, [0.0, 0.0, 1.0], 1e-3), "{o:?}");
    // A far colour is untouched.
    let o = srgb(run(Kind::ReplaceColor, &p, rgb([0.0, 1.0, 0.0])));
    assert!(near3(o, [0.0, 1.0, 0.0], 1e-3), "{o:?}");
    // Default params: white (the target) becomes the replacement hue at value 1.
    let o = srgb(run(Kind::ReplaceColor, &default_params(Kind::ReplaceColor), rgb([1.0; 3])));
    assert!(near3(o, [0.2, 0.45, 1.0], 2e-3), "{o:?}");
}

#[test]
fn selective_color_matches_happypaints_cpu_reference() {
    let golden: serde_json::Value = serde_json::from_str(include_str!("golden/selective_color.json")).unwrap();
    let mut checked = 0;
    for stack in golden.as_array().unwrap() {
        let samples: Vec<ColorSample> = serde_json::from_value(stack["samples"].clone()).unwrap();
        let p = Params { color_samples: Some(samples), ..Default::default() };
        for case in stack["out"].as_array().unwrap() {
            let i: [f32; 3] = serde_json::from_value(case["in"].clone()).unwrap();
            let want: [f32; 3] = serde_json::from_value(case["out"].clone()).unwrap();
            let got = selective_color_rgb(i, &p);
            assert!(near3(got, want, 2e-4), "in {i:?}: got {got:?}, want {want:?}");
            // Through the full pixel pipeline (linear storage ↔ sRGB work space).
            let px = srgb(run(Kind::SelectiveColor, &p, rgb(i)));
            assert!(near3(px, want, 2e-3), "pipeline {i:?}: {px:?} vs {want:?}");
            checked += 1;
        }
    }
    assert_eq!(checked, 108);
}

#[test]
fn selective_color_range_preview_and_cap() {
    let mut p = default_params(Kind::SelectiveColor);
    p.show_range = Some(true);
    let red = srgb(run(Kind::SelectiveColor, &p, rgb([1.0, 0.0, 0.0])));
    assert!(red[0] > 0.7 && red[1] < 0.3, "selected red floods red: {red:?}");
    let blue = srgb(run(Kind::SelectiveColor, &p, rgb([0.0, 0.0, 1.0])));
    assert!(near3(blue, [blue[0]; 3], 1e-3), "unselected goes grey: {blue:?}");
    // Samples past the cap are ignored.
    let mut many = vec![ColorSample { target: [1.0, 0.0, 0.0], reach: 0.25, falloff: 0.15, dh: 0.0, ds: 0.0, dv: 0.0, enabled: None }; 10];
    many[9].dh = 120.0;
    let q = Params { color_samples: Some(many), ..Default::default() };
    assert!(near3(selective_color_rgb([1.0, 0.0, 0.0], &q), [1.0, 0.0, 0.0], 1e-6));
}

fn tone_at(p: &Params, frame: Frame, s: [f32; 4], x: i32, y: i32) -> [f32; 4] {
    prepare_in(Kind::Tone, p, frame).color_px(s, x, y)
}

#[test]
fn tone_lays_dots_with_the_documented_coverage() {
    let frame = Frame::region(0, 0, 600, 600, 300.0);
    let mut p = default_params(Kind::Tone);
    p.angle = Some(0.0);
    // cell = 300 / 60 = 5 px: a 60×60 patch holds 144 whole cells.
    let mean = |p: &Params| {
        let mut sum = 0.0;
        for y in 0..60 {
            for x in 0..60 {
                sum += tone_at(p, frame, [0.0; 4], x, y)[3];
            }
        }
        sum / 3600.0
    };
    p.tone_level = Some(0.4);
    let r = 0.4 * 5.0 * 0.72;
    let expect = std::f32::consts::PI * r * r / 25.0;
    assert!((mean(&p) - expect).abs() < 0.03, "{} vs {expect}", mean(&p));
    p.tone_level = Some(0.0);
    assert_eq!(mean(&p), 0.0);
    // Hard dots are binary.
    p.tone_level = Some(0.5);
    p.hard_dots = Some(true);
    for x in 0..20 {
        let a = tone_at(&p, frame, [0.0; 4], x, 7)[3];
        assert!(a == 0.0 || a == 1.0);
    }
    // The ink colour is honoured (premultiplied by coverage).
    p.target = Some([1.0, 0.0, 0.0]);
    let o = tone_at(&p, frame, [0.0; 4], 2, 2);
    assert_eq!(o[3], 1.0, "cell centre is inked");
    assert!(o[0] > 0.99 && o[1] < 1e-6);
}

#[test]
fn tone_density_modes() {
    let frame = Frame::region(0, 0, 100, 100, 300.0);
    let mut p = default_params(Kind::Tone);
    p.tone_density = Some("luminance".into());
    p.hard_dots = Some(true);
    // White art carries no tone; black art is full.
    let white: f32 = (0..25).map(|i| tone_at(&p, frame, rgb([1.0; 3]), i % 5, i / 5)[3]).sum();
    assert_eq!(white, 0.0);
    let black: f32 = (0..25).map(|i| tone_at(&p, frame, rgb([0.0; 3]), i % 5, i / 5)[3]).sum();
    assert!(black > 20.0, "{black}");
    // Gradient: top at toneLevel 0, bottom at 1 (0 rad = top → bottom).
    p.tone_density = Some("gradient".into());
    (p.tone_level, p.tone_level_end) = (Some(0.0), Some(1.0));
    let top: f32 = (0..25).map(|i| tone_at(&p, frame, [0.0; 4], i % 5, i / 5)[3]).sum();
    let bottom: f32 = (0..25).map(|i| tone_at(&p, frame, [0.0; 4], i % 5, 95 + i / 5)[3]).sum();
    assert!(top < 1.0 && bottom > 20.0, "{top} {bottom}");
}

#[test]
fn solid_fill_covers_with_the_linear_colour() {
    let p = Params { fill_color: Some([0.5, 0.25, 1.0]), ..Default::default() };
    let o = prepare(Kind::SolidFill, &p).color_px([0.0; 4], 0, 0);
    assert!(near3(srgb(o), [0.5, 0.25, 1.0], 1e-3) && o[3] == 1.0, "{o:?}");
}

#[test]
fn gradient_fill_ramps_between_its_ends() {
    let frame = Frame::region(0, 0, 100, 100, 300.0);
    let p = default_params(Kind::GradientFill);
    let pr = prepare_in(Kind::GradientFill, &p, frame);
    // The ramp mixes linearised stops, so check linear values.
    let top = pr.color_px([0.0; 4], 50, 0);
    let mid = pr.color_px([0.0; 4], 50, 50);
    let bot = pr.color_px([0.0; 4], 50, 99);
    assert!(top[0] < 0.01 && bot[0] > 0.99, "{top:?} {bot:?}");
    assert!((mid[0] - 0.505).abs() < 0.01, "{mid:?}");
    // Reverse flips it; opacity stops become alpha.
    let mut r = p.clone();
    r.grad_reverse = Some(true);
    assert!(prepare_in(Kind::GradientFill, &r, frame).color_px([0.0; 4], 50, 0)[0] > 0.99);
    let mut t = p.clone();
    t.fill_stops.as_mut().unwrap()[1].opacity = 0.0;
    let o = prepare_in(Kind::GradientFill, &t, frame).color_px([0.0; 4], 50, 99);
    assert!(o[3] < 0.02, "{o:?}");
}

#[test]
fn pattern_tiles_match_happypaints_bytes() {
    let golden: serde_json::Value = serde_json::from_str(include_str!("golden/patterns.json")).unwrap();
    for (id, _) in PATTERNS {
        let want: Vec<u8> = serde_json::from_value(golden[id].clone()).unwrap();
        let got = pattern_pixels(id, [0.1, 0.2, 0.9], [1.0, 0.95, 0.4]);
        assert_eq!(got.data, want, "{id}");
    }
}

#[test]
fn pattern_fill_repeats_at_its_scale() {
    let frame = Frame::region(0, 0, 256, 256, 300.0);
    let mut p = default_params(Kind::PatternFill);
    p.pattern_scale = Some(2.0);
    let pr = prepare_in(Kind::PatternFill, &p, frame);
    for (x, y) in [(3, 5), (40, 17), (100, 90)] {
        let a = pr.color_px([0.0; 4], x, y);
        let b = pr.color_px([0.0; 4], x + 64, y);
        let c = pr.color_px([0.0; 4], x, y + 128);
        for k in 0..4 {
            assert!((a[k] - b[k]).abs() < 1e-4 && (a[k] - c[k]).abs() < 1e-4);
        }
        assert_eq!(a[3], 1.0);
    }
    // A checker: black and white both appear.
    let vals: Vec<f32> = (0..64).map(|x| pr.color_px([0.0; 4], x, 10)[0]).collect();
    assert!(vals.iter().any(|v| *v < 0.01) && vals.iter().any(|v| *v > 0.99));
}
