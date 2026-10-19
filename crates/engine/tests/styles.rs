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
