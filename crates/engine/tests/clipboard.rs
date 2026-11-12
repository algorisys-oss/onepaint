#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_formats::DecodedImage;
use onepaint_selection::{Mode, rect_polygon};

fn session() -> Session {
    let mut s = Session::new(200, 100, Some(Rgba::WHITE));
    s.execute(Command::SelectShape { contours: vec![rect_polygon([10.0, 10.0], [30.0, 20.0])], mode: Mode::Replace, antialias: false })
        .unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 1.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s
}

fn alpha_on(s: &Session, layer: &str, x: i32, y: i32) -> f32 {
    s.doc().layer(layer).unwrap().pixels.get(x, y)[3]
}

#[test]
fn copy_respects_selection_and_pastes_in_place() {
    let mut s = session();
    let r = s.copy_region().unwrap();
    assert_eq!((r.rect.x, r.rect.y, r.rect.w, r.rect.h), (10, 10, 20, 10));
    s.execute(Command::Deselect).unwrap();
    let src = s.active().to_string();
    s.paste_region(&r);
    let pasted = s.active().to_string();
    assert_ne!(pasted, src);
    assert_eq!(s.doc().find(&pasted).unwrap().name(), "Pasted");
    assert!(alpha_on(&s, &pasted, 15, 15) > 0.99);
    assert_eq!(alpha_on(&s, &pasted, 50, 50), 0.0);
    s.execute(Command::Undo).unwrap();
    assert!(s.doc().find(&pasted).is_none());
}

#[test]
fn paste_is_fitted_into_a_smaller_document() {
    let s = session();
    let r = s.copy_region().unwrap();
    let mut t = Session::new(16, 16, None);
    t.paste_region(&r);
    let l = t.active().to_string();
    // 20x10 at (10,10) → w = 16 at x = 0, h = 10 at y = clamp(10, 0, 6) = 6.
    assert!(alpha_on(&t, &l, 0, 6) > 0.99, "x/y clamped into the canvas");
    assert!(alpha_on(&t, &l, 15, 15) > 0.99);
    assert_eq!(alpha_on(&t, &l, 0, 5), 0.0);
}

#[test]
fn transparent_copy_is_none() {
    let s = Session::new(50, 50, None);
    assert!(s.copy_region().is_none());
}

#[test]
fn layer_via_copy_and_cut() {
    let mut s = session();
    let src = s.active().to_string();
    let n = s.history_len();
    s.execute(Command::LayerViaCopy { cut: true }).unwrap();
    let new = s.active().to_string();
    assert_eq!(s.doc().find(&new).unwrap().name(), "Layer via Cut");
    assert!(alpha_on(&s, &new, 15, 15) > 0.99);
    assert_eq!(alpha_on(&s, &src, 15, 15), 0.0);
    assert_eq!(s.history_len(), n + 1, "one undo step");
    s.execute(Command::Undo).unwrap();
    assert!(alpha_on(&s, &src, 15, 15) > 0.99);
    assert!(s.doc().find(&new).is_none());
    // No selection: Ctrl+J duplicates.
    s.execute(Command::Deselect).unwrap();
    let before = s.doc().layer_ids().len();
    s.execute(Command::LayerViaCopy { cut: false }).unwrap();
    assert_eq!(s.doc().layer_ids().len(), before + 1);
}

#[test]
fn place_image_scales_to_fit_and_centres() {
    let mut s = Session::new(100, 100, None);
    let img = DecodedImage { width: 400, height: 200, rgba: [255u8, 0, 0, 255].repeat(400 * 200) };
    s.place_image(&img);
    let l = s.active().to_string();
    assert_eq!(s.doc().find(&l).unwrap().name(), "Image");
    // 400x200 fits as 100x50 centred: rows 25..75.
    assert!(alpha_on(&s, &l, 50, 50) > 0.99);
    assert!(alpha_on(&s, &l, 2, 30) > 0.99);
    assert_eq!(alpha_on(&s, &l, 50, 10), 0.0);
    assert_eq!(alpha_on(&s, &l, 50, 90), 0.0);
    // Small images keep their size.
    let small = DecodedImage { width: 10, height: 10, rgba: [0u8, 255, 0, 255].repeat(100) };
    s.place_image(&small);
    let l2 = s.active().to_string();
    assert!(alpha_on(&s, &l2, 50, 50) > 0.99);
    assert_eq!(alpha_on(&s, &l2, 44, 50), 0.0);
}
