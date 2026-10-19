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
    // It comes back live (SoCo), its colour to the 8 bits Photoshop keeps…
    let a = s.adjustment.as_ref().expect("a live fill");
    assert_eq!(a.kind, "solid-fill");
    let c = a.params.fill_color.unwrap();
    assert!(c[0] == 1.0 && (c[1] - 128.0 / 255.0).abs() < 1e-6 && c[2] == 0.0, "{c:?}");
    // …and the record still carries the picture for readers without fills.
    let bytes = write_psd(&d, &flat).unwrap();
    assert!(bytes.windows(8).any(|w| w == b"8BIMSoCo"));
}

#[test]
fn adjustment_layers_survive_a_psd_round_trip() {
    use onepaint_filters::{Kind, default_params};
    let mut d = Document::new(16, 16, Some(Rgba::WHITE));
    let kinds =
        ["levels", "curves", "hue-saturation", "color-balance", "gradient-map", "exposure", "vibrance", "photo-filter", "channel-mixer"];
    for k in kinds {
        let mut l = Layer::new(k);
        l.adjustment = Some(Box::new(Adjustment { kind: k.into(), params: default_params(Kind::parse(k).unwrap()) }));
        d.root.push(Node::Layer(l));
    }
    let r = read_psd(&write_psd(&d, &vec![1.0f32; 16 * 16 * 4]).unwrap()).unwrap();
    for k in kinds {
        let l = r.root.iter().filter_map(|n| n.as_layer()).find(|l| l.name == k).unwrap();
        assert_eq!(l.adjustment.as_ref().map(|a| a.kind.as_str()), Some(k));
    }
}

fn find(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn an_untouched_psd_saves_back_byte_for_byte() {
    // A file another program wrote, and one onepaint wrote.
    let agpsd = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agpsd.psd")).unwrap();
    let mut d = Document::new(40, 30, Some(Rgba::WHITE));
    let mut l = Layer::new("Box");
    fill(&mut l.pixels, 5, 5, 10, 10, Rgba::new(1.0, 0.0, 0.0, 1.0).to_linear_premul());
    d.root.push(Node::Layer(l));
    let ours = write_psd(&d, &vec![1.0f32; 40 * 30 * 4]).unwrap();
    for bytes in [agpsd, ours] {
        let doc = read_psd(&bytes).unwrap();
        let flat = vec![0.5f32; (doc.width * doc.height * 4) as usize];
        assert!(write_psd(&doc, &flat).unwrap() == bytes, "untouched round trip changed the file");
    }
}

#[test]
fn edits_rewrite_only_the_changed_layers() {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agpsd.psd")).unwrap();
    let mut doc = read_psd(&bytes).unwrap();
    let flat = vec![0.5f32; (doc.width * doc.height * 4) as usize];
    // Paint on "Red box" (a new tile), rename the folder.
    if let Node::Layer(l) = &mut doc.root[1] {
        l.pixels.set(30, 20, [0.0, 0.0, 1.0, 1.0]);
    }
    if let Node::Group(g) = &mut doc.root[2] {
        g.name = "Renamed".into();
    }
    let out = write_psd(&doc, &flat).unwrap();
    assert!(out != bytes);
    // The untouched "Background" and "Masked" records are in the new file as they were.
    let fresh = read_psd(&bytes).unwrap();
    let src = onepaint_formats::psd_source::PsdSource::of(&fresh).unwrap();
    for i in [0, 3] {
        let rec = src.record_bytes(&fresh.root[i]).unwrap();
        assert!(find(&out, rec), "{} kept", fresh.root[i].name());
    }
    // The edits read back.
    let back = read_psd(&out).unwrap();
    let names: Vec<&str> = back.root.iter().map(|n| n.name()).collect();
    assert_eq!(names, vec!["Background", "Red box", "Renamed", "Masked"]);
    let red = back.root[1].as_layer().unwrap();
    assert!(red.pixels.get(30, 20)[2] > 0.99, "{:?}", red.pixels.get(30, 20));
    assert!(red.pixels.get(10, 10)[0] > 0.99);
    // A document not from a PSD writes as before (no source).
    assert!(Document::new(8, 8, None).source.is_none());
}

/// Spec 56: layer styles cross a PSD with their effects (`lfx2`/`lmfx`),
/// Fill opacity (`iOpa`) and Global Light (resources 1037/1049), on a layer
/// and on a group.
#[test]
fn layer_styles_cross_a_psd() {
    use onepaint_doc::styles::{EffectKind, LayerStyles, default_effect};
    let mut d = Document::new(64, 64, None);
    let mut l = Layer::new("Styled");
    l.pixels.set(10, 10, [1.0, 0.0, 0.0, 1.0]);
    let mut shadow = default_effect(EffectKind::DropShadow);
    (shadow.angle, shadow.distance) = (60.0, 12.0);
    let mut stroke2 = default_effect(EffectKind::Stroke);
    stroke2.size = 9.0;
    l.set_styles(Some(LayerStyles {
        fill_opacity: 0.6,
        effects: vec![default_effect(EffectKind::Stroke), stroke2, shadow, default_effect(EffectKind::Bevel)],
    }));
    let mut g = onepaint_doc::Group::new("Group");
    let mut inner = Layer::new("In");
    inner.pixels.set(20, 20, [0.0, 0.0, 1.0, 1.0]);
    g.children = vec![onepaint_doc::Node::Layer(inner)];
    let mut gs = default_effect(EffectKind::Stroke);
    gs.size = 7.0;
    g.set_styles(Some(LayerStyles { fill_opacity: 1.0, effects: vec![gs] }));
    d.root = vec![onepaint_doc::Node::Layer(l), onepaint_doc::Node::Group(g)];
    let bytes = write_psd(&d, &vec![0.0f32; 64 * 64 * 4]).unwrap();
    let r = read_psd(&bytes).unwrap();
    let back = r.root[0].as_layer().unwrap().styles().unwrap();
    assert!((back.fill_opacity - 0.6).abs() < 0.005, "{}", back.fill_opacity);
    let sizes: Vec<_> = back.effects.iter().filter(|e| e.kind == EffectKind::Stroke).map(|e| e.size).collect();
    assert_eq!(sizes, vec![3.0, 9.0], "both strokes, in order");
    let ds = back.effects.iter().find(|e| e.kind == EffectKind::DropShadow).unwrap();
    assert_eq!((ds.angle, ds.distance, ds.use_global_light), (60.0, 12.0, true), "linked to the file's light");
    assert!(back.effects.iter().any(|e| e.kind == EffectKind::Bevel));
    let onepaint_doc::Node::Group(g) = &r.root[1] else { panic!("a group") };
    assert_eq!(g.styles().unwrap().effects[0].size, 7.0);
}

/// Locks (`lspf`), colour labels (`lclr`) and guides (resource 1032) both
/// ways (happypaint v0.201.0's codec).
#[test]
fn locks_labels_and_guides_round_trip() {
    let mut d = Document::new(64, 48, Some(Rgba::WHITE));
    let mut locked = Layer::new("Locked");
    locked.locked = true;
    locked.extra.insert("colorLabel".into(), serde_json::json!("green"));
    let mut alpha = Layer::new("Alpha");
    alpha.alpha_lock = true;
    let mut g = Group::new("Folder");
    g.extra.insert("colorLabel".into(), serde_json::json!("violet"));
    g.children.push(Node::Layer(Layer::new("Inside")));
    d.root.extend([Node::Layer(locked), Node::Layer(alpha), Node::Group(g)]);
    let pi = std::f64::consts::PI;
    d.extra.insert(
        "guides".into(),
        serde_json::json!({"lines": [
            {"id": "a", "x": 0.0, "y": 12.5, "angle": 0.0},
            {"id": "b", "x": 30.0, "y": 0.0, "angle": pi / 2.0},
            {"id": "c", "x": 5.0, "y": 5.0, "angle": 0.7},
        ]}),
    );
    let bytes = write_psd(&d, &vec![1.0f32; 64 * 48 * 4]).unwrap();
    let r = read_psd(&bytes).unwrap();
    let layer = |name: &str| r.root.iter().find(|n| n.name() == name).and_then(|n| n.as_layer()).unwrap();
    assert!(layer("Locked").locked && !layer("Locked").alpha_lock);
    assert!(layer("Alpha").alpha_lock && !layer("Alpha").locked);
    assert_eq!(layer("Locked").extra.get("colorLabel"), Some(&serde_json::json!("green")));
    let Some(Node::Group(g)) = r.root.iter().find(|n| n.name() == "Folder") else { panic!() };
    assert_eq!(g.extra.get("colorLabel"), Some(&serde_json::json!("violet")));
    assert!(layer("Alpha").extra.get("colorLabel").is_none());
    // Only the horizontal and the vertical guide: Photoshop has no others.
    let lines = r.extra["guides"]["lines"].as_array().unwrap().clone();
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0]["y"].as_f64(), lines[0]["angle"].as_f64()), (Some(12.5), Some(0.0)));
    assert_eq!((lines[1]["x"].as_f64(), lines[1]["angle"].as_f64()), (Some(30.0), Some(pi / 2.0)));
}

/// An untouched file saves back byte for byte until its guides change;
/// then only the guides resource is new.
#[test]
fn moving_a_guide_rewrites_just_the_guides() {
    let mut d = Document::new(40, 30, Some(Rgba::WHITE));
    d.extra.insert("guides".into(), serde_json::json!({"lines": [{"id": "a", "x": 0.0, "y": 10.0, "angle": 0.0}]}));
    let first = write_psd(&d, &vec![1.0f32; 40 * 30 * 4]).unwrap();
    let mut doc = read_psd(&first).unwrap();
    let flat = vec![1.0f32; 40 * 30 * 4];
    assert!(write_psd(&doc, &flat).unwrap() == first, "untouched: the same bytes");
    doc.extra.insert("guides".into(), serde_json::json!({"lines": [{"id": "a", "x": 0.0, "y": 20.0, "angle": 0.0}]}));
    let moved = write_psd(&doc, &flat).unwrap();
    assert!(moved != first);
    let back = read_psd(&moved).unwrap();
    assert_eq!(back.extra["guides"]["lines"][0]["y"].as_f64(), Some(20.0));
    // The layers' records came through unchanged.
    let src = onepaint_formats::psd_source::PsdSource::of(&doc).unwrap();
    assert!(find(&moved, src.record_bytes(&doc.root[0]).unwrap()));
}

/// A PSD of any mode and depth with raw (uncompressed) channels: one layer
/// of `planes` (id, samples as stored) covering the whole w × h image, or,
/// with no planes, only a merged image of `merged` planes.
#[allow(clippy::too_many_arguments)]
fn raw_psd(
    mode: u16,
    depth: u16,
    w: u32,
    h: u32,
    colour_data: &[u8],
    resources: &[(u16, Vec<u8>)],
    planes: &[(i16, Vec<u8>)],
    merged: &[Vec<u8>],
) -> Vec<u8> {
    let mut o: Vec<u8> = Vec::new();
    let be16 = |o: &mut Vec<u8>, v: u16| o.extend(v.to_be_bytes());
    let be32 = |o: &mut Vec<u8>, v: u32| o.extend(v.to_be_bytes());
    o.extend(b"8BPS");
    be16(&mut o, 1);
    o.extend([0; 6]);
    be16(&mut o, merged.len().max(1) as u16);
    be32(&mut o, h);
    be32(&mut o, w);
    be16(&mut o, depth);
    be16(&mut o, mode);
    be32(&mut o, colour_data.len() as u32);
    o.extend(colour_data);
    let mut res = Vec::new();
    for (id, d) in resources {
        res.extend(b"8BIM");
        res.extend(id.to_be_bytes());
        res.extend([0, 0]);
        res.extend((d.len() as u32).to_be_bytes());
        res.extend(d);
        if d.len() % 2 == 1 {
            res.push(0);
        }
    }
    be32(&mut o, res.len() as u32);
    o.extend(res);
    // Layer and mask info.
    let mut li = Vec::new();
    if !planes.is_empty() {
        li.extend((-1i16).to_be_bytes());
        li.extend([0, 0, 0, 0, 0, 0, 0, 0]);
        li.extend(h.to_be_bytes());
        li.extend(w.to_be_bytes());
        li.extend((planes.len() as u16).to_be_bytes());
        for (id, d) in planes {
            li.extend(id.to_be_bytes());
            li.extend((d.len() as u32 + 2).to_be_bytes());
        }
        li.extend(b"8BIMnorm");
        li.extend([255, 0, 0, 0]);
        // Extra data: an empty mask, no blend ranges, the name "L" padded to 4.
        li.extend(12u32.to_be_bytes());
        li.extend([0; 4]);
        li.extend([0; 4]);
        li.extend([1, b'L', 0, 0]);
        for (_, d) in planes {
            li.extend([0, 0]);
            li.extend(d);
        }
        if li.len() % 2 == 1 {
            li.push(0);
        }
    }
    let mut lm = Vec::new();
    lm.extend((li.len() as u32).to_be_bytes());
    lm.extend(li);
    be32(&mut o, lm.len() as u32);
    o.extend(lm);
    be16(&mut o, 0);
    for m in merged {
        o.extend(m);
    }
    o
}

fn pixel(d: &Document, x: i32, y: i32) -> [u8; 4] {
    let n = d.root.iter().filter_map(|n| n.as_layer()).next_back().unwrap();
    let p = n.pixels.get(x, y);
    let a = p[3];
    let s = |v: f32| if a > 0.0 { (onepaint_color::linear_to_srgb(v / a) * 255.0).round() as u8 } else { 0 };
    [s(p[0]), s(p[1]), s(p[2]), (a * 255.0).round() as u8]
}

// happypaint psd-file.test.ts: 16-bit samples scale, 32-bit colour is linear light.
#[test]
fn deep_psds_read_at_full_precision() {
    let be16 = |v: &[u16]| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
    let planes = [(0, be16(&[0, 65535])), (1, be16(&[32896, 0])), (2, be16(&[65535, 0])), (-1, be16(&[65535, 32896]))];
    let d = read_psd(&raw_psd(3, 16, 2, 1, &[], &[], &planes, &[])).unwrap();
    assert_eq!([pixel(&d, 0, 0), pixel(&d, 1, 0)], [[0, 128, 255, 255], [255, 0, 0, 128]]);
    let bef = |v: &[f32]| v.iter().flat_map(|x| x.to_be_bytes()).collect::<Vec<u8>>();
    let planes = [(0, bef(&[0.2140411, 1.0])), (1, bef(&[0.0, 0.0])), (2, bef(&[1.0, 0.0])), (-1, bef(&[0.5, 1.0]))];
    let d = read_psd(&raw_psd(3, 32, 2, 1, &[], &[], &planes, &[])).unwrap();
    assert_eq!([pixel(&d, 0, 0), pixel(&d, 1, 0)], [[127, 0, 255, 128], [255, 0, 0, 255]]);
}

/// Spec 61: CMYK (inverted: 255 is no ink), Lab, Indexed with its
/// transparent colour, and Bitmap (1 is black, rows padded to a byte).
#[test]
fn other_colour_modes_read_as_rgb() {
    // CMYK: no ink is the paper (a warm off-white), full K is near black.
    let planes = [(0, vec![255, 255]), (1, vec![255, 255]), (2, vec![255, 255]), (3, vec![255, 0])];
    let d = read_psd(&raw_psd(4, 8, 2, 1, &[], &[], &planes, &[])).unwrap();
    let (paper, black) = (pixel(&d, 0, 0), pixel(&d, 1, 0));
    assert!(paper[0] > 240 && paper[2] < paper[0], "{paper:?}");
    assert!(black[0] < 70 && black[3] == 255, "{black:?}");
    // Lab: L 100 is white; L 50 with a = b = 0 is mid grey (sRGB 119).
    let planes = [(0, vec![255, 128]), (1, vec![128, 128]), (2, vec![128, 128])];
    let d = read_psd(&raw_psd(9, 8, 2, 1, &[], &[], &planes, &[])).unwrap();
    let (white, grey) = (pixel(&d, 0, 0), pixel(&d, 1, 0));
    assert!(white.iter().take(3).all(|v| *v >= 253), "{white:?}");
    assert!(grey.iter().take(3).all(|v| (115..=122).contains(v)), "{grey:?}");
    // Indexed: a planar table; index 1 is transparent (resource 1047).
    let mut table = vec![0u8; 768];
    (table[0], table[256], table[512]) = (10, 200, 30);
    let d = read_psd(&raw_psd(2, 8, 2, 1, &table, &[(1047, vec![0, 1])], &[], &[vec![0, 1]])).unwrap();
    assert_eq!(pixel(&d, 0, 0), [10, 200, 30, 255]);
    assert_eq!(pixel(&d, 1, 0)[3], 0);
    // Bitmap: 10 px wide, so each row takes two bytes; 1 is black.
    let rows = vec![0b1000_0000, 0b0100_0000, 0b0000_0000, 0b0000_0000];
    let d = read_psd(&raw_psd(0, 1, 10, 2, &[], &[], &[], &[rows])).unwrap();
    assert_eq!(pixel(&d, 0, 0)[0], 0);
    assert_eq!(pixel(&d, 1, 0)[0], 255);
    assert_eq!(pixel(&d, 9, 0)[0], 0, "the second byte of the row");
    assert_eq!(pixel(&d, 0, 1)[0], 255, "the next row starts on a byte");
}
