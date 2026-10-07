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

mod drag {
    use onepaint_engine::transform::{Handle, TransformParams};
    use onepaint_raster::Rect;

    const DOC: [f64; 2] = [100.0, 100.0];
    const BOX: Rect = Rect::new(20, 30, 40, 20);

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn pt_near(a: [f64; 2], b: [f64; 2]) -> bool {
        near(a[0], b[0]) && near(a[1], b[1])
    }

    #[test]
    fn corner_scales_uniformly_with_opposite_corner_pinned() {
        let p0 = TransformParams::default();
        let p = p0.scale_drag(
            DOC,
            BOX,
            Handle::Corner(2),
            [60.0, 50.0],
            [100.0, 70.0],
            false,
            false,
        );
        assert!(near(p.sx, 2.0) && near(p.sy, 2.0), "{p:?}");
        assert!(pt_near(p.to_affine(DOC).apply([20.0, 30.0]), [20.0, 30.0]));
        // Shift changes nothing for corners.
        let q = p0.scale_drag(
            DOC,
            BOX,
            Handle::Corner(2),
            [60.0, 50.0],
            [100.0, 70.0],
            true,
            false,
        );
        assert_eq!(p, q);
    }

    #[test]
    fn edge_scales_one_axis_and_can_mirror() {
        let p0 = TransformParams::default();
        let p = p0.scale_drag(
            DOC,
            BOX,
            Handle::Edge(1),
            [60.0, 40.0],
            [100.0, 40.0],
            false,
            false,
        );
        assert!(near(p.sx, 2.0) && near(p.sy, 1.0));
        assert!(pt_near(p.to_affine(DOC).apply([20.0, 40.0]), [20.0, 40.0]));
        let m = p0.scale_drag(
            DOC,
            BOX,
            Handle::Edge(1),
            [60.0, 40.0],
            [0.0, 40.0],
            false,
            false,
        );
        assert!(near(m.sx, -0.5));
        let s = p0.scale_drag(
            DOC,
            BOX,
            Handle::Edge(1),
            [60.0, 40.0],
            [100.0, 40.0],
            true,
            false,
        );
        assert!(near(s.sx, 2.0) && near(s.sy, 2.0));
        let s2 = p0.scale_drag(
            DOC,
            BOX,
            Handle::Edge(1),
            [60.0, 40.0],
            [100.0, 500.0],
            true,
            false,
        );
        assert_eq!(s, s2);
    }

    #[test]
    fn alt_scales_from_the_centre() {
        let p0 = TransformParams::default();
        let p = p0.scale_drag(
            DOC,
            BOX,
            Handle::Corner(2),
            [60.0, 50.0],
            [80.0, 60.0],
            false,
            true,
        );
        assert!(near(p.sx, 2.0) && near(p.sy, 2.0));
        assert!(pt_near(p.to_affine(DOC).apply([40.0, 40.0]), [40.0, 40.0]));
        let e = p0.scale_drag(
            DOC,
            BOX,
            Handle::Edge(1),
            [60.0, 40.0],
            [80.0, 40.0],
            false,
            true,
        );
        assert!(near(e.sx, 2.0) && near(e.sy, 1.0));
        let es = p0.scale_drag(
            DOC,
            BOX,
            Handle::Edge(1),
            [60.0, 40.0],
            [80.0, 40.0],
            true,
            true,
        );
        assert!(near(es.sx, 2.0) && near(es.sy, 2.0));
    }

    #[test]
    fn rotated_corner_drag_keeps_rotation() {
        let p0 = TransformParams {
            rotation: std::f64::consts::FRAC_PI_6,
            ..Default::default()
        };
        let m = p0.to_affine(DOC);
        let (tl, br) = (m.apply([20.0, 30.0]), m.apply([60.0, 50.0]));
        let far = [tl[0] + 2.0 * (br[0] - tl[0]), tl[1] + 2.0 * (br[1] - tl[1])];
        let p = p0.scale_drag(DOC, BOX, Handle::Corner(2), br, far, false, false);
        assert!(
            near(p.sx, 2.0) && near(p.sy, 2.0) && near(p.rotation, std::f64::consts::FRAC_PI_6)
        );
        assert!(pt_near(p.to_affine(DOC).apply([20.0, 30.0]), tl));
    }

    #[test]
    fn rotate_keeps_box_centre_and_move_translates() {
        let p0 = TransformParams::default();
        let c = [40.0, 40.0];
        let r = p0.rotate_drag(DOC, BOX, [40.0, 20.0], [60.0, 40.0]);
        assert!(near(r.rotation, std::f64::consts::FRAC_PI_2), "{r:?}");
        assert!(pt_near(r.to_affine(DOC).apply(c), c));
        let mv = p0.move_drag([10.0, 10.0], [17.0, 13.0], false);
        assert!(near(mv.dx, 7.0) && near(mv.dy, 3.0));
        let mv45 = p0.move_drag([0.0, 0.0], [10.0, 1.0], true);
        assert!(near(mv45.dy, 0.0));
    }

    #[test]
    fn handles_and_hit_testing() {
        let p0 = TransformParams::default();
        let hs = p0.handles(DOC, BOX);
        assert!(pt_near(hs.corners[2], [60.0, 50.0]) && pt_near(hs.edges[1], [60.0, 40.0]));
        assert_eq!(p0.hit(DOC, BOX, [61.0, 51.0], 3.0, 10.0), Handle::Corner(2));
        assert_eq!(p0.hit(DOC, BOX, [40.0, 30.5], 3.0, 10.0), Handle::Edge(0));
        assert_eq!(p0.hit(DOC, BOX, [40.0, 20.0], 3.0, 10.0), Handle::Rotate);
        assert_eq!(p0.hit(DOC, BOX, [5.0, 5.0], 3.0, 10.0), Handle::Body);
    }
}
