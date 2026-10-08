#![allow(clippy::unwrap_used)]
//! Comic panels in the session (spec 30): layout, split, reshape, style,
//! borders, clipping, undo and the `.happypaint` round trip.

use onepaint_color::Rgba;
use onepaint_doc::Node;
use onepaint_engine::*;
use onepaint_panel::{BORDER_LAYER, panel_of};

const W: u32 = 600;
const H: u32 = 400;

fn grid(s: &mut Session, rows: u32, cols: u32, gutter: f64, border: f64) {
    s.execute(Command::CreatePanelGrid { rows, cols, gutter, margin: 40.0, border }).unwrap();
}

fn px(s: &Session, x: i32, y: i32) -> [f32; 4] {
    let f = s.flatten();
    let i = ((y as u32 * W + x as u32) * 4) as usize;
    [f[i], f[i + 1], f[i + 2], f[i + 3]]
}

fn border_layer(s: &Session) -> Option<&onepaint_doc::Layer> {
    s.doc().root.iter().filter_map(Node::as_layer).find(|l| l.name == BORDER_LAYER)
}

/// Paint a horizontal red bar across the whole page on the active layer.
fn paint_bar(s: &mut Session, y: f64) {
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SetBrushSize { size: 30.0 }).unwrap();
    s.stroke_begin(onepaint_stroke::StrokePoint::at(0.0, y, 1.0, 0.0));
    let pts: Vec<_> = (1..=60).map(|i| onepaint_stroke::StrokePoint::at(i as f64 * 10.0, y, 1.0, i as f64 * 8.0)).collect();
    s.stroke_extend(&pts);
    s.stroke_end();
}

#[test]
fn a_grid_makes_panel_groups_with_art_and_one_border_layer_on_top() {
    let mut s = Session::new(W, H, None);
    grid(&mut s, 2, 2, 24.0, 4.0);
    let frames = s.panel_frames();
    assert_eq!(frames.len(), 4);
    let names: Vec<_> = s.doc().root.iter().filter(|n| n.is_group()).map(|n| n.name().to_string()).collect();
    assert_eq!(names, ["Panel 1", "Panel 2", "Panel 3", "Panel 4"]);
    // Each panel holds an empty Art layer, and the first one is active.
    let first = s.doc().root.iter().find_map(Node::as_group).unwrap();
    assert_eq!(first.children.len(), 1);
    assert_eq!(first.children[0].name(), "Art");
    assert_eq!(s.active(), first.children[0].id());
    // The border layer is NOT inside a panel group: it is last at top level.
    let top = s.doc().root.last().unwrap().as_layer().unwrap();
    assert_eq!(top.name, BORDER_LAYER);
    let paths = s.paths(&top.id);
    assert_eq!(paths.len(), 1, "one path object for a uniform page");
    assert_eq!(paths[0].subpaths.len(), 4);
    assert_eq!(paths[0].stroke.as_ref().unwrap().brush_id, "manga-millipen");
    // One undo removes the whole layout.
    s.execute(Command::Undo).unwrap();
    assert!(s.panel_frames().is_empty());
    assert!(border_layer(&s).is_none());
}

#[test]
fn art_is_clipped_to_its_panel_and_the_border_sits_on_the_inset_ring() {
    let mut s = Session::new(W, H, None);
    // 1×2 on 600×400 with a 40 margin: cells 40..300 and 300..560, inset by 12.
    grid(&mut s, 1, 2, 24.0, 4.0);
    paint_bar(&mut s, 200.0);
    let inside = px(&s, 150, 200);
    assert!(inside[0] > 0.99 && inside[3] > 0.99, "art inside the frame: {inside:?}");
    assert_eq!(px(&s, 20, 200)[3], 0.0, "outside the page margin");
    // The gutter between the panels is 288..312; the art only lives in panel 1,
    // whose clip ends at 288, and the border ink is 4 px around it.
    assert_eq!(px(&s, 300, 200)[3], 0.0, "middle of the gutter");
    assert_eq!(px(&s, 400, 200)[3], 0.0, "panel 2 has no art");
    let ink = px(&s, 288, 100);
    assert!(ink[3] > 0.9 && ink[0] < 0.1, "black border on the clip edge: {ink:?}");
}

#[test]
fn splitting_keeps_the_art_and_adds_an_empty_panel_above() {
    let mut s = Session::new(W, H, None);
    grid(&mut s, 1, 1, 20.0, 4.0);
    let id = s.panel_frames()[0].0.clone();
    paint_bar(&mut s, 200.0);
    s.execute(Command::SplitPanel { id: id.clone(), a: [300.0, -10.0], b: [300.0, 500.0] }).unwrap();
    let frames = s.panel_frames();
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].0, id, "the original keeps its slot");
    let xs = |o: &[onepaint_panel::geometry::Pt]| o.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    assert!((xs(&frames[0].1) - 300.0).abs() < 1e-9);
    let other = s.doc().find(&frames[1].0).unwrap().as_group().unwrap();
    assert_eq!(other.name, "Panel 2");
    assert_eq!(other.children.len(), 1);
    // The art still spans both halves, but now stops at the gutter.
    assert!(px(&s, 200, 200)[0] > 0.99);
    assert_eq!(px(&s, 300, 200)[3], 0.0);
    assert_eq!(px(&s, 400, 200)[0], 0.0, "panel 2 shows no art");
    assert_eq!(s.paths(border_layer(&s).unwrap().id.as_str())[0].subpaths.len(), 2, "borders follow the split");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.panel_frames().len(), 1);
    assert_eq!(s.paths(border_layer(&s).unwrap().id.as_str())[0].subpaths.len(), 1);
}

#[test]
fn a_cut_that_misses_or_a_tap_is_rejected() {
    let mut s = Session::new(W, H, None);
    grid(&mut s, 1, 1, 20.0, 4.0);
    let id = s.panel_frames()[0].0.clone();
    let n = s.history_len();
    assert!(s.execute(Command::SplitPanel { id, a: [590.0, 0.0], b: [590.0, 10.0] }).is_err());
    assert!(s.execute(Command::CreatePanel { a: [10.0, 10.0], b: [11.0, 11.0], gutter: 24.0, border: 4.0 }).is_err());
    assert_eq!(s.history_len(), n);
}

#[test]
fn creating_a_panel_from_a_drag() {
    let mut s = Session::new(W, H, None);
    s.execute(Command::CreatePanel { a: [300.0, 300.0], b: [100.0, 50.0], gutter: 24.0, border: 4.0 }).unwrap();
    let (id, outline) = s.panel_frames().remove(0);
    assert_eq!(outline[0], onepaint_panel::geometry::Pt::new(100.0, 50.0));
    assert_eq!(s.panel_at([150.0, 100.0]), Some(id));
    assert_eq!(s.panel_at([500.0, 100.0]), None);
    let g = s.doc().root.iter().find_map(Node::as_group).unwrap();
    assert_eq!(s.active(), g.children[0].id(), "ready to draw in");
}

#[test]
fn outline_drags_coalesce_into_one_undo_step() {
    let mut s = Session::new(W, H, None);
    grid(&mut s, 1, 1, 0.0, 4.0);
    let (id, before) = s.panel_frames().remove(0);
    let n = s.history_len();
    for dx in [10.0, 20.0, 30.0] {
        let moved: Vec<[f64; 2]> = before.iter().map(|p| [p.x + dx, p.y]).collect();
        s.execute(Command::SetPanelOutline { id: id.clone(), outline: moved }).unwrap();
    }
    s.execute(Command::SealHistory).unwrap();
    assert_eq!(s.history_len(), n + 1);
    assert_eq!(s.panel_frames()[0].1[0].x, before[0].x + 30.0);
    // Degenerate outlines are ignored rather than collapsing the frame.
    s.execute(Command::SetPanelOutline { id: id.clone(), outline: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]] }).unwrap();
    assert_eq!(s.panel_frames()[0].1[0].x, before[0].x + 30.0);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.panel_frames()[0].1, before);
}

#[test]
fn style_edits_apply_to_every_panel_and_a_zero_border_draws_nothing() {
    let mut s = Session::new(W, H, None);
    grid(&mut s, 2, 2, 24.0, 4.0);
    s.execute(Command::SetPanelStyle { id: None, gutter: Some(40.0), border: None }).unwrap();
    s.execute(Command::SetPanelStyle { id: None, gutter: Some(48.0), border: Some(6.0) }).unwrap();
    s.execute(Command::SealHistory).unwrap();
    for (id, _) in s.panel_frames() {
        let p = s.panel(&id).unwrap();
        assert_eq!((p.gutter, p.border), (Some(48.0), Some(6.0)));
    }
    assert_eq!(s.paths(border_layer(&s).unwrap().id.as_str())[0].stroke.as_ref().unwrap().size, 6.0);
    s.execute(Command::SetPanelStyle { id: None, gutter: None, border: Some(0.0) }).unwrap();
    assert!(border_layer(&s).is_none(), "no bordered panel, no border layer");
    s.execute(Command::Undo).unwrap();
    assert!(border_layer(&s).is_some());
    s.execute(Command::Undo).unwrap();
    let id = s.panel_frames()[0].0.clone();
    assert_eq!(s.panel(&id).unwrap().gutter, Some(24.0), "the coalesced drag undoes in one step");
}

#[test]
fn different_border_widths_get_separate_path_objects() {
    let mut s = Session::new(W, H, None);
    grid(&mut s, 1, 2, 24.0, 4.0);
    let id = s.panel_frames()[1].0.clone();
    s.execute(Command::SetPanelStyle { id: Some(id), gutter: None, border: Some(9.0) }).unwrap();
    let paths = s.paths(border_layer(&s).unwrap().id.as_str());
    let sizes: Vec<f64> = paths.iter().map(|p| p.stroke.as_ref().unwrap().size).collect();
    assert_eq!(sizes, [4.0, 9.0]);
}

#[test]
fn panels_round_trip_through_happypaint() {
    let mut s = Session::new(W, H, Some(Rgba::WHITE));
    grid(&mut s, 3, 2, 24.0, 4.0);
    let id = s.panel_frames()[2].0.clone();
    s.execute(Command::SplitPanel { id, a: [0.0, 0.0], b: [600.0, 400.0] }).unwrap();
    let before: Vec<_> = s.panel_frames().into_iter().map(|(id, _)| s.panel(&id).unwrap()).collect();
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(10, 10, None);
    t.open_happypaint(&bytes).unwrap();
    let after: Vec<_> = t.panel_frames().into_iter().map(|(id, _)| t.panel(&id).unwrap()).collect();
    assert_eq!(before, after);
    assert!(border_layer(&t).is_some());
}

#[test]
fn a_panel_that_is_also_an_animation_track_keeps_both() {
    let mut doc = onepaint_doc::Document::new(W, H, None);
    let mut g = onepaint_doc::Group::new("Panel 1");
    g.extra.insert("animTrack".into(), serde_json::json!(true));
    onepaint_panel::set_panel(
        &mut g,
        Some(onepaint_panel::PanelSpec::new(onepaint_panel::geometry::rect_outline([10.0, 10.0].into(), [200.0, 100.0].into()), 24.0, 4.0)),
    );
    g.children.push(Node::Layer(onepaint_doc::Layer::new("Art")));
    doc.root.push(Node::Group(g));
    let mut s = Session::from_document(doc);
    let bytes = s.save_happypaint().unwrap();
    let back = onepaint_formats::read_happypaint(&bytes).unwrap();
    let g = back.root.iter().find_map(Node::as_group).unwrap();
    assert!(panel_of(g).is_some());
    assert_eq!(g.extra.get("animTrack"), Some(&serde_json::json!(true)));
}

#[test]
fn a_document_with_no_panels_writes_no_panel_key() {
    let mut s = Session::new(64, 64, None);
    s.execute(Command::AddGroup).unwrap();
    let bytes = s.save_happypaint().unwrap();
    let len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let manifest = onepaint_formats::inflate_raw(&bytes[12..12 + len]).unwrap();
    let text = String::from_utf8(manifest).unwrap();
    assert!(text.contains("\"nodes\""), "the scan is not vacuous");
    assert!(!text.contains("\"panel\""));
}
