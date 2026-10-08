#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;
use onepaint_text::TextSpec;

fn hello(x: f64) -> TextSpec {
    TextSpec { text: "Hello".into(), x, y: 40.0, font_size_px: 40.0, color: "#ff0000".into(), ..Default::default() }
}

#[test]
fn add_update_delete_text_with_undo() {
    let mut s = Session::new(400, 200, Some(Rgba::WHITE));
    let n_layers = s.doc().root.len();
    s.execute(Command::AddText { spec: hello(20.0) }).unwrap();
    let id = s.active().to_string();
    assert_eq!(s.doc().root.len(), n_layers + 1);
    assert_eq!(s.texts(&id).len(), 1);
    let ink = || s.doc().layer(&id).unwrap().pixels.content_bounds();
    let b = ink().unwrap();
    assert!(b.x < 40, "{b:?}");
    assert_eq!(s.text_at(30.0, 60.0), Some((id.clone(), 0)));
    assert_eq!(s.text_at(390.0, 190.0), None);
    s.execute(Command::UpdateText { id: id.clone(), index: 0, spec: hello(200.0) }).unwrap();
    assert!(s.doc().layer(&id).unwrap().pixels.content_bounds().unwrap().x > 180);
    s.execute(Command::Undo).unwrap();
    assert!(s.doc().layer(&id).unwrap().pixels.content_bounds().unwrap().x < 40);
    // A second text goes onto the same (active) text layer.
    s.execute(Command::AddText { spec: hello(200.0) }).unwrap();
    assert_eq!(s.texts(&id).len(), 2);
    s.execute(Command::DeleteText { id: id.clone(), index: 1 }).unwrap();
    assert_eq!(s.texts(&id).len(), 1);
}

#[test]
fn painting_rasterises_and_files_keep_texts() {
    let mut s = Session::new(400, 200, Some(Rgba::WHITE));
    s.execute(Command::AddText { spec: hello(20.0) }).unwrap();
    let id = s.active().to_string();
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.texts(&id).len(), 1);
    assert_eq!(t.texts(&id)[0].text, "Hello");
    let n = s.history_len();
    s.stroke_begin(StrokePoint::at(10.0, 150.0, 1.0, 0.0));
    s.stroke_extend(&[StrokePoint::at(100.0, 150.0, 1.0, 50.0)]);
    s.stroke_end();
    assert!(s.texts(&id).is_empty(), "painting turned it into a plain layer");
    assert_eq!(s.history_len(), n + 2, "rasterise + stroke");
    s.execute(Command::Undo).unwrap();
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.texts(&id).len(), 1);
}

#[test]
fn live_edits_coalesce() {
    let mut s = Session::new(300, 100, None);
    s.execute(Command::AddText { spec: hello(10.0) }).unwrap();
    let id = s.active().to_string();
    let n = s.history_len();
    for x in [20.0, 30.0, 40.0] {
        s.execute(Command::UpdateText { id: id.clone(), index: 0, spec: hello(x) }).unwrap();
    }
    assert_eq!(s.history_len(), n + 1);
}

#[test]
fn balloons_and_runs_survive_save_and_open() {
    use onepaint_text::balloon::{Balloon, BalloonShape, Tail};
    use onepaint_text::runs::{StyleEdit, apply_style};
    let mut s = Session::new(400, 200, Some(Rgba::WHITE));
    let b = Balloon {
        shape: BalloonShape::Cloud,
        fill: "#ffffff".into(),
        stroke: Some("#000000".into()),
        stroke_width: Some(3.0),
        tail: Some(Tail { x: 40.0, y: 90.0, width: Some(16.0) }),
        ..Default::default()
    };
    let spec = apply_style(&TextSpec { balloon: Some(b), ..hello(60.0) }, 1..3, &StyleEdit::Bold);
    assert!(spec.runs.is_some());
    s.execute(Command::AddText { spec: spec.clone() }).unwrap();
    let id = s.active().to_string();
    // The balloon is drawn: its fill reaches beyond the glyphs.
    let ink = s.doc().layer(&id).unwrap().pixels.content_bounds().unwrap();
    assert!(ink.x < 50 && ink.y < 30, "{ink:?}");
    assert_eq!(s.text_at(62.0, 32.0), Some((id.clone(), 0)), "the balloon pad is hittable");
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.texts(&id), vec![spec]);
}

#[test]
fn rotating_and_scaling_text_keeps_it_live_but_stretching_bakes() {
    let mut s = Session::new(400, 200, Some(Rgba::WHITE));
    s.execute(Command::AddText { spec: hello(20.0) }).unwrap();
    let id = s.active().to_string();
    s.execute(Command::BeginTransform).unwrap();
    let (k, r) = (1.5f64, 0.3f64);
    let m = transform::Affine { a: k * r.cos(), b: k * r.sin(), c: -k * r.sin(), d: k * r.cos(), e: 5.0, f: 7.0 };
    s.execute(Command::SetTransform { matrix: m }).unwrap();
    s.execute(Command::CommitTransform).unwrap();
    let t = &s.texts(&id)[0];
    assert!((t.font_size_px - 60.0).abs() < 1e-9 && (t.rotation - r).abs() < 1e-9);
    let [x, y] = m.apply([20.0, 40.0]);
    assert!((t.x - x).abs() < 1e-9 && (t.y - y).abs() < 1e-9);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.texts(&id)[0].font_size_px, 40.0);
    s.execute(Command::BeginTransform).unwrap();
    s.execute(Command::SetTransform { matrix: transform::Affine { a: 2.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 } }).unwrap();
    s.execute(Command::CommitTransform).unwrap();
    assert!(!s.is_text_layer(&id), "a stretch can't be a font size");
    assert!(s.doc().layer(&id).unwrap().pixels.content_bounds().is_some());
}
