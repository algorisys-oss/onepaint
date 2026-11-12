#![allow(clippy::unwrap_used)]
//! Impasto height fields (spec 35 C): built by thick brushes, carved by the
//! eraser, levelled by blenders, lit by the compositor, undoable and saved.
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn pt(x: f64, y: f64) -> StrokePoint {
    StrokePoint { x, y, pressure: 1.0, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None }
}

fn drag(s: &mut Session, x0: f64, x1: f64, y: f64) {
    s.stroke_begin(pt(x0, y));
    let pts: Vec<_> = (0..=((x1 - x0) / 2.0) as i32).map(|i| pt(x0 + i as f64 * 2.0, y)).collect();
    s.stroke_extend(&pts);
    s.stroke_end();
}

fn brush(s: &mut Session, id: &str, size: f64) {
    s.execute(Command::SetTool { tool: Tool::Brush }).unwrap();
    s.execute(Command::SetBrushPreset { preset: Box::new(onepaint_brush::preset(id).unwrap()) }).unwrap();
    s.execute(Command::SetBrushSize { size }).unwrap();
}

fn session() -> Session {
    let mut s = Session::new(200, 120, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.8, 0.3, 0.2, 1.0) }).unwrap();
    s
}

fn height(s: &Session, x: i32, y: i32) -> f32 {
    s.doc().layer(s.active()).unwrap().height.as_ref().map_or(0.0, |h| h.get(x, y)[0])
}

#[test]
fn a_thick_dry_brush_builds_relief_that_undoes_with_the_stroke() {
    let mut s = session();
    brush(&mut s, "thick-paint", 30.0);
    drag(&mut s, 30.0, 170.0, 60.0);
    let h = height(&s, 100, 60);
    assert!(h > 0.05, "relief under the stroke: {h}");
    assert_eq!(height(&s, 100, 110), 0.0, "none beside it");
    s.execute(Command::Undo).unwrap();
    assert!(s.doc().layer(s.active()).unwrap().height.is_none(), "one undo step takes paint and relief");
    s.execute(Command::Redo).unwrap();
    assert!((height(&s, 100, 60) - h).abs() < 1e-3);
    // A flat brush leaves no field.
    let mut t = session();
    brush(&mut t, "hard-round", 30.0);
    drag(&mut t, 30.0, 170.0, 60.0);
    assert!(t.doc().layer(t.active()).unwrap().height.is_none());
}

#[test]
fn relief_lights_the_paint_and_the_flat_brush_stays_flat() {
    let mut thick = session();
    brush(&mut thick, "thick-paint", 30.0);
    drag(&mut thick, 30.0, 170.0, 60.0);
    // Flat-painted copy of the same pixels, for comparison.
    let mut flat_doc = thick.doc().clone();
    for n in flat_doc.root.iter_mut() {
        if let onepaint_doc::Node::Layer(l) = n {
            l.height = None;
        }
    }
    // Across the stroke the lit (top) flank and the far (bottom) flank differ
    // from the flat paint in opposite directions.
    let lit = |y| onepaint_compose::composite_px(thick.doc(), 100, y)[0];
    let plain = |y| onepaint_compose::composite_px(&flat_doc, 100, y)[0];
    let diffs: Vec<f32> = (46..=74).map(|y| lit(y) - plain(y)).collect();
    assert!(diffs.iter().any(|d| *d > 0.005) && diffs.iter().any(|d| *d < -0.005), "{diffs:?}");
    // Off the stroke nothing changes.
    assert_eq!(lit(110), plain(110));
}

#[test]
fn pigment_thins_a_dry_stroke_to_a_glaze() {
    let mut s = session();
    let mut b = onepaint_brush::preset("hard-round").unwrap();
    b.extra.insert("wet".into(), serde_json::json!({"wetness": 0, "load": 0.5}));
    s.execute(Command::SetBrushPreset { preset: Box::new(b) }).unwrap();
    s.execute(Command::SetBrushSize { size: 30.0 }).unwrap();
    drag(&mut s, 30.0, 170.0, 60.0);
    let a = s.doc().layer(s.active()).unwrap().pixels.get(100, 60)[3];
    assert!((a - 0.525).abs() < 0.02, "opacity × pigmentOpacity(0.5): {a}");
}

#[test]
fn the_eraser_carves_relief_and_cancel_restores_it() {
    let mut s = session();
    brush(&mut s, "thick-paint", 30.0);
    drag(&mut s, 30.0, 170.0, 60.0);
    let h = height(&s, 100, 60);
    // Cancelling a thick stroke puts the field back exactly.
    s.stroke_begin(pt(100.0, 60.0));
    s.stroke_extend(&[pt(110.0, 60.0), pt(120.0, 60.0)]);
    assert!(height(&s, 110, 60) > h, "stamped live");
    s.stroke_cancel();
    assert!((height(&s, 110, 60) - height(&s, 90, 60)).abs() < 0.05);
    s.execute(Command::SetTool { tool: Tool::Eraser }).unwrap();
    s.execute(Command::SetBrushSize { size: 40.0 }).unwrap();
    drag(&mut s, 80.0, 120.0, 60.0);
    assert!(height(&s, 100, 60) < h * 0.1, "carved: {}", height(&s, 100, 60));
    assert!(height(&s, 40, 60) > 0.05, "untouched outside the eraser");
    s.execute(Command::Undo).unwrap();
    assert!((height(&s, 100, 60) - h).abs() < 1e-3, "undo restores the carve");
}

#[test]
fn wet_oil_builds_relief_and_a_blender_levels_it() {
    let mut s = session();
    brush(&mut s, "impasto", 30.0);
    drag(&mut s, 30.0, 170.0, 60.0);
    let h = height(&s, 100, 60);
    assert!(h > 0.1, "wet impasto: {h}");
    brush(&mut s, "oil-blender", 40.0);
    drag(&mut s, 30.0, 170.0, 60.0);
    let after = height(&s, 100, 60);
    assert!(after < h * 0.9, "levelled {h} → {after}");
    // A blender over flat paint creates no field.
    let mut t = session();
    brush(&mut t, "oil-blender", 40.0);
    drag(&mut t, 30.0, 170.0, 60.0);
    assert!(t.doc().layer(t.active()).unwrap().height.is_none());
}

#[test]
fn bevel_from_alpha_replaces_the_field_and_undoes() {
    let mut s = session();
    s.execute(Command::SelectShape {
        contours: vec![onepaint_selection::rect_polygon([50.0, 20.0], [150.0, 100.0])],
        mode: onepaint_selection::Mode::Replace,
        antialias: false,
    })
    .unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    let id = s.active().to_string();
    s.execute(Command::BevelFromAlpha { id: id.clone(), depth: 0.6, softness: 0.25 }).unwrap();
    let (mid, rim) = (height(&s, 100, 60), height(&s, 52, 60));
    assert!(mid > rim && mid > 1.0, "dome: mid {mid}, rim {rim}");
    assert_eq!(height(&s, 10, 10), 0.0);
    s.execute(Command::Undo).unwrap();
    assert!(s.doc().layer(&id).unwrap().height.is_none());
    s.execute(Command::Redo).unwrap();
    s.execute(Command::ClearBevel { id: id.clone() }).unwrap();
    assert!(s.doc().layer(&id).unwrap().height.is_none());
    s.execute(Command::Undo).unwrap();
    assert!((height(&s, 100, 60) - mid).abs() < 1e-3);
}

#[test]
fn merge_down_adds_relief_and_files_keep_it() {
    let mut s = session();
    brush(&mut s, "thick-paint", 30.0);
    drag(&mut s, 30.0, 170.0, 60.0);
    let lower = height(&s, 100, 60);
    s.execute(Command::AddLayer).unwrap();
    drag(&mut s, 30.0, 170.0, 60.0);
    let upper = height(&s, 100, 60);
    let top = s.active().to_string();
    s.execute(Command::MergeDown { id: top }).unwrap();
    assert!((height(&s, 100, 60) - (lower + upper)).abs() < 0.02, "relief adds");

    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(10, 10, None);
    t.open_happypaint(&bytes).unwrap();
    let got = t.doc().root.iter().filter_map(|n| n.as_layer()).find_map(|l| l.height.as_ref().map(|h| h.get(100, 60)[0])).unwrap();
    assert!((got - (lower + upper)).abs() < 0.02, "saved and reopened: {got}");
}
