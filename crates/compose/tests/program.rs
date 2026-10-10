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
fn sand_tone_and_imported_pattern_layers_stay_on_the_cpu() {
    for kind in ["tone", "pattern-fill"] {
        let mut doc = random_doc(4, 300, 300, false);
        let mut l = Layer::new("adj");
        let mut params = onepaint_filters::default_params(Kind::parse(kind).unwrap());
        params.tone_shape = Some("sand".into());
        params.pattern_image = Some("data:image/png;base64,AAAA".into());
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

/// `doc` with fully opaque normal layers dropped into its top level and
/// into its first group.
fn with_opaque_layers(mut doc: onepaint_doc::Document, seed: u64) -> onepaint_doc::Document {
    let opaque = |name: &str, c: f32| {
        let mut l = Layer::new(name);
        for y in 0..doc.height as i32 {
            for x in 0..doc.width as i32 {
                l.pixels.set(x, y, [c, 0.5 * c, 0.2, 1.0]);
            }
        }
        l
    };
    let (a, b) = (opaque("opaque-top", 0.3), opaque("opaque-in", 0.7));
    let at = (seed as usize * 7) % (doc.root.len() + 1);
    doc.root.insert(at, Node::Layer(a));
    if let Some(Node::Group(g)) = doc.root.iter_mut().find(|n| matches!(n, Node::Group(_))) {
        let at = (seed as usize) % (g.children.len() + 1);
        g.children.insert(at, Node::Layer(b));
    }
    doc
}

#[test]
fn layers_under_an_opaque_tile_are_skipped_exactly() {
    use onepaint_compose::program::{PlanCache, plan_tiles_cached};
    let (mut shorter, mut hidden) = (0, 0);
    for seed in 0..30 {
        let doc = with_opaque_layers(random_doc(seed, 300, 300, true), seed);
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let cache = PlanCache::default();
        let (cut, cpu) = plan_tiles_cached(&doc, &keys, None, None, Some(&cache));
        assert!(cpu.is_empty());
        for p in &cut {
            assert_eq!(run(p), composite_tile(&doc, p.tx, p.ty, None), "seed {seed} tile {},{}", p.tx, p.ty);
            if p.ops.len() < plan_tile(&doc, p.tx, p.ty, None).unwrap().ops.len() {
                shorter += 1;
            }
        }
        // Split at a layer under the top opaque one: hidden, so no split
        // (unless the opaque layer is a clip base, which doesn't occlude).
        let at = doc.root.iter().position(|n| n.name() == "opaque-top").unwrap();
        let base = matches!(doc.root.get(at + 1), Some(Node::Layer(l)) if l.clipped);
        if !base {
            for n in &doc.root[..at] {
                let Node::Layer(l) = n else { continue };
                if l.adjustment.is_some() {
                    continue;
                }
                let (p, _) = plan_tiles_cached(&doc, &[(0, 0)], None, Some(&l.id), Some(&cache));
                assert!(p[0].split.is_none(), "seed {seed}: {} is hidden", l.id);
                hidden += 1;
            }
        }
    }
    assert!(shorter > 0, "some programs lost the layers under an opaque one");
    assert!(hidden > 0, "some hot layers were hidden");
}

#[test]
fn cached_plans_share_prepared_adjustments() {
    use onepaint_compose::program::{PlanCache, plan_tiles_cached};
    for seed in 0..12 {
        let doc = with_adjustments(random_doc(seed, 600, 300, false), seed);
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let cache = PlanCache::default();
        let (progs, cpu) = plan_tiles_cached(&doc, &keys, None, None, Some(&cache));
        assert!(cpu.is_empty());
        let mut seen = std::collections::HashMap::new();
        for p in &progs {
            assert_eq!(run(p), composite_tile(&doc, p.tx, p.ty, None), "seed {seed} tile {},{}", p.tx, p.ty);
            for op in &p.ops {
                if let Op::Adjust { adjust, .. } = op {
                    let key = format!("{:?} {:?}", adjust.prepared.kind, adjust.gpu.params);
                    let ptr = std::sync::Arc::as_ptr(&adjust.gpu) as usize;
                    assert_eq!(*seen.entry(key).or_insert(ptr), ptr, "seed {seed}: one prepared adjustment per layer");
                }
            }
        }
    }
}

#[test]
fn programs_match_the_tree_compositor_in_8bit_documents() {
    use onepaint_compose::program::{PlanCache, plan_tiles_cached, run_split};
    for seed in 0..40 {
        let mut doc = with_adjustments(random_doc(seed, 300, 300, seed % 2 == 0), seed);
        doc.depth = onepaint_doc::Depth::Rgba8;
        if seed % 3 == 0 {
            doc.set_paper(Some(onepaint_doc::Paper { grain_id: "canvas".into(), scale: Some(1.0), strength: 0.6 }));
        }
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let hot = doc.layer_ids()[seed as usize % doc.layer_ids().len()].clone();
        let (progs, cpu) = plan_tiles_cached(&doc, &keys, None, Some(&hot), Some(&PlanCache::default()));
        assert!(cpu.is_empty());
        for p in &progs {
            let want = composite_tile(&doc, p.tx, p.ty, None);
            assert_eq!(run(p), want, "seed {seed} tile {},{}", p.tx, p.ty);
            let split = run_split(p);
            assert!(split.iter().zip(&want).all(|(a, b)| (a - b).abs() <= 1e-5), "seed {seed} split");
        }
    }
}
