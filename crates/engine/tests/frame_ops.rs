#![allow(clippy::unwrap_used)]
//! Batch retiming and out of pegs (spec 24), with happypaint's cases
//! (`animation-store.test.ts:433-505, 930-1035`).
use onepaint_doc::{Document, Group, Layer, Node};
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

fn session(nodes: Vec<Node>) -> Session {
    let mut s = Session::new(16, 16, None);
    let mut d = Document::new(16, 16, None);
    d.root = nodes;
    s.replace_document(d);
    let mut a = s.anim_settings().clone();
    a.enabled = true;
    s.execute(Command::SetAnimation { settings: Box::new(a) }).unwrap();
    s
}

fn abc() -> Session {
    session(vec![layer("a", 1), layer("b", 1), layer("c", 1)])
}

fn shape(s: &Session) -> Vec<(String, usize)> {
    s.anim_frames().into_iter().map(|f| (f.id, f.hold)).collect()
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn set_duration_applies_to_every_selected_frame_in_one_step() {
    let mut s = abc();
    s.execute(Command::SetFramesDuration { ids: ids(&["a", "c"]), hold: 4 }).unwrap();
    assert_eq!(shape(&s), [("a".into(), 4), ("b".into(), 1), ("c".into(), 4)]);
    s.execute(Command::Undo).unwrap();
    assert_eq!(shape(&s), [("a".into(), 1), ("b".into(), 1), ("c".into(), 1)], "one undo step");
}

#[test]
fn split_cuts_a_held_frame_and_keeps_the_timing() {
    let mut s = session(vec![layer("a", 8), layer("b", 1)]);
    s.execute(Command::SplitFrames { ids: ids(&["a"]), duration: 2 }).unwrap();
    let f = shape(&s);
    assert_eq!(f.len(), 5, "4 pieces + b");
    assert_eq!(f[..4].iter().map(|x| x.1).collect::<Vec<_>>(), [2, 2, 2, 2]);
    assert_eq!(s.anim_total_slots(), 9);
    // The pieces carry the drawing.
    assert_eq!(s.doc().find(&f[1].0).unwrap().name(), "a copy");
}

#[test]
fn split_puts_the_leftover_on_the_last_piece() {
    let mut s = session(vec![layer("a", 5)]);
    s.execute(Command::SplitFrames { ids: ids(&["a"]), duration: 2 }).unwrap();
    assert_eq!(shape(&s).iter().map(|x| x.1).collect::<Vec<_>>(), [2, 2, 1]);
}

#[test]
fn split_skips_short_frames_and_groups() {
    let mut s = session(vec![layer("a", 2)]);
    s.execute(Command::SplitFrames { ids: ids(&["a"]), duration: 2 }).unwrap();
    assert_eq!(shape(&s).len(), 1);
    assert!(!s.can_undo(), "nothing to do pushes no step");
    let mut g = Group::new("g");
    g.id = "g".into();
    g.extra.insert("holdFrames".into(), serde_json::json!(4));
    g.children.push(layer("ink", 1));
    let mut s = session(vec![Node::Group(g)]);
    s.execute(Command::SplitFrames { ids: ids(&["g"]), duration: 2 }).unwrap();
    assert_eq!(shape(&s).len(), 1);
}

#[test]
fn in_betweens_insert_one_empty_frame_after_each() {
    let mut s = abc();
    s.execute(Command::AddInbetweens { ids: ids(&["a", "c"]) }).unwrap();
    let f: Vec<String> = shape(&s).into_iter().map(|x| x.0).collect();
    assert_eq!(f.len(), 5);
    assert_eq!((f[0].as_str(), f[2].as_str(), f[3].as_str()), ("a", "b", "c"));
    assert!(s.doc().find(&f[1]).unwrap().as_layer().unwrap().pixels.tile_count() == 0);
    assert_eq!(s.active(), f[4], "the last one is selected");
}

#[test]
fn duplicate_copies_each_frame_with_its_content() {
    let mut s = abc();
    s.execute(Command::DuplicateFrames { ids: ids(&["b"]) }).unwrap();
    let f: Vec<String> = shape(&s).into_iter().map(|x| x.0).collect();
    assert_eq!((f.len(), f[1].as_str(), f[3].as_str()), (4, "b", "c"));
    assert_eq!(s.doc().find(&f[2]).unwrap().name(), "b copy");
}

#[test]
fn delete_never_removes_the_last_frame_and_the_playhead_stays_real() {
    let mut s = abc();
    s.execute(Command::SetAnimSlot { slot: 2 }).unwrap();
    s.execute(Command::DeleteFrames { ids: ids(&["b", "c"]) }).unwrap();
    assert_eq!(shape(&s), [("a".into(), 1)]);
    assert!(s.anim_slot() < s.anim_total_slots());
    assert_eq!(s.active(), "a");
    s.execute(Command::DeleteFrames { ids: ids(&["a"]) }).unwrap();
    assert_eq!(shape(&s).len(), 1);
}

#[test]
fn track_sheets_ripple_instead_of_overlapping() {
    let mut s = session(vec![layer("a", 1), layer("b", 2), layer("c", 1)]);
    s.execute(Command::ConvertToTracks).unwrap();
    s.execute(Command::SetActiveLayer { id: "a".into() }).unwrap();
    let starts = |s: &Session| s.anim_frames().into_iter().map(|f| (f.start, f.hold)).collect::<Vec<_>>();
    s.execute(Command::SetFramesDuration { ids: ids(&["a"]), hold: 3 }).unwrap();
    assert_eq!(starts(&s), [(0, 3), (3, 2), (5, 1)], "later sheets pushed right");
    s.execute(Command::SplitFrames { ids: ids(&["b"]), duration: 1 }).unwrap();
    assert_eq!(starts(&s), [(0, 3), (3, 1), (4, 1), (5, 1)]);
    s.execute(Command::DuplicateFrames { ids: ids(&["c"]) }).unwrap();
    assert_eq!(starts(&s).last(), Some(&(6, 1)));
}

#[test]
fn pegs_move_ghosts_only_and_round_trip() {
    let mut s = abc();
    s.execute(Command::SetAnimSlot { slot: 1 }).unwrap();
    let mut a = s.anim_settings().clone();
    a.out_of_pegs.insert("a".into(), Peg { dx: 7.0, rot: 0.2, scale: 99.0, ..Default::default() });
    s.execute(Command::SetAnimation { settings: Box::new(a) }).unwrap();
    assert_eq!(s.anim_settings().out_of_pegs["a"].scale, 4.0, "scale clamped");
    // The drawing itself is untouched: frames, holds and layers.
    assert_eq!(shape(&s), [("a".into(), 1), ("b".into(), 1), ("c".into(), 1)]);
    let json = serde_json::to_value(s.anim_settings()).unwrap();
    assert_eq!(json["outOfPegs"]["a"]["dx"], 7.0, "stored as happypaint names it");
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(4, 4, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.anim_settings().out_of_pegs["a"].dx, 7.0);
}
