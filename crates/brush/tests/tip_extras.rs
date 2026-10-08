#![allow(clippy::unwrap_used)]
//! Dual brush, colour tips, grain movement, spacing jitter and the
//! fade-driven colour dynamics (happypaint `brush-engine.ts`, `stamp-renderer.ts`).
use onepaint_brush::*;
use onepaint_raster::{Rect, Surface};
use onepaint_stroke::StrokePoint;

fn line(b: &BrushPreset, color: [f32; 3], len: f64) -> (Vec<Dab>, Surface) {
    let mut e = StrokeEngine::new(b.clone(), color, 7);
    let mut dabs = e.begin(StrokePoint::at(20.0, 60.0, 1.0, 0.0));
    for i in 1..=(len as i32 / 2) {
        dabs.extend(e.add(StrokePoint::at(20.0 + i as f64 * 2.0, 60.0, 1.0, i as f64 * 8.0)));
    }
    dabs.extend(e.end());
    let mut s = Surface::new();
    for d in &dabs {
        e.rasterize(&mut s, d, None);
    }
    (dabs, s)
}

fn coverage(s: &Surface) -> f64 {
    s.read_rect(Rect::new(0, 0, 400, 140)).chunks_exact(4).map(|p| p[3] as f64).sum()
}

#[test]
fn a_dual_brush_carves_each_dab_with_its_second_tip() {
    let b = preset("textured-round").unwrap();
    assert_eq!(b.mask_tip(), Some(("bristle".into(), 0.85)));
    let mut plain = b.clone();
    plain.extra.remove("maskTip");
    let (with, without) = (coverage(&line(&b, [1.0; 3], 200.0).1), coverage(&line(&plain, [1.0; 3], 200.0).1));
    assert!(with < without * 0.9, "the mask removes coverage: {with} vs {without}");
    let mut off = b.clone();
    off.extra.insert("maskTip".into(), serde_json::json!({"id": "bristle", "strength": 0.0}));
    assert_eq!(coverage(&line(&off, [1.0; 3], 200.0).1), without, "strength 0 is no mask");
}

#[test]
fn colour_tips_tint_or_paint_their_own_colours() {
    let tint = color_tip_for("oil-daub").unwrap();
    assert!(tint.lum > 0.5 && tint.lum < 0.7 && tint.rgb.len() == tint.size * tint.size);
    let hue_spread = |s: &Surface| {
        let px: Vec<[f32; 4]> =
            s.read_rect(Rect::new(0, 0, 400, 140)).chunks_exact(4).filter(|p| p[3] > 0.5).map(|p| [p[0], p[1], p[2], p[3]]).collect();
        let rs: Vec<f32> = px.iter().map(|p| p[0] / p[3]).collect();
        let (lo, hi) = rs.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        (hi - lo, px.len())
    };
    // Tint (thick-paint): the brush colour, modulated by the daub's light and dark.
    let blue = [0.05, 0.1, 0.6];
    let (spread, n) = hue_spread(&line(&preset("thick-paint").unwrap(), blue, 200.0).1);
    assert!(n > 100 && spread > 0.01, "the daub varies the paint: {spread}");
    // Baked (rgba-oil): the tip's own colours, whatever the brush colour.
    let rgba = preset("rgba-oil").unwrap();
    assert!(rgba.use_tip_color());
    let (a, b) = (line(&rgba, [1.0, 0.0, 0.0], 200.0).1, line(&rgba, [0.0, 0.0, 1.0], 200.0).1);
    assert_eq!(a.read_rect(Rect::new(0, 0, 400, 140)), b.read_rect(Rect::new(0, 0, 400, 140)), "brush colour ignored");
    // Recolour shifts the baked colours.
    let mut shifted = rgba.clone();
    shifted.extra.insert("tipRecolor".into(), serde_json::json!({"hue": 0.5, "sat": 1.0, "val": 1.0}));
    assert_ne!(line(&shifted, [1.0; 3], 200.0).1.read_rect(Rect::new(0, 0, 400, 140)), a.read_rect(Rect::new(0, 0, 400, 140)));
}

#[test]
fn moving_grain_rolls_with_the_brush_from_the_first_dab() {
    let smoke = preset("smoke").unwrap();
    assert_eq!(smoke.grain.as_ref().and_then(|g| g.moves), Some(0.2));
    let (dabs, _) = line(&smoke, [1.0; 3], 200.0);
    assert_eq!(dabs[0].grain_off, [0.0, 0.0]);
    for d in &dabs {
        let expect = (d.x - dabs[0].x) * 0.2;
        // Scatter moves the dab, not the grain origin: compare against the sample point.
        assert!(d.grain_off[1] == 0.0 && (d.grain_off[0] - expect).abs() < d.radius * 2.0 * 0.2 + 1e-6);
    }
    assert!(dabs.last().unwrap().grain_off[0] > 20.0);
    let fog = preset("fog").unwrap();
    assert!(line(&fog, [1.0; 3], 100.0).0.iter().all(|d| d.grain_off == [0.0, 0.0]), "move 0 stays put");
}

#[test]
fn spacing_jitter_varies_the_gaps_deterministically() {
    let b = preset("hard-round").unwrap();
    let gaps = |b: &BrushPreset| {
        let (d, _) = line(b, [1.0; 3], 300.0);
        d.windows(2).map(|w| w[1].x - w[0].x).collect::<Vec<_>>()
    };
    let even = gaps(&b);
    let mut j = b.clone();
    j.spacing_jitter = Some(0.8);
    let uneven = gaps(&j);
    let spread = |g: &[f64]| g.iter().cloned().fold(f64::MIN, f64::max) - g.iter().cloned().fold(f64::MAX, f64::min);
    assert!(spread(&uneven) > spread(&even) + 0.5, "jittered gaps: {} vs {}", spread(&uneven), spread(&even));
    assert_eq!(uneven, gaps(&j), "same seed, same stroke");
}

#[test]
fn hue_shift_and_two_colour_load_follow_the_fade() {
    let mut b = preset("hard-round").unwrap();
    b.dynamics.get_or_insert_with(Default::default).color =
        Some(ColorDynamics { hue_shift: Some(0.5), hue: Some(0.0), sat: Some(0.0), val: Some(0.0), ..Default::default() });
    let (dabs, _) = line(&b, onepaint_color::Rgba::new(1.0, 0.0, 0.0, 1.0).to_linear_premul()[..3].try_into().unwrap(), 300.0);
    let first = dabs[0].color;
    let late = dabs[dabs.len() - 1].color;
    assert!(first[0] > 0.9 && first[2] < 0.05, "starts red: {first:?}");
    assert!(late[2] > 0.5 && late[0] < 0.1, "half the wheel later: cyan-ish {late:?}");
    // Two-colour load: red fades to blue over 50 dabs.
    let mut two = preset("hard-round").unwrap();
    two.dynamics.get_or_insert_with(Default::default).color =
        Some(ColorDynamics { to_color: Some([0.0, 0.0, 1.0]), hue: Some(0.0), sat: Some(0.0), val: Some(0.0), ..Default::default() });
    let (dabs, _) = line(&two, [1.0, 0.0, 0.0], 300.0);
    let late = dabs[dabs.len() - 1].color;
    assert!(late[2] > 0.9 && late[0] < 0.05, "ends blue: {late:?}");
    // marigold's preset drift parses.
    let m = preset("marigold").unwrap();
    let c = m.dynamics.unwrap().color.unwrap();
    assert_eq!((c.hue_shift, c.hue_shift_source.as_deref()), (Some(-0.14), Some("fade")));
}
