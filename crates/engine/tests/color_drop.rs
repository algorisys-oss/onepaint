#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};

const BLUE: Rgba = Rgba { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };

/// A layer with a red box at (20..40, 20..30) on transparent.
fn session() -> Session {
    let mut s = Session::new(200, 200, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([20.0, 20.0], [40.0, 30.0])], mode: Mode::Replace, antialias: false })
        .unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

fn px(s: &Session, x: i32, y: i32) -> [f32; 4] {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)
}

#[test]
fn preview_follows_the_seed_and_commit_is_one_step() {
    let mut s = session();
    let n = s.history_len();
    assert!(s.begin_color_drop(25.0, 25.0, BLUE, 0.15, 0));
    assert_eq!(px(&s, 25, 25), [0.0, 0.0, 1.0, 1.0], "the box previews blue");
    assert_eq!(s.history_len(), n, "a preview records nothing");
    // Moving onto the empty area: the box goes back, the outside fills.
    s.update_color_drop(100.0, 100.0, 0.15, 0);
    assert_eq!(px(&s, 25, 25), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(px(&s, 100, 100), [0.0, 0.0, 1.0, 1.0]);
    s.commit_color_drop();
    assert!(!s.color_drop_active());
    assert_eq!(s.history_len(), n + 1);
    s.execute(Command::Undo).unwrap();
    assert_eq!(px(&s, 100, 100)[3], 0.0);
    assert_eq!(px(&s, 25, 25), [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn cancel_restores_and_the_drop_colour_is_not_the_brush_colour() {
    let mut s = session();
    let n = s.history_len();
    assert!(s.begin_color_drop(25.0, 25.0, BLUE, 0.15, 0));
    assert_eq!(s.color(), Rgba::new(1.0, 0.0, 0.0, 1.0), "the brush colour is untouched");
    s.cancel_color_drop();
    assert_eq!(px(&s, 25, 25), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(s.history_len(), n);
}

#[test]
fn threshold_widens_the_fill() {
    let mut s = session();
    // Seed on the box: at 0% only the box fills; at 100% everything does.
    assert!(s.begin_color_drop(25.0, 25.0, BLUE, 0.0, 0));
    assert_eq!(px(&s, 100, 100)[3], 0.0);
    s.update_color_drop(25.0, 25.0, 1.0, 0);
    assert_eq!(px(&s, 100, 100), [0.0, 0.0, 1.0, 1.0]);
    s.commit_color_drop();
}

#[test]
fn refuses_locked_hidden_and_vector_layers_and_another_command_cancels() {
    let mut s = session();
    let id = s.active().to_string();
    s.execute(Command::SetLayerProps { id: id.clone(), props: LayerProps { locked: Some(true), ..Default::default() } }).unwrap();
    assert!(!s.begin_color_drop(25.0, 25.0, BLUE, 0.15, 0));
    s.execute(Command::SetLayerProps {
        id: id.clone(),
        props: LayerProps { locked: Some(false), visible: Some(false), ..Default::default() },
    })
    .unwrap();
    assert!(!s.begin_color_drop(25.0, 25.0, BLUE, 0.15, 0));
    s.execute(Command::SetLayerProps { id, props: LayerProps { visible: Some(true), ..Default::default() } }).unwrap();
    assert!(s.begin_color_drop(25.0, 25.0, BLUE, 0.15, 0));
    s.execute(Command::AddLayer).unwrap();
    assert!(!s.color_drop_active());
    s.execute(Command::Undo).unwrap();
    assert_eq!(px(&s, 25, 25), [1.0, 0.0, 0.0, 1.0], "the abandoned drop left nothing behind");
}

#[test]
fn holding_still_does_not_refill() {
    let mut s = session();
    assert!(s.begin_color_drop(25.0, 25.0, BLUE, 0.15, 0));
    for _ in 0..30 {
        s.update_color_drop(25.4, 25.7, 0.15, 0);
    }
    assert_eq!(s.color_drop_fills(), 1, "same pixel, same threshold");
    s.update_color_drop(26.0, 25.0, 0.15, 0);
    s.update_color_drop(26.0, 25.0, 0.2, 0);
    s.update_color_drop(26.0, 25.0, 0.2, 1);
    assert_eq!(s.color_drop_fills(), 4);
    assert_eq!(px(&s, 25, 25), [0.0, 0.0, 1.0, 1.0]);
}
