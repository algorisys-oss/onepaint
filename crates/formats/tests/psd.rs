#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_doc::*;
use onepaint_formats::psd::*;
use onepaint_raster::Surface;

fn fill(s: &mut Surface, x0: i32, y0: i32, w: i32, h: i32, p: [f32; 4]) {
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            s.set(x, y, p);
        }
    }
}

#[test]
fn packbits_round_trip_and_runs() {
    let rows: Vec<Vec<u8>> = vec![vec![], vec![7], vec![1, 2], vec![5; 300], (0..=255).collect(), vec![1, 1, 1, 2, 3, 3, 3, 3, 4, 5, 5]];
    for r in &rows {
        assert_eq!(&unpackbits(&packbits(r), r.len()).unwrap(), r, "{r:?}");
    }
    assert!(packbits(&[9; 128]).len() == 2, "a 128-byte run is one repeat packet");
    assert_eq!(packbits(&[1, 1, 1]), vec![0xfe, 1]);
}

#[test]
fn round_trip_through_our_writer_and_reader() {
    let mut d = Document::new(80, 60, Some(Rgba::WHITE));
    d.dpi = 144.0;
    let mut g = Group::new("Gruppe ✓");
    g.blend_mode = BlendMode::Screen;
    g.opacity = 0.75;
    let mut a = Layer::new("Ä layer");
    fill(&mut a.pixels, 10, 10, 20, 15, Rgba::new(1.0, 0.5, 0.0, 1.0).to_linear_premul());
    a.blend_mode = BlendMode::Multiply;
    a.visible = false;
    let mut m = Surface::new();
    fill(&mut m, 0, 0, 80, 60, [1.0, 1.0, 1.0, 1.0]);
    fill(&mut m, 0, 0, 15, 60, [0.0, 0.0, 0.0, 1.0]);
    a.mask = Some(m);
    a.mask_enabled = false;
    let mut c = Layer::new("Clipped");
    c.clipped = true;
    c.opacity = 0.5;
    fill(&mut c.pixels, 12, 12, 4, 4, Rgba::new(0.0, 0.0, 1.0, 0.5).to_linear_premul());
    g.children.push(Node::Layer(a));
    g.children.push(Node::Layer(c));
    d.root.push(Node::Group(g));
    let composite = vec![0.5f32; 80 * 60 * 4];
    let bytes = write_psd(&d, &composite).unwrap();
    assert_eq!(&bytes[..4], b"8BPS");
    let r = read_psd(&bytes).unwrap();
    assert_eq!((r.width, r.height, r.dpi), (80, 60, 144.0));
    assert_eq!(r.root.len(), 3);
    let g2 = r.root[2].as_group().unwrap();
    assert_eq!((g2.name.as_str(), g2.blend_mode), ("Gruppe ✓", BlendMode::Screen));
    assert!((g2.opacity - 0.75).abs() < 0.01);
    let a2 = g2.children[0].as_layer().unwrap();
    assert_eq!((a2.name.as_str(), a2.blend_mode, a2.visible, a2.mask_enabled), ("Ä layer", BlendMode::Multiply, false, false));
    let mk = a2.mask.as_ref().unwrap();
    assert!(mk.get(5, 5)[0] < 0.01 && mk.get(40, 5)[0] > 0.99);
    let px = a2.pixels.get(15, 15);
    let want = Rgba::new(1.0, 0.5, 0.0, 1.0).to_linear_premul();
    for i in 0..4 {
        assert!((px[i] - want[i]).abs() < 0.01, "{px:?}");
    }
    assert_eq!(a2.pixels.get(50, 50)[3], 0.0);
    let c2 = g2.children[1].as_layer().unwrap();
    assert!(c2.clipped && (c2.opacity - 0.5).abs() < 0.01);
    assert!((c2.pixels.get(13, 13)[3] - 0.5).abs() < 0.01);
}

#[test]
fn knockout_round_trips_as_the_knko_byte() {
    let mut d = Document::new(16, 16, Some(Rgba::WHITE));
    for k in [None, Some(Knockout::Shallow), Some(Knockout::Deep)] {
        let mut l = Layer::new("k");
        l.set_knockout(k);
        d.root.push(Node::Layer(l));
    }
    let bytes = write_psd(&d, &vec![1.0f32; 16 * 16 * 4]).unwrap();
    // Written only when set: two layers carry the block.
    assert_eq!(bytes.windows(8).filter(|w| w == b"8BIMknko").count(), 2);
    let r = read_psd(&bytes).unwrap();
    let got: Vec<_> = r.root.iter().filter_map(|n| n.as_layer()).filter(|l| l.name == "k").map(|l| l.knockout()).collect();
    assert_eq!(got, vec![None, Some(Knockout::Shallow), Some(Knockout::Deep)]);
}

#[test]
fn mask_density_feather_and_blend_if_round_trip() {
    // happypaint `psd-roundtrip.test.ts` "Looked".
    let mut d = Document::new(16, 16, Some(Rgba::WHITE));
    let mut l = Layer::new("Looked");
    fill(&mut l.pixels, 0, 0, 16, 16, Rgba::new(250.0 / 255.0, 120.0 / 255.0, 20.0 / 255.0, 1.0).to_linear_premul());
    let mut m = Surface::new();
    fill(&mut m, 0, 0, 16, 16, [0.0, 0.0, 0.0, 1.0]);
    l.mask = Some(m);
    l.set_mask_density(0.6);
    l.set_mask_feather(4.0);
    let mut b = BlendIf::default();
    b.0[0] = ChannelRanges { src: [0, 0, 180, 220], dst: [30, 60, 255, 255] };
    l.set_blend_if(Some(b));
    d.root.push(Node::Layer(l));
    let r = read_psd(&write_psd(&d, &vec![1.0f32; 16 * 16 * 4]).unwrap()).unwrap();
    let got = r.root.iter().filter_map(|n| n.as_layer()).find(|l| l.name == "Looked").unwrap();
    assert!((got.mask_density() - 0.6).abs() < 0.5 / 255.0);
    assert_eq!((got.mask_feather(), got.blend_if()), (4.0, Some(b)));
    // At their defaults nothing is written and nothing comes back.
    let plain = r.root.iter().filter_map(|n| n.as_layer()).find(|l| l.name != "Looked").unwrap();
    assert!(plain.extra.is_empty(), "{:?}", plain.extra);
}

#[test]
fn vector_masks_round_trip_as_vmsk() {
    // psd-roundtrip.test.ts: a 32 × 32 triangle (8, 4), (28, 28), (4, 28).
    let mut d = Document::new(32, 32, Some(Rgba::WHITE));
    let mut l = Layer::new("Masked");
    fill(&mut l.pixels, 0, 0, 32, 32, Rgba::new(0.0, 0.0, 1.0, 1.0).to_linear_premul());
    let tri: Vec<VectorKnot> =
        [[8.0, 4.0], [28.0, 28.0], [4.0, 28.0]].iter().map(|p| [p[0] - 1.5, p[1], p[0], p[1], p[0] + 1.5, p[1]]).collect();
    let v = VectorMask {
        paths: vec![
            VectorPath { knots: tri.clone(), ..Default::default() },
            VectorPath { op: VectorOp::Subtract, rule: FillRule::NonZero, open: true, knots: tri },
        ],
        invert: true,
        ..Default::default()
    };
    l.set_vector_mask(Some(v.clone()));
    d.root.push(Node::Layer(l));
    let r = read_psd(&write_psd(&d, &vec![1.0f32; 32 * 32 * 4]).unwrap()).unwrap();
    let got = r.root.iter().filter_map(|n| n.as_layer()).find(|l| l.name == "Masked").unwrap().vector_mask().unwrap();
    assert_eq!((got.invert, got.disabled, got.unlinked, got.paths.len()), (true, false, false, 2));
    for (a, b) in got.paths.iter().zip(&v.paths) {
        assert_eq!((a.op, a.rule, a.open, a.knots.len()), (b.op, b.rule, b.open, b.knots.len()));
        for (ka, kb) in a.knots.iter().zip(&b.knots) {
            assert!(ka.iter().zip(kb).all(|(x, y)| (x - y).abs() < 1e-4), "{ka:?} {kb:?}");
        }
    }
}

#[test]
fn reads_a_file_written_by_ag_psd() {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agpsd.psd")).unwrap();
    let d = read_psd(&bytes).unwrap();
    assert_eq!((d.width, d.height), (64, 48));
    let names: Vec<&str> = d.root.iter().map(|n| n.name()).collect();
    assert_eq!(names, vec!["Background", "Red box", "Folder", "Masked"]);
    let red = d.root[1].as_layer().unwrap();
    assert_eq!(red.blend_mode, BlendMode::Multiply);
    assert!((red.opacity - 0.5).abs() < 0.01);
    assert!(red.pixels.get(10, 10)[0] > 0.99 && red.pixels.get(4, 10)[3] == 0.0);
    let folder = d.root[2].as_group().unwrap();
    assert_eq!(folder.blend_mode, BlendMode::Screen);
    let kids: Vec<&str> = folder.children.iter().map(|n| n.name()).collect();
    assert_eq!(kids, vec!["Blue ✓", "Clip"]);
    let blue = folder.children[0].as_layer().unwrap();
    assert!(!blue.visible);
    assert!((blue.pixels.get(31, 21)[3] - 128.0 / 255.0).abs() < 0.01);
    assert!(folder.children[1].as_layer().unwrap().clipped);
    let masked = d.root[3].as_layer().unwrap();
    assert!(!masked.mask_enabled);
    let m = masked.mask.as_ref().unwrap();
    assert!(m.get(10, 10)[0] > 0.99 && m.get(50, 10)[0] < 0.01);
}

#[test]
fn fill_layers_export_their_picture() {
    let mut d = Document::new(64, 48, None);
    let mut l = Layer::new("Solid");
    let mut params = onepaint_filters::default_params(onepaint_filters::Kind::SolidFill);
    params.fill_color = Some([1.0, 0.5, 0.0]);
    l.adjustment = Some(Box::new(onepaint_doc::Adjustment { kind: "solid-fill".into(), params }));
    d.root.push(Node::Layer(l));
    let flat = vec![1.0f32; 64 * 48 * 4];
    let back = read_psd(&write_psd(&d, &flat).unwrap()).unwrap();
    let Some(Node::Layer(s)) = back.root.iter().find(|n| n.name() == "Solid") else { panic!("the fill layer") };
    let p = s.pixels.get(10, 10);
    let c = onepaint_color::Space::Linear.to_srgb8(p);
    // 0.5 · 255 = 127.5 rounds either way through the transfer table.
    assert!(c[0] == 255 && c[1].abs_diff(128) <= 1 && c[2] == 0 && c[3] == 255, "{c:?}");
}
