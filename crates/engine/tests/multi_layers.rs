#![allow(clippy::unwrap_used)]
//! Acting on several picked layers at once (happypaint's Layers panel
//! multi-select bar): group, merge (scattered too) and batches as one step.
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};

/// A new layer with a block of `rgba` over `[x0, x0 + 20) × [10, 30)`.
fn block(s: &mut Session, x0: f64, rgba: Rgba) -> String {
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([x0, 10.0], [x0 + 20.0, 30.0])], mode: Mode::Replace, antialias: false })
        .unwrap();
    s.execute(Command::SetColor { rgba }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s.active().to_string()
}

fn root_ids(s: &Session) -> Vec<String> {
    s.doc().root.iter().map(|n| n.id().to_string()).collect()
}

const RED: Rgba = Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };

#[test]
fn picked_layers_group_at_the_topmost_place() {
    let mut s = Session::new(100, 50, Some(Rgba::WHITE));
    let base = root_ids(&s);
    let a = block(&mut s, 0.0, RED);
    let b = block(&mut s, 30.0, RED);
    let c = block(&mut s, 60.0, RED);
    let n = s.history_len();
    s.execute(Command::GroupLayers { ids: vec![c.clone(), a.clone()] }).unwrap();
    assert_eq!(s.history_len(), n + 1);
    let g = s.active().to_string();
    assert_eq!(root_ids(&s), [base.clone(), vec![b.clone(), g.clone()]].concat(), "the group sits where the top pick was");
    let kids: Vec<String> = s.doc().find(&g).unwrap().as_group().unwrap().children.iter().map(|n| n.id().to_string()).collect();
    assert_eq!(kids, vec![a.clone(), c.clone()], "in their stacking order");
    assert_eq!(s.doc().find(&g).unwrap().as_group().unwrap().blend_mode, onepaint_doc::BlendMode::PassThrough);
    s.execute(Command::Undo).unwrap();
    assert_eq!(root_ids(&s), [base, vec![a, b, c]].concat());
}

#[test]
fn grouping_needs_two_picks_under_one_parent() {
    let mut s = Session::new(100, 50, Some(Rgba::WHITE));
    let a = block(&mut s, 0.0, RED);
    let b = block(&mut s, 30.0, RED);
    s.execute(Command::GroupLayer { id: b.clone() }).unwrap();
    let before = root_ids(&s);
    let n = s.history_len();
    s.execute(Command::GroupLayers { ids: vec![a.clone(), b.clone()] }).unwrap();
    assert_eq!((root_ids(&s), s.history_len()), (before, n), "b is in another parent: nothing to group");
}

/// happypaint v0.208.3: Layer 1 and Layer 3 merge; Layer 2 is untouched.
#[test]
fn scattered_picks_merge_and_the_layer_between_stays() {
    let mut s = Session::new(100, 50, Some(Rgba::WHITE));
    let base = root_ids(&s);
    let a = block(&mut s, 0.0, RED);
    let b = block(&mut s, 30.0, Rgba::new(0.0, 0.0, 1.0, 1.0));
    let c = block(&mut s, 60.0, RED);
    let n = s.history_len();
    assert_eq!(s.mergeable_run(&[a.clone(), c.clone()]), Some(vec![c.clone(), a.clone()]), "top-first");
    s.execute(Command::MergeLayers { ids: vec![a.clone(), c.clone()] }).unwrap();
    assert_eq!(s.history_len(), n + 1, "one undo step");
    assert_eq!(root_ids(&s), [base.clone(), vec![b.clone(), a.clone()]].concat(), "merged into the bottom pick, at the top pick's place");
    let l = s.doc().layer(&a).unwrap();
    assert!(l.pixels.get(5, 15)[3] > 0.99 && l.pixels.get(65, 15)[3] > 0.99, "both blocks");
    assert_eq!(l.pixels.get(35, 15)[3], 0.0, "not the layer between");
    assert!(s.doc().layer(&b).unwrap().pixels.get(35, 15)[3] > 0.99);
    s.execute(Command::Undo).unwrap();
    assert_eq!(root_ids(&s), [base, vec![a, b, c]].concat());
}

#[test]
fn merge_refuses_groups_and_lone_layers() {
    let mut s = Session::new(100, 50, Some(Rgba::WHITE));
    let a = block(&mut s, 0.0, RED);
    let b = block(&mut s, 30.0, RED);
    assert!(s.execute(Command::MergeLayers { ids: vec![a.clone()] }).is_err());
    s.execute(Command::GroupLayer { id: b.clone() }).unwrap();
    let g = s.doc().parent_of(&b).flatten().unwrap();
    assert!(s.mergeable_run(&[a, g]).is_none());
}

#[test]
fn a_batch_is_one_undo_step() {
    let mut s = Session::new(100, 50, Some(Rgba::WHITE));
    let a = block(&mut s, 0.0, RED);
    let b = block(&mut s, 30.0, RED);
    let before = root_ids(&s);
    let n = s.history_len();
    s.execute(Command::Batch { commands: vec![Command::DeleteLayer { id: a.clone() }, Command::DeleteLayer { id: b.clone() }] }).unwrap();
    assert_eq!(s.history_len(), n + 1);
    assert_eq!(root_ids(&s).len(), before.len() - 2);
    s.execute(Command::Undo).unwrap();
    assert_eq!(root_ids(&s), before);
    assert!(s.execute(Command::Batch { commands: vec![Command::Undo] }).is_err());
}
