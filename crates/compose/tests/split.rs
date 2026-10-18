#![allow(clippy::unwrap_used)]
//! Spec 38: a tile program split at the hot layer composites exactly like
//! the unsplit one, and its segment keys change only when their content does.
use onepaint_compose::program::{Op, TileProgram, plan_tile, plan_tile_split, run, run_split};
use onepaint_compose::testdocs::{random_doc, random_stroke};
use onepaint_compose::{StrokeOverlay, composite_tile};
use onepaint_doc::{BlendMode, Document, Group, Layer, Node};
use onepaint_raster::{TILE, Tile};
use std::sync::Arc;

const TILES: [(i32, i32); 4] = [(0, 0), (1, 0), (0, 1), (1, 1)];

fn close(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-5)
}

/// Every layer id in the tree, depth first.
fn all_layers(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        match n {
            Node::Layer(l) => out.push(l.id.clone()),
            Node::Group(g) => all_layers(&g.children, out),
        }
    }
}

fn sources(ops: &[Op]) -> usize {
    TileProgram { tx: 0, ty: 0, ops: ops.to_vec(), split: None, space: Default::default() }.sources().count()
}

#[test]
fn split_equals_unsplit_for_every_hot_layer() {
    for seed in 0..16 {
        let doc = random_doc(seed, 300, 300, true);
        let mut ids = Vec::new();
        all_layers(&doc.root, &mut ids);
        for hot in &ids {
            for (tx, ty) in TILES {
                let prog = plan_tile_split(&doc, tx, ty, None, Some(hot)).unwrap();
                let want = composite_tile(&doc, tx, ty, None);
                assert_eq!(run(&prog), want, "seed {seed}: the full ops are untouched");
                assert!(close(&run_split(&prog), &want), "seed {seed} hot {hot} tile {tx},{ty}");
            }
        }
    }
}

#[test]
fn split_equals_unsplit_with_a_stroke_on_the_hot_layer() {
    for seed in 0..40 {
        let doc = random_doc(seed, 300, 300, false);
        let (buf, id, opacity, mode, mask) = random_stroke(seed, &doc).unwrap();
        let ov = StrokeOverlay { layer_id: &id, stroke: &buf, opacity, mode, mask };
        for (tx, ty) in TILES {
            let prog = plan_tile_split(&doc, tx, ty, Some(&ov), Some(&id)).unwrap();
            assert!(close(&run_split(&prog), &composite_tile(&doc, tx, ty, Some(&ov))), "seed {seed} tile {tx},{ty}");
        }
    }
}

// ── Hand-built documents for the folding rules ──

fn solid(c: [f32; 4]) -> Arc<Tile> {
    let mut t = Tile::default();
    for y in 0..TILE as usize {
        for x in 0..TILE as usize {
            t.set(x, y, c);
        }
    }
    Arc::new(t)
}

fn layer(name: &str, c: [f32; 4], mode: BlendMode) -> Layer {
    let mut l = Layer::new(name);
    l.pixels.set_tile(0, 0, Some(solid(c)));
    l.blend_mode = mode;
    l
}

/// Bottom to top: three Normal layers, the hot layer, then `above`.
fn stack(above: Vec<Node>) -> Document {
    let mut d = Document::new(256, 256, None);
    for i in 0..3 {
        d.root.push(Node::Layer(layer(&format!("b{i}"), [0.1 * i as f32, 0.2, 0.3, 0.5], BlendMode::Normal)));
    }
    d.root.push(Node::Layer(layer("hot", [0.4, 0.1, 0.1, 0.6], BlendMode::Normal)));
    d.root.extend(above);
    d
}

/// A split's parts as op lists, for asserting on.
struct Parts {
    prefix: Option<Seg>,
    hot: Vec<Op>,
    suffix: Option<Seg>,
    tail: Vec<Op>,
}

struct Seg {
    key: u64,
    ops: Vec<Op>,
}

fn split_of(d: &Document, hot: &str) -> Parts {
    let id = find(d, hot);
    let p = plan_tile_split(d, 0, 0, None, Some(&id)).unwrap();
    assert!(close(&run_split(&p), &composite_tile(d, 0, 0, None)));
    let s = p.split.clone().unwrap();
    let seg = |g: Option<onepaint_compose::program::Segment>| g.map(|g| Seg { key: g.key, ops: p.at(&g.range).to_vec() });
    Parts { prefix: seg(s.prefix), hot: p.at(&s.hot).to_vec(), suffix: seg(s.suffix), tail: p.at(&s.tail).to_vec() }
}

fn find(d: &Document, name: &str) -> String {
    fn walk(nodes: &[Node], name: &str) -> Option<String> {
        nodes.iter().find_map(|n| match n {
            Node::Layer(l) if l.name == name => Some(l.id.clone()),
            Node::Group(g) if g.name == name => Some(g.id.clone()),
            Node::Group(g) => walk(&g.children, name),
            _ => None,
        })
    }
    walk(&d.root, name).unwrap()
}

fn normal(name: &str) -> Node {
    Node::Layer(layer(name, [0.2, 0.3, 0.1, 0.4], BlendMode::Normal))
}

#[test]
fn normal_layers_above_fold_into_one_image() {
    let d = stack(vec![normal("a0"), normal("a1")]);
    let s = split_of(&d, "hot");
    assert_eq!(s.prefix.as_ref().map(|p| sources(&p.ops)), Some(3));
    assert_eq!(sources(&s.hot), 1);
    assert_eq!(s.suffix.as_ref().map(|p| sources(&p.ops)), Some(2));
    assert!(s.tail.is_empty());
}

#[test]
fn a_non_normal_layer_above_stops_the_fold() {
    let multiply = Node::Layer(layer("m", [0.5, 0.5, 0.5, 0.5], BlendMode::Multiply));
    let d = stack(vec![normal("a0"), normal("a1"), multiply, normal("a2")]);
    let s = split_of(&d, "hot");
    assert_eq!(s.suffix.as_ref().map(|p| sources(&p.ops)), Some(2), "a0 and a1 fold");
    assert_eq!(sources(&s.tail), 2, "the multiply layer and everything after it run each frame");
}

#[test]
fn inherit_alpha_above_stops_the_fold() {
    let mut ia = layer("ia", [0.5, 0.5, 0.5, 0.5], BlendMode::Normal);
    ia.inherit_alpha = true;
    let d = stack(vec![Node::Layer(ia), normal("a0"), normal("a1")]);
    let s = split_of(&d, "hot");
    assert!(s.suffix.is_none());
    assert_eq!(sources(&s.tail), 3);
}

#[test]
fn isolated_normal_groups_fold_and_screen_groups_do_not() {
    let group = |name: &str, mode| {
        let mut g = Group::new(name);
        g.blend_mode = mode;
        g.children =
            vec![normal(&format!("{name}-0")), Node::Layer(layer(&format!("{name}-1"), [0.3, 0.3, 0.3, 0.3], BlendMode::Multiply))];
        Node::Group(g)
    };
    let d = stack(vec![group("gn", BlendMode::Normal), normal("a0")]);
    let s = split_of(&d, "hot");
    assert_eq!(s.suffix.as_ref().map(|p| sources(&p.ops)), Some(3));
    assert!(s.tail.is_empty());
    let d = stack(vec![group("gs", BlendMode::Screen), normal("a0")]);
    let s = split_of(&d, "hot");
    assert!(s.suffix.is_none());
    assert_eq!(sources(&s.tail), 3);
}

#[test]
fn a_clip_family_with_a_normal_base_folds() {
    let mut rider = layer("rider", [0.9, 0.1, 0.1, 1.0], BlendMode::Normal);
    rider.clipped = true;
    let d = stack(vec![normal("base"), Node::Layer(rider)]);
    let s = split_of(&d, "hot");
    assert_eq!(s.suffix.as_ref().map(|p| sources(&p.ops)), Some(2));
}

#[test]
fn segments_reading_fewer_than_two_sources_are_not_cached() {
    let mut d = Document::new(256, 256, None);
    d.root.push(normal("b0"));
    d.root.push(Node::Layer(layer("hot", [0.4, 0.1, 0.1, 0.6], BlendMode::Normal)));
    d.root.push(normal("a0"));
    let s = split_of(&d, "hot");
    assert!(s.prefix.is_none() && s.suffix.is_none());
    assert_eq!(sources(&s.hot), 2, "the lone prefix layer runs with the hot ops");
    assert_eq!(sources(&s.tail), 1);
}

#[test]
fn the_hot_unit_is_the_top_level_group_or_clip_family() {
    let mut g = Group::new("g");
    g.children = vec![normal("g0"), Node::Layer(layer("inner", [0.4, 0.1, 0.1, 0.6], BlendMode::Normal)), normal("g1")];
    let d = stack(vec![Node::Group(g), normal("a0"), normal("a1")]);
    let s = split_of(&d, "inner");
    assert_eq!(sources(&s.hot), 3, "the whole group runs each frame");
    assert_eq!(s.prefix.as_ref().map(|p| sources(&p.ops)), Some(4));
    // A clipped layer: its family is the hot unit.
    let mut rider = layer("rider", [0.9, 0.1, 0.1, 1.0], BlendMode::Normal);
    rider.clipped = true;
    let d = stack(vec![Node::Layer(rider), normal("a0"), normal("a1")]);
    let s = split_of(&d, "rider");
    assert_eq!(sources(&s.hot), 2, "base and rider");
    assert_eq!(s.prefix.as_ref().map(|p| sources(&p.ops)), Some(3));
}

#[test]
fn keys_change_only_with_their_segment() {
    let base = stack(vec![normal("a0"), normal("a1")]);
    let keys = |d: &Document| {
        let s = split_of(d, "hot");
        (s.prefix.unwrap().key, s.suffix.unwrap().key)
    };
    let k0 = keys(&base);
    assert_eq!(keys(&base), k0, "stable");
    let set = |d: &mut Document, name: &str, f: &dyn Fn(&mut Layer)| {
        for n in &mut d.root {
            if let Node::Layer(l) = n
                && l.name == name
            {
                f(l);
            }
        }
    };
    // The hot layer's opacity, mode and pixels: neither key moves.
    let mut d = base.clone();
    set(&mut d, "hot", &|l| {
        l.opacity = 0.3;
        l.blend_mode = BlendMode::Screen;
        l.pixels.set_tile(0, 0, Some(solid([0.0, 0.5, 0.0, 0.5])));
    });
    assert_eq!(keys(&d), k0);
    // Anything below: the prefix key only.
    for f in
        [&(|l: &mut Layer| l.opacity = 0.5) as &dyn Fn(&mut Layer), &|l: &mut Layer| l.blend_mode = BlendMode::Overlay, &|l: &mut Layer| {
            l.pixels.set_tile(0, 0, Some(solid([0.0, 0.0, 0.5, 0.5])))
        }]
    {
        let mut d = base.clone();
        set(&mut d, "b1", f);
        let k = keys(&d);
        assert!(k.0 != k0.0 && k.1 == k0.1);
    }
    // Hiding a layer below changes the prefix key; the same rules hold above.
    let mut d = base.clone();
    set(&mut d, "b2", &|l| l.visible = false);
    assert_ne!(keys(&d).0, k0.0);
    let mut d = base.clone();
    set(&mut d, "a1", &|l| l.opacity = 0.5);
    let k = keys(&d);
    assert!(k.0 == k0.0 && k.1 != k0.1);
    // Moving a layer across the split changes both.
    let mut d = base.clone();
    let i = d.root.iter().position(|n| n.name() == "b0").unwrap();
    let b0 = d.root.remove(i);
    d.root.push(b0);
    let k = keys(&d);
    assert!(k.0 != k0.0 && k.1 != k0.1, "{k0:?} {k:?}");
}

#[test]
fn no_hot_layer_means_no_split() {
    let d = stack(vec![normal("a0")]);
    assert!(plan_tile(&d, 0, 0, None).unwrap().split.is_none());
    assert!(plan_tile_split(&d, 0, 0, None, Some("not-a-layer")).unwrap().split.is_none());
}

#[test]
fn an_adjustment_at_or_above_the_hot_layer_keeps_the_tile_unsplit() {
    use onepaint_compose::testdocs::{Rng, random_adjustment};
    use onepaint_doc::Layer;
    let mut doc = random_doc(7, 300, 300, false);
    // Blend If keeps tiles unsplit too, and a vector mask may empty the
    // layer here: this is about adjustments only.
    fn plain(nodes: &mut [Node]) {
        for n in nodes {
            match n {
                Node::Layer(l) => {
                    l.set_blend_if(None);
                    l.set_vector_mask(None);
                }
                Node::Group(g) => plain(&mut g.children),
            }
        }
    }
    plain(&mut doc.root);
    let below = doc.root.iter().find_map(|n| matches!(n, Node::Layer(_)).then(|| n.id().to_string())).unwrap();
    let mut adj = Layer::new("adj");
    adj.adjustment = Some(Box::new(random_adjustment(&mut Rng::new(3))));
    let adj_id = adj.id.clone();
    doc.root.push(Node::Layer(adj));
    for hot in [&below, &adj_id] {
        let p = plan_tile_split(&doc, 0, 0, None, Some(hot)).unwrap();
        assert!(p.split.is_none(), "hot {hot}");
    }
    // Below the hot layer it is part of the cached image, as any layer is.
    doc.root.rotate_right(1);
    let p = plan_tile_split(&doc, 0, 0, None, Some(&below)).unwrap();
    assert!(p.split.is_some());
}
