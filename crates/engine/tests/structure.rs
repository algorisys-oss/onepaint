#![allow(clippy::unwrap_used)]
//! The Layer and Image menus' structure commands.
use onepaint_color::Rgba;
use onepaint_doc::Node;
use onepaint_engine::*;

fn painted() -> Session {
    let mut s = Session::new(64, 64, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SelectShape {
        contours: vec![onepaint_selection::rect_polygon([0.0, 0.0], [32.0, 64.0])],
        mode: onepaint_selection::Mode::Replace,
        antialias: false,
    })
    .unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

#[test]
fn group_and_ungroup_keep_the_layer_and_its_place() {
    let mut s = painted();
    let id = s.active().to_string();
    let n = s.doc().root.len();
    s.execute(Command::GroupLayer { id: id.clone() }).unwrap();
    assert_eq!(s.doc().root.len(), n);
    let g = s.doc().root[n - 1].as_group().unwrap();
    assert_eq!(g.children[0].id(), id);
    assert_eq!(s.active(), id, "the layer stays active");
    // happypaint v0.211.11: a grouped Color Dodge layer keeps blending with
    // the painting below.
    assert_eq!(g.blend_mode, onepaint_doc::BlendMode::PassThrough);
    let gid = g.id.clone();
    s.execute(Command::Ungroup { id: gid.clone() }).unwrap();
    assert_eq!(s.doc().root[n - 1].id(), id);
    assert!(s.doc().find(&gid).is_none());
    assert!(s.execute(Command::Ungroup { id }).is_err(), "a layer isn't a group");
    s.execute(Command::Undo).unwrap();
    assert!(s.doc().find(&gid).is_some(), "one undo step");
}

#[test]
fn a_grouped_blend_layer_looks_the_same() {
    let mut s = painted();
    let id = s.active().to_string();
    s.execute(Command::SetLayerProps {
        id: id.clone(),
        props: LayerProps { blend_mode: Some(onepaint_doc::BlendMode::ColorDodge), ..Default::default() },
    })
    .unwrap();
    let before = s.flatten();
    s.execute(Command::GroupLayer { id }).unwrap();
    let after = s.flatten();
    let worst = before.iter().zip(&after).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-5, "pass-through: the dodge still sees the white below ({worst})");
}

#[test]
fn merge_visible_and_flatten_keep_the_picture() {
    let mut s = painted();
    let before = s.flatten();
    s.execute(Command::MergeVisible).unwrap();
    let merged = s.doc().layer(s.active()).unwrap();
    assert_eq!(merged.name, "Merged");
    let px = merged.pixels.get(10, 10);
    assert!((px[0] - 1.0).abs() < 1e-3 && px[1] < 1e-3 && px[3] > 0.999, "{px:?}");
    assert!((merged.pixels.get(50, 10)[1] - 1.0).abs() < 1e-3, "the white background too");
    s.execute(Command::Undo).unwrap();
    let n = s.doc().root.len();
    s.execute(Command::FlattenImage).unwrap();
    assert_eq!(s.doc().root.len(), 1);
    assert!(matches!(&s.doc().root[0], Node::Layer(l) if l.name == "Background"));
    let after = s.flatten();
    assert!(before.iter().zip(&after).all(|(a, b)| (a - b).abs() < 2e-3), "the same picture");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().root.len(), n);
}

#[test]
fn invert_mask_swaps_hidden_and_shown_everywhere() {
    let mut s = painted();
    s.execute(Command::SelectShape {
        contours: vec![onepaint_selection::rect_polygon([0.0, 0.0], [16.0, 16.0])],
        mode: onepaint_selection::Mode::Replace,
        antialias: false,
    })
    .unwrap();
    s.execute(Command::AddMask).unwrap();
    s.execute(Command::Deselect).unwrap();
    let id = s.active().to_string();
    let m = |s: &Session, x, y| s.doc().layer(&id).unwrap().mask.as_ref().unwrap().get(x, y)[0];
    assert!(m(&s, 5, 5) > 0.99 && m(&s, 40, 40) < 0.01);
    s.execute(Command::InvertMask { id: id.clone() }).unwrap();
    assert!(m(&s, 5, 5) < 0.01 && m(&s, 40, 40) > 0.99);
    s.execute(Command::Undo).unwrap();
    assert!(m(&s, 5, 5) > 0.99);
}
