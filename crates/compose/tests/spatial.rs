#![allow(clippy::unwrap_used)]
//! Spatial adjustment layers as tile programs (spec 52): the program's CPU
//! run equals the oracle (the document with the layers rendered).
use onepaint_compose::composite_tile;
use onepaint_compose::program::{Op, plan_tile, run};
use onepaint_compose::spatial::rendered;
use onepaint_compose::testdocs::{random_doc, with_spatial};

#[test]
fn spatial_programs_match_the_rendered_document() {
    let mut planned = 0;
    for seed in 0..12 {
        let doc = with_spatial(random_doc(seed, 300, 300, seed % 2 == 0), seed);
        let want_doc = rendered(&doc);
        for (tx, ty) in doc.bounds().tiles() {
            let Some(prog) = plan_tile(&doc, tx, ty, None) else { continue };
            let has =
                prog.ops.iter().any(|o| matches!(o, Op::Layer { content, .. } | Op::PushContent(content) if content.spatial.is_some()));
            planned += usize::from(has);
            // The oracle filters over the content's region, the program over the
            // tile ± reach: the same taps, but `x + offset` at other magnitudes,
            // so the bilinear weights differ in their last bits (an f16 step).
            let (got, want) = (run(&prog), composite_tile(&want_doc, tx, ty, None));
            let worst = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst <= 2e-3, "seed {seed} tile {tx},{ty}: {worst}");
        }
    }
    assert!(planned > 10, "{planned} spatial programs");
}

#[test]
fn a_spatial_layer_in_a_group_stays_on_the_cpu() {
    use onepaint_doc::{Adjustment, Group, Layer, Node};
    let mut doc = random_doc(3, 300, 300, false);
    let mut blur = Layer::new("blur");
    blur.adjustment = Some(Box::new(Adjustment {
        kind: "gaussian-blur".into(),
        params: serde_json::from_value(serde_json::json!({"radius": 4.0})).unwrap(),
    }));
    let mut g = Group::new("g");
    g.children.push(Node::Layer(blur));
    doc.root.push(Node::Group(g));
    assert!(doc.bounds().tiles().all(|(tx, ty)| plan_tile(&doc, tx, ty, None).is_none()));
}
