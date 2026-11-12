#![allow(clippy::unwrap_used)]
//! Spec 49: 8-bit documents keep their layers as 8-bit tiles.
use onepaint_color::Rgba;
use onepaint_doc::{Depth, Document, Node};
use onepaint_engine::*;
use onepaint_raster::{Store, Surface};
use onepaint_stroke::StrokePoint;

fn session(depth: Depth) -> Session {
    Session::from_document(Document::new_in(600, 400, Some(Rgba::WHITE), depth))
}

fn draw_line(s: &mut Session, y: f64) {
    s.stroke_begin(StrokePoint::at(20.0, y, 1.0, 0.0));
    for i in 1..40 {
        s.stroke_extend(&[StrokePoint::at(20.0 + i as f64 * 13.0, y + (i as f64 * 0.4).sin() * 30.0, 1.0, i as f64 * 8.0)]);
    }
    s.stroke_end();
}

fn stores(s: &Surface) -> Vec<Store> {
    s.tiles().map(|(_, t)| t.store()).collect()
}

/// Every pixel and mask tile of the document is in the store its depth asks for.
fn all_stored(s: &Session, pixels: Store, mask: Store) {
    fn walk(nodes: &[Node], pixels: Store, mask: Store) {
        for n in nodes {
            match n {
                Node::Group(g) => walk(&g.children, pixels, mask),
                Node::Layer(l) => {
                    assert!(stores(&l.pixels).iter().all(|&t| t == pixels), "{}: {:?}", l.name, stores(&l.pixels));
                    if let Some(m) = &l.mask {
                        assert!(stores(m).iter().all(|&t| t == mask), "{} mask: {:?}", l.name, stores(m));
                    }
                }
            }
        }
    }
    walk(&s.doc().root, pixels, mask);
}

#[test]
fn edits_in_an_8bit_document_land_as_8bit_tiles() {
    let mut s = session(Depth::Rgba8);
    all_stored(&s, Store::Unorm8, Store::Unorm8);
    s.execute(Command::SetColor { rgba: Rgba::new(0.9, 0.3, 0.1, 1.0) }).unwrap();
    draw_line(&mut s, 120.0);
    let id = s.active().to_string();
    assert!(s.doc().layer(&id).unwrap().pixels.tile_count() > 0);
    all_stored(&s, Store::Unorm8, Store::Unorm8);
    // Bucket fill, a filter, a mask stroke, a transform and a merge.
    s.execute(Command::Fill { x: 300, y: 350, tolerance: 0.1, grow: 0 }).unwrap();
    s.execute(Command::BeginFilter { kind: "invert".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: onepaint_filters::Params::default() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    s.execute(Command::AddMask).unwrap();
    draw_line(&mut s, 200.0);
    s.execute(Command::BeginTransform { ids: vec![] }).unwrap();
    s.execute(Command::SetTransform { matrix: transform::Affine::translate(17.0, 9.0) }).unwrap();
    s.execute(Command::CommitTransform).unwrap();
    s.execute(Command::MergeDown { id: s.active().to_string() }).unwrap();
    all_stored(&s, Store::Unorm8, Store::Unorm8);
    // Undo and redo restore exactly.
    let after = s.flatten();
    for _ in 0..3 {
        s.execute(Command::Undo).unwrap();
    }
    all_stored(&s, Store::Unorm8, Store::Unorm8);
    for _ in 0..3 {
        s.execute(Command::Redo).unwrap();
    }
    assert_eq!(s.flatten(), after);
}

#[test]
fn float_documents_keep_f16_tiles() {
    let mut s = session(Depth::Rgba16F);
    draw_line(&mut s, 120.0);
    s.execute(Command::AddMask).unwrap();
    draw_line(&mut s, 200.0);
    all_stored(&s, Store::F16, Store::F16);
}

#[test]
fn an_8bit_stroke_core_matches_the_float_one() {
    // Soft edges blend in gamma space in 8-bit documents (spec 50); where the
    // paint is opaque the two agree to the byte.
    let (mut a, mut b) = (session(Depth::Rgba8), session(Depth::Rgba16F));
    for s in [&mut a, &mut b] {
        s.execute(Command::SetColor { rgba: Rgba::new(0.2, 0.5, 0.8, 1.0) }).unwrap();
        draw_line(s, 150.0);
    }
    let (fa, fb) = (a.flatten_linear(), b.flatten_linear());
    let id = b.active().to_string();
    let layer = b.doc().layer(&id).unwrap();
    let mut checked = 0;
    for (i, (p, q)) in fa.chunks_exact(4).zip(fb.chunks_exact(4)).enumerate() {
        let (x, y) = ((i % 600) as i32, (i / 600) as i32);
        if layer.pixels.get(x, y)[3] < 1.0 {
            continue;
        }
        checked += 1;
        let enc = onepaint_color::linear_to_srgb;
        for c in 0..3 {
            assert!((enc(p[c]) - enc(q[c])).abs() <= 1.0 / 255.0, "{x},{y}");
        }
    }
    assert!(checked > 100);
}

#[test]
fn an_8bit_page_takes_half_the_tile_memory() {
    let bytes = |s: &Session| -> usize {
        s.doc()
            .root
            .iter()
            .filter_map(|n| match n {
                Node::Layer(l) => Some(l.pixels.tiles().map(|(_, t)| t.byte_size()).sum::<usize>()),
                _ => None,
            })
            .sum()
    };
    let (mut a, mut b) = (session(Depth::Rgba8), session(Depth::Rgba16F));
    for s in [&mut a, &mut b] {
        for i in 0..10 {
            s.execute(Command::AddLayer).unwrap();
            draw_line(s, 30.0 + i as f64 * 35.0);
        }
    }
    assert_eq!(bytes(&a) * 2, bytes(&b));
}

#[test]
fn an_8bit_document_saves_and_reopens_as_8bit() {
    let mut s = session(Depth::Rgba8);
    draw_line(&mut s, 150.0);
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(10, 10, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.doc().depth, Depth::Rgba8);
    all_stored(&t, Store::Unorm8, Store::Unorm8);
    let (fa, fb) = (s.flatten(), t.flatten());
    let worst = fa.iter().zip(&fb).map(|(p, q)| (p - q).abs()).fold(0.0f32, f32::max);
    assert!(worst <= 1.0 / 255.0, "{worst}");
}
