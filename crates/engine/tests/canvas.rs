#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};

fn session() -> Session {
    let mut s = Session::new(100, 80, Some(Rgba::WHITE));
    s.execute(Command::SelectShape { contours: vec![rect_polygon([10.0, 10.0], [20.0, 20.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

#[test]
fn resize_canvas_extends_with_offset_and_undoes() {
    let mut s = session();
    s.execute(Command::ResizeCanvas { width: 140, height: 100, dx: 20, dy: 10 }).unwrap();
    assert_eq!((s.doc().width, s.doc().height), (140, 100));
    let c = s.pick_color(35, 25);
    assert!(c.r > 0.99 && c.g < 0.01, "content moved by the offset");
    // The background layer is not extended: the new strip is transparent.
    assert_eq!(s.doc().root[0].as_layer().unwrap().pixels.get(5, 5)[3], 0.0);
    assert!(s.drain_events().contains(&Event::DocumentReplaced));
    s.execute(Command::Undo).unwrap();
    assert_eq!((s.doc().width, s.doc().height), (100, 80));
    assert!(s.pick_color(15, 15).g < 0.01);
}

#[test]
fn crop_keeps_offcanvas_pixels_until_saved() {
    let mut s = session();
    s.execute(Command::ResizeCanvas { width: 50, height: 50, dx: -10, dy: -10 }).unwrap();
    assert!(s.pick_color(5, 5).g < 0.01, "red block now at 0..10");
    s.execute(Command::ResizeCanvas { width: 100, height: 80, dx: 10, dy: 10 }).unwrap();
    assert!(s.pick_color(15, 15).g < 0.01, "uncrop restores it");
}

#[test]
fn image_size_resamples_every_layer() {
    let mut s = session();
    s.execute(Command::ScaleImage { width: 200, height: 160 }).unwrap();
    assert_eq!((s.doc().width, s.doc().height), (200, 160));
    assert!(s.pick_color(30, 30).g < 0.01, "red block scaled to 20..40");
    assert!(s.pick_color(50, 50).g > 0.99);
    assert!(s.pick_color(199, 159).g > 0.99, "background still covers the canvas");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().width, 100);
}
