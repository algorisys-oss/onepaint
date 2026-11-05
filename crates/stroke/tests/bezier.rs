#![allow(clippy::unwrap_used)]
//! Cases ported from happypaint `engine/stroke/bezier-path-ops.test.ts`, plus edits.
use onepaint_stroke::bezier::*;

fn a(x: f64, y: f64, h: Option<([f64; 2], [f64; 2])>, kind: AnchorKind) -> PathAnchor {
    let mut p = PathAnchor { x, y, kind, ..Default::default() };
    if let Some((i, o)) = h {
        (p.in_x, p.in_y, p.out_x, p.out_y) = (Some(i[0]), Some(i[1]), Some(o[0]), Some(o[1]));
    }
    p
}

fn curve() -> PathSpec {
    let mut first = PathAnchor::corner(0.0, 0.0);
    (first.out_x, first.out_y) = (Some(30.0), Some(0.0));
    PathSpec {
        subpaths: vec![Subpath {
            closed: false,
            anchors: vec![first, a(100.0, 50.0, Some(([-20.0, -10.0], [20.0, 10.0])), AnchorKind::Smooth), PathAnchor::corner(200.0, 0.0)],
        }],
        stroke: Some(PathStroke { brush_id: "ink".into(), size: 10.0, color: [1.0, 0.0, 0.0], opacity: 1.0, seed: None }),
        fill: None,
    }
}

fn near(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() < eps
}

#[test]
fn translation_moves_anchors_not_handles() {
    let t = affine(&curve(), &Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 5.0, f: -7.0 });
    let p = t.subpaths[0].anchors[1];
    assert_eq!([p.x, p.y, p.in_x.unwrap(), p.in_y.unwrap(), p.out_x.unwrap(), p.out_y.unwrap()], [105.0, 43.0, -20.0, -10.0, 20.0, 10.0]);
    assert_eq!(t.stroke.unwrap().size, 10.0);
}

#[test]
fn affine_maps_control_points_exactly() {
    let (r, sx, sy) = (0.7f64, 2.0, 0.5);
    let m = Affine { a: r.cos() * sx, b: r.sin() * sx, c: -r.sin() * sy, d: r.cos() * sy, e: 3.0, f: 4.0 };
    let src = curve();
    let t = affine(&src, &m);
    let map = |p: [f64; 2]| [m.a * p[0] + m.c * p[1] + m.e, m.b * p[0] + m.d * p[1] + m.f];
    for (sa, ta) in src.subpaths[0].anchors.iter().zip(&t.subpaths[0].anchors) {
        let p = map([sa.x, sa.y]);
        assert!(near(ta.x, p[0], 1e-9) && near(ta.y, p[1], 1e-9));
        if sa.has_out() {
            let h = map([sa.x + sa.out_handle()[0], sa.y + sa.out_handle()[1]]);
            assert!(near(ta.x + ta.out_handle()[0], h[0], 1e-9) && near(ta.y + ta.out_handle()[1], h[1], 1e-9));
        }
        assert_eq!(ta.kind, sa.kind);
    }
    assert!(near(t.stroke.unwrap().size, 10.0, 1e-9), "|det| = 1");
    let half = affine(&src, &Affine { a: 0.5, b: 0.0, c: 0.0, d: 0.5, e: 0.0, f: 0.0 });
    assert!(near(half.stroke.unwrap().size, 5.0, 1e-9));
}

#[test]
fn bounds_and_area() {
    let b = bounds(&[curve()]).unwrap();
    assert!(near(b[0], -5.0, 1e-6) && near(b[0] + b[2], 205.0, 1e-6) && near(b[1], -5.0, 1e-6));
    assert!(bounds(&[]).is_none());
    let c = area_contours(&[curve()]);
    assert_eq!(c.len(), 1);
    assert_eq!(*c[0].last().unwrap(), [0.0, 0.0], "an open path closes back to its start");
    let dot = PathSpec { subpaths: vec![Subpath { anchors: vec![PathAnchor::corner(5.0, 5.0)], closed: false }], ..Default::default() };
    assert!(area_contours(&[dot]).is_empty());
}

#[test]
fn simulated_pressure_tapers() {
    let line: Vec<[f64; 2]> = (0..=100).map(|i| [i as f64 * 10.0, 0.0]).collect();
    let p = simulated_pressure(&line);
    assert!(near(p[0], 0.05, 1e-6) && near(p[100], 0.05, 1e-6));
    assert_eq!(p[50], 1.0);
    assert!(p[10] < p[20]);
    for i in 0..=100 {
        assert!(near(p[i], p[100 - i], 1e-9));
    }
    let long: Vec<[f64; 2]> = (0..=1000).map(|i| [i as f64 * 10.0, 0.0]).collect();
    assert_eq!(simulated_pressure(&long)[40], 1.0, "taper capped at 400 px");
}

#[test]
fn flatten_and_editing() {
    let s = &curve().subpaths[0];
    let poly = flatten_subpath(s, 4.0);
    assert_eq!(poly[0], [0.0, 0.0]);
    assert_eq!(*poly.last().unwrap(), [200.0, 0.0]);
    assert!(poly.len() > 10);
    let straight = Subpath {
        anchors: vec![PathAnchor::corner(0.0, 0.0), PathAnchor::corner(10.0, 0.0), PathAnchor::corner(10.0, 10.0)],
        closed: true,
    };
    assert_eq!(flatten_subpath(&straight, 4.0), vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 0.0]]);

    // Insertion keeps the curve: the split point lies on the original.
    let mut sub = s.clone();
    let orig = segment_controls(&sub.anchors[0], &sub.anchors[1]);
    insert_anchor_at(&mut sub, 0, 0.4);
    let on = cubic_at(&orig, 0.4);
    assert!(near(sub.anchors[1].x, on[0], 1e-9) && near(sub.anchors[1].y, on[1], 1e-9));
    let left = segment_controls(&sub.anchors[0], &sub.anchors[1]);
    let q = cubic_at(&left, 0.5);
    let q0 = cubic_at(&orig, 0.2);
    assert!(near(q[0], q0[0], 1e-9) && near(q[1], q0[1], 1e-9), "halves follow the original");

    let mut an = sub.anchors.clone();
    set_handle(&mut an, 2, HandleSide::Out, [5.0, 6.0]);
    assert_eq!(an[2].in_handle(), [-5.0, -6.0], "smooth mirrors");
    toggle_anchor_kind(&mut an, 2);
    assert!(an[2].kind == AnchorKind::Corner && !an[2].has_in());
    toggle_anchor_kind(&mut an, 2);
    assert!(an[2].kind == AnchorKind::Smooth && an[2].has_out());
    let c = constrain_handle(10.0, 3.0);
    assert!(near(c[1], 0.0, 1e-9) && near(c[0], 10.0f64.hypot(3.0), 1e-9));
    let hit = closest_segment(s, [100.0, 49.0], 5.0).unwrap();
    assert!(hit.0 <= 1 && hit.2 < 5.0);
    assert!(closest_segment(s, [100.0, 400.0], 5.0).is_none());
    let mut two = Subpath { anchors: an[..2].to_vec(), closed: false };
    delete_anchor(&mut two, 0);
    assert_eq!(two.anchors.len(), 2, "keeps at least two");
}

#[test]
fn json_matches_happypaint_field_names() {
    let v = serde_json::to_value(curve()).unwrap();
    assert_eq!(v["subpaths"][0]["anchors"][1]["inX"], -20.0);
    assert_eq!(v["subpaths"][0]["anchors"][1]["kind"], "smooth");
    assert_eq!(v["stroke"]["brushId"], "ink");
    assert!(v["subpaths"][0]["anchors"][2].get("outX").is_none());
    let back: PathSpec = serde_json::from_value(v).unwrap();
    assert_eq!(back, curve());
}

// Photoshop's mid-draw Pen gestures (happypaint `bezier-path-handles.test.ts`,
// v0.204.13): the in-handle (the curve already drawn) never moves.
fn smooth_pair() -> Vec<PathAnchor> {
    vec![a(0.0, 0.0, None, AnchorKind::Corner), a(100.0, 0.0, Some(([-30.0, -10.0], [30.0, 10.0])), AnchorKind::Smooth)]
}

#[test]
fn cut_out_handle_keeps_the_in_handle_and_makes_a_corner() {
    let mut v = smooth_pair();
    cut_out_handle(&mut v, 1);
    assert_eq!((v[1].out_x, v[1].out_y), (None, None));
    assert_eq!((v[1].in_x, v[1].in_y), (Some(-30.0), Some(-10.0)));
    assert_eq!(v[1].kind, AnchorKind::Corner);
    let before = v.clone();
    cut_out_handle(&mut v, 5);
    assert_eq!(v, before, "a bad index is ignored");
}

#[test]
fn set_out_handle_only_moves_the_out_handle_alone() {
    let mut v = smooth_pair();
    set_out_handle_only(&mut v, 1, [0.0, 50.0]);
    assert_eq!((v[1].out_x, v[1].out_y), (Some(0.0), Some(50.0)));
    assert_eq!((v[1].in_x, v[1].in_y), (Some(-30.0), Some(-10.0)));
    assert_eq!(v[1].kind, AnchorKind::Corner);
    // An anchor with no in-handle gains just the out-handle.
    set_out_handle_only(&mut v, 0, [20.0, 5.0]);
    assert_eq!((v[0].out_x, v[0].out_y), (Some(20.0), Some(5.0)));
    assert_eq!(v[0].in_x, None);
}
