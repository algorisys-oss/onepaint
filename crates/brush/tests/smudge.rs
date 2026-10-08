#![allow(clippy::unwrap_used)]
use onepaint_brush::smudge::{Smudge, SmudgeParams, WetParams};
use onepaint_brush::symmetry::{Symmetry, SymmetryMode};
use onepaint_brush::{Tip, presets};
use onepaint_raster::{Rect, Surface};
use onepaint_stroke::StrokePoint;

fn pt(x: f64, y: f64) -> StrokePoint {
    StrokePoint { x, y, pressure: 1.0, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None }
}

fn params(strength: f64, wet: Option<WetParams>) -> SmudgeParams {
    SmudgeParams {
        size: 20.0,
        strength,
        hardness: 0.35,
        tip: Tip::Circle,
        size_pressure: 0.0,
        flow_pressure: 0.0,
        spacing: None,
        wet,
        impasto: 0.0,
        blend: 0.0,
    }
}

const DOC: Rect = Rect { x: 0, y: 0, w: 200, h: 100 };

fn red_square() -> Surface {
    let mut s = Surface::new();
    let r = Rect::new(10, 30, 40, 40);
    s.write_rect(r, &[1.0, 0.0, 0.0, 1.0].repeat((r.w * r.h) as usize));
    s
}

fn drag(sm: &mut Smudge, s: &mut Surface, x0: f64, x1: f64, y: f64) {
    let mut x = x0;
    while x <= x1 {
        sm.add(s, pt(x, y), None);
        x += 2.0;
    }
}

#[test]
fn symmetry_transforms_follow_the_table() {
    let sym = |mode| Symmetry { mode, ..Default::default() };
    assert_eq!(sym(SymmetryMode::Off).transforms(100, 100).len(), 1);
    let v = sym(SymmetryMode::Vertical).transforms(100, 80);
    let (x, y) = v[1].apply(10.0, 20.0);
    assert!((x - 90.0).abs() < 1e-9 && (y - 20.0).abs() < 1e-9, "vertical mirrors x: {x},{y}");
    let h = sym(SymmetryMode::Horizontal).transforms(100, 80);
    let (x, y) = h[1].apply(10.0, 20.0);
    assert!((x - 10.0).abs() < 1e-9 && (y - 60.0).abs() < 1e-9);
    assert_eq!(sym(SymmetryMode::Both).transforms(100, 80).len(), 4);
    let r = Symmetry { mode: SymmetryMode::Radial, radial_count: 4, ..Default::default() }.transforms(100, 100);
    assert_eq!(r.len(), 4);
    let (x, y) = r[1].apply(60.0, 50.0);
    assert!((x - 50.0).abs() < 1e-9 && (y - 60.0).abs() < 1e-9, "90° rotation: {x},{y}");
    let r = Symmetry { mode: SymmetryMode::Radial, radial_count: 99, ..Default::default() }.transforms(10, 10);
    assert_eq!(r.len(), 24);
}

#[test]
fn mirrored_brush_stroke_is_pixel_symmetric() {
    let b = presets().into_iter().find(|b| b.tip_id == "round" || b.id.contains("round")).unwrap();
    let mut b = b;
    b.size = 10.0;
    b.taper = None;
    let mut e = onepaint_brush::StrokeEngine::new(b, [0.0, 0.0, 1.0], 7);
    e.set_symmetry(Symmetry { mode: SymmetryMode::Vertical, ..Default::default() }.transforms(100, 60));
    let mut dabs = e.begin(pt(10.0, 30.0));
    for i in 1..20 {
        dabs.extend(e.add(pt(10.0 + i as f64, 30.0 + i as f64 * 0.5)));
    }
    dabs.extend(e.end());
    let mut s = Surface::new();
    for d in &dabs {
        onepaint_brush::rasterize_dab(&mut s, d, false, None);
    }
    let mut ink = 0.0;
    for y in 0..60 {
        for x in 0..50 {
            let (a, b) = (s.get(x, y)[3], s.get(99 - x, y)[3]);
            assert!((a - b).abs() < 1e-3, "({x},{y}) {a} vs {b}");
            ink += a;
        }
    }
    assert!(ink > 10.0);
}

#[test]
fn smudge_drags_red_into_white() {
    let mut s = red_square();
    let mut sm = Smudge::new(params(0.6, None), None, None, Vec::new(), DOC, false, pt(40.0, 50.0));
    drag(&mut sm, &mut s, 40.0, 80.0, 50.0);
    let px = s.get(60, 50);
    assert!(px[3] > 0.05 && px[0] > 0.05, "red carried out: {px:?}");
    assert!(px[1].abs() < 1e-4, "only red is carried");
}

#[test]
fn zero_strength_changes_nothing() {
    let mut s = red_square();
    let before = s.read_rect(DOC);
    let mut sm = Smudge::new(params(0.0, None), None, None, Vec::new(), DOC, false, pt(40.0, 50.0));
    drag(&mut sm, &mut s, 40.0, 80.0, 50.0);
    assert_eq!(s.read_rect(DOC), before);
}

#[test]
fn alpha_lock_adds_no_coverage_outside() {
    let mut s = red_square();
    let mut sm = Smudge::new(params(0.9, None), None, None, Vec::new(), DOC, true, pt(40.0, 50.0));
    drag(&mut sm, &mut s, 40.0, 80.0, 50.0);
    assert_eq!(s.get(60, 50)[3], 0.0);
    assert!((s.get(45, 50)[3] - 1.0).abs() < 1e-3);
}

#[test]
fn mixer_paints_its_reservoir_on_transparent() {
    let wet = WetParams { mix: 0.0, load: 1.0, wetness: 0.5, pigment: false };
    let mut s = Surface::new();
    let mut sm = Smudge::new(params(0.9, Some(wet)), Some([0.0, 1.0, 0.0]), None, Vec::new(), DOC, false, pt(20.0, 50.0));
    drag(&mut sm, &mut s, 20.0, 60.0, 50.0);
    let px = s.get(40, 50);
    assert!(px[3] > 0.5 && px[1] > 0.5 && px[0] < 1e-3, "{px:?}");
}

#[test]
fn mixer_runs_out_of_paint() {
    let wet = WetParams { mix: 0.3, load: 0.05, wetness: 0.0, pigment: false };
    let mut s = Surface::new();
    let mut sm = Smudge::new(params(0.9, Some(wet)), Some([1.0, 0.0, 0.0]), None, Vec::new(), DOC, false, pt(10.0, 50.0));
    drag(&mut sm, &mut s, 10.0, 190.0, 50.0);
    assert!(s.get(20, 50)[3] > s.get(180, 50)[3] + 0.2, "head {} tail {}", s.get(20, 50)[3], s.get(180, 50)[3]);
}

#[test]
fn pigment_mixer_turns_yellow_on_blue_green() {
    let wet = WetParams { mix: 0.5, load: 1.0, wetness: 1.0, pigment: true };
    let mut s = Surface::new();
    let r = Rect::new(0, 0, 200, 100);
    s.write_rect(r, &[0.0, 0.0, 1.0, 1.0].repeat((r.w * r.h) as usize));
    let mut sm = Smudge::new(params(0.98, Some(wet)), Some([1.0, 1.0, 0.0]), None, Vec::new(), DOC, false, pt(20.0, 50.0));
    drag(&mut sm, &mut s, 20.0, 100.0, 50.0);
    let px = s.get(60, 50);
    assert!(px[1] > px[0] && px[1] > px[2] * 0.8, "greenish: {px:?}");
}

#[test]
fn radial_smudge_smears_every_copy() {
    let mut s = Surface::new();
    let r = Rect::new(0, 0, 200, 100);
    s.write_rect(r, &[1.0, 1.0, 1.0, 1.0].repeat((r.w * r.h) as usize));
    let tf = Symmetry { mode: SymmetryMode::Radial, radial_count: 2, ..Default::default() }.transforms(200, 100);
    let wet = WetParams { mix: 0.0, load: 1.0, wetness: 0.0, pigment: false };
    let mut sm = Smudge::new(params(0.9, Some(wet)), Some([0.0, 0.0, 0.0]), None, tf, DOC, false, pt(20.0, 50.0));
    drag(&mut sm, &mut s, 20.0, 50.0, 50.0);
    assert!(s.get(35, 50)[0] < 0.5 && s.get(164, 49)[0] < 0.5, "both copies darkened");
}
