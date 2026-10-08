#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_doc::BlendMode;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn new_session() -> Session {
    Session::new(512, 384, Some(Rgba::WHITE))
}

fn draw_line(s: &mut Session, y: f64) {
    s.stroke_begin(StrokePoint::at(20.0, y, 1.0, 0.0));
    for i in 1..20 {
        s.stroke_extend(&[StrokePoint::at(20.0 + i as f64 * 10.0, y, 1.0, i as f64 * 8.0)]);
    }
    s.stroke_end();
}

#[test]
fn new_document_layers() {
    let s = new_session();
    assert_eq!(s.doc().root.len(), 2);
    assert_eq!(s.doc().find(s.active()).unwrap().name(), "Layer 1");
}

#[test]
fn stroke_then_undo_redo_is_exact() {
    let mut s = new_session();
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    let before = s.doc().layer(s.active()).unwrap().pixels.clone();
    assert_eq!(before.tile_count(), 0);
    draw_line(&mut s, 100.0);
    let after_px = s.doc().layer(s.active()).unwrap().pixels.get(100, 100);
    assert!(after_px[3] > 0.9 && after_px[0] > 0.9, "{after_px:?}");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().layer(s.active()).unwrap().pixels.tile_count(), 0);
    s.execute(Command::Redo).unwrap();
    assert_eq!(s.doc().layer(s.active()).unwrap().pixels.get(100, 100), after_px);
    let ev = s.drain_events();
    assert!(ev.iter().any(|e| matches!(e, Event::HistoryChanged { can_undo: true, .. })));
}

#[test]
fn eraser_removes_paint_and_alpha_lock_blocks_erase() {
    let mut s = new_session();
    draw_line(&mut s, 50.0);
    s.execute(Command::SetTool { tool: Tool::Eraser }).unwrap();
    draw_line(&mut s, 50.0);
    assert!(s.doc().layer(s.active()).unwrap().pixels.get(100, 50)[3] < 0.01);
    s.execute(Command::Undo).unwrap();
    let id = s.active().to_string();
    s.execute(Command::SetLayerProps { id, props: LayerProps { alpha_lock: Some(true), ..Default::default() } }).unwrap();
    let n = s.history_len();
    draw_line(&mut s, 50.0);
    assert!(s.doc().layer(s.active()).unwrap().pixels.get(100, 50)[3] > 0.9);
    assert_eq!(s.history_len(), n, "alpha-locked erase adds no history");
}

#[test]
fn composite_matches_full_flatten_after_stroke() {
    let mut s = new_session();
    s.execute(Command::SetColor { rgba: Rgba::new(0.2, 0.4, 0.9, 1.0) }).unwrap();
    draw_line(&mut s, 200.0);
    let keys = s.take_dirty_tiles();
    assert!(!keys.is_empty());
    let full = onepaint_compose::flatten(s.doc(), None);
    for ((tx, ty), t) in s.composite(&keys) {
        let (x, y) = (tx * 256 + 100, ty * 256 + (200 - ty * 256).clamp(0, 255));
        if x >= 512 || y >= 384 {
            continue;
        }
        let p = t.get((x - tx * 256) as usize, (y - ty * 256) as usize);
        let i = ((y as usize * 512) + x as usize) * 4;
        for c in 0..4 {
            assert!((p[c] - full[i + c]).abs() < 2e-3);
        }
    }
}

#[test]
fn live_stroke_preview_equals_flattened_result() {
    let mut s = new_session();
    s.stroke_begin(StrokePoint::at(30.0, 30.0, 1.0, 0.0));
    s.stroke_extend(&[StrokePoint::at(120.0, 30.0, 1.0, 50.0)]);
    let live = s.composite(&[(0, 0)])[0].1.get(80, 30);
    s.stroke_end();
    let done = s.composite(&[(0, 0)])[0].1.get(80, 30);
    for c in 0..4 {
        assert!((live[c] - done[c]).abs() < 2e-3, "{live:?} vs {done:?}");
    }
}

#[test]
fn layer_tree_ops_are_undoable() {
    let mut s = new_session();
    s.execute(Command::AddLayer).unwrap();
    assert_eq!(s.doc().root.len(), 3);
    let new_id = s.active().to_string();
    assert_eq!(s.doc().root[2].id(), new_id);
    s.execute(Command::MoveLayer { id: new_id.clone(), parent: None, index: 0 }).unwrap();
    assert_eq!(s.doc().root[0].id(), new_id);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().root[2].id(), new_id);
    s.execute(Command::DeleteLayer { id: new_id.clone() }).unwrap();
    assert_eq!(s.doc().root.len(), 2);
    assert_ne!(s.active(), new_id);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().root.len(), 3);
    s.execute(Command::AddGroup).unwrap();
    assert!(s.doc().find(s.active()).unwrap().is_group());
}

#[test]
fn cannot_delete_last_layer() {
    let mut s = Session::new(64, 64, None);
    let id = s.active().to_string();
    assert!(s.execute(Command::DeleteLayer { id }).is_err());
}

#[test]
fn props_edits_coalesce_per_field() {
    let mut s = new_session();
    let id = s.active().to_string();
    let n = s.history_len();
    for o in [0.9, 0.8, 0.7] {
        s.execute(Command::SetLayerProps { id: id.clone(), props: LayerProps { opacity: Some(o), ..Default::default() } }).unwrap();
    }
    assert_eq!(s.history_len(), n + 1);
    s.execute(Command::SetLayerProps { id: id.clone(), props: LayerProps { blend_mode: Some(BlendMode::Multiply), ..Default::default() } })
        .unwrap();
    assert_eq!(s.history_len(), n + 2);
    s.execute(Command::Undo).unwrap();
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().find(&id).unwrap().opacity(), 1.0);
}

#[test]
fn merge_down_bakes_into_lower_layer() {
    let mut s = new_session();
    draw_line(&mut s, 60.0);
    let top = s.active().to_string();
    let bottom = s.doc().root[0].id().to_string();
    s.execute(Command::MergeDown { id: top }).unwrap();
    assert_eq!(s.doc().root.len(), 1);
    assert_eq!(s.active(), bottom);
    let p = s.doc().layer(&bottom).unwrap().pixels.get(100, 60);
    assert!(p[0] < 0.1 && p[3] > 0.99, "{p:?}");
}

#[test]
fn command_serde_round_trip() {
    let c = Command::SetLayerProps { id: "x".into(), props: LayerProps { opacity: Some(0.5), ..Default::default() } };
    let j = serde_json::to_string(&c).unwrap();
    let back: Command = serde_json::from_str(&j).unwrap();
    assert_eq!(back, c);
}

#[test]
fn eyedropper_reads_composite() {
    let s = new_session();
    let c = s.pick_color(10, 10);
    assert!((c.r - 1.0).abs() < 1e-3 && (c.a - 1.0).abs() < 1e-3);
}

#[test]
fn save_open_round_trip_and_png_export() {
    let mut s = new_session();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.5, 1.0, 1.0) }).unwrap();
    draw_line(&mut s, 120.0);
    let bytes = s.save_happypaint().unwrap();
    assert!(onepaint_formats::read_thumbnail(&bytes).unwrap().is_some());
    let mut t = Session::new(10, 10, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!((t.doc().width, t.doc().height), (512, 384));
    assert!(t.drain_events().contains(&Event::DocumentReplaced));
    let a = s.pick_color(100, 120);
    let b = t.pick_color(100, 120);
    assert!((a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01, "{a:?} {b:?}");
    let png = s.export_png().unwrap();
    let (w, h, px) = onepaint_formats::decode_png_rgba8(&png).unwrap();
    assert_eq!((w, h), (512, 384));
    assert_eq!(&px[..4], &[255, 255, 255, 255]);
    assert!(t.open_happypaint(b"garbage").is_err());
}

#[test]
fn new_layers_are_numbered_sequentially() {
    let mut s = new_session();
    s.execute(Command::AddLayer).unwrap();
    assert_eq!(s.doc().find(s.active()).unwrap().name(), "Layer 2");
    s.execute(Command::AddLayer).unwrap();
    assert_eq!(s.doc().find(s.active()).unwrap().name(), "Layer 3");
}

#[test]
fn open_image_as_document() {
    let px: Vec<u8> = (0..30 * 20).flat_map(|i| [255, (i % 255) as u8, 0, 255]).collect();
    let png = onepaint_formats::encode_png_rgba8(30, 20, &px).unwrap();
    let mut s = Session::new(10, 10, None);
    s.open_image(&png, "photo").unwrap();
    assert_eq!((s.doc().width, s.doc().height, s.doc().name.as_str()), (30, 20, "photo"));
    assert_eq!(s.doc().root.len(), 1);
    let c = s.pick_color(0, 0);
    assert!((c.r - 1.0).abs() < 1e-3 && c.g < 1e-3);
    assert!(s.open_image(b"junk", "x").is_err());
}

#[test]
fn layer_drop_resolution() {
    // root: [Background, Layer 1, Group(child), Layer 3]
    let mut s = new_session();
    let bg = s.doc().root[0].id().to_string();
    let l1 = s.doc().root[1].id().to_string();
    s.execute(Command::AddGroup).unwrap();
    let g = s.active().to_string();
    s.execute(Command::AddLayer).unwrap();
    let l3 = s.active().to_string();
    s.execute(Command::MoveLayer { id: l3.clone(), parent: Some(g.clone()), index: 0 }).unwrap();
    s.execute(Command::SetActiveLayer { id: g.clone() }).unwrap();
    s.execute(Command::AddLayer).unwrap();
    let top = s.active().to_string();
    // Dropping bg on the upper half of `top` → above top (root index 4 after removal → 3).
    assert_eq!(s.resolve_drop(&bg, &top, 0.2), Some((None, 3)));
    // Lower half of l1 → just below l1: bg is already there, so it's a no-op position 0.
    assert_eq!(s.resolve_drop(&bg, &l1, 0.8), Some((None, 0)));
    // Upper third of a group → above it; below that → into it, at the top.
    assert_eq!(s.resolve_drop(&bg, &g, 0.2), Some((None, 2)));
    assert_eq!(s.resolve_drop(&bg, &g, 0.6), Some((Some(g.clone()), 1)));
    // A group can't go into itself or its children.
    assert_eq!(s.resolve_drop(&g, &l3, 0.2), None);
    assert_eq!(s.resolve_drop(&g, &g, 0.6), None);
    // Applying a resolved drop moves the layer.
    let (p, i) = s.resolve_drop(&bg, &g, 0.6).unwrap();
    s.execute(Command::MoveLayer { id: bg.clone(), parent: p, index: i }).unwrap();
    assert_eq!(s.doc().parent_of(&bg), Some(Some(g.clone())));
}

#[test]
fn colour_labels_set_clear_and_undo() {
    let mut s = new_session();
    let id = s.active().to_string();
    s.execute(Command::SetColorLabel { id: id.clone(), label: Some("blue".into()) }).unwrap();
    assert_eq!(s.color_label(&id).as_deref(), Some("blue"));
    s.execute(Command::SetColorLabel { id: id.clone(), label: None }).unwrap();
    assert_eq!(s.color_label(&id), None);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.color_label(&id).as_deref(), Some("blue"));
    assert!(s.execute(Command::SetColorLabel { id, label: Some("chartreuse".into()) }).is_err());
}

#[test]
fn strokes_use_the_brush_blend_mode() {
    use onepaint_color::Rgba;
    // A multiply preset darkens instead of covering.
    // (The library's multiply presets are all wet, and the wet path is source-over
    // as in happypaint; a dry copy keeps the mode.)
    let mut b = onepaint_brush::presets().into_iter().find(|p| p.blend_mode() == onepaint_doc::BlendMode::Multiply).unwrap();
    b.extra.remove("wet");
    let mut s = Session::new(64, 64, None);
    s.execute(Command::SelectAll).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 1.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s.execute(Command::SetBrushPreset { preset: Box::new(b) }).unwrap();
    s.execute(Command::SetBrushSize { size: 30.0 }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 1.0, 1.0, 1.0) }).unwrap();
    let pt = |x: f64| onepaint_stroke::StrokePoint { x, y: 32.0, pressure: 1.0, ..Default::default() };
    s.stroke_begin(pt(10.0));
    s.stroke_extend(&[pt(30.0), pt(54.0)]);
    s.stroke_end();
    let c = s.pick_color(32, 32);
    // Normal would mix in cyan's blue; multiply keeps blue at 0 and darkens red.
    assert!(c.b < 0.01 && c.r < 0.95 && c.g > 0.9, "yellow × cyan multiplies toward green: {c:?}");
}
