#![allow(clippy::unwrap_used)]
use onepaint_color::{linear_to_srgb, srgb_to_linear};
use onepaint_filters::*;
use onepaint_stroke::CurvePoint;

/// An opaque grey whose sRGB value is `v`, as a premultiplied linear pixel.
fn grey(v: f32) -> [f32; 4] {
    let l = srgb_to_linear(v);
    [l, l, l, 1.0]
}

fn srgb(p: [f32; 4]) -> [f32; 3] {
    let a = p[3];
    [0, 1, 2].map(|i| linear_to_srgb(p[i] / a))
}

fn near(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

fn run(kind: Kind, p: Params, px: [f32; 4]) -> [f32; 4] {
    prepare(kind, &p).color_px(px, 0, 0)
}

#[test]
fn kinds_parse_and_classify() {
    for k in Kind::ALL {
        assert_eq!(Kind::parse(k.as_str()), Some(k));
    }
    assert_eq!(Kind::parse("clouds"), Some(Kind::Clouds));
    assert_eq!(Kind::parse("pattern-fill-from-the-future"), None);
    assert!(Kind::Curves.is_color() && !Kind::GaussianBlur.is_color());
}

#[test]
fn brightness_contrast() {
    let o = run(Kind::BrightnessContrast, Params { brightness: Some(0.2), ..Default::default() }, grey(0.5));
    assert!(near(srgb(o)[0], 0.7, 1e-4), "{:?}", srgb(o));
    let o = run(Kind::BrightnessContrast, Params { contrast: Some(0.5), ..Default::default() }, grey(0.5));
    assert!(near(srgb(o)[0], 0.5, 1e-4));
    let o = run(Kind::BrightnessContrast, Params { contrast: Some(0.5), ..Default::default() }, grey(0.75));
    assert!(near(srgb(o)[0], 1.0, 1e-4));
    let o = run(Kind::BrightnessContrast, Params { contrast: Some(-0.5), ..Default::default() }, grey(1.0));
    assert!(near(srgb(o)[0], 0.75, 1e-4));
}

#[test]
fn simple_colour_kinds() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let inv = run(Kind::Invert, Params::default(), red);
    assert_eq!(srgb(inv).map(|v| (v * 1000.0).round() / 1000.0), [0.0, 1.0, 1.0]);
    let t = run(Kind::Threshold, Params::default(), grey(0.6));
    assert!(near(srgb(t)[0], 1.0, 1e-5));
    let t = run(Kind::Threshold, Params::default(), grey(0.4));
    assert!(near(t[0], 0.0, 1e-6));
    let p = run(Kind::Posterize, Params { levels: Some(2.0), ..Default::default() }, grey(0.6));
    assert!(near(srgb(p)[0], 1.0, 1e-5));
    let l = run(Kind::Levels, Params { gamma: Some(2.0), ..Default::default() }, grey(0.25));
    assert!(near(srgb(l)[0], 0.5, 1e-4));
    // Photoshop's default weights: pure red reads its Reds weight.
    let bw = run(Kind::BlackWhite, Params::default(), red);
    let s = srgb(bw);
    assert!(near(s[0], 0.4, 1e-4) && near(s[1], 0.4, 1e-4), "{s:?}");
    // Half-transparent pixels keep alpha and stay premultiplied.
    let half = [0.5, 0.0, 0.0, 0.5];
    let o = run(Kind::Invert, Params::default(), half);
    assert!(near(o[3], 0.5, 1e-6) && near(o[0], 0.0, 1e-6) && near(o[1], 0.5, 1e-5));
}

#[test]
fn hue_saturation() {
    let c = [0.2, 0.5, 0.8, 1.0];
    let o = run(Kind::HueSaturation, Params::default(), c);
    for i in 0..4 {
        assert!(near(o[i], c[i], 1e-5), "{o:?}");
    }
    let g = run(Kind::HueSaturation, Params { hue: Some(120.0), ..Default::default() }, [1.0, 0.0, 0.0, 1.0]);
    assert!(g[1] > 0.99 && g[0] < 1e-4 && g[2] < 1e-4, "{g:?}");
    let d = run(Kind::HueSaturation, Params { saturation: Some(-1.0), ..Default::default() }, [1.0, 0.0, 0.0, 1.0]);
    assert!(near(d[0], d[1], 1e-5) && near(d[1], d[2], 1e-5));
}

/// happypaint v0.201.3 Black & White: the primary hue's weight over
/// max − mid, the secondary's over mid − min; the tint at the grey's
/// luminosity; Amount mixes.
#[test]
fn black_white_weighs_hues_and_tints() {
    // (0.8, 0.5, 0.2) in sRGB: red primary, yellow secondary.
    let c = [srgb_to_linear(0.8), srgb_to_linear(0.5), srgb_to_linear(0.2), 1.0];
    let p = Params { bw_reds: Some(1.0), bw_yellows: Some(0.5), ..Default::default() };
    let grey = 0.2 + (0.5 - 0.2) * 0.5 + (0.8 - 0.5) * 1.0;
    assert!(near(srgb(run(Kind::BlackWhite, p.clone(), c))[1], grey, 1e-4));
    let half = srgb(run(Kind::BlackWhite, Params { amount: Some(0.5), ..p.clone() }, c));
    assert!(near(half[0], 0.8 + (grey - 0.8) * 0.5, 1e-4), "{half:?}");
    // A tint keeps the grey's luminosity (0.3, 0.59, 0.11).
    let t = srgb(run(Kind::BlackWhite, Params { bw_tint: Some(true), bw_tint_color: Some([0.9, 0.6, 0.3]), ..p }, c));
    assert!(near(0.3 * t[0] + 0.59 * t[1] + 0.11 * t[2], grey, 1e-3) && t[0] > t[2], "{t:?}");
}

/// happypaint v0.201.3 Color Balance: shifts per tone range, weighted by
/// HSL lightness; Preserve Luminosity (on by default) puts the luminosity
/// back, so mid-grey + red 100 is (217, 89, 89).
#[test]
fn color_balance_shifts_by_range_and_keeps_luminosity() {
    let mid = srgb(run(Kind::ColorBalance, Params { cyan_red: Some(1.0), ..Default::default() }, grey(0.5)));
    assert!(near(mid[0], 0.85, 1e-3) && near(mid[1], 0.35, 1e-3), "{mid:?}");
    let raw = Params { cyan_red: Some(1.0), preserve_lum: Some(false), ..Default::default() };
    assert!(near(srgb(run(Kind::ColorBalance, raw, grey(0.5)))[0], 1.0, 1e-4));
    // Shadows reach black; highlights reach white; each leaves the others.
    let sh = Params { cb_shadow_cr: Some(1.0), preserve_lum: Some(false), ..Default::default() };
    assert!(near(srgb(run(Kind::ColorBalance, sh.clone(), grey(0.0)))[0], 0.7, 1e-4));
    assert!(near(srgb(run(Kind::ColorBalance, sh, grey(1.0)))[1], 1.0, 1e-4));
    let hi = Params { cb_high_yb: Some(-1.0), preserve_lum: Some(false), ..Default::default() };
    assert!(near(srgb(run(Kind::ColorBalance, hi.clone(), grey(1.0)))[2], 0.3, 1e-4));
    assert!(near(srgb(run(Kind::ColorBalance, hi, grey(0.0)))[2], 0.0, 1e-6));
}

#[test]
fn curves_lut_and_texel_lookup() {
    let lut = build_curves_lut(&Params::default());
    for i in [0usize, 64, 128, 255] {
        assert_eq!(lut[i][0] as usize, i);
    }
    // Lookup at v interpolates between texel centres (v·256 − 0.5).
    let v: f32 = 0.5;
    let t = v * 256.0 - 0.5;
    let exp = (127.0 + (t - 127.0)) / 255.0;
    assert!(near(lut_sample(&lut, v, 0), exp, 1e-6));
    // Identity curves reproduce the input within 8-bit quantisation.
    let o = run(Kind::Curves, Params::default(), grey(0.3));
    assert!(near(srgb(o)[0], 0.3, 1.0 / 255.0));
    assert!(is_identity(Kind::Curves, &Params::default()));
    let inverted = Params { curve: Some(vec![CurvePoint::new(0.0, 1.0), CurvePoint::new(1.0, 0.0)]), ..Default::default() };
    assert!(!is_identity(Kind::Curves, &inverted));
    let o = run(Kind::Curves, inverted, grey(0.2));
    assert!(near(srgb(o)[0], 0.8, 2.0 / 255.0));
}

#[test]
fn gradient_map_black_to_white_is_luma() {
    let o = run(Kind::GradientMap, Params::default(), [1.0, 0.0, 0.0, 1.0]);
    assert!(near(srgb(o)[0], 0.299, 2.0 / 255.0));
}

#[test]
fn noise_is_deterministic_and_zero_is_identity() {
    let p = Params { amount: Some(0.5), ..Default::default() };
    let pr = prepare(Kind::Noise, &p);
    let a = pr.color_px(grey(0.5), 10, 20);
    let b = pr.color_px(grey(0.5), 10, 20);
    let c = pr.color_px(grey(0.5), 11, 20);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert!(a[0] == a[1] && a[1] == a[2], "mono by default");
    assert!(is_identity(Kind::Noise, &Params::default()));
}

#[test]
fn blur_and_sharpen() {
    let (w, h) = (40usize, 30usize);
    let flat = [0.3f32, 0.2, 0.1, 1.0].repeat(w * h);
    let b = apply_spatial(Kind::GaussianBlur, &Params { radius: Some(5.0), ..Default::default() }, &flat, w, h);
    assert!(b.iter().zip(&flat).all(|(x, y)| (x - y).abs() < 1e-5), "flat stays flat");
    // A single bright pixel spreads but keeps its total (away from edges).
    let mut dot = vec![0.0f32; w * h * 4];
    let i = (15 * w + 20) * 4;
    dot[i..i + 4].copy_from_slice(&[1.0, 1.0, 1.0, 1.0]);
    let b = apply_spatial(Kind::GaussianBlur, &Params { radius: Some(4.0), ..Default::default() }, &dot, w, h);
    let total: f32 = b.chunks_exact(4).map(|p| p[3]).sum();
    assert!(near(total, 1.0, 1e-3), "{total}");
    assert!(b[i + 3] < 0.5 && b[i + 3] > 0.0);
    let m = apply_spatial(Kind::MotionBlur, &Params { distance: Some(6.0), angle: Some(0.0), ..Default::default() }, &dot, w, h);
    assert_eq!(m[((15 + 3) * w + 20) * 4 + 3], 0.0, "horizontal motion blur stays on the row");
    assert!(m[(15 * w + 23) * 4 + 3] > 0.0);
    let s = apply_spatial(Kind::Sharpen, &Params { amount: Some(0.5), ..Default::default() }, &flat, w, h);
    assert!(s.iter().zip(&flat).all(|(x, y)| (x - y).abs() < 1e-5));
    assert!(is_identity(Kind::GaussianBlur, &Params::default()));
}

#[test]
fn params_round_trip_happypaint_json() {
    let j = serde_json::json!({"brightness": 0.5, "contrast": -0.4, "curveMode": "linear", "gradientStops": [{"pos": 0, "color": [0, 0, 0]}], "zzz": 1});
    let p: Params = serde_json::from_value(j.clone()).unwrap();
    assert_eq!(p.brightness, Some(0.5));
    assert_eq!(p.curve_mode.as_deref(), Some("linear"));
    let back = serde_json::to_value(&p).unwrap();
    assert_eq!(back.get("zzz"), Some(&serde_json::json!(1)), "unknown fields survive");
    assert_eq!(default_params(Kind::Levels).gamma, Some(1.0));
}
