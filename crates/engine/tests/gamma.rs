#![allow(clippy::unwrap_used)]
//! Spec 50: 8-bit documents hold sRGB values and blend them as happypaint does.
use onepaint_color::Rgba;
use onepaint_doc::{BlendMode, Depth, Document, Layer, Node};
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn session(depth: Depth, bg: Option<Rgba>) -> Session {
    Session::from_document(Document::new_in(300, 200, bg, depth))
}

fn filled(name: &str, c: Rgba, depth: Depth) -> Layer {
    let mut l = Layer::new(name);
    let space = if depth == Depth::Rgba8 { onepaint_color::Space::Gamma } else { onepaint_color::Space::Linear };
    l.pixels = Document::filled(300, 200, space.pixel(c));
    l
}

#[test]
fn grey_at_half_opacity_over_white_blends_in_gamma_space() {
    for (depth, want) in [(Depth::Rgba8, 0.75), (Depth::Rgba16F, 0.80)] {
        let mut d = Document::new_in(300, 200, Some(Rgba::WHITE), depth);
        let mut l = filled("grey", Rgba::new(0.5, 0.5, 0.5, 1.0), depth);
        l.opacity = 0.5;
        d.root.push(Node::Layer(l));
        let s = Session::from_document(d);
        let c = s.sample_color(10, 10).unwrap();
        assert!((c.r - want).abs() < 0.006, "{depth:?}: {}", c.r);
    }
}

#[test]
fn multiply_adds_nothing_over_transparency_in_8bit_documents() {
    for (depth, shows) in [(Depth::Rgba8, false), (Depth::Rgba16F, true)] {
        let mut d = Document::new_in(300, 200, None, depth);
        let mut l = filled("ink", Rgba::new(0.2, 0.4, 0.6, 1.0), depth);
        l.blend_mode = BlendMode::Multiply;
        d.root.push(Node::Layer(l));
        let s = Session::from_document(d);
        assert_eq!(s.sample_color(5, 5).is_some(), shows, "{depth:?}");
    }
}

#[test]
fn a_grey_brush_stores_reads_exports_and_saves_as_128() {
    let mut s = session(Depth::Rgba8, Some(Rgba::WHITE));
    s.execute(Command::SetColor { rgba: Rgba::from_hex("#808080").unwrap() }).unwrap();
    s.stroke_begin(StrokePoint::at(40.0, 100.0, 1.0, 0.0));
    for i in 1..20 {
        s.stroke_extend(&[StrokePoint::at(40.0 + i as f64 * 10.0, 100.0, 1.0, i as f64 * 8.0)]);
    }
    s.stroke_end();
    let id = s.active().to_string();
    let p = s.doc().layer(&id).unwrap().pixels.get(150, 100);
    assert_eq!(((p[0] * 255.0).round(), (p[3] * 255.0).round()), (128.0, 255.0), "{p:?}");
    assert_eq!(s.sample_color(150, 100).unwrap().to_hex(), "#808080");
    let png = s.export_image(&Default::default()).unwrap();
    let img = onepaint_formats::decode_image(&png).unwrap();
    let i = ((100 * img.width + 150) * 4) as usize;
    assert_eq!(&img.rgba[i..i + 4], &[128, 128, 128, 255]);
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(10, 10, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.doc().layer(&id).unwrap().pixels.get(150, 100), p);
}

#[test]
fn pasting_between_depths_keeps_the_colour() {
    let mut a = session(Depth::Rgba8, None);
    a.execute(Command::SetColor { rgba: Rgba::from_hex("#40a0c0").unwrap() }).unwrap();
    a.execute(Command::FillSelection).unwrap();
    let region = a.copy_region().unwrap();
    let mut b = session(Depth::Rgba16F, None);
    b.paste_region(&region);
    assert_eq!(b.sample_color(20, 20).unwrap().to_hex(), "#40a0c0");
}
