#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_filters::gradient::{Shape as GShape, fg_transparent};
use onepaint_selection::{Mode, rect_polygon};
use onepaint_stroke::shape::Shape;

fn px(s: &Session, x: i32, y: i32) -> [f32; 4] {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)
}

#[test]
fn linear_gradient_fg_to_transparent_with_preview_and_undo() {
    let mut s = Session::new(200, 50, Some(Rgba::WHITE));
    s.gradient_begin();
    s.gradient_update([0.0, 0.0], [200.0, 0.0], GShape::Linear, &fg_transparent([0.0, 0.0, 1.0]));
    assert_eq!(px(&s, 10, 10)[3], 0.0, "nothing committed during preview");
    let live = s.composite(&[(0, 0)]);
    assert!(live[0].1.get(1, 10)[0] < 0.05, "preview shows blue on the left");
    s.gradient_end();
    let (l, r) = (px(&s, 1, 10), px(&s, 198, 10));
    assert!(l[3] > 0.98 && l[2] > 0.98, "{l:?}");
    assert!(r[3] < 0.02);
    let mid = px(&s, 100, 10)[3];
    assert!((mid - 0.5).abs() < 0.02, "{mid}");
    s.execute(Command::Undo).unwrap();
    assert_eq!(px(&s, 1, 10)[3], 0.0);
}

#[test]
fn gradient_respects_selection() {
    let mut s = Session::new(100, 50, None);
    s.execute(Command::SelectShape { contours: vec![rect_polygon([0.0, 0.0], [50.0, 50.0])], mode: Mode::Replace }).unwrap();
    s.gradient_begin();
    s.gradient_update([0.0, 0.0], [100.0, 0.0], GShape::Linear, &onepaint_filters::gradient::fg_bg([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    s.gradient_end();
    assert!(px(&s, 10, 10)[3] > 0.99);
    assert_eq!(px(&s, 80, 10)[3], 0.0);
}

#[test]
fn stroke_shape_fills_under_the_outline_in_one_step() {
    let mut s = Session::new(120, 120, None);
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    let n = s.history_len();
    s.stroke_shape(&Shape::Rect { cx: 60.0, cy: 60.0, hw: 40.0, hh: 30.0 }, true);
    assert_eq!(s.history_len(), n + 1);
    assert!(px(&s, 60, 60)[3] > 0.99, "filled");
    assert!(px(&s, 20, 60)[3] > 0.5, "outline on the left edge");
    assert_eq!(px(&s, 5, 5)[3], 0.0);
    s.execute(Command::Undo).unwrap();
    assert_eq!(px(&s, 60, 60)[3], 0.0);
    s.stroke_shape(&Shape::Ellipse { cx: 60.0, cy: 60.0, rx: 40.0, ry: 40.0 }, false);
    assert_eq!(px(&s, 60, 60)[3], 0.0, "outline only");
    assert!(px(&s, 100, 60)[3] > 0.5);
}
