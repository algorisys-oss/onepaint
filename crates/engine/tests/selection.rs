#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};
use onepaint_stroke::StrokePoint;

fn session() -> Session {
    Session::new(256, 256, Some(Rgba::WHITE))
}

/// A horizontal line across the whole canvas (it splits it in two).
fn line(s: &mut Session, y: f64) {
    s.stroke_begin(StrokePoint::at(-30.0, y, 1.0, 0.0));
    for i in 1..=32 {
        s.stroke_extend(&[StrokePoint::at(-30.0 + i as f64 * 10.0, y, 1.0, i as f64 * 8.0)]);
    }
    s.stroke_end();
}

fn alpha(s: &Session, x: i32, y: i32) -> f32 {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)[3]
}

fn select_rect(s: &mut Session, x0: f64, y0: f64, x1: f64, y1: f64, mode: Mode) {
    s.execute(Command::SelectShape { contours: vec![rect_polygon([x0, y0], [x1, y1])], mode, antialias: false }).unwrap();
}

#[test]
fn painting_is_clipped_to_the_selection() {
    let mut s = session();
    select_rect(&mut s, 100.0, 0.0, 150.0, 256.0, Mode::Replace);
    assert!(s.selection().is_some());
    line(&mut s, 128.0);
    assert!(alpha(&s, 120, 128) > 0.9);
    assert_eq!(alpha(&s, 60, 128), 0.0);
    assert_eq!(alpha(&s, 200, 128), 0.0);
}

#[test]
fn selection_changes_are_undoable() {
    let mut s = session();
    select_rect(&mut s, 10.0, 10.0, 50.0, 50.0, Mode::Replace);
    select_rect(&mut s, 40.0, 40.0, 80.0, 80.0, Mode::Add);
    let m = s.selection().unwrap();
    assert_eq!((m.get(20, 20), m.get(70, 70), m.get(20, 70)), (255, 255, 0));
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.selection().unwrap().get(70, 70), 0);
    s.execute(Command::Deselect).unwrap();
    assert!(s.selection().is_none());
    s.execute(Command::Undo).unwrap();
    assert!(s.selection().is_some());
    s.execute(Command::Redo).unwrap();
    assert!(s.selection().is_none());
}

#[test]
fn select_all_invert_and_grow() {
    let mut s = session();
    s.execute(Command::InvertSelection).unwrap();
    assert!(s.selection().is_none(), "invert without a selection does nothing");
    s.execute(Command::SelectAll).unwrap();
    s.execute(Command::InvertSelection).unwrap();
    assert!(s.selection().is_none(), "inverting everything clears it");
    select_rect(&mut s, 100.0, 100.0, 110.0, 110.0, Mode::Replace);
    s.execute(Command::GrowSelection { delta: 5 }).unwrap();
    let m = s.selection().unwrap();
    assert_eq!((m.get(96, 96), m.get(94, 94)), (255, 0));
    s.execute(Command::GrowSelection { delta: -7 }).unwrap();
    assert_eq!(s.selection().unwrap().get(100, 100), 0);
    assert_eq!(s.selection().unwrap().get(104, 104), 255);
    s.execute(Command::InvertSelection).unwrap();
    assert_eq!(s.selection().unwrap().get(0, 0), 255);
}

#[test]
fn subtract_to_empty_clears() {
    let mut s = session();
    select_rect(&mut s, 10.0, 10.0, 20.0, 20.0, Mode::Replace);
    select_rect(&mut s, 0.0, 0.0, 30.0, 30.0, Mode::Subtract);
    assert!(s.selection().is_none());
}

#[test]
fn feather_softens_the_clip_mask_only() {
    let mut s = session();
    select_rect(&mut s, 100.0, 100.0, 150.0, 150.0, Mode::Replace);
    s.execute(Command::SetFeather { radius: 4.0 }).unwrap();
    let clip = s.clip_mask().unwrap();
    let edge = clip.get(100, 125);
    assert!(edge > 0 && edge < 255, "{edge}");
    assert_eq!(s.selection().unwrap().get(100, 125), 255, "base stays hard");
}

/// happypaint v0.209.0 (artist round 35): a bucket fill inside a feathered
/// selection fades out across the feather; it used to fill hard-edged.
#[test]
fn a_bucket_fill_follows_the_feather() {
    let mut s = session();
    select_rect(&mut s, 64.0, 64.0, 192.0, 192.0, Mode::Replace);
    s.execute(Command::SetFeather { radius: 8.0 }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 1.0, 1.0) }).unwrap();
    s.execute(Command::Fill { x: 128, y: 128, tolerance: 0.1, grow: 0 }).unwrap();
    assert!(alpha(&s, 128, 128) > 0.99, "the middle is filled");
    let edge = alpha(&s, 64, 128);
    assert!((0.3..0.7).contains(&edge), "about half on the edge: {edge}");
    let outside = alpha(&s, 56, 128);
    assert!(outside > 0.05 && outside < 0.3, "the feather reaches outside: {outside}");
    assert_eq!(alpha(&s, 20, 128), 0.0);
}

#[test]
fn feather_goes_up_to_1000_px() {
    let mut s = session();
    s.execute(Command::SetFeather { radius: 5000.0 }).unwrap();
    assert_eq!(s.feather(), FEATHER_MAX);
    s.execute(Command::SetFeather { radius: f32::NAN }).unwrap();
    assert_eq!(s.feather(), FEATHER_MAX);
    s.execute(Command::SetFeather { radius: 250.4 }).unwrap();
    assert_eq!(s.feather(), 250.0);
}

/// Anti-alias (happypaint v0.209.0): an ellipse's edge is partly selected;
/// without it the edge is a 0/255 staircase.
#[test]
fn an_anti_aliased_ellipse_has_a_soft_edge() {
    let ellipse = onepaint_selection::ellipse_polygon([20.0, 30.0], [220.0, 200.0]);
    let soft = |antialias| {
        let mut s = session();
        s.execute(Command::SelectShape { contours: vec![ellipse.clone()], mode: Mode::Replace, antialias }).unwrap();
        s.selection().unwrap().data().iter().filter(|&&v| v != 0 && v != 255).count()
    };
    assert_eq!(soft(false), 0);
    assert!(soft(true) > 100, "{}", soft(true));
}

#[test]
fn fill_floods_the_enclosed_region() {
    let mut s = session();
    // Draw a closed black ring on Layer 1 is fiddly; use the background instead.
    let bg = s.doc().root[0].id().to_string();
    s.execute(Command::SetActiveLayer { id: bg }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::BLACK }).unwrap();
    line(&mut s, 128.0);
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::Fill { x: 100, y: 40, tolerance: 0.15, grow: 1 }).unwrap();
    let px = |s: &Session, y| s.doc().layer(s.active()).unwrap().pixels.get(100, y);
    assert!(px(&s, 40)[0] > 0.99 && px(&s, 40)[1] < 0.01, "top region filled red");
    assert!(px(&s, 220)[1] > 0.99, "bottom region stays white");
    s.execute(Command::Undo).unwrap();
    assert!(px(&s, 40)[1] > 0.99);
}

#[test]
fn fill_is_confined_to_the_selection_and_respects_alpha_lock() {
    let mut s = session();
    select_rect(&mut s, 0.0, 0.0, 64.0, 64.0, Mode::Replace);
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 1.0, 1.0) }).unwrap();
    s.execute(Command::Fill { x: 10, y: 10, tolerance: 0.1, grow: 0 }).unwrap();
    assert!(alpha(&s, 10, 10) > 0.99);
    assert_eq!(alpha(&s, 100, 100), 0.0);
    // Alpha lock: filling a transparent layer changes nothing visible.
    let mut t = session();
    let id = t.active().to_string();
    t.execute(Command::SetLayerProps { id, props: LayerProps { alpha_lock: Some(true), ..Default::default() } }).unwrap();
    t.execute(Command::Fill { x: 10, y: 10, tolerance: 0.1, grow: 0 }).unwrap();
    assert_eq!(alpha(&t, 10, 10), 0.0);
}

#[test]
fn clear_and_fill_selection() {
    let mut s = session();
    line(&mut s, 128.0);
    select_rect(&mut s, 100.0, 0.0, 150.0, 256.0, Mode::Replace);
    s.execute(Command::ClearSelected).unwrap();
    assert_eq!(alpha(&s, 120, 128), 0.0);
    assert!(alpha(&s, 60, 128) > 0.9);
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 1.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    assert!(alpha(&s, 120, 10) > 0.99);
    assert_eq!(alpha(&s, 60, 10), 0.0);
    s.execute(Command::Deselect).unwrap();
    s.execute(Command::ClearSelected).unwrap();
    assert_eq!(s.doc().layer(s.active()).unwrap().pixels.tile_count(), 0);
}

#[test]
fn magic_wand_on_layer_and_composite() {
    let mut s = session();
    line(&mut s, 128.0);
    s.execute(Command::SelectByColor { x: 100, y: 128, tolerance: 0.1, mode: Mode::Replace, contiguous: true, sample_all: false }).unwrap();
    let m = s.selection().unwrap();
    assert_eq!((m.get(100, 128), m.get(100, 20)), (255, 0));
    // Composite sampling: the white background region is one connected area above the line.
    s.execute(Command::SelectByColor { x: 100, y: 20, tolerance: 0.1, mode: Mode::Replace, contiguous: true, sample_all: true }).unwrap();
    let m = s.selection().unwrap();
    assert_eq!((m.get(5, 5), m.get(100, 128), m.get(100, 240)), (255, 0, 0));
}
