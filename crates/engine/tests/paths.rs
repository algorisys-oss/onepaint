#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::path_ops::StrokePathTool;
use onepaint_engine::*;
use onepaint_selection::Mode;
use onepaint_stroke::bezier::*;

fn square(x0: f64, y0: f64, x1: f64, y1: f64, stroke: bool, fill: bool) -> PathSpec {
    PathSpec {
        subpaths: vec![Subpath {
            anchors: vec![PathAnchor::corner(x0, y0), PathAnchor::corner(x1, y0), PathAnchor::corner(x1, y1), PathAnchor::corner(x0, y1)],
            closed: true,
        }],
        stroke: stroke.then(|| PathStroke {
            brush_id: "hard-round".into(),
            size: 6.0,
            color: [1.0, 0.0, 0.0],
            opacity: 1.0,
            seed: Some(7),
        }),
        fill: fill.then_some(PathFill { color: [0.0, 0.0, 1.0], opacity: 1.0 }),
    }
}

fn px(s: &Session, id: &str, x: i32, y: i32) -> [f32; 4] {
    s.doc().layer(id).unwrap().pixels.get(x, y)
}

#[test]
fn a_path_layer_renders_and_round_trips() {
    let mut s = Session::new(120, 100, Some(Rgba::WHITE));
    s.execute(Command::CreatePathLayer { spec: Box::new(square(20.0, 20.0, 80.0, 70.0, true, true)) }).unwrap();
    let id = s.active().to_string();
    assert!(s.is_path_layer(&id));
    let edge = px(&s, &id, 50, 20);
    assert!(edge[3] > 0.9 && edge[0] > 0.9, "red stroke on the edge: {edge:?}");
    let inside = px(&s, &id, 50, 45);
    assert!(inside[3] > 0.99 && inside[2] > 0.99, "blue fill inside: {inside:?}");
    assert_eq!(px(&s, &id, 5, 5)[3], 0.0);
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.paths(&id), vec![square(20.0, 20.0, 80.0, 70.0, true, true)]);
}

#[test]
fn edits_coalesce_and_removing_the_last_object_leaves_paint() {
    let mut s = Session::new(120, 100, None);
    s.execute(Command::CreatePathLayer { spec: Box::new(square(20.0, 20.0, 80.0, 70.0, true, false)) }).unwrap();
    let id = s.active().to_string();
    let h0 = s.history_len();
    for x in [82.0, 84.0, 86.0] {
        s.execute(Command::UpdatePathObject { id: id.clone(), index: 0, spec: Some(Box::new(square(20.0, 20.0, x, 70.0, true, false))) })
            .unwrap();
    }
    assert_eq!(s.history_len(), h0 + 1, "a drag is one step");
    assert!(px(&s, &id, 86, 45)[3] > 0.5);
    s.execute(Command::Undo).unwrap();
    assert!(px(&s, &id, 86, 45)[3] < 0.01 && px(&s, &id, 80, 45)[3] > 0.5);
    s.execute(Command::AddPathObject { id: id.clone(), spec: Box::new(square(5.0, 5.0, 15.0, 15.0, true, false)) }).unwrap();
    assert_eq!(s.paths(&id).len(), 2);
    s.execute(Command::UpdatePathObject { id: id.clone(), index: 1, spec: None }).unwrap();
    s.execute(Command::UpdatePathObject { id: id.clone(), index: 0, spec: None }).unwrap();
    assert!(!s.is_path_layer(&id));
}

#[test]
fn painting_on_a_path_layer_rasterises_it() {
    let mut s = Session::new(120, 100, None);
    s.execute(Command::CreatePathLayer { spec: Box::new(square(20.0, 20.0, 80.0, 70.0, true, false)) }).unwrap();
    let id = s.active().to_string();
    s.stroke_begin(onepaint_stroke::StrokePoint { x: 10.0, y: 90.0, pressure: 1.0, ..Default::default() });
    s.stroke_end();
    assert!(!s.is_path_layer(&id));
    assert!(px(&s, &id, 50, 20)[3] > 0.9, "the rendering stays");
}

#[test]
fn path_ops_select_fill_mask_and_stroke_onto_the_layer_below() {
    let mut s = Session::new(120, 100, None);
    let below = s.active().to_string();
    s.execute(Command::CreatePathLayer { spec: Box::new(square(20.0, 20.0, 80.0, 70.0, false, false)) }).unwrap();
    assert_eq!(s.path_op_target().as_deref(), Some(below.as_str()));

    s.execute(Command::SelectFromPath { mode: Mode::Replace }).unwrap();
    assert!(s.selection().unwrap().coverage(50, 45) > 0.5 && s.selection().unwrap().coverage(5, 5) == 0.0);
    s.execute(Command::Deselect).unwrap();

    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 1.0, 0.0, 1.0) }).unwrap();
    let h0 = s.history_len();
    s.execute(Command::FillPath).unwrap();
    assert_eq!(s.history_len(), h0 + 1);
    let p = px(&s, &below, 50, 45);
    assert!(p[1] > 0.99 && p[3] > 0.99, "{p:?}");
    assert_eq!(px(&s, &below, 5, 5)[3], 0.0);

    // Create Vector Mask: the paths become the layer's vector mask.
    s.execute(Command::MaskFromPath).unwrap();
    let v = s.doc().layer(&below).unwrap().vector_mask().unwrap();
    assert_eq!(v.paths.len(), 1);
    assert_eq!(v.paths[0].knots[2], [80.0, 70.0, 80.0, 70.0, 80.0, 70.0]);
    assert!(s.doc().layer(&below).unwrap().mask.is_none(), "no pixel mask");

    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 1.0, 1.0) }).unwrap();
    let h1 = s.history_len();
    s.execute(Command::StrokePath { tool: StrokePathTool::Brush, simulate_pressure: true }).unwrap();
    assert_eq!(s.history_len(), h1 + 1, "one transaction");
    let e = px(&s, &below, 50, 20);
    assert!(e[0] > 0.9 && e[2] > 0.9, "magenta along the path: {e:?}");
    assert!(s.is_path_layer(s.active()), "the path layer stays active");
}

#[test]
fn ops_without_a_layer_below_are_refused() {
    let mut s = Session::new(64, 64, None);
    let only = s.active().to_string();
    // Put the path at the bottom of the stack.
    s.execute(Command::CreatePathLayer { spec: Box::new(square(5.0, 5.0, 30.0, 30.0, true, false)) }).unwrap();
    let path = s.active().to_string();
    s.execute(Command::MoveLayer { id: path.clone(), parent: None, index: 0 }).unwrap();
    assert!(s.path_op_target().is_none());
    assert!(s.execute(Command::FillPath).is_err());
    let _ = only;
}

#[test]
fn sealing_separates_two_drags() {
    let mut s = Session::new(120, 100, None);
    s.execute(Command::CreatePathLayer { spec: Box::new(square(20.0, 20.0, 80.0, 70.0, true, false)) }).unwrap();
    let id = s.active().to_string();
    let h0 = s.history_len();
    s.execute(Command::UpdatePathObject { id: id.clone(), index: 0, spec: Some(Box::new(square(20.0, 20.0, 82.0, 70.0, true, false))) })
        .unwrap();
    s.execute(Command::SealHistory).unwrap();
    s.execute(Command::UpdatePathObject { id: id.clone(), index: 0, spec: Some(Box::new(square(20.0, 20.0, 84.0, 70.0, true, false))) })
        .unwrap();
    assert_eq!(s.history_len(), h0 + 2);
}

#[test]
fn fill_shape_fills_the_silhouette_with_symmetry() {
    let mut s = Session::new(100, 80, None);
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 1.0, 1.0) }).unwrap();
    s.execute(Command::SetSymmetry { symmetry: Symmetry { mode: SymmetryMode::Vertical, ..Default::default() } }).unwrap();
    let tri = vec![[10.0, 10.0], [40.0, 10.0], [25.0, 50.0]];
    s.execute(Command::FillShape { points: tri, opacity: 0.5 }).unwrap();
    let id = s.active().to_string();
    let p = px(&s, &id, 25, 20);
    assert!((p[3] - 0.5).abs() < 0.02 && p[2] > 0.45, "half-opacity blue: {p:?}");
    assert!(px(&s, &id, 74, 20)[3] > 0.45, "mirrored");
    assert_eq!(px(&s, &id, 50, 70)[3], 0.0);
    s.execute(Command::Undo).unwrap();
    assert_eq!(px(&s, &id, 25, 20)[3], 0.0, "one undo step");
}

#[test]
fn free_transform_keeps_a_path_editable_and_undoes_in_one_step() {
    let mut s = Session::new(200, 160, Some(Rgba::WHITE));
    s.execute(Command::CreatePathLayer { spec: Box::new(square(20.0, 20.0, 60.0, 60.0, true, true)) }).unwrap();
    let id = s.active().to_string();
    let n = s.history_len();
    s.execute(Command::BeginTransform { ids: vec![] }).unwrap();
    // Non-uniform: paths take any affine map exactly.
    let m = transform::Affine { a: 2.0, b: 0.0, c: 0.0, d: 1.0, e: 10.0, f: 30.0 };
    s.execute(Command::SetTransform { matrix: m }).unwrap();
    s.execute(Command::CommitTransform).unwrap();
    assert_eq!(s.history_len(), n + 1);
    let p = s.paths(&id);
    assert_eq!(p.len(), 1, "still a path layer");
    let a = &p[0].subpaths[0].anchors;
    assert_eq!((a[0].x, a[0].y, a[2].x, a[2].y), (50.0, 50.0, 130.0, 90.0));
    assert!(px(&s, &id, 90, 70)[2] > 0.99, "re-rendered fill inside the new square");
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.paths(&id), vec![square(20.0, 20.0, 60.0, 60.0, true, true)]);
    assert_eq!(px(&s, &id, 90, 70)[3], 0.0);
}

#[test]
fn a_distorted_path_bakes_to_pixels() {
    let mut s = Session::new(200, 160, Some(Rgba::WHITE));
    s.execute(Command::CreatePathLayer { spec: Box::new(square(20.0, 20.0, 60.0, 60.0, true, true)) }).unwrap();
    let id = s.active().to_string();
    s.execute(Command::BeginTransform { ids: vec![] }).unwrap();
    s.execute(Command::BeginDistort).unwrap();
    s.execute(Command::SetWarp { warp: transform::WarpMode::Quad([[10.0, 10.0], [70.0, 20.0], [60.0, 60.0], [20.0, 70.0]]) }).unwrap();
    s.execute(Command::CommitTransform).unwrap();
    assert!(!s.is_path_layer(&id));
    assert!(px(&s, &id, 40, 40)[3] > 0.9);
}
