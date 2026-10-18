#![allow(clippy::unwrap_used)]
//! Mask density and Blend If (happypaint v0.202.0, `compositor.ts` MASKMUL
//! and BLEND). The numbers come from `mask-params.test.ts` and
//! `docs/graphics/01-rendering-and-compositing.md`.
use onepaint_compose::blend::{blend_if_weight, blend_range_weight};
use onepaint_compose::*;
use onepaint_doc::*;
use onepaint_raster::{Rect, Surface};

fn fill(s: &mut Surface, r: Rect, p: [f32; 4]) {
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            s.set(x, y, p);
        }
    }
}

fn layer(name: &str, p: [f32; 4]) -> Layer {
    let mut l = Layer::new(name);
    fill(&mut l.pixels, Rect::new(0, 0, 8, 8), p);
    l
}

fn doc(nodes: Vec<Node>) -> Document {
    let mut d = Document::new(64, 64, None);
    d.root = nodes;
    d
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5)
}

const WHITE: [f32; 4] = [1.0; 4];
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn bif_gray(src: BlendRange, dst: BlendRange) -> BlendIf {
    let mut b = BlendIf::default();
    b.0[0] = ChannelRanges { src, dst };
    b
}

#[test]
fn range_weights_match_mask_params_test() {
    let w = |r, v: u8| blend_range_weight(r, v as f32 / 255.0);
    let got: Vec<f32> = [0, 199, 200, 201].iter().map(|&v| w([0, 0, 200, 200], v)).collect();
    assert_eq!(got, vec![1.0, 1.0, 1.0, 0.0]);
    assert_eq!(w([0, 0, 200, 240], 220), 0.5);
    assert_eq!(w([40, 80, 255, 255], 60), 0.5);
    assert_eq!(w([40, 80, 255, 255], 39), 0.0);
}

#[test]
fn density_scales_what_the_mask_hides() {
    let mut l = layer("l", RED);
    let mut m = Surface::new();
    fill(&mut m, Rect::new(0, 0, 8, 8), BLACK);
    l.mask = Some(m);
    l.set_mask_density(0.5);
    // Half hidden: `1 − 0.5 · (1 − 0)`.
    assert!(close(composite_px(&doc(vec![Node::Layer(l.clone())]), 2, 2), [0.5, 0.0, 0.0, 0.5]));
    // A mask with no tile here hides it all at full density, half at 0.5.
    let mut far = layer("far", RED);
    fill(&mut far.pixels, Rect::new(300, 300, 4, 4), RED);
    far.mask = Some(Surface::new());
    far.set_mask_density(0.5);
    assert!(close(composite_px(&doc(vec![Node::Layer(far.clone())]), 301, 301), [0.5, 0.0, 0.0, 0.5]));
    far.set_mask_density(1.0);
    assert_eq!(composite_px(&doc(vec![Node::Layer(far)]), 301, 301)[3], 0.0);
    // Density 0: the mask stays but hides nothing.
    l.set_mask_density(0.0);
    assert!(close(composite_px(&doc(vec![Node::Layer(l)]), 2, 2), RED));
}

#[test]
fn this_layer_gray_fades_between_the_white_halves() {
    // Pure red is gray 0.3 · 255, 76 in f32 (the docs' worked example); white
    // halves 60 / 200 let (200 − 76) / 140 ≈ 0.886 through.
    let w = (200.0 - 76.0) / 140.0;
    let mut l = layer("red", RED);
    l.set_blend_if(Some(bif_gray([0, 0, 60, 200], FULL_RANGE)));
    let d = doc(vec![Node::Layer(layer("bg", WHITE)), Node::Layer(l)]);
    let p = composite_px(&d, 1, 1);
    assert!(close(p, [1.0, 1.0 - w, 1.0 - w, 1.0]), "{p:?}");
    let s = onepaint_color::Space::Linear;
    assert!((blend_if_weight(s, &bif_gray([0, 0, 60, 200], FULL_RANGE), WHITE, RED) - w).abs() < 1e-6);
}

#[test]
fn underlying_layer_reads_a_transparent_backdrop_as_black() {
    let mut l = layer("red", RED);
    l.set_blend_if(Some(bif_gray(FULL_RANGE, [30, 30, 255, 255])));
    assert_eq!(composite_px(&doc(vec![Node::Layer(l.clone())]), 1, 1)[3], 0.0);
    // Over white it shows.
    let d = doc(vec![Node::Layer(layer("bg", WHITE)), Node::Layer(l)]);
    assert!(close(composite_px(&d, 1, 1), RED));
}

#[test]
fn riders_and_inherit_alpha_ignore_blend_if_but_a_base_applies_it_to_its_family() {
    let hide = bif_gray([0, 0, 0, 0], FULL_RANGE); // only tone 0 passes
    // A rider's Blend If does nothing.
    let mut rider = layer("rider", RED);
    rider.clipped = true;
    rider.set_blend_if(Some(hide));
    let d = doc(vec![Node::Layer(layer("bg", WHITE)), Node::Layer(layer("base", WHITE)), Node::Layer(rider)]);
    assert!(close(composite_px(&d, 1, 1), RED));
    // The base's Blend If reads the family (red: gray 77) and hides it.
    let mut base = layer("base", BLACK);
    base.set_blend_if(Some(hide));
    let mut rider = layer("rider", RED);
    rider.clipped = true;
    let d = doc(vec![Node::Layer(layer("bg", WHITE)), Node::Layer(base), Node::Layer(rider)]);
    assert!(close(composite_px(&d, 1, 1), WHITE));
    // Inherit alpha: none either.
    let mut ia = layer("ia", RED);
    ia.inherit_alpha = true;
    ia.set_blend_if(Some(hide));
    let d = doc(vec![Node::Layer(layer("bg", WHITE)), Node::Layer(ia)]);
    assert!(close(composite_px(&d, 1, 1), RED));
}

#[test]
fn adjustment_layers_take_density_and_blend_if() {
    let mut inv = Layer::new("invert");
    inv.adjustment = Some(Box::new(Adjustment { kind: "invert".into(), params: Default::default() }));
    let mut m = Surface::new();
    fill(&mut m, Rect::new(0, 0, 8, 8), BLACK);
    inv.mask = Some(m);
    inv.set_mask_density(0.5);
    let d = doc(vec![Node::Layer(layer("bg", WHITE)), Node::Layer(inv.clone())]);
    let p = composite_px(&d, 1, 1);
    assert!(close(p, [0.5, 0.5, 0.5, 1.0]), "{p:?}");
    // Underlying white hidden by Blend If: the adjustment does nothing.
    inv.set_mask_density(1.0);
    inv.mask = None;
    inv.set_blend_if(Some(bif_gray(FULL_RANGE, [0, 0, 200, 200])));
    let d = doc(vec![Node::Layer(layer("bg", WHITE)), Node::Layer(inv)]);
    assert!(close(composite_px(&d, 1, 1), WHITE));
}

#[test]
fn feather_blurs_the_mask_before_density() {
    // Red over the left half; the mask shows its left half, feathered by 4 px.
    let mut l = Layer::new("red");
    fill(&mut l.pixels, Rect::new(0, 0, 64, 64), RED);
    let mut m = Surface::new();
    fill(&mut m, Rect::new(0, 0, 32, 64), [1.0; 4]);
    fill(&mut m, Rect::new(32, 0, 32, 64), BLACK);
    l.mask = Some(m.clone());
    l.set_mask_feather(4.0);
    let f = onepaint_compose::feather::feathered_tile(&m, 4.0, 0, 0, [64, 64]).unwrap();
    let d = doc(vec![Node::Layer(l.clone())]);
    for x in [24, 30, 31, 32, 33, 40] {
        let a = composite_px(&d, x, 10)[3];
        // The feathered mask is stored as an f16 tile, like any mask.
        assert!((a - f[10 * 256 + x as usize]).abs() < 1e-3, "x {x}: {a}");
    }
    // The edge is soft now: not 1 then 0.
    assert!(composite_px(&d, 31, 10)[3] < 0.9 && composite_px(&d, 32, 10)[3] > 0.1);
    // Density applies to the feathered mask.
    l.set_mask_density(0.5);
    let a = composite_px(&doc(vec![Node::Layer(l)]), 33, 10)[3];
    assert!((a - (1.0 - 0.5 * (1.0 - f[10 * 256 + 33]))).abs() < 1e-3);
}
