#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};
use onepaint_stroke::StrokePoint;

fn pt(x: f64, y: f64) -> StrokePoint {
    StrokePoint { x, y, pressure: 1.0, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None }
}

/// A transparent top layer with a red square on it.
fn session() -> Session {
    let mut s = Session::new(200, 100, None);
    s.execute(Command::SelectShape { contours: vec![rect_polygon([10.0, 30.0], [50.0, 70.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

fn active_px(s: &Session, x: i32, y: i32) -> [f32; 4] {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)
}

fn drag(s: &mut Session, x0: f64, x1: f64, y: f64) {
    s.stroke_begin(pt(x0, y));
    let pts: Vec<_> = (0..=((x1 - x0) / 2.0) as i32).map(|i| pt(x0 + i as f64 * 2.0, y)).collect();
    s.stroke_extend(&pts);
    s.stroke_end();
}

#[test]
fn smudge_is_one_undo_step() {
    let mut s = session();
    s.execute(Command::SetTool { tool: Tool::Smudge }).unwrap();
    s.execute(Command::SetSmudgeStrength { strength: 0.8 }).unwrap();
    drag(&mut s, 40.0, 90.0, 50.0);
    assert!(active_px(&s, 60, 50)[3] > 0.05, "smeared red past the square");
    s.execute(Command::Undo).unwrap();
    assert_eq!(active_px(&s, 60, 50)[3], 0.0);
    assert!((active_px(&s, 20, 50)[0] - 1.0).abs() < 1e-3);
}

#[test]
fn smudge_cancel_restores_exactly() {
    let mut s = session();
    let before = s.doc().layer(s.active()).unwrap().pixels.read_rect(s.doc().bounds());
    s.execute(Command::SetTool { tool: Tool::Smudge }).unwrap();
    s.stroke_begin(pt(40.0, 50.0));
    s.stroke_extend(&[pt(60.0, 50.0), pt(80.0, 50.0)]);
    assert!(s.is_stroking());
    s.stroke_cancel();
    assert!(!s.is_stroking());
    assert_eq!(s.doc().layer(s.active()).unwrap().pixels.read_rect(s.doc().bounds()), before);
}

#[test]
fn mixer_keep_colour_holds_the_reservoir() {
    let mut s = session();
    s.execute(Command::SetTool { tool: Tool::Mixer }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 1.0, 1.0) }).unwrap();
    s.execute(Command::SetMixer { mixer: MixerSettings { wet: 1.0, load: 1.0, mix: 0.8, keep: true } }).unwrap();
    drag(&mut s, 15.0, 45.0, 50.0);
    // The kept brush is contaminated by the red it crossed.
    s.execute(Command::SetMixer { mixer: MixerSettings { wet: 0.0, load: 1.0, mix: 0.0, keep: true } }).unwrap();
    drag(&mut s, 100.0, 140.0, 50.0);
    let px = active_px(&s, 120, 50);
    assert!(px[3] > 0.5 && px[0] > 0.02, "carried red: {px:?}");
    // Clean reloads pure blue.
    s.execute(Command::CleanMixer).unwrap();
    drag(&mut s, 100.0, 140.0, 80.0);
    let px = active_px(&s, 120, 80);
    assert!(px[2] > 0.5 && px[0] < 1e-3, "clean blue: {px:?}");
}

#[test]
fn symmetric_brush_stroke_paints_the_mirror() {
    let mut s = Session::new(200, 100, None);
    s.execute(Command::SetSymmetry { symmetry: Symmetry { mode: SymmetryMode::Vertical, ..Default::default() } }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 0.0, 1.0) }).unwrap();
    drag(&mut s, 20.0, 60.0, 50.0);
    assert!(active_px(&s, 40, 50)[3] > 0.5);
    assert!(active_px(&s, 159, 50)[3] > 0.5, "mirrored across x = 100");
    // Settings survive opening another document.
    s.replace_document(onepaint_doc::Document::new(10, 10, None));
    assert_eq!(s.paint_settings().symmetry.mode, SymmetryMode::Vertical);
}
