#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};
use onepaint_stroke::StrokePoint;

fn red_layer() -> Session {
    let mut s = Session::new(128, 64, Some(Rgba::WHITE));
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SelectAll).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

fn line(s: &mut Session, y: f64) {
    s.stroke_begin(StrokePoint::at(-20.0, y, 1.0, 0.0));
    for i in 1..=20 {
        s.stroke_extend(&[StrokePoint::at(-20.0 + i as f64 * 10.0, y, 1.0, i as f64 * 8.0)]);
    }
    s.stroke_end();
}

fn is_red(s: &Session, x: i32, y: i32) -> bool {
    let c = s.pick_color(x, y);
    c.r > 0.99 && c.g < 0.01
}

#[test]
fn white_mask_changes_nothing_and_black_paint_hides() {
    let mut s = red_layer();
    let id = s.active().to_string();
    s.execute(Command::AddMask).unwrap();
    assert!(s.doc().layer(&id).unwrap().mask.is_some());
    assert!(s.mask_target(), "a new mask becomes the edit target");
    assert!(is_red(&s, 30, 32));
    s.execute(Command::SetColor { rgba: Rgba::BLACK }).unwrap();
    line(&mut s, 32.0);
    assert!(!is_red(&s, 30, 32), "black on the mask hides the layer");
    assert!(is_red(&s, 30, 5));
    assert!(s.doc().layer(&id).unwrap().pixels.get(30, 32)[3] > 0.99, "pixels untouched");
    s.execute(Command::SetTool { tool: Tool::Eraser }).unwrap();
    line(&mut s, 32.0);
    assert!(is_red(&s, 30, 32), "the eraser paints white on the mask");
    s.execute(Command::Undo).unwrap();
    assert!(!is_red(&s, 30, 32));
    s.execute(Command::Undo).unwrap();
    assert!(is_red(&s, 30, 32));
}

#[test]
fn mask_from_selection_apply_and_delete() {
    let mut s = red_layer();
    let id = s.active().to_string();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([0.0, 0.0], [64.0, 64.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::AddMask).unwrap();
    assert!(is_red(&s, 10, 10));
    assert!(!is_red(&s, 100, 10));
    s.execute(Command::SetMaskEnabled { id: id.clone(), enabled: false }).unwrap();
    assert!(is_red(&s, 100, 10));
    s.execute(Command::SetMaskEnabled { id: id.clone(), enabled: true }).unwrap();
    s.execute(Command::ApplyMask { id: id.clone() }).unwrap();
    let l = s.doc().layer(&id).unwrap();
    assert!(l.mask.is_none());
    assert_eq!(l.pixels.get(100, 10)[3], 0.0);
    assert!(!s.mask_target());
    s.execute(Command::Undo).unwrap();
    s.execute(Command::DeleteMask { id: id.clone() }).unwrap();
    assert!(is_red(&s, 100, 10));
}

#[test]
fn switching_layers_resets_the_target() {
    let mut s = red_layer();
    s.execute(Command::AddMask).unwrap();
    let bg = s.doc().root[0].id().to_string();
    s.execute(Command::SetActiveLayer { id: bg }).unwrap();
    assert!(!s.mask_target());
    s.execute(Command::SetMaskTarget { on: true }).unwrap();
    assert!(!s.mask_target(), "no mask on the background");
}
