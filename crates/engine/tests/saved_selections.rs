#![allow(clippy::unwrap_used)]
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};

fn select(s: &mut Session, a: [f64; 2], b: [f64; 2], mode: Mode) {
    s.execute(Command::SelectShape { contours: vec![rect_polygon(a, b)], mode }).unwrap();
}

fn selected(s: &Session, x: i32, y: i32) -> bool {
    s.selection().is_some_and(|m| m.coverage(x, y) > 0.5)
}

#[test]
fn save_deselect_load_and_combine() {
    let mut s = Session::new(100, 80, None);
    assert!(s.execute(Command::SaveSelection { name: "none".into() }).is_err(), "nothing to save");
    select(&mut s, [10.0, 10.0], [30.0, 30.0], Mode::Replace);
    s.execute(Command::SaveSelection { name: "Left".into() }).unwrap();
    let id = s.saved_selections()[0].id.clone();
    assert!(id.starts_with("sel-"));
    s.execute(Command::Deselect).unwrap();
    s.execute(Command::LoadSelection { id: id.clone(), mode: Mode::Replace }).unwrap();
    assert!(selected(&s, 20, 20) && !selected(&s, 50, 20));
    select(&mut s, [50.0, 10.0], [70.0, 30.0], Mode::Replace);
    s.execute(Command::LoadSelection { id: id.clone(), mode: Mode::Add }).unwrap();
    assert!(selected(&s, 20, 20) && selected(&s, 60, 20));
    s.execute(Command::LoadSelection { id, mode: Mode::Subtract }).unwrap();
    assert!(!selected(&s, 20, 20) && selected(&s, 60, 20));
}

#[test]
fn save_and_delete_are_undoable() {
    let mut s = Session::new(100, 80, None);
    select(&mut s, [10.0, 10.0], [30.0, 30.0], Mode::Replace);
    s.execute(Command::SaveSelection { name: "A".into() }).unwrap();
    s.execute(Command::Undo).unwrap();
    assert!(s.saved_selections().is_empty());
    s.execute(Command::Redo).unwrap();
    let id = s.saved_selections()[0].id.clone();
    s.execute(Command::DeleteSavedSelection { id }).unwrap();
    assert!(s.saved_selections().is_empty());
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.saved_selections()[0].name, "A");
}

#[test]
fn saved_selections_travel_with_the_file() {
    let mut s = Session::new(100, 80, None);
    let empty = s.save_happypaint().unwrap();
    assert!(!onepaint_formats::read_happypaint(&empty).unwrap().extra.contains_key("savedSelections"));
    select(&mut s, [10.0, 10.0], [30.0, 30.0], Mode::Replace);
    s.execute(Command::SaveSelection { name: "Keep".into() }).unwrap();
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.saved_selections().len(), 1);
    assert_eq!(t.saved_selections()[0].name, "Keep");
    let id = t.saved_selections()[0].id.clone();
    t.execute(Command::LoadSelection { id, mode: Mode::Replace }).unwrap();
    assert!(selected(&t, 20, 20));

    // An entry stored for another canvas size is skipped.
    let mut doc = onepaint_formats::read_happypaint(&bytes).unwrap();
    doc.width = 101;
    let mut u = Session::from_document(doc);
    assert!(u.saved_selections().is_empty());
    let _ = u.drain_events();
}

#[test]
fn canvas_size_shifts_saved_masks_and_undo_restores() {
    let mut s = Session::new(100, 80, None);
    select(&mut s, [10.0, 10.0], [30.0, 30.0], Mode::Replace);
    s.execute(Command::SaveSelection { name: "A".into() }).unwrap();
    s.execute(Command::ResizeCanvas { width: 140, height: 100, dx: 20, dy: 10 }).unwrap();
    let m = s.saved_selections()[0].mask.clone();
    assert_eq!((m.width(), m.height()), (140, 100));
    assert!(m.coverage(35, 25) > 0.5 && m.coverage(15, 15) < 0.5);
    s.execute(Command::ScaleImage { width: 70, height: 50 }).unwrap();
    assert_eq!(s.saved_selections()[0].mask.width(), 70);
    s.execute(Command::Undo).unwrap();
    s.execute(Command::Undo).unwrap();
    let m = s.saved_selections()[0].mask.clone();
    assert_eq!(m.width(), 100);
    assert!(m.coverage(15, 15) > 0.5);
}
