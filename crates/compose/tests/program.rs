#![allow(clippy::unwrap_used)]
//! The flattened tile programs reproduce the tree compositor exactly.
use onepaint_compose::program::{Op, plan_tile, run};
use onepaint_compose::testdocs::{Rng, random_adjustment, random_doc, random_stroke, with_adjustments};
use onepaint_compose::{GHOST_PEG_KEY, StrokeOverlay, composite_tile};
use onepaint_doc::{Adjustment, Group, Layer, Node};
use onepaint_filters::Kind;

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

#[test]
fn colour_adjustment_layers_plan_and_match_the_tree_compositor() {
    for seed in 0..60 {
        let doc = with_adjustments(random_doc(seed, 300, 300, false), seed);
        for (tx, ty) in TILES {
            let prog = plan_tile(&doc, tx, ty, None).unwrap();
            assert_eq!(run(&prog), composite_tile(&doc, tx, ty, None), "seed {seed} tile {tx},{ty}");
        }
    }
}

#[test]
fn random_adjustments_carry_their_params() {
    let mut r = Rng::new(9);
    for _ in 0..100 {
        let a = random_adjustment(&mut r);
        let v = serde_json::to_value(&a.params).unwrap();
        assert!(a.kind == "invert" || v.as_object().is_some_and(|o| !o.is_empty()), "{} parsed to defaults", a.kind);
    }
}

#[test]
fn sand_tone_and_pattern_fill_layers_stay_on_the_cpu() {
    for kind in ["tone", "pattern-fill"] {
        let mut doc = random_doc(4, 300, 300, false);
        let mut l = Layer::new("adj");
        let mut params = onepaint_filters::default_params(Kind::parse(kind).unwrap());
        params.tone_shape = Some("sand".into());
        l.adjustment = Some(Box::new(Adjustment { kind: kind.into(), params }));
        doc.root.push(Node::Layer(l));
        assert!(plan_tile(&doc, 0, 0, None).is_none(), "{kind}");
    }
}

fn papered(seed: u64) -> onepaint_doc::Document {
    let mut doc = with_adjustments(random_doc(seed, 300, 300, false), seed);
    let grain = ["rough-tooth", "canvas", "linen"][seed as usize % 3];
    let scale = Some(0.5 + (seed % 4) as f64 * 0.4);
    doc.set_paper(Some(onepaint_doc::Paper { grain_id: grain.into(), scale, strength: 0.3 + (seed % 3) as f64 * 0.3 }));
    doc
}

#[test]
fn paper_shades_programs_like_the_tree_compositor() {
    for seed in 0..24 {
        let doc = papered(seed);
        for (tx, ty) in TILES {
            let prog = plan_tile(&doc, tx, ty, None).unwrap();
            assert!(matches!(prog.ops.last(), Some(Op::Paper { .. })));
            assert_eq!(run(&prog), composite_tile(&doc, tx, ty, None), "seed {seed} tile {tx},{ty}");
        }
    }
}

#[test]
fn relief_programs_match_the_tree_compositor() {
    use onepaint_compose::testdocs::with_relief;
    // 300 × 300: tiles end inside and at the document's edge.
    for seed in 0..30 {
        let doc = with_relief(random_doc(seed, 300, 300, false), seed);
        for (tx, ty) in TILES {
            let prog = plan_tile(&doc, tx, ty, None).unwrap();
            assert_eq!(run(&prog), composite_tile(&doc, tx, ty, None), "seed {seed} tile {tx},{ty}");
        }
    }
}
