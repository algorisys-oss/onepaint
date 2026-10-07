#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
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

fn layer(name: &str, r: Rect, p: [f32; 4]) -> Layer {
    let mut l = Layer::new(name);
    fill(&mut l.pixels, r, p);
    l
}

fn doc(nodes: Vec<Node>) -> Document {
    let mut d = Document::new(64, 64, None);
    d.root = nodes;
    d
}

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

#[test]
fn opacity_and_visibility() {
    let mut a = layer("a", Rect::new(0, 0, 8, 8), RED);
    a.opacity = 0.5;
    let mut b = layer("b", Rect::new(0, 0, 8, 8), BLUE);
    b.visible = false;
    let d = doc(vec![Node::Layer(a), Node::Layer(b)]);
    assert_eq!(composite_px(&d, 1, 1), [0.5, 0.0, 0.0, 0.5]);
}

#[test]
fn clipped_layer_stays_inside_base_and_ignores_own_mode() {
    let base = layer("base", Rect::new(0, 0, 4, 4), BLUE);
    let mut clip = layer("clip", Rect::new(0, 0, 8, 8), GREEN);
    clip.clipped = true;
    clip.blend_mode = BlendMode::Multiply; // ignored
    let d = doc(vec![Node::Layer(base), Node::Layer(clip)]);
    assert_eq!(composite_px(&d, 1, 1), GREEN);
    assert_eq!(composite_px(&d, 6, 6), [0.0; 4]);
}

#[test]
fn clipped_layer_without_base_renders_normally() {
    let mut clip = layer("clip", Rect::new(0, 0, 8, 8), GREEN);
    clip.clipped = true;
    let d = doc(vec![Node::Layer(clip)]);
    assert_eq!(composite_px(&d, 6, 6), GREEN);
}

#[test]
fn hidden_base_hides_family() {
    let mut base = layer("base", Rect::new(0, 0, 4, 4), BLUE);
    base.visible = false;
    let mut clip = layer("clip", Rect::new(0, 0, 8, 8), GREEN);
    clip.clipped = true;
    let d = doc(vec![Node::Layer(base), Node::Layer(clip)]);
    assert_eq!(composite_px(&d, 1, 1), [0.0; 4]);
}

#[test]
fn groups_are_isolated() {
    let bottom = layer("bottom", Rect::new(0, 0, 8, 8), [0.5, 0.5, 0.5, 1.0]);
    let mut child = layer("child", Rect::new(0, 0, 8, 8), [0.5, 0.0, 0.0, 0.5]);
    child.blend_mode = BlendMode::Multiply;
    let mut g = Group::new("g");
    g.children.push(Node::Layer(child));
    let d = doc(vec![Node::Layer(bottom), Node::Group(g)]);
    // Multiply inside an isolated group sees transparency, so the group
    // result is the child itself, then composited normally.
    let exp = blend::blend_px(
        BlendMode::Normal,
        [0.5, 0.5, 0.5, 1.0],
        [0.5, 0.0, 0.0, 0.5],
        1.0,
    );
    let got = composite_px(&d, 1, 1);
    for i in 0..4 {
        assert!((got[i] - exp[i]).abs() < 1e-3, "{got:?} vs {exp:?}");
    }
}

#[test]
fn mask_scales_coverage() {
    let mut a = layer("a", Rect::new(0, 0, 8, 8), RED);
    let mut m = Surface::new();
    fill(&mut m, Rect::new(0, 0, 8, 8), [0.5, 0.5, 0.5, 1.0]);
    a.mask = Some(m);
    let d = doc(vec![Node::Layer(a.clone())]);
    assert_eq!(composite_px(&d, 1, 1), [0.5, 0.0, 0.0, 0.5]);
    a.mask_enabled = false;
    let d = doc(vec![Node::Layer(a)]);
    assert_eq!(composite_px(&d, 1, 1), RED);
}

#[test]
fn inherit_alpha_uses_backdrop_alpha() {
    let bottom = layer("bottom", Rect::new(0, 0, 4, 4), BLUE);
    let mut top = layer("top", Rect::new(0, 0, 8, 8), RED);
    top.inherit_alpha = true;
    let d = doc(vec![Node::Layer(bottom), Node::Layer(top)]);
    assert_eq!(composite_px(&d, 1, 1), RED);
    assert_eq!(composite_px(&d, 6, 6), [0.0; 4]);
}

#[test]
fn background_document_composites_white() {
    let d = Document::new(300, 300, Some(Rgba::WHITE));
    let p = composite_px(&d, 280, 280);
    assert!((p[0] - 1.0).abs() < 1e-3 && (p[3] - 1.0).abs() < 1e-3);
}

#[test]
fn tile_composite_matches_full_flatten() {
    let mut a = layer("a", Rect::new(10, 10, 300, 20), RED);
    a.opacity = 0.7;
    let mut b = layer("b", Rect::new(250, 0, 20, 300), BLUE);
    b.blend_mode = BlendMode::Screen;
    let mut d = Document::new(320, 300, Some(Rgba::WHITE));
    d.root.push(Node::Layer(a));
    d.root.push(Node::Layer(b));
    let full = flatten(&d, None);
    let tiles = composite_tiles(&d, &[(0, 0), (1, 0), (0, 1), (1, 1)], None);
    for ((tx, ty), t) in tiles {
        for (x, y) in [(5, 5), (100, 15), (255, 15), (260, 200), (10, 299)] {
            if x / 256 == tx && y / 256 == ty {
                let tp = t.get((x - tx * 256) as usize, (y - ty * 256) as usize);
                let i = ((y * 320 + x) * 4) as usize;
                for c in 0..4 {
                    assert!((tp[c] - full[i + c]).abs() < 2e-3, "({x},{y}) {tp:?}");
                }
            }
        }
    }
}
