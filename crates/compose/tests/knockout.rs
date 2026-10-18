#![allow(clippy::unwrap_used)]
//! Knockout (happypaint v0.202.2): a layer's shape punches through what is
//! under it to the Background (top level), a Pass Through group's backdrop,
//! or transparency (an isolated group), and then the layer draws as usual.
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

fn background(locked: bool) -> Node {
    let mut l = layer("Background", WHITE);
    l.locked = locked;
    Node::Layer(l)
}

/// Blue at 50%, knocked out.
fn hole(k: Knockout) -> Node {
    let mut l = layer("hole", BLUE);
    l.opacity = 0.5;
    l.set_knockout(Some(k));
    Node::Layer(l)
}

fn doc(nodes: Vec<Node>) -> Document {
    let mut d = Document::new(64, 64, None);
    d.root = nodes;
    d
}

fn group(name: &str, mode: BlendMode, children: Vec<Node>) -> Node {
    let mut g = Group::new(name);
    g.blend_mode = mode;
    g.children = children;
    Node::Group(g)
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5)
}

const WHITE: [f32; 4] = [1.0; 4];
const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

#[test]
fn the_key_round_trips_and_none_drops_it() {
    let mut l = Layer::new("l");
    assert_eq!(l.knockout(), None);
    l.set_knockout(Some(Knockout::Deep));
    assert_eq!(l.extra.get(KNOCKOUT_KEY).and_then(|v| v.as_str()), Some("deep"));
    assert_eq!(l.knockout(), Some(Knockout::Deep));
    l.set_knockout(None);
    assert!(!l.extra.contains_key(KNOCKOUT_KEY));
}

#[test]
fn top_level_reveals_a_locked_background() {
    // Green over white, then the hole: mix(green, white, 0.5), blue at 50% on that.
    let d = doc(vec![background(true), Node::Layer(layer("green", GREEN)), hole(Knockout::Shallow)]);
    assert!(close(composite_px(&d, 1, 1), [0.25, 0.5, 0.75, 1.0]), "{:?}", composite_px(&d, 1, 1));
    // Deep is the same at the top level.
    let d = doc(vec![background(true), Node::Layer(layer("green", GREEN)), hole(Knockout::Deep)]);
    assert!(close(composite_px(&d, 1, 1), [0.25, 0.5, 0.75, 1.0]));
    // Outside the hole's shape nothing changes.
    assert!(close(composite_px(&d, 20, 20), [0.0; 4]));
}

#[test]
fn top_level_reveals_transparency_without_a_background() {
    // An unlocked bottom layer isn't a Background: mix(green, 0, 0.5) + blue.
    let d = doc(vec![background(false), Node::Layer(layer("green", GREEN)), hole(Knockout::Deep)]);
    assert!(close(composite_px(&d, 1, 1), [0.0, 0.25, 0.5, 0.75]), "{:?}", composite_px(&d, 1, 1));
    // Nor is a locked one with a layer clipped to it (happypaint snapshots at node 1).
    let mut clipped = layer("clipped", GREEN);
    clipped.clipped = true;
    clipped.opacity = 0.0;
    let d = doc(vec![background(true), Node::Layer(clipped), Node::Layer(layer("green", GREEN)), hole(Knockout::Deep)]);
    assert!(close(composite_px(&d, 1, 1), [0.0, 0.25, 0.5, 0.75]));
}

#[test]
fn an_isolated_group_punches_to_transparency() {
    // Inside the group: green, punched to 0 by half, blue on top; the group
    // then lands on the white Background.
    let d = doc(vec![background(true), group("g", BlendMode::Normal, vec![Node::Layer(layer("green", GREEN)), hole(Knockout::Deep)])]);
    let inner = [0.0, 0.25, 0.5, 0.75];
    let want = [inner[0] + 0.25, inner[1] + 0.25, inner[2] + 0.25, 1.0];
    assert!(close(composite_px(&d, 1, 1), want), "{:?}", composite_px(&d, 1, 1));
}

#[test]
fn a_pass_through_group_punches_to_its_backdrop() {
    // The hole reveals the backdrop at the group's start (green), not the
    // Background: mix(white, green, 0.5), then blue at 50%.
    let inner = vec![Node::Layer(layer("white", WHITE)), hole(Knockout::Shallow)];
    let d = doc(vec![background(false), Node::Layer(layer("green", GREEN)), group("g", BlendMode::PassThrough, inner)]);
    assert!(close(composite_px(&d, 1, 1), [0.25, 0.5, 0.75, 1.0]), "{:?}", composite_px(&d, 1, 1));
    // Below 100% the group still fades against that backdrop.
    let inner = vec![Node::Layer(layer("white", WHITE)), hole(Knockout::Shallow)];
    let mut g = group("g", BlendMode::PassThrough, inner);
    let Node::Group(gg) = &mut g else { unreachable!() };
    gg.opacity = 0.5;
    let d = doc(vec![Node::Layer(layer("green", GREEN)), g]);
    let full = [0.25, 0.5, 0.75, 1.0];
    let want: [f32; 4] = std::array::from_fn(|c| GREEN[c] * 0.5 + full[c] * 0.5);
    assert!(close(composite_px(&d, 1, 1), want), "{:?}", composite_px(&d, 1, 1));
}

#[test]
fn the_mask_shapes_the_hole_and_a_hidden_layer_punches_nothing() {
    let mut h = layer("hole", BLUE);
    h.opacity = 0.5;
    h.set_knockout(Some(Knockout::Deep));
    let mut m = Surface::new();
    fill(&mut m, Rect::new(0, 0, 4, 8), [1.0; 4]);
    h.mask = Some(m);
    let d = doc(vec![background(false), Node::Layer(layer("green", GREEN)), Node::Layer(h.clone())]);
    assert!(close(composite_px(&d, 1, 1), [0.0, 0.25, 0.5, 0.75]));
    // Masked out: plain green.
    assert!(close(composite_px(&d, 6, 1), GREEN));
    h.visible = false;
    let d = doc(vec![background(false), Node::Layer(layer("green", GREEN)), Node::Layer(h)]);
    assert!(close(composite_px(&d, 1, 1), GREEN));
}

#[test]
fn the_punch_ignores_the_blend_mode() {
    // A Multiply hole still punches by its shape, then multiplies onto it.
    let mut h = layer("hole", BLUE);
    h.blend_mode = BlendMode::Multiply;
    h.set_knockout(Some(Knockout::Shallow));
    let d = doc(vec![background(true), Node::Layer(layer("green", GREEN)), Node::Layer(h)]);
    // The hole reveals white fully (alpha 1, opacity 1); white × blue = blue.
    assert!(close(composite_px(&d, 1, 1), BLUE), "{:?}", composite_px(&d, 1, 1));
}

#[test]
fn tile_programs_match_the_oracle() {
    use onepaint_compose::program::{Op, PlanCache, plan_tiles_cached, run, run_split};
    // An opaque layer over the Background can't cut it away: the hole still
    // reveals it. Split at the hole, the program still runs whole.
    let mut green = Layer::new("green");
    fill(&mut green.pixels, Rect::new(0, 0, 256, 256), GREEN);
    let mut d = doc(vec![background(true), Node::Layer(green), hole(Knockout::Deep)]);
    let hot = d.layer_ids()[2].clone();
    let cache = PlanCache::default();
    for pass in 0..2 {
        let (progs, cpu) = plan_tiles_cached(&d, &[(0, 0)], None, Some(&hot), Some(&cache));
        assert!(cpu.is_empty());
        let p = &progs[0];
        assert!(p.ops.iter().any(|o| matches!(o, Op::Snapshot)), "pass {pass}");
        assert!(p.split.is_none());
        let want = composite_tile(&d, 0, 0, None);
        for got in [run(p), run_split(p)] {
            assert!(want.iter().zip(&got).all(|(a, b)| (a - b).abs() < 1e-5), "pass {pass}");
        }
    }
    // Without a Background the hole is transparent and the split stays.
    let Node::Layer(b) = &mut d.root[0] else { unreachable!() };
    b.locked = false;
    let (progs, _) = plan_tiles_cached(&d, &[(0, 0)], None, Some(&hot), Some(&cache));
    assert!(!progs[0].ops.iter().any(|o| matches!(o, Op::Snapshot)));
    let want = composite_tile(&d, 0, 0, None);
    for got in [run(&progs[0]), run_split(&progs[0])] {
        assert!(want.iter().zip(&got).all(|(a, b)| (a - b).abs() < 1e-5));
    }
}
