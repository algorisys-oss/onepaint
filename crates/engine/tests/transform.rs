#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::transform::Affine;
use onepaint_engine::*;
use onepaint_selection::{Mode, rect_polygon};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

fn session_with_block() -> Session {
    let mut s = Session::new(200, 200, Some(Rgba::WHITE));
    s.execute(Command::SelectShape {
        contours: vec![rect_polygon([20.0, 20.0], [40.0, 30.0])],
        mode: Mode::Replace,
    })
    .unwrap();
    s.execute(Command::SetColor {
        rgba: Rgba::new(1.0, 0.0, 0.0, 1.0),
    })
    .unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

fn px(s: &Session, x: i32, y: i32) -> [f32; 4] {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)
}

#[test]
fn affine_basics() {
    let t = Affine::translate(5.0, -2.0);
    assert_eq!(t.apply([1.0, 1.0]), [6.0, -1.0]);
    let r = Affine::rotate_about(std::f64::consts::FRAC_PI_2, [10.0, 10.0]);
    let p = r.apply([20.0, 10.0]);
    assert!((p[0] - 10.0).abs() < 1e-9 && (p[1] - 20.0).abs() < 1e-9);
    let m = Affine::scale_about(2.0, 3.0, [1.0, 1.0]).then(&Affine::translate(1.0, 0.0));
    let q = m.inverse().unwrap().apply(m.apply([7.0, -4.0]));
    assert!((q[0] - 7.0).abs() < 1e-9 && (q[1] + 4.0).abs() < 1e-9);
}

#[test]
fn translate_moves_pixels_exactly_and_commits_once() {
    let mut s = session_with_block();
    let n = s.history_len();
    s.execute(Command::BeginTransform).unwrap();
    assert!(s.transform_active());
    // During the transform the composite shows the moved content, the layer keeps the hole.
    s.execute(Command::SetTransform {
        matrix: Affine::translate(50.0, 10.0),
    })
    .unwrap();
    let live = s.composite(&[(0, 0)]);
    let at = |x: i32, y: i32| live[0].1.get(x as usize, y as usize);
    assert!(
        (at(75, 35)[1]).abs() < 1e-3,
        "moved block visible in composite"
    );
    assert!(
        (at(25, 25)[1] - 1.0).abs() < 1e-3,
        "hole shows the background"
    );
    s.execute(Command::CommitTransform).unwrap();
    assert!(!s.transform_active());
    assert_eq!(px(&s, 75, 35), RED);
    assert_eq!(px(&s, 25, 25)[3], 0.0);
    assert_eq!(s.history_len(), n + 1);
    s.execute(Command::Undo).unwrap();
    assert_eq!(px(&s, 25, 25), RED);
    assert_eq!(px(&s, 75, 35)[3], 0.0);
}

#[test]
fn cancel_restores_exactly() {
    let mut s = session_with_block();
    let before = s.doc().layer(s.active()).unwrap().pixels.clone();
    let n = s.history_len();
    s.execute(Command::BeginTransform).unwrap();
    s.execute(Command::SetTransform {
        matrix: Affine::rotate_about(0.7, [30.0, 25.0]),
    })
    .unwrap();
    s.execute(Command::CancelTransform).unwrap();
    let after = &s.doc().layer(s.active()).unwrap().pixels;
    for (x, y) in [(25, 25), (30, 28), (45, 35), (10, 10)] {
        assert_eq!(after.get(x, y), before.get(x, y));
    }
    assert_eq!(s.history_len(), n);
}

#[test]
fn scale_doubles_the_block() {
    let mut s = session_with_block();
    s.execute(Command::BeginTransform).unwrap();
    s.execute(Command::SetTransform {
        matrix: Affine::scale_about(2.0, 2.0, [20.0, 20.0]),
    })
    .unwrap();
    s.execute(Command::CommitTransform).unwrap();
    assert!(px(&s, 58, 38)[3] > 0.99, "{:?}", px(&s, 58, 38));
    assert_eq!(px(&s, 62, 42)[3], 0.0);
}

#[test]
fn transform_lifts_only_the_selection() {
    let mut s = session_with_block();
    s.execute(Command::SelectShape {
        contours: vec![rect_polygon([20.0, 20.0], [30.0, 30.0])],
        mode: Mode::Replace,
    })
    .unwrap();
    s.execute(Command::BeginTransform).unwrap();
    s.execute(Command::SetTransform {
        matrix: Affine::translate(0.0, 100.0),
    })
    .unwrap();
    s.execute(Command::CommitTransform).unwrap();
    assert_eq!(px(&s, 25, 125), RED, "selected half moved");
    assert_eq!(px(&s, 35, 25), RED, "unselected half stayed");
    assert_eq!(px(&s, 25, 25)[3], 0.0);
}

#[test]
fn painting_or_undo_commits_or_cancels_a_pending_transform() {
    let mut s = session_with_block();
    s.execute(Command::BeginTransform).unwrap();
    s.execute(Command::SetTransform {
        matrix: Affine::translate(100.0, 0.0),
    })
    .unwrap();
    s.execute(Command::Undo).unwrap();
    assert!(!s.transform_active());
    assert_eq!(px(&s, 25, 25), RED, "undo during a transform cancels it");
}
