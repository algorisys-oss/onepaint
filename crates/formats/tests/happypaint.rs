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
    let mut next = header(VERSION + 1, 0).to_vec();
    next.extend_from_slice(&[0; 4]);
    assert!(matches!(read_happypaint(&next), Err(FormatError::UnsupportedVersion(v)) if v == VERSION + 1));
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

#[test]
fn reads_count_progress_and_stop_when_cancelled() {
    let mut d = Document::new(600, 520, Some(Rgba::WHITE));
    let mut l = Layer::new("ink");
    fill(&mut l.pixels, 10, 10, 500, 400, [0.2, 0.1, 0.0, 0.5]);
    let mut m = Surface::new();
    fill(&mut m, 0, 0, 600, 520, [0.5, 0.5, 0.5, 1.0]);
    l.mask = Some(m);
    d.root.push(Node::Group(Group { children: vec![Node::Layer(l)], ..Group::new("g") }));
    let bytes = write_happypaint(&d, None, "t").unwrap();
    let p = Progress::default();
    let back = read_happypaint_with(&bytes, &p).unwrap();
    // Every tile and mask tile is counted, inside the group too.
    assert_eq!(p.fraction(), Some(1.0));
    assert_eq!(back.root.len(), d.root.len());
    let c = Progress::default();
    c.cancel();
    assert!(matches!(read_happypaint_with(&bytes, &c), Err(FormatError::Cancelled)));
    let flat = vec![1.0f32; 600 * 520 * 4];
    let psd = psd::write_psd(&d, &flat).unwrap();
    let p = Progress::default();
    psd::read_psd_with(&psd, &p).unwrap();
    assert_eq!(p.fraction(), Some(1.0));
    assert!(matches!(psd::read_psd_with(&psd, &c), Err(FormatError::Cancelled)));
}

#[test]
fn photoshop_modes_and_pass_through_survive_a_round_trip() {
    let mut d = Document::new(300, 200, Some(Rgba::WHITE));
    let mut g = Group::new("g");
    g.blend_mode = BlendMode::PassThrough;
    let mut l = Layer::new("ink");
    fill(&mut l.pixels, 10, 10, 50, 50, [0.2, 0.1, 0.0, 1.0]);
    l.blend_mode = BlendMode::VividLight;
    g.children.push(Node::Layer(l));
    let mut f = Layer::new("future");
    f.extra.insert(RAW_BLEND_KEY.into(), serde_json::json!("some-future-mode"));
    g.children.push(Node::Layer(f));
    d.root.push(Node::Group(g));
    let modes = |d: &Document| -> Vec<BlendMode> {
        let Node::Group(g) = d.root.last().unwrap() else { panic!("a group") };
        vec![g.blend_mode, g.children[0].blend_mode()]
    };
    let back = read_happypaint(&write_happypaint(&d, None, "t").unwrap()).unwrap();
    assert_eq!(modes(&back), [BlendMode::PassThrough, BlendMode::VividLight]);
    // A name onepaint doesn't know is kept and saved back.
    let Node::Group(g) = back.root.last().unwrap() else { panic!("a group") };
    let Node::Layer(f) = &g.children[1] else { panic!("a layer") };
    assert_eq!(f.extra.get(RAW_BLEND_KEY).and_then(|v| v.as_str()), Some("some-future-mode"));
    let flat = vec![1.0f32; 300 * 200 * 4];
    let psd = psd::read_psd(&psd::write_psd(&back, &flat).unwrap()).unwrap();
    assert_eq!(modes(&psd), [BlendMode::PassThrough, BlendMode::VividLight]);
}

/// A file around a hand-written manifest (no chunks unless given).
fn file_with(manifest: &serde_json::Value, chunks: &[u8]) -> Vec<u8> {
    let z = deflate_raw(&serde_json::to_vec(manifest).unwrap());
    let mut f = header(9, z.len() as u32).to_vec();
    f.extend_from_slice(&z);
    f.extend_from_slice(chunks);
    f
}

fn too_large(r: Result<Document, FormatError>) -> String {
    match r {
        Err(FormatError::TooLarge(m)) => m,
        other => panic!("expected TooLarge, got {:?}", other.map(|d| d.name)),
    }
}

// happypaint `storage/container-guard.ts` (review F10, v0.205.3).
#[test]
fn hostile_files_fail_with_a_message() {
    use serde_json::json;
    let doc = |w: u64, h: u64| json!({"id": "d", "name": "n", "width": w, "height": h});
    // An impossible canvas.
    let m = too_large(read_happypaint(&file_with(&json!({"doc": doc(1_000_000_000, 4), "nodes": []}), &[])));
    assert!(m.contains("canvas size"), "{m}");
    assert!(read_happypaint(&file_with(&json!({"doc": doc(0, 4), "nodes": []}), &[])).is_err());
    assert!(read_happypaint(&file_with(&json!({"doc": doc(limits::MAX_CANVAS_SIDE, 1), "nodes": []}), &[])).is_ok());

    // Groups nested past the depth budget.
    let mut n = json!({"id": "leaf", "name": "L"});
    for i in 0..limits::MAX_DEPTH {
        n = json!({"id": format!("g{i}"), "name": "G", "children": [n]});
    }
    let m = too_large(read_happypaint(&file_with(&json!({"doc": doc(4, 4), "nodes": [n]}), &[])));
    assert!(m.contains("too deeply"), "{m}");

    // Too many layers.
    let many: Vec<_> = (0..=limits::MAX_NODES).map(|i| json!({"id": format!("l{i}"), "name": "L"})).collect();
    let m = too_large(read_happypaint(&file_with(&json!({"doc": doc(4, 4), "nodes": many}), &[])));
    assert!(m.contains("too many layers"), "{m}");

    // Deflated data that unpacks past a budget stops there.
    let z = deflate_raw(&[b' '; 4096]);
    assert_eq!(inflate_raw_max(&z, 4096, "file").unwrap().len(), 4096);
    let m = match inflate_raw_max(&z, 4095, "file") {
        Err(FormatError::TooLarge(m)) => m,
        other => panic!("expected TooLarge, got {other:?}"),
    };
    assert!(m.contains("too large to open"), "{m}");

    // The recording reader uses the same budgets.
    let z = deflate_raw(&serde_json::to_vec(&json!({"doc": doc(1 << 20, 4), "nodes": []})).unwrap());
    let mut rec = b"HPRC".to_vec();
    rec.extend_from_slice(&[1, 0, 0, 0]);
    rec.extend_from_slice(&(z.len() as u32).to_le_bytes());
    rec.extend_from_slice(&z);
    assert!(matches!(happy_rec::read_happypaint_rec(&rec), Err(FormatError::TooLarge(_))));
}

// happypaint `storage/tile-decode.ts` (review F03, v0.205.2): one bad tile
// costs that tile, not the layer or the file.
#[test]
fn an_unreadable_tile_is_left_blank_and_its_layer_kept() {
    let mut d = Document::new_in(512, 256, None, Depth::Rgba8);
    d.root.clear();
    let mut ink = Layer::new("ink");
    // The two tiles differ, so their chunks aren't shared.
    fill(&mut ink.pixels, 0, 0, 256, 256, [0.5, 0.0, 0.0, 0.5]);
    fill(&mut ink.pixels, 256, 0, 256, 256, [0.0, 0.5, 0.0, 0.5]);
    let mut m = Surface::new();
    fill(&mut m, 0, 0, 256, 256, [0.2, 0.2, 0.2, 1.0]);
    fill(&mut m, 256, 0, 256, 256, [0.0, 0.0, 0.0, 1.0]);
    ink.mask = Some(m);
    d.root.push(Node::Layer(ink));
    d.root.push(Node::Layer(Layer::new("other")));
    let mut bytes = write_happypaint(&d, None, "t").unwrap();

    // Break the first colour tile's PNG (its IHDR) and the first mask tile's.
    let pngs: Vec<usize> = bytes.windows(8).enumerate().filter(|(_, w)| *w == b"\x89PNG\r\n\x1a\n").map(|(i, _)| i).collect();
    assert_eq!(pngs.len(), 4, "two colour tiles, two mask tiles");
    for &at in &[pngs[0], pngs[2]] {
        bytes[at + 16..at + 24].fill(0xff);
    }

    let p = Progress::default();
    let back = read_happypaint_with(&bytes, &p).unwrap();
    assert_eq!(p.damaged_layers(), vec!["ink".to_string()]);
    assert_eq!(back.root.len(), 2, "the layer is kept");
    let Node::Layer(l) = &back.root[0] else { panic!() };
    assert_eq!(l.pixels.get(10, 10)[3], 0.0, "the broken tile is blank");
    assert!(l.pixels.get(300, 10)[3] > 0.4, "the other tile is intact");
    let m = l.mask.as_ref().unwrap();
    assert_eq!(m.get(10, 10)[0], 1.0, "a broken mask tile reveals");
    assert_eq!(m.get(300, 10)[0], 0.0, "the other mask tile is intact");
    assert_eq!(p.fraction(), Some(1.0), "every tile still counts");

    // A clean file notes nothing.
    let p = Progress::default();
    read_happypaint_with(&write_happypaint(&d, None, "t").unwrap(), &p).unwrap();
    assert!(p.damaged_layers().is_empty());
}

// happypaint v0.207.0 (review F07): float drawings keep their exact samples.
#[test]
fn float_drawings_round_trip_exactly_as_v10() {
    for (depth, fmt) in [(Depth::Rgba16F, "f16"), (Depth::Rgba32F, "f32")] {
        let mut d = Document::new_in(300, 40, None, depth);
        d.root.clear();
        let mut l = Layer::new("hdr");
        // Nearby values 8 bits merged, and one brighter than white.
        fill(&mut l.pixels, 0, 0, 2, 1, [0.5, 0.5, 0.5, 1.0]);
        fill(&mut l.pixels, 2, 0, 2, 1, [0.501, 0.501, 0.501, 1.0]);
        fill(&mut l.pixels, 260, 10, 4, 4, [3.0, 1.5, 0.25, 1.0]);
        // Off-canvas pixels keep theirs too.
        fill(&mut l.pixels, -6, 5, 4, 2, [2.0, 0.0, 0.0, 1.0]);
        let mut m = Surface::new();
        fill(&mut m, 0, 0, 300, 40, [0.5, 0.5, 0.5, 1.0]);
        l.mask = Some(m);
        d.root.push(Node::Layer(l));
        let bytes = write_happypaint(&d, None, "t").unwrap();
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION_FLOAT);
        let manifest: serde_json::Value =
            serde_json::from_slice(&inflate_raw(&bytes[12..12 + u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize]).unwrap())
                .unwrap();
        let node = &manifest["nodes"][0];
        assert!(node["tiles"].as_array().unwrap().iter().all(|t| t["fmt"] == fmt), "{depth:?}");
        assert!(node["maskTiles"].as_array().unwrap().iter().all(|t| t.get("fmt").is_none()), "masks stay PNG");
        assert_eq!(node["overflow"]["fmt"], fmt);

        let back = read_happypaint(&bytes).unwrap();
        assert_eq!(back.depth, depth);
        let (Node::Layer(a), Node::Layer(b)) = (&d.root[0], &back.root[0]) else { panic!() };
        for (x, y) in [(0, 0), (2, 0), (261, 11), (-5, 5), (100, 20)] {
            assert_eq!(a.pixels.get(x, y), b.pixels.get(x, y), "{depth:?} at {x},{y}");
        }
        assert_ne!(b.pixels.get(0, 0), b.pixels.get(2, 0), "0.5 and 0.501 stay apart (one byte in 8 bits)");
        assert_eq!(b.pixels.get(261, 11)[0], 3.0, "brighter than white survives");
        assert!((b.mask.as_ref().unwrap().get(5, 5)[0] - 0.5).abs() < 1.0 / 255.0);
    }

    // An 8-bit drawing still writes v9 PNG tiles, which older readers open.
    let d = Document::new_in(64, 64, Some(Rgba::WHITE), Depth::Rgba8);
    let bytes = write_happypaint(&d, None, "t").unwrap();
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION_PLAIN);
}

#[test]
fn unknown_or_broken_float_tiles() {
    use serde_json::json;
    let tile = encode_float_tile(&[0.75f32; 4 * 4], 2, 2, FloatFormat::F16).unwrap();
    let node =
        |fmt: &str, len: usize| json!({"id": "l", "name": "L", "tiles": [{"key": 0, "fmt": fmt, "chunk": {"offset": 0, "len": len}}]});
    let doc = json!({"id": "d", "name": "n", "width": 2, "height": 2, "depth": "16-bit-float"});

    let ok = read_happypaint(&file_with(&json!({"doc": doc, "nodes": [node("f16", tile.len())]}), &tile)).unwrap();
    let Node::Layer(l) = &ok.root[0] else { panic!() };
    assert_eq!(l.pixels.get(1, 1), [0.75; 4]);

    // An unknown format is a broken file, as in happypaint.
    assert!(matches!(
        read_happypaint(&file_with(&json!({"doc": doc, "nodes": [node("f64", tile.len())]}), &tile)),
        Err(FormatError::Corrupt(_))
    ));

    // A float tile that won't decode costs that tile only.
    let mut broken = tile.clone();
    broken[4] = 7;
    let p = Progress::default();
    let back = read_happypaint_with(&file_with(&json!({"doc": doc, "nodes": [node("f16", broken.len())]}), &broken), &p).unwrap();
    assert_eq!(p.damaged_layers(), vec!["L".to_string()]);
    let Node::Layer(l) = &back.root[0] else { panic!() };
    assert!(l.pixels.tile(0, 0).is_none());
}

// happypaint v0.208.0 (review F14): relief is stored as 256² tiles, only
// where there is any; the full-canvas `height` PNG is still read.
#[test]
fn impasto_relief_is_written_as_tiles_v11() {
    use serde_json::json;
    let mut d = Document::new_in(600, 300, None, Depth::Rgba8);
    d.root.clear();
    let mut h = Surface::new();
    h.set(10, 10, [2.0; 4]);
    h.set(520, 280, [7.5; 4]);
    let mut l = Layer::new("Thick");
    l.height = Some(h.clone());
    d.root.push(Node::Layer(l));
    let bytes = write_happypaint(&d, None, "t").unwrap();
    assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), VERSION_RELIEF);
    let len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let manifest: serde_json::Value = serde_json::from_slice(&inflate_raw(&bytes[12..12 + len]).unwrap()).unwrap();
    let node = &manifest["nodes"][0];
    assert!(node.get("height").is_none());
    let keys: Vec<u64> = node["heightTiles"].as_array().unwrap().iter().map(|t| t["key"].as_u64().unwrap()).collect();
    assert_eq!(keys.len(), 2, "only the two tiles with relief: {keys:?}");

    let back = read_happypaint(&bytes).unwrap();
    let got = back.root[0].as_layer().unwrap().height.as_ref().unwrap();
    for (x, y, v) in [(10, 10, 2.0), (520, 280, 7.5), (300, 100, 0.0)] {
        assert!((got.get(x, y)[0] - v).abs() < 0.01, "({x},{y})");
    }
    assert!(back.root[0].as_layer().unwrap().extra.get("heightTiles").is_none(), "not kept as an extra too");

    // An older file's full-canvas PNG reads into the same field.
    let png = encode_height_png(&h, onepaint_raster::Rect::new(0, 0, 600, 300)).unwrap().unwrap();
    let doc = json!({"id": "d", "name": "n", "width": 600, "height": 300});
    let old = file_with(&json!({"doc": doc, "nodes": [{"id": "l", "name": "Thick", "height": {"offset": 0, "len": png.len()}}]}), &png);
    let back = read_happypaint(&old).unwrap();
    assert!((back.root[0].as_layer().unwrap().height.as_ref().unwrap().get(520, 280)[0] - 7.5).abs() < 0.01);

    // A relief tile that won't decode stays flat; the rest come back.
    let good = encode_height_png(&h, onepaint_raster::Rect::new(0, 0, 256, 256)).unwrap().unwrap();
    let bad = vec![0u8; 20];
    let mut chunks = good.clone();
    chunks.extend_from_slice(&bad);
    let tiles = json!([
        {"key": 0, "chunk": {"offset": 0, "len": good.len()}},
        {"key": 2 << 16, "chunk": {"offset": good.len(), "len": bad.len()}}
    ]);
    let p = Progress::default();
    let back =
        read_happypaint_with(&file_with(&json!({"doc": doc, "nodes": [{"id": "l", "name": "Thick", "heightTiles": tiles}]}), &chunks), &p)
            .unwrap();
    assert_eq!(p.damaged_layers(), vec!["Thick".to_string()]);
    assert!((back.root[0].as_layer().unwrap().height.as_ref().unwrap().get(10, 10)[0] - 2.0).abs() < 0.01);
}

// Autosave re-encodes only changed tiles (happypaint v0.205.0 reads back only
// what changed): a cached write must equal a fresh one, byte for byte.
#[test]
fn cached_writes_match_fresh_ones_and_pick_up_changes() {
    for depth in [Depth::Rgba8, Depth::Rgba16F] {
        let mut d = Document::new_in(600, 300, Some(Rgba::WHITE), depth);
        let Node::Layer(l) = d.root.last_mut().unwrap() else { panic!() };
        fill(&mut l.pixels, 10, 10, 500, 200, [0.3, 0.1, 0.0, 0.6]);
        l.mask = Some(Surface::new());
        let mut h = Surface::new();
        h.set(20, 20, [3.0; 4]);
        l.height = Some(h);
        let mut cache = ChunkCache::default();
        let first = write_happypaint_cached(&d, None, "t", &mut cache).unwrap();
        assert_eq!(first, write_happypaint(&d, None, "t").unwrap());
        assert!(!cache.is_empty());

        // One tile changes; the rest come from the cache.
        let Node::Layer(l) = d.root.last_mut().unwrap() else { panic!() };
        l.pixels.set(300, 100, [0.0, 0.0, 0.9, 1.0]);
        let second = write_happypaint_cached(&d, None, "t", &mut cache).unwrap();
        assert_eq!(second, write_happypaint(&d, None, "t").unwrap(), "{depth:?}");
        let back = read_happypaint(&second).unwrap();
        let Node::Layer(b) = back.root.last().unwrap() else { panic!() };
        assert!(b.pixels.get(300, 100)[2] > 0.8, "the changed tile was re-encoded");

        // A layer that's gone leaves the cache.
        let n = cache.len();
        d.root.pop();
        write_happypaint_cached(&d, None, "t", &mut cache).unwrap();
        assert!(cache.len() < n);
    }
}
