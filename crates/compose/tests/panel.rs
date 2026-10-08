#![allow(clippy::unwrap_used)]
//! Panel groups clip their composite to the frame (spec 30, compositor.ts:1540-1548).

use onepaint_compose::*;
use onepaint_doc::*;
use onepaint_panel::geometry::{Pt, rect_outline};
use onepaint_panel::{PanelSpec, set_panel};
use onepaint_raster::{Rect, Surface};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

fn fill(s: &mut Surface, r: Rect, p: [f32; 4]) {
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            s.set(x, y, p);
        }
    }
}

fn panel_group(a: Pt, b: Pt, gutter: f64, art: [f32; 4]) -> Group {
    let mut g = Group::new("Panel");
    set_panel(&mut g, Some(PanelSpec::new(rect_outline(a, b), gutter, 0.0)));
    let mut l = Layer::new("Art");
    fill(&mut l.pixels, Rect::new(0, 0, 600, 300), art);
    g.children.push(Node::Layer(l));
    g
}

fn doc(nodes: Vec<Node>) -> Document {
    let mut d = Document::new(600, 300, None);
    d.root = nodes;
    d
}

#[test]
fn a_panel_group_is_clipped_to_its_inset_frame() {
    // Art covers the whole page; the frame is 100..500 × 50..250 with a 20 px gutter.
    let g = panel_group(Pt::new(100.0, 50.0), Pt::new(500.0, 250.0), 20.0, RED);
    let d = doc(vec![Node::Group(g)]);
    assert_eq!(composite_px(&d, 300, 150), RED);
    assert_eq!(composite_px(&d, 50, 150), [0.0; 4], "outside the frame");
    assert_eq!(composite_px(&d, 105, 150), [0.0; 4], "in the half gutter");
    assert_eq!(composite_px(&d, 115, 150), RED, "past the half gutter");
    // The tile past x = 512 is entirely outside: the fast path skips it.
    assert_eq!(composite_px(&d, 550, 150), [0.0; 4]);
}

#[test]
fn the_edge_is_antialiased() {
    let g = panel_group(Pt::new(100.5, 50.0), Pt::new(500.0, 250.0), 0.0, RED);
    let d = doc(vec![Node::Group(g)]);
    let edge = composite_px(&d, 100, 150);
    assert!((edge[3] - 128.0 / 255.0).abs() < 1e-3, "{edge:?}");
}

#[test]
fn riders_go_on_top_of_the_clipped_group() {
    // A layer clipped to the panel group rides the framed composite.
    let g = panel_group(Pt::new(100.0, 50.0), Pt::new(500.0, 250.0), 0.0, RED);
    let mut rider = Layer::new("tone");
    fill(&mut rider.pixels, Rect::new(0, 0, 600, 300), GREEN);
    rider.clipped = true;
    let d = doc(vec![Node::Group(g), Node::Layer(rider)]);
    assert_eq!(composite_px(&d, 300, 150), GREEN);
    assert_eq!(composite_px(&d, 50, 150), [0.0; 4]);
}

#[test]
fn a_plain_group_is_not_clipped() {
    let mut g = panel_group(Pt::new(100.0, 50.0), Pt::new(500.0, 250.0), 0.0, RED);
    set_panel(&mut g, None);
    let d = doc(vec![Node::Group(g)]);
    assert_eq!(composite_px(&d, 50, 150), RED);
}

#[test]
fn a_panel_with_no_interior_does_not_clip() {
    let g = panel_group(Pt::new(100.0, 50.0), Pt::new(500.0, 70.0), 80.0, RED);
    let d = doc(vec![Node::Group(g)]);
    assert_eq!(composite_px(&d, 50, 150), RED);
}

#[test]
fn nested_panel_groups_clip_too() {
    let inner = panel_group(Pt::new(100.0, 50.0), Pt::new(500.0, 250.0), 0.0, RED);
    let mut outer = Group::new("Page");
    outer.children.push(Node::Group(inner));
    let d = doc(vec![Node::Group(outer)]);
    assert_eq!(composite_px(&d, 50, 150), [0.0; 4]);
    assert_eq!(composite_px(&d, 300, 150), RED);
}
