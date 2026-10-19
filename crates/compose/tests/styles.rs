#![allow(clippy::unwrap_used)]
//! Layer styles (spec 56) in the tree compositor: each effect's pixels at
//! points worked out from happypaint's `STYLE_SRC`.
use onepaint_compose::composite_tile;
use onepaint_compose::program::plan_tile;
use onepaint_doc::styles::{EffectKind, LayerStyles, default_effect};
use onepaint_doc::{BlendMode, Document, Group, Layer, Node};
use onepaint_raster::TILE;

fn square(name: &str, x0: i32, y0: i32, size: i32, c: [f32; 4]) -> Layer {
    let mut l = Layer::new(name);
    for y in y0..y0 + size {
        for x in x0..x0 + size {
            l.pixels.set(x, y, c);
        }
    }
    l
}

fn doc_with(l: Layer) -> Document {
    let mut d = Document::new(300, 300, None);
    d.root = vec![Node::Layer(l)];
    d
}

/// The composite at doc pixel (x, y).
fn px(d: &Document, x: i32, y: i32) -> [f32; 4] {
    let t = composite_tile(d, x.div_euclid(TILE), y.div_euclid(TILE), None);
    let o = ((y.rem_euclid(TILE) * TILE + x.rem_euclid(TILE)) * 4) as usize;
    [t[o], t[o + 1], t[o + 2], t[o + 3]]
}

fn near(a: [f32; 4], b: [f32; 4], tol: f32) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() <= tol)
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

fn styled(effects: Vec<onepaint_doc::styles::Effect>, fill: f32) -> Document {
    let mut l = square("a", 100, 100, 60, RED);
    l.set_styles(Some(LayerStyles { fill_opacity: fill, effects }));
    doc_with(l)
}

fn effect(kind: EffectKind, f: impl FnOnce(&mut onepaint_doc::styles::Effect)) -> onepaint_doc::styles::Effect {
    let mut e = default_effect(kind);
    f(&mut e);
    e
}

#[test]
fn a_colour_overlay_covers_the_pixels() {
    let d = styled(vec![effect(EffectKind::ColorOverlay, |e| e.color = [0.0, 1.0, 0.0])], 1.0);
    assert!(near(px(&d, 130, 130), [0.0, 1.0, 0.0, 1.0], 1e-6));
    assert_eq!(px(&d, 90, 130)[3], 0.0, "nothing outside");
    // At Fill 0 the pixels go and the overlay stays.
    let d = styled(vec![effect(EffectKind::ColorOverlay, |e| e.color = [0.0, 1.0, 0.0])], 0.0);
    assert!(near(px(&d, 130, 130), [0.0, 1.0, 0.0, 1.0], 1e-6));
    assert!(near(styled(vec![], 0.0).pipe(|d| px(&d, 130, 130)), [0.0; 4], 1e-6));
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}
impl<T> Pipe for T {}

#[test]
fn an_outside_stroke_is_its_size_wide() {
    let d = styled(vec![effect(EffectKind::Stroke, |e| (e.size, e.color) = (3.0, [0.0; 3]))], 1.0);
    for x in 97..100 {
        assert!(near(px(&d, x, 130), [0.0, 0.0, 0.0, 1.0], 1e-6), "x {x}: {:?}", px(&d, x, 130));
    }
    assert_eq!(px(&d, 96, 130)[3], 0.0);
    assert!(near(px(&d, 130, 130), RED, 1e-6), "the pixels stay on top");
}

#[test]
fn a_drop_shadow_falls_away_from_the_light() {
    // Light from the right (0°), 10 px, hard edged: the shadow shows 10 px left.
    let d = styled(
        vec![effect(EffectKind::DropShadow, |e| {
            (e.angle, e.distance, e.size, e.use_global_light, e.blend_mode) = (0.0, 10.0, 0.0, false, BlendMode::Normal)
        })],
        1.0,
    );
    assert!(near(px(&d, 90, 130), [0.0, 0.0, 0.0, 0.75], 1e-6), "{:?}", px(&d, 90, 130));
    assert!(near(px(&d, 95, 130), [0.0, 0.0, 0.0, 0.75], 1e-6));
    assert_eq!(px(&d, 89, 130)[3], 0.0);
    assert_eq!(px(&d, 165, 130)[3], 0.0, "nothing on the lit side");
    assert!(near(px(&d, 130, 130), RED, 1e-6), "knocked out under the pixels");
}

#[test]
fn an_inner_shadow_darkens_the_far_edge() {
    // Multiply black, 75%, 5 px from the right: the right edge is 25% red.
    let d =
        styled(vec![effect(EffectKind::InnerShadow, |e| (e.angle, e.distance, e.size, e.use_global_light) = (0.0, 5.0, 0.0, false))], 1.0);
    assert!(near(px(&d, 159, 130), [0.25, 0.0, 0.0, 1.0], 1e-5), "{:?}", px(&d, 159, 130));
    assert!(near(px(&d, 150, 130), RED, 1e-6));
    assert!(near(px(&d, 101, 130), RED, 1e-6), "the lit side is clear");
}

#[test]
fn a_bevel_lights_one_side_and_shades_the_other() {
    let d = styled(vec![effect(EffectKind::Bevel, |e| e.size = 8.0)], 1.0);
    // Light from 120° (upper left): a white highlight top-left, a shadow bottom-right.
    let tl = px(&d, 102, 102);
    let br = px(&d, 157, 157);
    assert!(tl[1] > 0.05 && tl[2] > 0.05, "highlight {tl:?}");
    assert!(br[0] < 0.95, "shadow {br:?}");
    assert!(near(px(&d, 130, 130), RED, 1e-6), "flat in the middle");
}

#[test]
fn effects_cross_tile_seams() {
    // The square spans the seam at x = 256; its stroke is the same either side.
    let mut l = square("a", 230, 100, 60, RED);
    let stroke = effect(EffectKind::Stroke, |e| (e.size, e.color) = (5.0, [0.0; 3]));
    let glow = effect(EffectKind::OuterGlow, |e| e.size = 9.0);
    l.set_styles(Some(LayerStyles { fill_opacity: 1.0, effects: vec![stroke, glow] }));
    let d = doc_with(l);
    for k in 0..12 {
        let (left, right) = (px(&d, 229 - k, 130), px(&d, 290 + k, 130));
        assert!(near(left, right, 1e-6), "{k} px out: {left:?} {right:?}");
    }
}

#[test]
fn a_group_strokes_its_children_together() {
    let mut g = Group::new("g");
    g.children = vec![Node::Layer(square("a", 100, 100, 30, RED)), Node::Layer(square("b", 130, 100, 30, [0.0, 0.0, 1.0, 1.0]))];
    g.set_styles(Some(LayerStyles {
        fill_opacity: 1.0,
        effects: vec![effect(EffectKind::Stroke, |e| (e.size, e.color) = (2.0, [0.0, 1.0, 0.0]))],
    }));
    let mut d = Document::new(300, 300, None);
    d.root = vec![Node::Group(g)];
    assert!(near(px(&d, 98, 110), [0.0, 1.0, 0.0, 1.0], 1e-6), "round the union");
    assert!(near(px(&d, 129, 110), RED, 1e-6) && near(px(&d, 130, 110), [0.0, 0.0, 1.0, 1.0], 1e-6), "not between the children");
    assert!(near(px(&d, 161, 110), [0.0, 1.0, 0.0, 1.0], 1e-6));
}

/// Every effect in its default mode over a backdrop, on a layer and on a
/// group: the planned programs, run on the CPU, match the tree compositor
/// (their content goes through f16, as on the GPU). Families and wide
/// margins stay on the CPU.
fn styled_docs() -> Vec<Document> {
    let all = |k: EffectKind| effect(k, |_| {});
    let mut docs = Vec::new();
    for effects in [
        vec![all(EffectKind::DropShadow), all(EffectKind::Stroke)],
        vec![all(EffectKind::InnerShadow), all(EffectKind::InnerGlow), all(EffectKind::OuterGlow)],
        vec![all(EffectKind::Bevel), all(EffectKind::Satin)],
        vec![all(EffectKind::GradientOverlay), all(EffectKind::PatternOverlay), all(EffectKind::ColorOverlay)],
        vec![effect(EffectKind::Stroke, |e| e.position = "center".into()), effect(EffectKind::Stroke, |e| e.position = "inside".into())],
    ] {
        let mut l = square("a", 180, 100, 60, [0.3, 0.6, 0.2, 1.0]);
        l.set_styles(Some(LayerStyles { fill_opacity: 0.7, effects }));
        let mut d = doc_with(l);
        // Something under it, half see-through, for the modes to blend with.
        d.root.insert(0, Node::Layer(square("under", 150, 80, 120, [0.4, 0.1, 0.3, 0.6])));
        docs.push(d);
    }
    let mut g = Group::new("g");
    g.children = vec![Node::Layer(square("a", 200, 100, 30, RED)), Node::Layer(square("b", 230, 120, 30, [0.0, 0.0, 1.0, 1.0]))];
    g.set_styles(Some(LayerStyles {
        fill_opacity: 1.0,
        effects: vec![effect(EffectKind::OuterGlow, |e| e.size = 14.0), effect(EffectKind::Stroke, |e| e.size = 4.0)],
    }));
    let mut d = Document::new(300, 300, Some(onepaint_color::Rgba::WHITE));
    d.root.push(Node::Group(g));
    docs.push(d);
    docs
}

#[test]
fn styled_programs_match_the_tree_compositor() {
    use onepaint_compose::program::{plan_tile, run};
    for (n, d) in styled_docs().iter().enumerate() {
        for (tx, ty) in d.bounds().tiles() {
            let prog = plan_tile(d, tx, ty, None).unwrap_or_else(|| panic!("doc {n} tile {tx},{ty} plans"));
            let (got, want) = (run(&prog), composite_tile(d, tx, ty, None));
            let worst = got.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst <= 4e-3, "doc {n} tile {tx},{ty}: {worst}");
        }
    }
}

#[test]
fn some_styled_nodes_stay_on_the_cpu() {
    // A clip family on a styled base.
    let mut base = square("a", 100, 100, 60, RED);
    base.set_styles(Some(LayerStyles { fill_opacity: 1.0, effects: vec![default_effect(EffectKind::Stroke)] }));
    let mut rider = square("r", 110, 110, 20, [0.0, 1.0, 0.0, 1.0]);
    rider.clipped = true;
    let mut d = doc_with(base);
    d.root.push(Node::Layer(rider));
    assert!(plan_tile(&d, 0, 0, None).is_none());
    // A margin wider than a tile.
    let d = styled(vec![effect(EffectKind::OuterGlow, |e| e.size = 200.0)], 1.0);
    assert!(plan_tile(&d, 0, 0, None).is_none());
    // Styles that change nothing plan as a plain layer.
    let mut l = square("a", 100, 100, 60, RED);
    l.set_styles(Some(LayerStyles { fill_opacity: 1.0, effects: vec![effect(EffectKind::Stroke, |e| e.enabled = false)] }));
    assert!(plan_tile(&doc_with(l), 0, 0, None).is_some());
}

/// A tile reads the shape from its neighbours as far as the effects reach:
/// a glow from a shape wholly in tile 0 spreads into tile 1 the same as it
/// does on the shape's other side.
#[test]
fn effects_reach_into_tiles_without_the_shape() {
    let mut l = square("a", 180, 100, 60, RED);
    let glow = effect(EffectKind::OuterGlow, |e| (e.size, e.blend_mode) = (30.0, BlendMode::Normal));
    let shadow = effect(EffectKind::DropShadow, |e| {
        (e.angle, e.distance, e.size, e.use_global_light, e.blend_mode) = (90.0, 20.0, 6.0, false, BlendMode::Normal)
    });
    l.set_styles(Some(LayerStyles { fill_opacity: 1.0, effects: vec![glow, shadow] }));
    let d = doc_with(l);
    for k in 0..32 {
        let (left, right) = (px(&d, 179 - k, 130), px(&d, 240 + k, 130));
        assert!(near(left, right, 1e-6), "{k} px out: {left:?} {right:?}");
    }
    assert!(px(&d, 262, 130)[3] > 0.05, "the glow reaches past the seam");
}
