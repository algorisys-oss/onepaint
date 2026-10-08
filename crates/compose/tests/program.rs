#![allow(clippy::unwrap_used)]
//! The flattened tile programs reproduce the tree compositor exactly.
use onepaint_compose::program::{Op, plan_tile, run};
use onepaint_compose::testdocs::{random_doc, random_stroke};
use onepaint_compose::{GHOST_PEG_KEY, StrokeOverlay, composite_tile};
use onepaint_doc::{Group, Node};

const TILES: [(i32, i32); 4] = [(0, 0), (1, 0), (0, 1), (1, 1)];

#[test]
fn programs_match_the_tree_compositor() {
    for seed in 0..60 {
        let doc = random_doc(seed, 300, 300, true);
        for (tx, ty) in TILES {
            let prog = plan_tile(&doc, tx, ty, None).unwrap();
            assert_eq!(run(&prog), composite_tile(&doc, tx, ty, None), "seed {seed} tile {tx},{ty}");
        }
    }
}

#[test]
fn programs_match_with_a_stroke_in_progress() {
    for seed in 0..60 {
        let doc = random_doc(seed, 300, 300, false);
        let (buf, id, opacity, mode, mask) = random_stroke(seed, &doc).unwrap();
        let ov = StrokeOverlay { layer_id: &id, stroke: &buf, opacity, mode, mask };
        for (tx, ty) in TILES {
            let prog = plan_tile(&doc, tx, ty, Some(&ov)).unwrap();
            assert_eq!(run(&prog), composite_tile(&doc, tx, ty, Some(&ov)), "seed {seed} tile {tx},{ty}");
        }
    }
}

#[test]
fn unsupported_features_stay_on_the_cpu() {
    let mut doc = random_doc(3, 300, 300, false);
    let mut g = Group::new("ghost");
    g.children = std::mem::take(&mut doc.root);
    g.extra.insert(GHOST_PEG_KEY.into(), serde_json::json!([4.0, 0.0, 0.0, 1.0]));
    doc.root = vec![Node::Group(g)];
    assert!(plan_tile(&doc, 0, 0, None).is_none());
}

#[test]
fn empty_tiles_plan_to_nothing() {
    let doc = random_doc(5, 300, 300, false);
    let prog = plan_tile(&doc, 7, 7, None).unwrap();
    assert!(prog.ops.iter().all(|op| matches!(op, Op::PushZero | Op::Pop { .. })));
    assert!(run(&prog).iter().all(|v| *v == 0.0));
}
