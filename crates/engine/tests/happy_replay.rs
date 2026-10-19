#![allow(clippy::unwrap_used)]
//! Replaying happypaint recordings (spec 62).
use onepaint_engine::recording::Replay;
use serde_json::json;

fn rec(commands: serde_json::Value) -> Vec<u8> {
    let manifest = json!({
        "version": 1, "durationMs": 1000,
        "doc": {"width": 200, "height": 120, "activeLayerId": "a"},
        "nodes": [{"id": "a", "name": "Paper", "visible": true, "opacity": 1, "blendMode": "normal"}],
        "commands": commands,
    });
    onepaint_formats::happy_rec::write_happypaint_rec(&manifest, &[])
}

fn pt(x: f64, y: f64) -> serde_json::Value {
    json!({"x": x, "y": y, "pressure": 0.8})
}

#[test]
fn a_happypaint_session_replays() {
    let brush = serde_json::to_value(onepaint_brush::preset("hard-round").unwrap()).unwrap();
    let line: Vec<_> = (0..40).map(|i| pt(20.0 + i as f64 * 4.0, 60.0)).collect();
    let bytes = rec(json!([
        {"t": 0, "op": "addLayer", "args": {"id": "b"}},
        {"t": 10, "op": "beginStroke", "args": {"first": pt(20.0, 60.0), "brush": brush, "color": [1, 0, 0], "erase": false, "seed": 7}},
        {"t": 20, "op": "continueStroke", "args": {"points": line}},
        {"t": 30, "op": "endStroke", "args": {}},
        {"t": 40, "op": "setLayerProps", "args": {"id": "b", "props": {"name": "Ink", "opacity": 0.5}}},
        {"t": 50, "op": "setActiveLayer", "args": {"id": "a"}},
        {"t": 60, "op": "floodFill", "args": {"point": {"x": 5, "y": 5}, "color": [0, 0, 1], "tolerance": 0.1, "grow": 0}},
        {"t": 70, "op": "undo", "args": {}},
        {"t": 80, "op": "redo", "args": {}},
        {"t": 90, "op": "createPanel", "args": {}},
    ]));
    let (mut r, skipped) = Replay::from_happypaint(&bytes).unwrap();
    assert_eq!(skipped, ["createPanel"]);
    r.step(r.len());
    let d = r.session.doc();
    let ink =
        d.root.iter().filter_map(|n| n.as_layer()).find(|l| l.name == "Ink").expect("the new layer, renamed through its happypaint id");
    assert_eq!(ink.opacity, 0.5);
    let p = ink.pixels.get(100, 60);
    assert!(p[3] > 0.9 && p[0] > 0.9 && p[2] < 0.05, "a red stroke: {p:?}");
    let paper = d.layer("a").unwrap().pixels.get(5, 5);
    assert!(paper[2] > 0.9 && paper[3] > 0.99, "the fill, redone: {paper:?}");
}

/// Old recordings: a gradient's {color, color2} (before v0.188.0).
#[test]
fn old_gradients_replay() {
    let bytes = rec(json!([
        {"t": 0, "op": "gradient", "args": {"opacity": 1, "a": {"x": 0, "y": 0}, "b": {"x": 200, "y": 0}, "color": [0, 1, 0], "color2": null}},
    ]));
    let (mut r, skipped) = Replay::from_happypaint(&bytes).unwrap();
    assert!(skipped.is_empty());
    r.step(r.len());
    let l = r.session.doc().layer("a").unwrap();
    let (left, right) = (l.pixels.get(2, 10), l.pixels.get(197, 10));
    assert!(left[3] > 0.95 && left[1] > 0.9, "{left:?}");
    assert!(right[3] < 0.05, "fades to clear: {right:?}");
}

/// Live watercolour: the stroke, then the recorded sim steps dry it in.
#[test]
fn wet_strokes_replay_through_their_ticks() {
    let line: Vec<_> = (0..30).map(|i| pt(40.0 + i as f64 * 4.0, 60.0)).collect();
    let mut cmds = vec![
        json!({"t": 0, "op": "beginWet", "args": {"first": pt(40.0, 60.0), "size": 30, "fluid": {"water": 0.05, "pigment": 0.7}, "color": [0.1, 0.3, 0.8], "impasto": 0}}),
        json!({"t": 10, "op": "continueWet", "args": {"points": line}}),
        json!({"t": 20, "op": "endWet", "args": {}}),
    ];
    cmds.extend((0..400).map(|i| json!({"t": 30 + i, "op": "wetTick", "args": {"steps": 4}})));
    let (mut r, skipped) = Replay::from_happypaint(&rec(serde_json::Value::Array(cmds))).unwrap();
    assert!(skipped.is_empty());
    r.step(r.len());
    assert!(!r.session.wet_active(), "dried by the recorded steps");
    let p = r.session.doc().layer("a").unwrap().pixels.get(100, 60);
    assert!(p[3] > 0.2 && p[2] > p[0], "blue paint settled: {p:?}");
}

/// Text and path objects: created under happypaint's ids, then edited
/// through them.
#[test]
fn text_and_paths_replay() {
    let text = json!({"text": "Hello", "x": 20, "y": 30, "fontSizePx": 24, "color": "#112233"});
    let path = serde_json::to_value(onepaint_stroke::bezier::PathSpec::default()).unwrap();
    let bytes = rec(json!([
        {"t": 0, "op": "createTextLayer", "args": {"id": "t1", "spec": text}},
        {"t": 5, "op": "moveTextObject", "args": {"id": "t1", "index": 0, "dx": 10, "dy": -5}},
        {"t": 10, "op": "createPathLayer", "args": {"id": "p1", "name": "Outline", "spec": path}},
    ]));
    let (mut r, skipped) = Replay::from_happypaint(&bytes).unwrap();
    assert!(skipped.is_empty(), "{skipped:?}");
    r.step(r.len());
    let s = &r.session;
    let t = s.doc().root.iter().filter_map(|n| n.as_layer()).find(|l| s.is_text_layer(&l.id)).expect("a text layer");
    let spec = &s.texts(&t.id)[0];
    assert_eq!((spec.text.as_str(), spec.x, spec.y), ("Hello", 30.0, 25.0));
    assert!(s.doc().root.iter().any(|n| n.name() == "Outline" && s.is_path_layer(n.id())));
}
