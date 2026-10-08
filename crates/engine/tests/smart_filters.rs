#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::smart_filter_ops::SmartFilter;
use onepaint_engine::*;
use onepaint_filters::{Kind, default_params};

fn red() -> Session {
    let mut s = Session::new(64, 64, None);
    s.execute(Command::SelectAll).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

fn shown(s: &Session) -> [f32; 4] {
    s.composite(&[(0, 0)])[0].1.get(10, 10)
}

#[test]
fn a_smart_filter_is_non_destructive_and_toggleable() {
    let mut s = red();
    let id = s.active().to_string();
    s.execute(Command::AddSmartFilter { id: id.clone(), kind: "invert".into(), params: default_params(Kind::Invert) }).unwrap();
    let p = shown(&s);
    assert!(p[0] < 0.01 && p[1] > 0.99 && p[2] > 0.99, "shown inverted: {p:?}");
    assert!(s.doc().layer(&id).unwrap().pixels.get(10, 10)[0] > 0.99, "pixels untouched");
    let mut f = s.smart_filters(&id);
    f[0].enabled = Some(false);
    s.execute(Command::SetSmartFilters { id: id.clone(), filters: f }).unwrap();
    assert!(shown(&s)[0] > 0.99, "disabled");
    s.execute(Command::Undo).unwrap();
    assert!(shown(&s)[0] < 0.01, "undo re-enables");
    s.execute(Command::Undo).unwrap();
    assert!(s.smart_filters(&id).is_empty() && shown(&s)[0] > 0.99);
}

#[test]
fn stacks_apply_in_order_and_include_spatial_kinds() {
    let mut s = red();
    let id = s.active().to_string();
    // Blur then invert: the result is still a flat field, inverted.
    let mut blur = default_params(Kind::GaussianBlur);
    blur.radius = Some(4.0);
    s.execute(Command::AddSmartFilter { id: id.clone(), kind: "gaussian-blur".into(), params: blur }).unwrap();
    s.execute(Command::AddSmartFilter { id: id.clone(), kind: "invert".into(), params: default_params(Kind::Invert) }).unwrap();
    assert_eq!(s.smart_filters(&id).len(), 2);
    assert!(shown(&s)[1] > 0.99);
    assert!(s.flatten()[(10 * 64 + 10) * 4 + 1] > 0.99, "exports see the filters");
    assert!(
        s.execute(Command::AddSmartFilter { id: id.clone(), kind: "clouds".into(), params: Default::default() }).is_err(),
        "generators are not smart filters"
    );
}

#[test]
fn bake_keeps_the_look_and_drops_the_stack() {
    let mut s = red();
    let id = s.active().to_string();
    s.execute(Command::AddSmartFilter { id: id.clone(), kind: "invert".into(), params: default_params(Kind::Invert) }).unwrap();
    s.execute(Command::BakeSmartFilters { id: id.clone() }).unwrap();
    assert!(s.smart_filters(&id).is_empty());
    assert!(s.doc().layer(&id).unwrap().pixels.get(10, 10)[1] > 0.99);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.smart_filters(&id).len(), 1);
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(4, 4, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.smart_filters(&id), vec![SmartFilter { enabled: None, ..s.smart_filters(&id)[0].clone() }]);
}
