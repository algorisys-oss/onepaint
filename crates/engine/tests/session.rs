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
        s.stroke_extend(&[StrokePoint::at(
            20.0 + i as f64 * 10.0,
            y,
            1.0,
            i as f64 * 8.0,
        )]);
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
    s.execute(Command::SetColor {
        rgba: Rgba::new(1.0, 0.0, 0.0, 1.0),
    })
    .unwrap();
    let before = s.doc().layer(s.active()).unwrap().pixels.clone();
    assert_eq!(before.tile_count(), 0);
    draw_line(&mut s, 100.0);
    let after_px = s.doc().layer(s.active()).unwrap().pixels.get(100, 100);
    assert!(after_px[3] > 0.9 && after_px[0] > 0.9, "{after_px:?}");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().layer(s.active()).unwrap().pixels.tile_count(), 0);
    s.execute(Command::Redo).unwrap();
    assert_eq!(
        s.doc().layer(s.active()).unwrap().pixels.get(100, 100),
        after_px
    );
    let ev = s.drain_events();
    assert!(
        ev.iter()
            .any(|e| matches!(e, Event::HistoryChanged { can_undo: true, .. }))
    );
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
    s.execute(Command::SetLayerProps {
        id,
        props: LayerProps {
            alpha_lock: Some(true),
            ..Default::default()
        },
    })
    .unwrap();
    let n = s.history_len();
    draw_line(&mut s, 50.0);
    assert!(s.doc().layer(s.active()).unwrap().pixels.get(100, 50)[3] > 0.9);
    assert_eq!(s.history_len(), n, "alpha-locked erase adds no history");
}

#[test]
fn composite_matches_full_flatten_after_stroke() {
    let mut s = new_session();
    s.execute(Command::SetColor {
        rgba: Rgba::new(0.2, 0.4, 0.9, 1.0),
    })
    .unwrap();
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
    s.execute(Command::MoveLayer {
        id: new_id.clone(),
        parent: None,
        index: 0,
    })
    .unwrap();
    assert_eq!(s.doc().root[0].id(), new_id);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().root[2].id(), new_id);
    s.execute(Command::DeleteLayer { id: new_id.clone() })
        .unwrap();
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
        s.execute(Command::SetLayerProps {
            id: id.clone(),
            props: LayerProps {
                opacity: Some(o),
                ..Default::default()
            },
        })
        .unwrap();
    }
    assert_eq!(s.history_len(), n + 1);
    s.execute(Command::SetLayerProps {
        id: id.clone(),
        props: LayerProps {
            blend_mode: Some(BlendMode::Multiply),
            ..Default::default()
        },
    })
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
    let c = Command::SetLayerProps {
        id: "x".into(),
        props: LayerProps {
            opacity: Some(0.5),
            ..Default::default()
        },
    };
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
