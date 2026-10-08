//! Speech balloons (spec 28): ports `engine/text/balloon.test.ts` plus
//! goldens rendered by happypaint's own `rasterizeTexts` in Chromium.
#![allow(clippy::unwrap_used)]
use onepaint_text::balloon::*;
use onepaint_text::*;

fn spec() -> TextSpec {
    TextSpec { text: "hi".into(), font_size_px: 40.0, x: 100.0, y: 200.0, ..Default::default() }
}

fn balloon() -> Balloon {
    Balloon { shape: BalloonShape::Oval, fill: "#ffffff".into(), ..Default::default() }
}

fn with(b: Balloon) -> TextSpec {
    TextSpec { balloon: Some(b), ..spec() }
}

fn tail(x: f64, y: f64, width: Option<f64>) -> Option<Tail> {
    Some(Tail { x, y, width })
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn padding() {
    assert_eq!(balloon_pad(&spec(), 40.0), 0.0);
    let s = with(balloon());
    assert!(close(balloon_pad(&s, 80.0), balloon_pad(&s, 40.0) * 2.0));
    let plain = balloon_pad(&s, 40.0);
    let outlined = balloon_pad(&with(Balloon { stroke: Some("#000".into()), stroke_width: Some(10.0), ..balloon() }), 40.0);
    assert!(close(outlined - plain, 5.0));
    // A stroke width without a stroke colour is not counted.
    assert!(close(balloon_pad(&with(Balloon { stroke_width: Some(10.0), ..balloon() }), 40.0), plain));
    assert!(close(plain, 40.0 * 0.55));
}

#[test]
fn spec_font_size_is_the_largest_run() {
    assert_eq!(spec_font_size(&spec()), 40.0);
    let runs = vec![
        TextRun { text: "a".into(), font_size_px: Some(12.0), ..Default::default() },
        TextRun { text: "b".into(), font_size_px: Some(90.0), ..Default::default() },
    ];
    assert_eq!(spec_font_size(&TextSpec { runs: Some(runs), ..spec() }), 90.0);
    let one = vec![TextRun { text: "a".into(), ..Default::default() }];
    assert_eq!(spec_font_size(&TextSpec { runs: Some(one), ..spec() }), 40.0);
}

#[test]
fn tail_extent_points() {
    assert!(balloon_extra_local_points(&with(balloon())).is_empty());
    assert!(balloon_extra_local_points(&spec()).is_empty());
    let pts = balloon_extra_local_points(&with(Balloon { tail: tail(300.0, -180.0, None), ..balloon() }));
    assert!(pts.contains(&[300.0, -180.0]));
    let pts = balloon_extra_local_points(&with(Balloon { tail: tail(300.0, -180.0, Some(40.0)), ..balloon() }));
    let xs: Vec<f64> = pts.iter().map(|p| p[0]).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p[1]).collect();
    let min = |v: &[f64]| v.iter().copied().fold(f64::INFINITY, f64::min);
    let max = |v: &[f64]| v.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert_eq!((min(&xs), max(&xs), min(&ys), max(&ys)), (280.0, 320.0, -200.0, -160.0));
}

#[test]
fn bounds_grow_to_the_balloon_and_tail() {
    let (w, h) = (120.0, 60.0);
    let ab = |a: Aabb| [a.min_x, a.min_y, a.max_x, a.max_y];
    assert_eq!(ab(spec_aabb(&spec(), w, h, 0.0, &[])), [100.0, 200.0, 220.0, 260.0]);
    assert_eq!(ab(spec_aabb(&spec(), w, h, 10.0, &[])), [90.0, 190.0, 230.0, 270.0]);
    let s = with(Balloon { tail: tail(400.0, -300.0, Some(0.0)), ..balloon() });
    assert_eq!(spec_aabb(&s, w, h, 0.0, &[]).max_x, 220.0);
    let t = spec_aabb(&s, w, h, 0.0, &balloon_extra_local_points(&s));
    assert_eq!((t.max_x, t.min_y), (500.0, -100.0));
    // The tail rotates with the object.
    let s = TextSpec { rotation: std::f64::consts::FRAC_PI_2, ..with(Balloon { tail: tail(400.0, 0.0, Some(0.0)), ..balloon() }) };
    assert!(close(spec_aabb(&s, w, h, 0.0, &balloon_extra_local_points(&s)).max_y, 600.0));
    assert!(close(spec_aabb(&s, w, h, 0.0, &[]).max_y, 200.0 + w));
    // A tail inside the block does not shrink the bounds.
    let s = with(Balloon { tail: tail(10.0, 10.0, Some(0.0)), ..balloon() });
    assert_eq!(ab(spec_aabb(&s, w, h, 20.0, &balloon_extra_local_points(&s))), [80.0, 180.0, 240.0, 280.0]);
}

/// Hand-computed path geometry for each shape around a 200×80 block at 40 px.
#[test]
fn path_geometry() {
    let s = |shape| with(Balloon { shape, ..balloon() });
    let pad = 22.0; // 40 · 0.55
    let (w, h) = (200.0 + 2.0 * pad, 80.0 + 2.0 * pad);
    let (cx, cy) = (100.0, 40.0);
    let bbox = |paths: &[Vec<[f64; 2]>]| {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for p in paths.iter().flatten() {
            b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
        }
        b
    };
    // Oval: one ellipse; every vertex on it.
    let oval = balloon_paths(&s(BalloonShape::Oval), 200.0, 80.0);
    assert_eq!(oval.len(), 1);
    for p in &oval[0] {
        let e = ((p[0] - cx) / (w / 2.0)).powi(2) + ((p[1] - cy) / (h / 2.0)).powi(2);
        assert!((e - 1.0).abs() < 1e-9, "{p:?}");
    }
    let b = bbox(&oval);
    assert!((b[0] + pad).abs() < 1e-6 && (b[2] - 222.0).abs() < 1e-6 && (b[1] + pad).abs() < 0.1 && (b[3] - 102.0).abs() < 0.1, "{b:?}");
    // Rounded: the padded rectangle, corners cut by r = min(w,h)·0.28.
    let rounded = balloon_paths(&s(BalloonShape::Rounded), 200.0, 80.0);
    let b = bbox(&rounded);
    assert!(b.iter().zip([-pad, -pad, 222.0, 102.0]).all(|(a, e)| (a - e).abs() < 1e-6), "{b:?}");
    let r = h.min(w) * 0.28;
    assert!(rounded[0].iter().all(|p| !(p[0] < -pad + r * 0.29 && p[1] < -pad + r * 0.29)), "corner is rounded");
    // Burst: 32 vertices alternating 1.22 / 0.86 of the ellipse radius.
    let burst = balloon_paths(&s(BalloonShape::Burst), 200.0, 80.0);
    assert_eq!(burst[0].len(), 32);
    assert!((burst[0][0][0] - (cx + w / 2.0 * 1.22)).abs() < 1e-9 && (burst[0][0][1] - cy).abs() < 1e-9);
    let t = std::f64::consts::PI / 16.0;
    assert!((burst[0][1][0] - (cx + t.cos() * w / 2.0 * 0.86)).abs() < 1e-9);
    // Cloud: 12 lobes plus the ellipse.
    let cloud = balloon_paths(&s(BalloonShape::Cloud), 200.0, 80.0);
    assert_eq!(cloud.len(), 13);
    let rr = w.min(h) * 0.19;
    let b = bbox(&cloud);
    assert!((b[2] - (cx + w / 2.0 + rr)).abs() < 0.1, "{b:?}");
    // Tail: a triangle from the centre, base ⟂ the direction, out to the tip.
    let tl = balloon_paths(&with(Balloon { tail: tail(300.0, 40.0, Some(30.0)), ..balloon() }), 200.0, 80.0);
    assert_eq!(tl.len(), 2);
    let tri = &tl[1];
    assert_eq!(tri.len(), 3);
    assert!((tri[0][0] - cx).abs() < 1e-9 && (tri[0][1] - (cy + 15.0)).abs() < 1e-9, "{tri:?}");
    assert_eq!(tri[1], [300.0, 40.0]);
    assert!((tri[2][1] - (cy - 15.0)).abs() < 1e-9);
}

// --- Rendering ---------------------------------------------------------------

/// The golden setup: a blank 200×80 block (" " with a 200 px box, 40 px at line
/// height 2) at (60, 60) on a 400×240 document.
fn golden_spec(b: Balloon) -> TextSpec {
    TextSpec {
        text: " ".into(),
        font_size_px: 40.0,
        line_height: 2.0,
        box_width: Some(200.0),
        x: 60.0,
        y: 60.0,
        balloon: Some(b),
        ..Default::default()
    }
}

/// Straight sRGB RGBA in 0..1 from the rendered (premultiplied linear) surface.
fn ours(s: &onepaint_raster::Surface, x: i32, y: i32) -> [f32; 4] {
    let p = s.get(x, y);
    if p[3] <= 0.0 {
        return [0.0; 4];
    }
    let c = |v: f32| onepaint_color::linear_to_srgb(v / p[3]);
    [c(p[0]), c(p[1]), c(p[2]), p[3]]
}

fn golden(name: &str) -> (u32, u32, Vec<u8>) {
    let path = format!("{}/tests/golden/balloon_{name}.png", env!("CARGO_MANIFEST_DIR"));
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    (info.width, info.height, buf)
}

/// Fraction of the shape's pixels that differ from happypaint's (beyond AA).
fn mismatch(name: &str, b: Balloon) -> f64 {
    let (w, h, g) = golden(name);
    let s = Renderer::new().render(&[golden_spec(b)], w, h);
    let (mut bad, mut shape) = (0usize, 0usize);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            let gp = [g[i] as f32 / 255.0, g[i + 1] as f32 / 255.0, g[i + 2] as f32 / 255.0, g[i + 3] as f32 / 255.0];
            let op = ours(&s, x, y);
            if gp[3] <= 0.0 && op[3] <= 0.0 {
                continue;
            }
            shape += 1;
            let alpha_off = (gp[3] - op[3]).abs() > 0.35;
            let colour_off = gp[3] > 0.6 && op[3] > 0.6 && (0..3).any(|c| (gp[c] - op[c]).abs() > 0.35);
            if alpha_off || colour_off {
                bad += 1;
            }
        }
    }
    assert!(shape > 1000, "{name}: empty");
    bad as f64 / shape as f64
}

fn red(shape: BalloonShape) -> Balloon {
    Balloon { shape, fill: "#ff0000".into(), ..Default::default() }
}

fn stroked(shape: BalloonShape) -> Balloon {
    Balloon { stroke: Some("#0000ff".into()), stroke_width: Some(6.0), ..red(shape) }
}

#[test]
fn fills_match_happypaint_goldens() {
    for (name, shape) in
        [("oval", BalloonShape::Oval), ("rounded", BalloonShape::Rounded), ("burst", BalloonShape::Burst), ("cloud", BalloonShape::Cloud)]
    {
        let m = mismatch(name, red(shape));
        assert!(m < 0.01, "{name}: {:.2}% off", m * 100.0);
    }
}

#[test]
fn strokes_match_happypaint_goldens() {
    for (name, shape) in
        [("oval_stroke", BalloonShape::Oval), ("rounded_stroke", BalloonShape::Rounded), ("burst_stroke", BalloonShape::Burst)]
    {
        let m = mismatch(name, stroked(shape));
        assert!(m < 0.01, "{name}: {:.2}% off", m * 100.0);
    }
}

#[test]
fn cloud_stroke_outlines_only_the_union() {
    let s = Renderer::new().render(&[golden_spec(stroked(BalloonShape::Cloud))], 400, 240);
    // The right lobe is centred on the ellipse at block (222, 40) with r ≈ 23.6:
    // its inner arc lies inside the body and stays fill, its outer arc is stroked.
    for x in 256..=260 {
        let p = ours(&s, x, 100);
        assert!(p[0] > 0.9 && p[2] < 0.1, "inner arc at {x}: {p:?}");
    }
    let outer = (300..=310).map(|x| ours(&s, x, 100)).any(|p| p[2] > 0.9 && p[0] < 0.1);
    assert!(outer, "outer arc stroked");
}

#[test]
fn tail_reaches_its_tip_and_leaves_no_wedge() {
    let b = Balloon { tail: tail(300.0, 130.0, Some(30.0)), ..red(BalloonShape::Oval) };
    let (w, h, g) = golden("oval_tail");
    let s = Renderer::new().render(&[golden_spec(b)], w, h);
    // Outside the body the tail matches happypaint's.
    for (x, y) in [(300, 165), (330, 178), (270, 150)] {
        let i = ((y * w + x) * 4) as usize;
        assert!(g[i + 3] > 200, "golden tail at {x},{y}");
        assert!(ours(&s, x as i32, y as i32)[3] > 0.8, "tail at {x},{y}");
    }
    // happypaint's nonzero fill leaves a hole where the tail overlaps the body;
    // onepaint fills the union (divergence #11).
    let i = ((105 * w + 180) * 4) as usize;
    assert_eq!(g[i + 3], 0, "the golden has the wedge");
    assert!(ours(&s, 180, 105)[3] > 0.99);
}

#[test]
fn balloon_draws_behind_the_text_and_grows_bounds() {
    let r = Renderer::new();
    let text = TextSpec { text: "WOW".into(), font_size_px: 40.0, x: 100.0, y: 100.0, color: "#000000".into(), ..Default::default() };
    let b = Balloon {
        shape: BalloonShape::Oval,
        fill: "#ffff00".into(),
        stroke: Some("#0000ff".into()),
        stroke_width: Some(4.0),
        ..Default::default()
    };
    let with_b = TextSpec { balloon: Some(b), ..text.clone() };
    let plain = r.render(std::slice::from_ref(&text), 500, 400);
    let s = r.render(std::slice::from_ref(&with_b), 500, 400);
    let pb = plain.content_bounds().unwrap();
    let sb = s.content_bounds().unwrap();
    assert!(sb.x < pb.x - 15 && sb.y < pb.y - 15 && sb.w > pb.w + 30, "{pb:?} {sb:?}");
    // Glyph pixels stay black (text on top); just inside the pad is yellow fill.
    let mut glyphs = 0;
    for y in pb.y..pb.y + pb.h {
        for x in pb.x..pb.x + pb.w {
            if plain.get(x, y)[3] > 0.99 {
                glyphs += 1;
                let p = ours(&s, x, y);
                assert!(p[0] < 0.1 && p[1] < 0.1 && p[2] < 0.1, "{x},{y} {p:?}");
            }
        }
    }
    assert!(glyphs > 50);
    let q = r.quad(&with_b).unwrap();
    let cy = ((q[0][1] + q[2][1]) / 2.0) as i32;
    let fill = ours(&s, (q[0][0] + 14.0) as i32, cy);
    assert!(fill[0] > 0.95 && fill[1] > 0.95 && fill[2] < 0.05, "{fill:?}");
    // The stroke ring sits on the left wall at the centre line.
    let left = (q[0][0] + 0.5).round() as i32; // quad = block ± pad + stroke/2
    let ring = (left - 2..left + 6).map(|x| ours(&s, x, cy)).find(|p| p[2] > 0.9 && p[0] < 0.1);
    assert!(ring.is_some(), "blue stroke near x={left}");
    // The quad grows to the balloon pad (and the hit test follows it).
    let tq = r.quad(&text).unwrap();
    let pad = balloon_pad(&with_b, 40.0);
    assert!((tq[0][0] - q[0][0] - pad).abs() < 1e-6, "{tq:?} {q:?}");
    assert!(quad_contains(&q, [q[0][0] + 2.0, cy as f64], 0.0));
    // bgColor alone keeps the historical (unpadded) quad.
    let bg = TextSpec { bg_color: Some("#ffffff".into()), ..text.clone() };
    assert_eq!(r.quad(&bg).unwrap(), tq);
}

#[test]
fn tail_grows_the_quad_and_rotates() {
    let r = Renderer::new();
    let b = Balloon { tail: tail(250.0, 150.0, Some(20.0)), ..red(BalloonShape::Oval) };
    let s = TextSpec { text: "Hi".into(), x: 50.0, y: 50.0, balloon: Some(b), ..Default::default() };
    let q = r.quad(&s).unwrap();
    assert!((q[2][0] - (50.0 + 260.0)).abs() < 1e-6 && (q[2][1] - (50.0 + 160.0)).abs() < 1e-6, "{q:?}");
    let px = r.render(std::slice::from_ref(&s), 400, 400);
    assert!(px.get(253, 174)[3] > 0.5, "tail drawn towards its tip");
}

#[test]
fn tail_angle_control() {
    let t = tail_from_angle(40.0, 90.0);
    assert!(t.x.abs() < 1e-9 && (t.y - 88.0).abs() < 1e-9 && t.width == Some(20.0), "{t:?}");
    assert!((tail_angle(Some(&t)) - 90.0).abs() < 1e-9);
    assert!((tail_angle(Some(&tail_from_angle(10.0, 315.0))) - 315.0).abs() < 1e-9);
    assert_eq!(tail_from_angle(10.0, 0.0).width, Some(8.0), "8 px minimum");
    assert_eq!(tail_angle(None), 90.0);
    let d = default_balloon(BalloonShape::Cloud);
    assert_eq!((d.fill.as_str(), d.stroke.as_deref(), d.stroke_width, d.tail), ("#ffffff", Some("#000000"), Some(3.0), None));
}

#[test]
fn balloon_json_round_trip() {
    let j = serde_json::json!({"text": "a", "x": 1, "y": 2,
        "balloon": {"shape": "burst", "fill": "#fff", "stroke": "#000", "strokeWidth": 3.0, "pad": 0.4, "tail": {"x": 5.0, "y": 6.0, "width": 7.0}}});
    let s: TextSpec = serde_json::from_value(j.clone()).unwrap();
    let b = s.balloon.clone().unwrap();
    assert_eq!(b.shape, BalloonShape::Burst);
    assert_eq!(b.tail, tail(5.0, 6.0, Some(7.0)));
    assert!(!s.extra.contains_key("balloon"));
    let back = serde_json::to_value(&s).unwrap();
    assert_eq!(back["balloon"], j["balloon"]);
    // Unknown shapes read as ovals, like happypaint's default branch.
    let o: Balloon = serde_json::from_value(serde_json::json!({"shape": "wobbly", "fill": "#fff"})).unwrap();
    assert_eq!(o.shape, BalloonShape::Oval);
}
