#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_doc::*;

#[test]
fn new_document_has_background_and_layer1() {
    let d = Document::new(300, 200, Some(Rgba::WHITE));
    assert_eq!(d.root.len(), 2);
    let bg = d.root[0].as_layer().unwrap();
    assert_eq!(bg.name, "Background");
    let p = bg.pixels.get(299, 199);
    assert!((p[3] - 1.0).abs() < 1e-3 && (p[0] - 1.0).abs() < 1e-3);
    // Background is clipped to the canvas.
    assert_eq!(bg.pixels.get(300, 0)[3], 0.0);
    assert_eq!(d.root[1].as_layer().unwrap().name, "Layer 1");
    assert_eq!(d.dpi, 300.0);

    let t = Document::new(10, 10, None);
    assert_eq!(t.root.len(), 1);
    assert_eq!(t.root[0].name(), "Layer 1");
}

#[test]
fn blend_mode_strings() {
    for m in BlendMode::ALL {
        assert_eq!(BlendMode::parse(m.as_str()), m);
    }
    assert_eq!(BlendMode::parse("color-dodge"), BlendMode::ColorDodge);
    assert_eq!(BlendMode::parse("nonsense"), BlendMode::Normal);
    assert_eq!(BlendMode::ALL.len(), 20);
    assert!(!BlendMode::LAYER_MODES.contains(&BlendMode::Pigment));
}

#[test]
fn tree_find_insert_remove() {
    let mut d = Document::new(10, 10, None);
    let l1 = d.root[0].id().to_string();
    let g = Node::Group(Group::new("G"));
    let gid = g.id().to_string();
    d.insert(None, 1, g);
    let l2 = Node::Layer(Layer::new("L2"));
    let l2id = l2.id().to_string();
    d.insert(Some(&gid), 0, l2);
    assert_eq!(d.path_of(&l2id), Some(vec![1, 0]));
    assert_eq!(d.parent_of(&l2id), Some(Some(gid.clone())));
    assert_eq!(d.parent_of(&l1), Some(None));
    assert!(d.find(&l2id).is_some());
    let (node, parent, idx) = d.remove(&l2id).unwrap();
    assert_eq!((parent, idx), (Some(gid.clone()), 0));
    assert_eq!(node.id(), l2id);
    assert!(d.find(&l2id).is_none());
    let ids: Vec<_> = d.layer_ids();
    assert_eq!(ids, vec![l1]);
}

#[test]
fn ids_are_unique() {
    let a = Layer::new("a");
    let b = Layer::new("b");
    assert_ne!(a.id, b.id);
}
