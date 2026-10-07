#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_filters::Params;
use onepaint_selection::{Mode, rect_polygon};

fn red_session() -> Session {
    let mut s = Session::new(64, 64, Some(Rgba::WHITE));
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SelectAll).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

fn layer_px(s: &Session, x: i32, y: i32) -> [f32; 4] {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)
}

#[test]
fn destructive_filter_previews_commits_once_and_cancels() {
    let mut s = red_session();
    let n = s.history_len();
    s.execute(Command::BeginFilter { kind: "invert".into() }).unwrap();
    assert!(s.filter_active());
    s.execute(Command::UpdateFilter { params: Params::default() }).unwrap();
    let p = layer_px(&s, 10, 10);
    assert!(p[0] < 1e-3 && p[1] > 0.99, "preview shows cyan {p:?}");
    s.execute(Command::CancelFilter).unwrap();
    assert!(layer_px(&s, 10, 10)[0] > 0.99);
    assert_eq!(s.history_len(), n);
    s.execute(Command::BeginFilter { kind: "invert".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: Params::default() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    assert!(!s.filter_active());
    assert_eq!(s.history_len(), n + 1);
    s.execute(Command::Undo).unwrap();
    assert!(layer_px(&s, 10, 10)[0] > 0.99);
}

#[test]
fn updates_restart_from_the_original() {
    let mut s = red_session();
    s.execute(Command::BeginFilter { kind: "brightness-contrast".into() }).unwrap();
    for b in [0.3, 0.3, 0.3] {
        s.execute(Command::UpdateFilter { params: Params { brightness: Some(-b), ..Default::default() } }).unwrap();
    }
    let once = layer_px(&s, 5, 5)[0];
    s.execute(Command::CommitFilter).unwrap();
    let mut t = red_session();
    t.execute(Command::BeginFilter { kind: "brightness-contrast".into() }).unwrap();
    t.execute(Command::UpdateFilter { params: Params { brightness: Some(-0.3), ..Default::default() } }).unwrap();
    assert_eq!(once, layer_px(&t, 5, 5)[0]);
}

#[test]
fn filters_respect_the_selection_and_blur_works() {
    let mut s = red_session();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([0.0, 0.0], [32.0, 64.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::BeginFilter { kind: "invert".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: Params::default() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    assert!(layer_px(&s, 10, 10)[0] < 1e-3);
    assert!(layer_px(&s, 50, 10)[0] > 0.99);
    // Blur across the invert edge softens it.
    s.execute(Command::Deselect).unwrap();
    s.execute(Command::BeginFilter { kind: "gaussian-blur".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: Params { radius: Some(4.0), ..Default::default() } }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    let edge = layer_px(&s, 32, 10)[0];
    assert!(edge > 0.1 && edge < 0.9, "{edge}");
}

#[test]
fn adjustment_layer_composites_live_and_round_trips() {
    let mut s = red_session();
    s.execute(Command::AddAdjustmentLayer { kind: "invert".into(), params: Params::default() }).unwrap();
    let id = s.active().to_string();
    let c = s.pick_color(10, 10);
    assert!(c.r < 0.01 && c.g > 0.99, "invert layer turns red cyan {c:?}");
    s.execute(Command::SetLayerProps { id: id.clone(), props: LayerProps { opacity: Some(0.5), ..Default::default() } }).unwrap();
    let h = s.pick_color(10, 10);
    assert!(h.r > 0.3 && h.r < 0.9, "half strength {h:?}");
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    let back = t.doc().find(&id).unwrap().as_layer().unwrap().adjustment.clone().unwrap();
    assert_eq!(back.kind, "invert");
    assert!((t.pick_color(10, 10).r - h.r).abs() < 0.02);
}
