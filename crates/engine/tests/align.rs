#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::align::{AlignMode, Axis, align_offsets, distribute_offsets};
use onepaint_engine::*;
use onepaint_raster::Rect;
use onepaint_selection::Mode;
use onepaint_text::TextSpec;

fn rect_polygon(a: [f64; 2], b: [f64; 2]) -> Vec<[f64; 2]> {
    vec![a, [b[0], a[1]], b, [a[0], b[1]]]
}

/// A new raster layer holding a filled rectangle.
fn block(s: &mut Session, x: f64, y: f64, w: f64, h: f64) -> String {
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([x, y], [x + w, y + h])], mode: Mode::Replace }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s.active().to_string()
}

fn bounds(s: &Session, id: &str) -> Rect {
    s.doc().layer(id).unwrap().pixels.content_bounds().unwrap()
}

#[test]
fn align_left_moves_pixels_in_one_undo_step() {
    let mut s = Session::new(200, 100, Some(Rgba::WHITE));
    let a = block(&mut s, 10.0, 10.0, 20.0, 20.0);
    let b = block(&mut s, 100.0, 60.0, 40.0, 30.0);
    let ids = s.transformable_ids(&[a.clone(), b.clone()]);
    let items = s.layer_content_bounds(&ids);
    assert_eq!(items, vec![(a.clone(), Rect::new(10, 10, 20, 20)), (b.clone(), Rect::new(100, 60, 40, 30))]);
    let n = s.history_len();
    let moves = align_offsets(&items, AlignMode::Left, s.doc().bounds());
    s.execute(Command::AlignLayers { moves }).unwrap();
    assert_eq!(s.history_len(), n + 1);
    assert_eq!(bounds(&s, &a), Rect::new(0, 10, 20, 20));
    assert_eq!(bounds(&s, &b), Rect::new(0, 60, 40, 30));
    s.execute(Command::Undo).unwrap();
    assert_eq!(bounds(&s, &a), Rect::new(10, 10, 20, 20));
    assert_eq!(bounds(&s, &b), Rect::new(100, 60, 40, 30));
}

#[test]
fn distribute_keeps_the_outer_layers() {
    let mut s = Session::new(200, 100, Some(Rgba::WHITE));
    let a = block(&mut s, 0.0, 0.0, 10.0, 10.0);
    let b = block(&mut s, 25.0, 0.0, 10.0, 10.0);
    let c = block(&mut s, 90.0, 0.0, 10.0, 10.0);
    let items = s.layer_content_bounds(&[a.clone(), b.clone(), c.clone()]);
    s.execute(Command::AlignLayers { moves: distribute_offsets(&items, Axis::Horizontal) }).unwrap();
    assert_eq!((bounds(&s, &a).x, bounds(&s, &b).x, bounds(&s, &c).x), (0, 45, 90));
}

#[test]
fn hidden_locked_empty_and_background_rules() {
    let mut s = Session::new(200, 100, Some(Rgba::WHITE));
    let a = block(&mut s, 10.0, 10.0, 20.0, 20.0);
    let hidden = block(&mut s, 50.0, 10.0, 20.0, 20.0);
    let locked = block(&mut s, 90.0, 10.0, 20.0, 20.0);
    s.execute(Command::AddLayer).unwrap();
    let empty = s.active().to_string();
    s.execute(Command::SetLayerProps { id: hidden.clone(), props: LayerProps { visible: Some(false), ..Default::default() } }).unwrap();
    s.execute(Command::SetLayerProps { id: locked.clone(), props: LayerProps { locked: Some(true), ..Default::default() } }).unwrap();
    let ids = s.transformable_ids(&[a.clone(), hidden.clone(), locked.clone(), empty.clone()]);
    assert_eq!(ids, vec![a.clone(), empty.clone()]);
    assert_eq!(s.layer_content_bounds(&ids).len(), 1, "an empty layer has no artwork to measure");
    // Moves for layers that can't move are dropped.
    let n = s.history_len();
    let moves = vec![align::LayerMove { id: hidden.clone(), dx: 5, dy: 0 }, align::LayerMove { id: locked.clone(), dx: 5, dy: 0 }];
    s.execute(Command::AlignLayers { moves }).unwrap();
    assert_eq!(s.history_len(), n, "nothing moved, nothing recorded");
    assert_eq!(bounds(&s, &hidden).x, 50);
}

#[test]
fn a_picked_group_aligns_its_children() {
    let mut s = Session::new(200, 100, Some(Rgba::WHITE));
    let a = block(&mut s, 10.0, 10.0, 20.0, 20.0);
    s.execute(Command::AddGroup).unwrap();
    let group = s.active().to_string();
    s.execute(Command::MoveLayer { id: a.clone(), parent: Some(group.clone()), index: 0 }).unwrap();
    assert_eq!(s.doc().parent_of(&a), Some(Some(group.clone())));
    assert_eq!(s.transformable_ids(std::slice::from_ref(&group)), vec![a]);

    // A hidden group hides its children whatever their own flags say, so
    // they don't move (happypaint review N8, v0.205.3).
    s.execute(Command::SetLayerProps { id: group.clone(), props: LayerProps { visible: Some(false), ..Default::default() } }).unwrap();
    assert!(s.transformable_ids(&[group]).is_empty());
}

#[test]
fn text_and_path_layers_stay_vector() {
    let mut s = Session::new(400, 200, Some(Rgba::WHITE));
    s.execute(Command::AddText {
        spec: TextSpec { text: "Hi".into(), x: 100.0, y: 40.0, font_size_px: 40.0, color: "#ff0000".into(), ..Default::default() },
    })
    .unwrap();
    let t = s.active().to_string();
    let before = bounds(&s, &t);
    s.execute(Command::AlignLayers { moves: vec![align::LayerMove { id: t.clone(), dx: -50, dy: 7 }] }).unwrap();
    let spec = &s.texts(&t)[0];
    assert_eq!((spec.x, spec.y), (50.0, 47.0));
    let after = bounds(&s, &t);
    assert_eq!((after.x, after.y), (before.x - 50, before.y + 7));
}
