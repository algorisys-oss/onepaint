#![allow(clippy::unwrap_used)]
//! Spec 18 spatial kinds: blur gallery, distort, stylize and pixelate.

use onepaint_filters::*;

fn flat(w: usize, h: usize, px: [f32; 4]) -> Vec<f32> {
    (0..w * h).flat_map(|_| px).collect()
}

/// A left/right split: opaque red on the left half, opaque blue on the right.
fn split(w: usize, h: usize) -> Vec<f32> {
    (0..w * h).flat_map(|i| if i % w < w / 2 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] }).collect()
}

/// A smooth gradient image (distinct values everywhere).
fn ramp(w: usize, h: usize) -> Vec<f32> {
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
            [x, y, 0.5 * x * y, 1.0]
        })
        .collect()
}

fn px(buf: &[f32], w: usize, x: usize, y: usize) -> [f32; 4] {
    let i = (y * w + x) * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

fn close(a: [f32; 4], b: [f32; 4], eps: f32) -> bool {
    (0..4).all(|k| (a[k] - b[k]).abs() <= eps)
}

fn all_close(a: &[f32], b: &[f32], eps: f32) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= eps)
}

const W: usize = 64;
const H: usize = 48;

#[test]
fn flat_fields_stay_flat() {
    let grey = [0.3, 0.3, 0.3, 1.0];
    let src = flat(W, H, grey);
    for k in [
        Kind::RadialBlur,
        Kind::TiltShift,
        Kind::IrisBlur,
        Kind::FieldBlur,
        Kind::SpinBlur,
        Kind::Twirl,
        Kind::Pinch,
        Kind::Ripple,
        Kind::PolarCoordinates,
        Kind::Mosaic,
        Kind::Crystallize,
        Kind::OilPaint,
        Kind::Wind,
        Kind::ChromaticAberration,
        Kind::Glitch,
    ] {
        let out = apply_spatial(k, &default_params(k), &src, W, H);
        assert!(all_close(&out, &src, 1e-4), "{k:?} changed a flat field");
    }
}

#[test]
fn identity_params_return_the_source() {
    let src = ramp(W, H);
    for (k, json) in [
        (Kind::RadialBlur, r#"{"amount":0}"#),
        (Kind::Twirl, r#"{"angle":0}"#),
        (Kind::Pinch, r#"{"amount":0}"#),
        (Kind::Ripple, r#"{"amplitude":0}"#),
        (Kind::Mosaic, r#"{"cellSize":1}"#),
        (Kind::OilPaint, r#"{"radius":0}"#),
        (Kind::TiltShift, r#"{"radius":0}"#),
        (Kind::SpinBlur, r#"{"spin":0}"#),
        (Kind::Bloom, r#"{"amount":0}"#),
    ] {
        let p: Params = serde_json::from_str(json).unwrap();
        assert_eq!(apply_spatial(k, &p, &src, W, H), src, "{k:?}");
    }
}

#[test]
fn distorts_leave_the_outside_of_their_circle_alone() {
    let src = ramp(W, H);
    for k in [Kind::Twirl, Kind::Pinch] {
        let mut p = default_params(k);
        p.extent = Some(0.5); // radius = 0.25 · 48 = 12 px around (32, 24)
        let out = apply_spatial(k, &p, &src, W, H);
        assert_eq!(px(&out, W, 2, 2), px(&src, W, 2, 2), "{k:?}");
        assert_eq!(px(&out, W, 60, 40), px(&src, W, 60, 40), "{k:?}");
        assert!(!close(px(&out, W, 36, 24), px(&src, W, 36, 24), 1e-4), "{k:?} moved the inside");
    }
    // Spin blur is exact outside its ellipse.
    let mut p = default_params(Kind::SpinBlur);
    p.extent = Some(0.4);
    let out = apply_spatial(Kind::SpinBlur, &p, &src, W, H);
    assert_eq!(px(&out, W, 1, 1), px(&src, W, 1, 1));
}

#[test]
fn twirl_rotates_by_the_documented_angle() {
    let src = ramp(W, H);
    let mut p = default_params(Kind::Twirl);
    (p.angle, p.extent) = (Some(90.0), Some(1.0)); // R = 24
    let out = apply_spatial(Kind::Twirl, &p, &src, W, H);
    // Pixel (38, 24): d = (6.5, 0.5), r ≈ 6.52, θ = 90°·(1 − r/24)².
    let d = [6.5f32, 0.5];
    let r = d[0].hypot(d[1]);
    let a = (90.0f32).to_radians() * (1.0 - r / 24.0).powi(2);
    let q = [32.0 + a.cos() * d[0] - a.sin() * d[1], 24.0 + a.sin() * d[0] + a.cos() * d[1]];
    // Bilinear read of the ramp at that doc position (texel centres at +0.5).
    let (fx, fy) = (q[0] - 0.5, q[1] - 0.5);
    let want = [fx / W as f32, fy / H as f32];
    let got = px(&out, W, 38, 24);
    assert!((got[0] - want[0]).abs() < 2e-3 && (got[1] - want[1]).abs() < 2e-3, "{got:?} vs {want:?}");
}

#[test]
fn to_polar_gathers_the_top_row_into_the_centre() {
    // Top row red, everything else blue.
    let src: Vec<f32> = (0..W * H).flat_map(|i| if i / W == 0 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] }).collect();
    let out = apply_spatial(Kind::PolarCoordinates, &default_params(Kind::PolarCoordinates), &src, W, H);
    assert!(px(&out, W, 32, 24)[0] > 0.2, "centre reads the top row: {:?}", px(&out, W, 32, 24));
    assert!(px(&out, W, 32, 2)[2] > 0.99, "the rim reads the bottom: {:?}", px(&out, W, 32, 2));
}

#[test]
fn ripple_displaces_by_at_most_its_height() {
    let src = ramp(W, H);
    let p = default_params(Kind::Ripple);
    let out = apply_spatial(Kind::Ripple, &p, &src, W, H);
    assert!(!all_close(&out, &src, 1e-4));
    let amp = p.amplitude.unwrap() * 1.36;
    for (x, y) in [(20, 20), (40, 30), (10, 40)] {
        let g = px(&out, W, x, y);
        let (gx, gy) = (g[0] * W as f32 + 0.5, g[1] * H as f32 + 0.5);
        assert!((gx - (x as f32 + 0.5)).abs() <= amp + 0.5 && (gy - (y as f32 + 0.5)).abs() <= amp + 0.5);
    }
    assert!(reach(Kind::Ripple, &p) as f32 >= amp);
}

#[test]
fn radial_zoom_keeps_the_centre_and_blurs_the_rim() {
    // Red left of x = 16, blue elsewhere: that edge is not radial to the centre.
    let src: Vec<f32> = (0..W * H).flat_map(|i| if i % W < 16 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] }).collect();
    let p = Params { amount: Some(1.0), ..default_params(Kind::RadialBlur) };
    let out = apply_spatial(Kind::RadialBlur, &p, &src, W, H);
    // The centre reads only itself.
    assert!(close(px(&out, W, 32, 24), [0.0, 0.0, 1.0, 1.0], 1e-4));
    // A corner's ray toward the centre crosses the edge and mixes.
    let e = px(&out, W, 2, 2);
    assert!(e[0] > 0.05 && e[2] > 0.05, "{e:?}");
    let src = split(W, H);
    // Spin: a pixel on the horizontal axis right of centre stays blue-ish but the arc mixes.
    let mut s = p.clone();
    s.radial_mode = Some("spin".into());
    let o = apply_spatial(Kind::RadialBlur, &s, &src, W, H);
    assert!(close(px(&o, W, 63, 24), [0.0, 0.0, 1.0, 1.0], 0.05));
}

#[test]
fn tilt_shift_keeps_the_band_sharp() {
    let src = split(W, H);
    let p = default_params(Kind::TiltShift); // focus band ±0.1·48 ≈ 4.8 px around y = 24
    let out = apply_spatial(Kind::TiltShift, &p, &src, W, H);
    for x in 28..36 {
        assert_eq!(px(&out, W, x, 24), px(&src, W, x, 24), "inside the band");
    }
    let far = px(&out, W, 31, 1);
    assert!(far[2] > 0.1 && far[0] > 0.1, "outside the band blurs the edge: {far:?}");
    // Iris: the core is sharp.
    let o = apply_spatial(Kind::IrisBlur, &default_params(Kind::IrisBlur), &src, W, H);
    assert_eq!(px(&o, W, 31, 24), px(&src, W, 31, 24));
    // Bokeh variant still keeps the core sharp and blurs the corners.
    let mut b = default_params(Kind::IrisBlur);
    b.bokeh = Some(1.0);
    let o = apply_spatial(Kind::IrisBlur, &b, &src, W, H);
    assert_eq!(px(&o, W, 31, 24), px(&src, W, 31, 24));
    assert!(!close(px(&o, W, 31, 0), px(&src, W, 31, 0), 1e-3));
}

#[test]
fn field_blur_is_sharp_at_a_zero_pin() {
    let src = split(W, H);
    let p: Params = serde_json::from_str(r#"{"pins":[{"x":0.5,"y":0.0,"blur":0},{"x":0.5,"y":1.0,"blur":20}]}"#).unwrap();
    let out = apply_spatial(Kind::FieldBlur, &p, &src, W, H);
    assert!(close(px(&out, W, 31, 0), px(&src, W, 31, 0), 0.02));
    let b = px(&out, W, 31, 47);
    assert!(b[2] > 0.15, "{b:?}");
}

#[test]
fn mosaic_cells_are_constant() {
    let src = ramp(W, H);
    let p = Params { cell_size: Some(8.0), ..Default::default() };
    let out = apply_spatial(Kind::Mosaic, &p, &src, W, H);
    let c = px(&out, W, 8, 8);
    for y in 8..16 {
        for x in 8..16 {
            assert!(close(px(&out, W, x, y), c, 1e-6));
        }
    }
    // The tile mean of a linear ramp is its centre value.
    assert!((c[0] - 11.5 / W as f32).abs() < 1e-4, "{c:?}");
}

#[test]
fn crystallize_makes_facets_from_the_source() {
    let src = split(W, H);
    let p = Params { cell_size: Some(10.0), ..Default::default() };
    let out = apply_spatial(Kind::Crystallize, &p, &src, W, H);
    let reds = (0..W).filter(|&x| px(&out, W, x, 5)[0] > 0.99).count();
    assert!(reds > 20 && reds < 44, "{reds}");
    assert!(reach(Kind::Crystallize, &p) >= 30);
}

#[test]
fn emboss_relief_and_grey() {
    let grey = flat(W, H, [0.2, 0.2, 0.2, 1.0]);
    let out = apply_spatial(Kind::Emboss, &default_params(Kind::Emboss), &grey, W, H);
    // Grey relief of a flat field is 0.5 sRGB.
    let half = onepaint_color::srgb_to_linear(0.5);
    assert!(close(px(&out, W, 30, 20), [half, half, half, 1.0], 1e-3));
    // Colour mode at zero strength is the identity.
    let p = Params { mono: Some(false), amount: Some(0.0), ..Default::default() };
    assert_eq!(apply_spatial(Kind::Emboss, &p, &grey, W, H), grey);
    // An edge lights up on one side and shades on the other.
    let out = apply_spatial(Kind::Emboss, &default_params(Kind::Emboss), &split(W, H), W, H);
    let (l, r) = (px(&out, W, 31, 20)[0], px(&out, W, 32, 20)[0]);
    assert!((l - half).abs() > 0.01 || (r - half).abs() > 0.01);
}

#[test]
fn oil_paint_and_wind_change_edges_only() {
    let src = split(W, H);
    let out = apply_spatial(Kind::OilPaint, &default_params(Kind::OilPaint), &src, W, H);
    assert!(close(px(&out, W, 5, 20), [1.0, 0.0, 0.0, 1.0], 1e-3));
    // Wind blowing right carries the brighter (red has luma .299, blue .114) side over.
    let p = Params { angle: Some(0.0), distance: Some(20.0), amount: Some(1.0), ..Default::default() };
    let out = apply_spatial(Kind::Wind, &p, &src, W, H);
    assert!(px(&out, W, 33, 20)[0] > 0.1, "{:?}", px(&out, W, 33, 20));
    assert!(close(px(&out, W, 5, 20), [1.0, 0.0, 0.0, 1.0], 1e-6));
}

#[test]
fn bloom_screens_the_glow_and_keeps_alpha() {
    // A bright square on a dark opaque field.
    let src: Vec<f32> = (0..W * H)
        .flat_map(|i| {
            let (x, y) = (i % W, i / W);
            if (28..36).contains(&x) && (20..28).contains(&y) { [1.0, 1.0, 1.0, 1.0] } else { [0.01, 0.01, 0.01, 1.0] }
        })
        .collect();
    let out = apply_spatial(Kind::Bloom, &default_params(Kind::Bloom), &src, W, H);
    assert!(px(&out, W, 26, 24)[0] > 0.02, "glow spills: {:?}", px(&out, W, 26, 24));
    assert!(out.chunks(4).all(|p| (p[3] - 1.0).abs() < 1e-6));
    // Threshold 1: nothing is bright enough.
    let p = Params { threshold: Some(1.0), ..default_params(Kind::Bloom) };
    let o = apply_spatial(Kind::Bloom, &p, &src, W, H);
    assert!(close(px(&o, W, 5, 5), px(&src, W, 5, 5), 1e-4));
    // Transparent stays transparent.
    let clear = flat(W, H, [0.0; 4]);
    assert!(apply_spatial(Kind::Bloom, &default_params(Kind::Bloom), &clear, W, H).iter().all(|v| *v == 0.0));
}

#[test]
fn chromatic_and_glitch_split_channels() {
    let src = split(W, H);
    let out = apply_spatial(Kind::ChromaticAberration, &Params { amount: Some(1.0), ..Default::default() }, &src, W, H);
    // The centre column barely shifts; the alpha is the green tap's.
    assert!(out.chunks(4).all(|p| (p[3] - 1.0).abs() < 1e-6));
    let g = apply_spatial(Kind::Glitch, &Params { amount: Some(1.0), ..Default::default() }, &ramp(W, H), W, H);
    assert!(!all_close(&g, &ramp(W, H), 1e-4));
    assert_eq!(g, apply_spatial(Kind::Glitch, &Params { amount: Some(1.0), ..Default::default() }, &ramp(W, H), W, H));
}

#[test]
fn halftone_of_white_is_white_and_dark_gets_dots() {
    // White still gets a sub-pixel speck at each cell centre (radius 0 with a
    // ±1 px feather), as in happypaint; the field stays essentially white.
    let white = flat(W, H, [1.0; 4]);
    let out = apply_spatial(Kind::Halftone, &default_params(Kind::Halftone), &white, W, H);
    let mean = out.chunks(4).map(|p| p[0]).sum::<f32>() / (W * H) as f32;
    assert!(mean > 0.85, "{mean}");
    let dark = flat(W, H, [0.05, 0.05, 0.05, 1.0]);
    let out = apply_spatial(Kind::Halftone, &default_params(Kind::Halftone), &dark, W, H);
    let inked = out.chunks(4).filter(|p| p[0] < 0.05).count();
    assert!(inked > W * H / 2, "{inked}");
}

#[test]
fn line_art_draws_edges_on_a_clear_layer() {
    for mode in ["sobel", "xdog"] {
        let mut p = default_params(Kind::LineArt);
        p.line_mode = Some(mode.into());
        let flat_out = apply_spatial(Kind::LineArt, &p, &flat(W, H, [0.5, 0.5, 0.5, 1.0]), W, H);
        assert!(flat_out.chunks(4).all(|q| q[3] < 0.05), "{mode}: a flat field has no lines");
        let white_black: Vec<f32> = (0..W * H).flat_map(|i| if i % W < W / 2 { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] }).collect();
        let out = apply_spatial(Kind::LineArt, &p, &white_black, W, H);
        let edge = (28..36).map(|x| px(&out, W, x, 20)[3]).fold(0.0, f32::max);
        assert!(edge > 0.5, "{mode}: the edge inks ({edge})");
        assert!(px(&out, W, 2, 20)[3] < 0.05);
        assert_eq!(px(&out, W, 31, 20)[0], 0.0, "black lines");
    }
}

#[test]
fn regions_match_the_whole_document() {
    // A local-reach kind run over a sub-region with its margin agrees with the full run.
    let src = ramp(W, H);
    for k in [Kind::Mosaic, Kind::Emboss, Kind::Halftone, Kind::OilPaint] {
        let p = default_params(k);
        let full = apply_spatial(k, &p, &src, W, H);
        let (x0, y0, rw, rh) = (8usize, 4usize, 40usize, 36usize);
        let mut sub = Vec::with_capacity(rw * rh * 4);
        for y in y0..y0 + rh {
            sub.extend_from_slice(&src[(y * W + x0) * 4..(y * W + x0 + rw) * 4]);
        }
        let frame = Frame::region(x0 as i32, y0 as i32, W as u32, H as u32, 300.0);
        let part = apply_spatial_in(k, &p, &sub, rw, rh, frame);
        let r = reach(k, &p) as usize;
        for y in r.min(rh / 2)..rh.saturating_sub(r).max(rh / 2) {
            for x in r.min(rw / 2)..rw.saturating_sub(r).max(rw / 2) {
                assert!(close(px(&part, rw, x, y), px(&full, W, x + x0, y + y0), 1e-5), "{k:?} at {x},{y}");
            }
        }
    }
}
