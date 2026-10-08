#![allow(clippy::unwrap_used)]
//! Exposure-sheet editing (spec 24), with happypaint's numbers
//! (`animation-store.test.ts:687-938`).
use onepaint_doc::{Document, Layer, Node};
use onepaint_engine::anim::*;
use onepaint_engine::*;

fn layer(name: &str, hold: u32) -> Node {
    let mut l = Layer::new(name);
    l.id = name.into();
    if hold != 1 {
        l.extra.insert("holdFrames".into(), serde_json::json!(hold));
    }
    Node::Layer(l)
}

/// A flipbook a, b (hold 2), c, converted to one track.
fn tracks() -> Session {
    let mut s = Session::new(16, 16, None);
    let mut d = Document::new(16, 16, None);
    d.root = vec![layer("a", 1), layer("b", 2), layer("c", 1)];
    s.replace_document(d);
    let mut a = s.anim_settings().clone();
    a.enabled = true;
    s.execute(Command::SetAnimation { settings: Box::new(a) }).unwrap();
    s.execute(Command::ConvertToTracks).unwrap();
    s
}

fn sheets(s: &Session, track: usize) -> Vec<(String, usize, usize)> {
    tracks_of(&s.doc().root)[track].sheets.clone().unwrap().into_iter().map(|f| (f.id, f.hold, f.start)).collect()
}

#[test]
fn converting_keeps_the_timing() {
    let s = tracks();
    assert!(is_tracks_doc(&s.doc().root));
    assert_eq!(sheets(&s, 0), [("a".into(), 1, 0), ("b".into(), 2, 1), ("c".into(), 1, 3)]);
    assert_eq!(s.anim_total_slots(), 4);
}

#[test]
fn moving_a_sheet_clamps_between_its_neighbours_and_is_one_step() {
    let mut s = tracks();
    // b (hold 2) sits between a@0 and c@3: it can only start at 1.
    s.execute(Command::MoveSheet { id: "b".into(), column: 0 }).unwrap();
    s.execute(Command::MoveSheet { id: "b".into(), column: 99 }).unwrap();
    assert_eq!(sheets(&s, 0)[1].2, 1);
    // The last sheet is free to the right; the first stops at 0.
    for col in [5, 6, 7] {
        s.execute(Command::MoveSheet { id: "c".into(), column: col }).unwrap();
    }
    assert_eq!((sheets(&s, 0)[2].2, s.anim_total_slots()), (7, 8));
    s.execute(Command::MoveSheet { id: "a".into(), column: -5 }).unwrap();
    assert_eq!(sheets(&s, 0)[0].2, 0);
    // A drag's moves undo together.
    s.execute(Command::Undo).unwrap();
    assert_eq!(sheets(&s, 0)[2].2, 3);
}

#[test]
fn adding_sheets_and_tracks() {
    let mut s = tracks();
    s.execute(Command::MoveSheet { id: "c".into(), column: 6 }).unwrap();
    let track = tracks_of(&s.doc().root)[0].id.clone();
    // Occupied columns are refused; a gap takes a new drawing, neighbours stay.
    assert!(s.execute(Command::AddSheet { track: track.clone(), column: 2 }).is_err());
    s.execute(Command::AddSheet { track: track.clone(), column: 4 }).unwrap();
    let sh = sheets(&s, 0);
    assert_eq!(sh.iter().map(|x| x.2).collect::<Vec<_>>(), [0, 1, 4, 6]);
    assert_eq!((s.active(), s.anim_slot()), (sh[2].0.as_str(), 4));
    // Past the end grows the timeline.
    s.execute(Command::AddSheet { track: track.clone(), column: 9 }).unwrap();
    assert_eq!(s.anim_total_slots(), 10);
    // A new track holds one blank sheet, on top; tracks reorder and delete.
    s.execute(Command::AddTrack).unwrap();
    let ts = tracks_of(&s.doc().root);
    assert_eq!((ts.len(), ts[1].sheets.as_ref().unwrap().len()), (2, 1));
    let top = ts[1].id.clone();
    s.execute(Command::MoveTrack { id: top.clone(), delta: -1 }).unwrap();
    assert_eq!(tracks_of(&s.doc().root)[0].id, top);
    assert!(s.execute(Command::MoveTrack { id: top.clone(), delta: -1 }).is_err(), "already at the end");
    s.execute(Command::DeleteTrack { id: top }).unwrap();
    assert!(s.execute(Command::DeleteTrack { id: track }).is_err(), "the last sheet track stays");
}

#[test]
fn conversion_needs_a_flipbook() {
    let mut s = Session::new(8, 8, None);
    let mut d = Document::new(8, 8, None);
    d.root = vec![layer("only", 1)];
    s.replace_document(d);
    assert!(s.execute(Command::ConvertToTracks).is_err());
    let mut t = tracks();
    assert!(t.execute(Command::ConvertToTracks).is_err(), "already tracks");
}
