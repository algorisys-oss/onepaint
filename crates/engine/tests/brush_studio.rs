#![allow(clippy::unwrap_used)]
//! Brush studio (spec 27): the studio edits the whole current preset live.

use onepaint_brush::preset;
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn line(s: &mut Session) {
    s.stroke_begin(StrokePoint::at(40.0, 100.0, 1.0, 0.0));
    for i in 1..=20 {
        s.stroke_extend(&[StrokePoint::at(40.0 + i as f64 * 6.0, 100.0, 1.0, i as f64 * 16.0)]);
    }
    s.stroke_end();
}

fn alpha(s: &Session, x: i32, y: i32) -> f32 {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)[3]
}

#[test]
fn set_brush_preset_replaces_the_current_brush_and_changes_strokes() {
    let mut s = Session::new(256, 256, Some(Rgba::WHITE));
    s.execute(Command::SetBrush { preset_id: "hard-round".into() }).unwrap();
    let mut p = s.current_brush().clone();
    p.size = 8.0;
    p.size_pressure = 0.0;
    s.execute(Command::SetBrushPreset { preset: Box::new(p.clone()) }).unwrap();
    assert_eq!(s.current_brush(), &p);
    line(&mut s);
    assert!(alpha(&s, 100, 100) > 0.5);
    assert!(alpha(&s, 100, 112) < 0.01, "8 px brush stays thin");

    s.execute(Command::Undo).unwrap();
    p.size = 40.0;
    s.execute(Command::SetBrushPreset { preset: Box::new(p) }).unwrap();
    line(&mut s);
    assert!(alpha(&s, 100, 112) > 0.5, "40 px brush reaches 12 px off the line");
}

#[test]
fn set_brush_preset_routes_erasers_to_the_eraser_tool() {
    let mut s = Session::new(64, 64, None);
    let mut e = preset("eraser-hard").unwrap();
    e.size = 77.0;
    s.execute(Command::SetBrushPreset { preset: Box::new(e) }).unwrap();
    assert_eq!(s.tool(), Tool::Eraser);
    assert_eq!(s.current_brush().size, 77.0);
    let b = preset("hard-round").unwrap();
    s.execute(Command::SetBrushPreset { preset: Box::new(b) }).unwrap();
    assert_eq!(s.tool(), Tool::Brush);
    assert_eq!(s.current_brush().id, "hard-round");
}

#[test]
fn set_brush_preset_is_not_undoable_and_serialises() {
    let mut s = Session::new(64, 64, None);
    let before = s.history_len();
    let p = preset("soft-round").unwrap();
    let cmd = Command::SetBrushPreset { preset: Box::new(p) };
    let json = serde_json::to_string(&cmd).unwrap();
    let back: Command = serde_json::from_str(&json).unwrap();
    s.execute(back).unwrap();
    assert_eq!(s.current_brush().id, "soft-round");
    assert_eq!(s.history_len(), before);
}

/// Any brush works as an eraser: picked on the Eraser it stays one;
/// `SetCurrentBrush` edits the current slot in place.
#[test]
fn the_eraser_keeps_any_brush_it_picks() {
    let mut s = Session::new(64, 64, None);
    s.execute(Command::SetTool { tool: Tool::Eraser }).unwrap();
    s.execute(Command::SetBrush { preset_id: "pencil".into() }).unwrap();
    assert_eq!((s.tool(), s.current_brush().id.as_str()), (Tool::Eraser, "pencil"));
    let mut b = s.current_brush().clone();
    b.tip_angle_deg = Some(30.0);
    s.execute(Command::SetCurrentBrush { preset: Box::new(b) }).unwrap();
    assert_eq!((s.tool(), s.current_brush().tip_angle_deg), (Tool::Eraser, Some(30.0)));
    // On the Brush, an eraser preset still picks the Eraser.
    s.execute(Command::SetTool { tool: Tool::Brush }).unwrap();
    s.execute(Command::SetBrush { preset_id: "eraser-hard".into() }).unwrap();
    assert_eq!(s.tool(), Tool::Eraser);
}
