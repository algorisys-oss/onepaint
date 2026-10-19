#![allow(clippy::unwrap_used)]
//! Layer styles through the engine (spec 56): set as a layer property,
//! undone in one step, and redrawn as far as they reach.
use onepaint_doc::styles::{EffectKind, LayerStyles, default_effect};
use onepaint_engine::{Command, LayerProps, Session};

fn stroke_of(size: f32) -> serde_json::Value {
    let mut e = default_effect(EffectKind::Stroke);
    e.size = size;
    LayerStyles { fill_opacity: 1.0, effects: vec![e] }.to_json()
}

#[test]
fn styles_set_as_a_property_and_undo_in_one_step() {
    let mut s = Session::new(600, 400, Some(onepaint_color::Rgba::WHITE));
    let id = s.active().to_string();
    for size in [3.0, 6.0, 9.0] {
        // A dialog drag: the same key, one undo step.
        let props = LayerProps { styles: Some(Some(stroke_of(size))), ..Default::default() };
        s.execute(Command::SetLayerProps { id: id.clone(), props }).unwrap();
    }
    let size = |s: &Session| s.doc().layer(&id).and_then(|l| l.styles()).map(|st| st.effects[0].size);
    assert_eq!(size(&s), Some(9.0));
    s.execute(Command::Undo).unwrap();
    assert_eq!(size(&s), None, "one step back to none");
    s.execute(Command::Redo).unwrap();
    let props = LayerProps { styles: Some(None), ..Default::default() };
    s.execute(Command::SetLayerProps { id: id.clone(), props }).unwrap();
    assert_eq!(size(&s), None, "cleared");
}

#[test]
fn adjustment_layers_take_no_styles() {
    let mut s = Session::new(200, 200, None);
    s.execute(Command::AddAdjustmentLayer { kind: "invert".into(), params: Default::default() }).unwrap();
    let id = s.active().to_string();
    let props = LayerProps { styles: Some(Some(stroke_of(4.0))), ..Default::default() };
    s.execute(Command::SetLayerProps { id: id.clone(), props }).unwrap();
    assert!(s.doc().layer(&id).unwrap().styles().is_none());
}

#[test]
fn painting_redraws_as_far_as_the_effects_reach() {
    let mut s = Session::new(800, 400, None);
    let id = s.active().to_string();
    let props = LayerProps { styles: Some(Some(stroke_of(40.0))), ..Default::default() };
    s.execute(Command::SetLayerProps { id, props }).unwrap();
    s.take_dirty_tiles();
    // A dab near the right edge of tile 0: its 40 px stroke reaches tile 1.
    s.stroke_begin(onepaint_stroke::StrokePoint::at(230.0, 100.0, 1.0, 0.0));
    s.stroke_end();
    let dirty = s.take_dirty_tiles();
    assert!(dirty.contains(&(1, 0)), "{dirty:?}");
}

fn square_session() -> (Session, String) {
    let mut s = Session::new(600, 400, None);
    let id = s.active().to_string();
    s.execute(Command::SetColor { rgba: onepaint_color::Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    let rect = onepaint_selection::rect_polygon([100.0, 100.0], [240.0, 240.0]);
    s.execute(Command::SelectShape { contours: vec![rect], mode: onepaint_selection::Mode::Replace }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    (s, id)
}

#[test]
fn baking_turns_the_effects_into_pixels() {
    let (mut s, id) = square_session();
    let props = LayerProps { styles: Some(Some(stroke_of(6.0))), ..Default::default() };
    s.execute(Command::SetLayerProps { id: id.clone(), props }).unwrap();
    let before = s.flatten();
    s.execute(Command::BakeStyles { id: id.clone() }).unwrap();
    let l = s.doc().layer(&id).unwrap();
    assert!(l.styles().is_none());
    let b = l.pixels.content_bounds().unwrap();
    assert!(b.x <= 95 && b.right() >= 245, "the stroke is pixels now: {b:?}");
    let after = s.flatten();
    let worst = before.iter().zip(&after).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 2e-3, "looks the same: {worst}");
    s.execute(Command::Undo).unwrap();
    assert!(s.doc().layer(&id).unwrap().styles().is_some(), "one undo step back");
}

#[test]
fn global_light_moves_every_linked_effect() {
    let (mut s, id) = square_session();
    let shadow = |linked: bool| {
        let mut e = default_effect(EffectKind::DropShadow);
        e.use_global_light = linked;
        LayerStyles { fill_opacity: 1.0, effects: vec![e] }.to_json()
    };
    s.execute(Command::SetLayerProps { id: id.clone(), props: LayerProps { styles: Some(Some(shadow(true))), ..Default::default() } })
        .unwrap();
    s.execute(Command::AddLayer).unwrap();
    let other = s.active().to_string();
    s.execute(Command::SetLayerProps { id: other.clone(), props: LayerProps { styles: Some(Some(shadow(false))), ..Default::default() } })
        .unwrap();
    s.execute(Command::ApplyGlobalLight { angle: 45.0, altitude: 20.0 }).unwrap();
    let angle = |id: &str| s.doc().layer(id).unwrap().styles().unwrap().effects[0].angle;
    assert_eq!((angle(&id), angle(&other)), (45.0, 120.0), "linked moves, unlinked stays");
}

#[test]
fn merging_down_bakes_the_upper_layers_effects() {
    let (mut s, id) = square_session();
    let props = LayerProps { styles: Some(Some(stroke_of(30.0))), ..Default::default() };
    s.execute(Command::SetLayerProps { id: id.clone(), props }).unwrap();
    // The square ends 16 px before tile 1; its 30 px stroke reaches into it.
    s.execute(Command::AddLayer).unwrap();
    let below = s.active().to_string();
    s.execute(Command::MoveLayer { id: below.clone(), parent: None, index: 0 }).unwrap();
    s.execute(Command::MergeDown { id: id.clone() }).unwrap();
    let l = s.doc().layer(&below).unwrap();
    assert!(l.styles().is_none());
    assert!(l.pixels.tile(1, 0).is_some(), "the stroke past the square's tiles is baked too");
}
