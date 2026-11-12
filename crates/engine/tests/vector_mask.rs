#![allow(clippy::unwrap_used)]
//! Vector masks (happypaint v0.202.1, `vector-mask.test.ts`).
use onepaint_color::Rgba;
use onepaint_doc::{VectorMask, VectorPath};
use onepaint_engine::path_ops::vector_mask_from_paths;
use onepaint_engine::*;
use onepaint_stroke::bezier::*;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> VectorMask {
    let knots = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].iter().map(|p| [p[0], p[1], p[0], p[1], p[0], p[1]]).collect();
    VectorMask { paths: vec![VectorPath { knots, ..Default::default() }], ..Default::default() }
}

#[test]
fn pen_paths_convert_like_vector_mask_from_path_specs() {
    let smooth =
        PathAnchor { x: 30.0, y: 10.0, in_x: Some(-5.0), in_y: Some(-2.0), out_x: Some(5.0), out_y: Some(2.0), kind: AnchorKind::Smooth };
    let a = Subpath { anchors: vec![PathAnchor::corner(10.0, 10.0), smooth, PathAnchor::corner(20.0, 30.0)], closed: true };
    let b = Subpath {
        anchors: vec![PathAnchor { in_x: Some(4.0), in_y: Some(-4.0), ..PathAnchor::corner(8.0, 8.0) }, PathAnchor::corner(1.0, 1.0)],
        closed: false,
    };
    let lone = Subpath { anchors: vec![PathAnchor::corner(5.0, 5.0)], closed: false };
    let v = vector_mask_from_paths(&[PathSpec { subpaths: vec![a, b, lone], stroke: None, fill: None }]).unwrap();
    assert_eq!(v.paths.iter().map(|p| p.knots.len()).collect::<Vec<_>>(), vec![3, 2]);
    assert_eq!(v.paths[0].knots[0], [10.0, 10.0, 10.0, 10.0, 10.0, 10.0]);
    assert_eq!(v.paths[0].knots[1], [25.0, 8.0, 30.0, 10.0, 35.0, 12.0]);
    assert_eq!(v.paths[1].knots[0], [12.0, 4.0, 8.0, 8.0, 8.0, 8.0]);
    assert!(vector_mask_from_paths(&[]).is_none());
}

#[test]
fn free_transform_maps_the_vector_mask_in_the_same_step_unless_unlinked() {
    for unlinked in [false, true] {
        let mut s = Session::new(200, 160, Some(Rgba::WHITE));
        s.execute(Command::AddLayer).unwrap();
        s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
        s.execute(Command::SelectAll).unwrap();
        s.execute(Command::FillSelection).unwrap();
        s.execute(Command::Deselect).unwrap();
        let id = s.active().to_string();
        let v = VectorMask { unlinked, ..rect(10.0, 10.0, 50.0, 50.0) };
        s.execute(Command::SetLayerProps {
            id: id.clone(),
            props: LayerProps { vector_mask: Some(Some(v.clone())), ..Default::default() },
        })
        .unwrap();
        let n = s.history_len();
        s.execute(Command::BeginTransform { ids: vec![] }).unwrap();
        let m = transform::Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 20.0, f: 5.0 };
        s.execute(Command::SetTransform { matrix: m }).unwrap();
        s.execute(Command::CommitTransform).unwrap();
        assert_eq!(s.history_len(), n + 1, "one step");
        let got = s.doc().layer(&id).unwrap().vector_mask().unwrap();
        let want = if unlinked { v.clone() } else { v.transformed([1.0, 0.0, 0.0, 1.0, 20.0, 5.0]) };
        assert_eq!(got, want);
        s.execute(Command::Undo).unwrap();
        assert_eq!(s.doc().layer(&id).unwrap().vector_mask().unwrap(), v);
    }
}

#[test]
fn crop_and_scale_move_the_mask_and_the_composite_honours_it() {
    let mut s = Session::new(200, 160, None);
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SelectAll).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    let id = s.active().to_string();
    let props = LayerProps { vector_mask: Some(Some(rect(10.0, 10.0, 50.0, 50.0))), ..Default::default() };
    s.execute(Command::SetLayerProps { id: id.clone(), props }).unwrap();
    let alpha = |s: &Session, x, y| onepaint_compose::composite_px(s.doc(), x, y)[3];
    assert!(alpha(&s, 20, 20) > 0.99 && alpha(&s, 80, 80) < 0.01);
    s.execute(Command::ResizeCanvas { width: 200, height: 160, dx: 30, dy: 0 }).unwrap();
    assert_eq!(s.doc().layer(&id).unwrap().vector_mask().unwrap().paths[0].knots[0][2], 40.0);
    s.execute(Command::ScaleImage { width: 400, height: 320 }).unwrap();
    assert_eq!(s.doc().layer(&id).unwrap().vector_mask().unwrap().paths[0].knots[0][2], 80.0);
    // Deleting it shows the whole layer again.
    s.execute(Command::SetLayerProps { id: id.clone(), props: LayerProps { vector_mask: Some(None), ..Default::default() } }).unwrap();
    assert!(s.doc().layer(&id).unwrap().vector_mask().is_none());
}
