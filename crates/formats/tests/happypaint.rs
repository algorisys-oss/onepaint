#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_doc::*;
use onepaint_formats::*;
use onepaint_raster::Surface;
use std::path::PathBuf;

#[test]
fn header_bytes() {
    assert_eq!(header(6, 1234), [0x48, 0x50, 0x4E, 0x54, 0x06, 0x00, 0x00, 0x00, 0xD2, 0x04, 0x00, 0x00]);
}

#[test]
fn rejects_bad_magic_version_and_bounds() {
    assert!(matches!(read_happypaint(b"NOPE00000000"), Err(FormatError::BadMagic)));
    let mut v10 = header(10, 0).to_vec();
    v10.extend_from_slice(&[0; 4]);
    assert!(matches!(read_happypaint(&v10), Err(FormatError::UnsupportedVersion(10))));
    assert!(read_happypaint(&header(9, 999)).is_err());
    // A manifest whose chunk ref runs past EOF is an error, not a panic.
    let json = br#"{"app":"t","doc":{"id":"d","name":"n","width":4,"height":4,"createdAt":0,"updatedAt":0},
      "nodes":[{"id":"l","name":"L","visible":true,"opacity":1,"blendMode":"normal","tiles":[{"key":0,"chunk":{"offset":0,"len":99}}]}]}"#;
    let z = deflate_raw(json);
    let mut f = header(9, z.len() as u32).to_vec();
    f.extend_from_slice(&z);
    assert!(read_happypaint(&f).is_err());
}

fn fill(s: &mut Surface, x0: i32, y0: i32, w: i32, h: i32, p: [f32; 4]) {
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            s.set(x, y, p);
        }
    }
}

#[test]
fn round_trip_tree_pixels_and_extras() {
    let mut d = Document::new(300, 280, Some(Rgba::WHITE));
    d.name = "Round trip".into();
    d.extra.insert("guides".into(), serde_json::json!({"visible": true}));
    let mut g = Group::new("Group");
    g.opacity = 0.5;
    g.blend_mode = BlendMode::Screen;
    let mut a = Layer::new("A");
    fill(&mut a.pixels, 10, 10, 20, 20, Rgba::new(1.0, 0.5, 0.0, 1.0).to_linear_premul());
    fill(&mut a.pixels, 260, 260, 20, 20, Rgba::new(0.0, 0.0, 1.0, 0.5).to_linear_premul());
    a.extra.insert("colorLabel".into(), serde_json::json!("red"));
    let mut m = Surface::new();
    fill(&mut m, 0, 0, 300, 280, [1.0, 1.0, 1.0, 1.0]);
    fill(&mut m, 0, 0, 15, 280, [0.5, 0.5, 0.5, 1.0]);
    a.mask = Some(m);
    a.mask_enabled = false;
    let mut c = Layer::new("Clip");
    c.clipped = true;
    c.alpha_lock = true;
    fill(&mut c.pixels, 0, 0, 5, 5, [0.0, 1.0, 0.0, 1.0]);
    c.chunks.insert("height".into(), vec![1, 2, 3]);
    g.children.push(Node::Layer(a));
    g.children.push(Node::Layer(c));
    d.root.push(Node::Group(g));

    let bytes = write_happypaint(&d, Some(&[0xFF, 0xD8, 0xFF, 0xD9]), "test").unwrap();
    let r = read_happypaint(&bytes).unwrap();
    assert_eq!((r.name.as_str(), r.width, r.height), ("Round trip", 300, 280));
    assert_eq!(r.extra.get("guides"), d.extra.get("guides"));
    assert_eq!(r.root.len(), 3);
    let g2 = r.root[2].as_group().unwrap();
    assert_eq!((g2.opacity, g2.blend_mode), (0.5, BlendMode::Screen));
    let a2 = g2.children[0].as_layer().unwrap();
    assert_eq!(a2.extra.get("colorLabel"), Some(&serde_json::json!("red")));
    assert!(!a2.mask_enabled);
    let mk = a2.mask.as_ref().unwrap();
    assert!((mk.get(5, 5)[0] - 0.5).abs() < 0.01 && (mk.get(100, 5)[0] - 1.0).abs() < 0.01);
    let orig = Rgba::new(1.0, 0.5, 0.0, 1.0).to_linear_premul();
    let got = a2.pixels.get(15, 15);
    for i in 0..4 {
        assert!((got[i] - orig[i]).abs() < 0.01, "{got:?} vs {orig:?}");
    }
    let semi = a2.pixels.get(270, 270);
    assert!((semi[3] - 0.5).abs() < 0.01 && (semi[2] - 0.5).abs() < 0.01, "{semi:?}");
    assert_eq!(a2.pixels.get(100, 100), [0.0; 4]);
    let c2 = g2.children[1].as_layer().unwrap();
    assert!(c2.clipped && c2.alpha_lock);
    assert_eq!(c2.chunks.get("height"), Some(&vec![1, 2, 3]));
    // Thumbnail is the first chunk.
    assert_eq!(read_thumbnail(&bytes).unwrap(), Some(vec![0xFF, 0xD8, 0xFF, 0xD9]));
}

#[test]
fn identical_tiles_are_deduplicated() {
    let mut d = Document::new(512, 256, None);
    let l = d.root[0].as_layer_mut().unwrap();
    fill(&mut l.pixels, 0, 0, 512, 256, [0.25, 0.25, 0.25, 1.0]);
    let one = write_happypaint(&d, None, "t").unwrap();
    let mut d1 = Document::new(256, 256, None);
    fill(&mut d1.root[0].as_layer_mut().unwrap().pixels, 0, 0, 256, 256, [0.25, 0.25, 0.25, 1.0]);
    let single = write_happypaint(&d1, None, "t").unwrap();
    // The second identical tile adds only a manifest entry, not a second PNG.
    assert!(one.len() < single.len() + 200, "{} vs {}", one.len(), single.len());
}

#[test]
fn srgb8_encoding_rules() {
    assert_eq!(premul_linear_to_srgb8([0.0, 0.0, 0.0, 0.0]), [0, 0, 0, 0]);
    assert_eq!(premul_linear_to_srgb8([1.0, 0.0, 0.0, 1.0]), [255, 0, 0, 255]);
    let half_grey = onepaint_color::srgb_to_linear(0.5);
    assert_eq!(premul_linear_to_srgb8([half_grey * 0.5, 0.0, 0.0, 0.5]), [128, 0, 0, 128]);
    let p = srgb8_to_premul_linear([128, 0, 0, 128]);
    assert!((p[3] - 128.0 / 255.0).abs() < 1e-6);
}

fn samples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../happypaint/samples/examples")
}

#[test]
fn happypaint_samples_open_and_match_previews() {
    let dir = samples_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("skipping: {} not found", dir.display());
        return;
    };
    let mut n = 0;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("happypaint") {
            continue;
        }
        let doc = read_happypaint(&std::fs::read(&p).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        n += 1;
        // Animated docs preview a single frame; animation is not in slice 1.
        if doc.extra.get("animation").and_then(|a| a.get("enabled")).and_then(|v| v.as_bool()) == Some(true) {
            eprintln!("{}: animated, pixel check skipped", p.display());
            continue;
        }
        let preview = p.with_extension("preview.png");
        let Ok(pb) = std::fs::read(&preview) else {
            continue;
        };
        let (pw, ph, prgba) = decode_png_rgba8(&pb).unwrap();
        if (pw, ph) != (doc.width, doc.height) {
            continue;
        }
        // The previews come from happypaint's headless generator, which blends in
        // sRGB (gamma) space. Rebuild that composite from the decoded layers so the
        // check isolates decoding (layer order, placement, opacity, transfer curves).
        let (w, h) = (doc.width as i32, doc.height as i32);
        let mut acc = vec![[0.0f32; 4]; (w * h) as usize];
        for node in &doc.root {
            let Some(l) = node.as_layer() else { continue };
            if !l.visible {
                continue;
            }
            let px = l.pixels.read_rect(doc.bounds());
            for (i, p) in px.chunks_exact(4).enumerate() {
                let s = onepaint_color::prem_lin_to_srgb([p[0], p[1], p[2], p[3]]).map(|v| v * l.opacity);
                let k = 1.0 - s[3];
                let d = &mut acc[i];
                for c in 0..4 {
                    d[c] = s[c] + d[c] * k;
                }
            }
        }
        let mut err = 0.0f64;
        for (i, p) in acc.iter().enumerate() {
            for c in 0..3 {
                let ours = ((p[c] + 1.0 - p[3]).clamp(0.0, 1.0) * 255.0).round() as f64;
                err += (ours - prgba[i * 4 + c] as f64).abs();
            }
        }
        let mae = err / (acc.len() * 3) as f64;
        eprintln!("{}: mean abs error {mae:.3}/255", p.display());
        assert!(mae <= 2.0, "{}: mean abs error {mae}", p.display());
    }
    assert!(n > 0, "no samples read");
}

#[test]
fn impasto_height_round_trips_as_a_16_bit_png() {
    let doc = onepaint_raster::Rect::new(0, 0, 40, 30);
    let mut h = Surface::new();
    h.set(3, 4, [1.25; 4]);
    h.set(39, 29, [15.5; 4]);
    h.set(20, 10, [40.0; 4]); // over the store max: clamped to 16
    let png = encode_height_png(&h, doc).unwrap().unwrap();
    let back = decode_height_png(&png, doc).unwrap();
    for (x, y, v) in [(3, 4, 1.25), (39, 29, 15.5), (20, 10, 16.0), (0, 0, 0.0)] {
        assert!((back.get(x, y)[0] - v).abs() < 0.01, "({x},{y}): {:?}", back.get(x, y));
    }
    // happypaint's bytes: q = h/16·65535, R = high, G = low (height-codec.ts).
    let (_, _, rgba) = decode_png_rgba8(&png).unwrap();
    let i = (4 * 40 + 3) * 4;
    let q = ((rgba[i] as u32) << 8) | rgba[i + 1] as u32;
    assert_eq!(q, (1.25 / 16.0 * 65535.0) as u32);
    assert!(encode_height_png(&Surface::new(), doc).unwrap().is_none(), "flat fields are omitted");

    // Through the file: the layer gets its field back.
    let mut d = Document::new(40, 30, None);
    let mut l = Layer::new("Thick");
    l.height = Some(h);
    d.root.push(Node::Layer(l));
    let r = read_happypaint(&write_happypaint(&d, None, "test").unwrap()).unwrap();
    let got = r.root.iter().filter_map(|n| n.as_layer()).find(|l| l.name == "Thick").unwrap().height.as_ref().unwrap();
    assert!((got.get(3, 4)[0] - 1.25).abs() < 0.01);
}
