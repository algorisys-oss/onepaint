#![allow(clippy::unwrap_used)]
use onepaint_formats::palette::*;
use onepaint_formats::zip::{read_zip, write_zip};
use std::io::Write;

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

#[test]
fn gimp_gpl() {
    let text = "GIMP Palette\nName: Sunset\nColumns: 4\n# comment\n255   0   0\tRed\n  0 128 255 Sky\n300 1 2\n";
    let p = parse_palette("x.gpl", text.as_bytes()).unwrap();
    assert_eq!(p[0].name, "Sunset");
    assert_eq!(p[0].colors, ["#ff0000", "#0080ff", "#ff0102"]);
    assert!(parse_palette("x.gpl", b"255 0 0").is_err(), "header required");
}

#[test]
fn photoshop_aco_v1_and_v2() {
    let mut b = Vec::new();
    b.extend(be16(1));
    b.extend(be16(5));
    for e in [[0u16, 65535, 0, 0, 0], [1, 0, 65535, 65535, 0], [2, 65535, 0, 65535, 65535], [8, 5000, 0, 0, 0], [7, 1, 2, 3, 4]] {
        for v in e {
            b.extend(be16(v));
        }
    }
    let p = parse_palette("Brand.aco", &b).unwrap();
    assert_eq!(p[0].name, "Brand");
    // RGB red; HSB hue 0 full → red; CMYK (0 = full ink): w=65535 → c=0, x=0 → m=1, y=65535 → y=0, z=65535 → k=0: magenta; grey 50%.
    assert_eq!(p[0].colors, ["#ff0000", "#ff0000", "#ff00ff", "#808080"]);
    let mut v2 = Vec::new();
    v2.extend(be16(2));
    v2.extend(be16(1));
    for v in [0u16, 0, 0, 65535, 0] {
        v2.extend(be16(v));
    }
    v2.extend(be16(0));
    v2.extend(be16(2));
    v2.extend([0, b'B', 0, 0]);
    let p = parse_palette("b.aco", &v2).unwrap();
    assert_eq!(p[0].colors, ["#0000ff"]);
    assert!(parse_palette("b.aco", &[0, 3, 0, 0]).is_err());
}

#[test]
fn adobe_ase() {
    let mut b = b"ASEF".to_vec();
    b.extend([0, 1, 0, 0]);
    b.extend(4u32.to_be_bytes());
    let color = |model: &[u8; 4], ch: &[f32]| {
        let mut body = Vec::new();
        body.extend(be16(2));
        body.extend([0, b'A', 0, 0]);
        body.extend(model);
        for c in ch {
            body.extend(c.to_be_bytes());
        }
        body.extend(be16(2));
        let mut blk = be16(1).to_vec();
        blk.extend((body.len() as u32).to_be_bytes());
        blk.extend(body);
        blk
    };
    b.extend([0xc0, 0x01, 0, 0, 0, 0]); // group start (flattened)
    b.extend(color(b"RGB ", &[0.0, 1.0, 0.0]));
    b.extend(color(b"CMYK", &[1.0, 0.0, 0.0, 0.0]));
    b.extend(color(b"Gray", &[1.0]));
    let p = parse_palette("Swatch Book.ase", &b).unwrap();
    assert_eq!(p[0].name, "Swatch Book");
    assert_eq!(p[0].colors, ["#00ff00", "#00ffff", "#ffffff"]);
    assert!(parse_palette("x.ase", b"NOPE00000000").is_err());
}

#[test]
fn swatches_round_trip() {
    let colors: Vec<String> = ["#ff0000", "#3a7bd5", "#ffffff", "#000000", "#c0ffee"].iter().map(|s| s.to_string()).collect();
    let bytes = export_swatches("Mine", &colors);
    let entries = read_zip(&bytes).unwrap();
    assert_eq!(entries[0].0, "Swatches.json");
    let json: serde_json::Value = serde_json::from_slice(&entries[0].1).unwrap();
    assert_eq!(json[0]["swatches"].as_array().unwrap().len(), 30, "null-padded to 30 slots");
    let p = parse_palette("x.swatches", &bytes).unwrap();
    assert_eq!(p[0].name, "Mine");
    assert_eq!(p[0].colors.len(), 5);
    for (a, b) in p[0].colors.iter().zip(&colors) {
        let (ca, cb) = (onepaint_color::Rgba::from_hex(a).unwrap(), onepaint_color::Rgba::from_hex(b).unwrap());
        assert!(
            (ca.r - cb.r).abs() <= 1.0 / 255.0 + 1e-6
                && (ca.g - cb.g).abs() <= 1.0 / 255.0 + 1e-6
                && (ca.b - cb.b).abs() <= 1.0 / 255.0 + 1e-6,
            "{a} vs {b}"
        );
    }
}

#[test]
fn reads_deflated_entries_like_procreate_writes() {
    let json = br#"{"name":"","swatches":[{"hue":0.6666667,"saturation":1,"brightness":1},null]}"#;
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(json).unwrap();
    let comp = enc.finish().unwrap();
    let mut crc = flate2::Crc::new();
    crc.update(json);
    let name = b"Swatches.json";
    let mut z = Vec::new();
    let header = |v: &mut Vec<u8>| {
        v.extend(20u16.to_le_bytes());
        v.extend(0u16.to_le_bytes());
        v.extend(8u16.to_le_bytes());
        v.extend(0u32.to_le_bytes());
        v.extend(crc.sum().to_le_bytes());
        v.extend((comp.len() as u32).to_le_bytes());
        v.extend((json.len() as u32).to_le_bytes());
        v.extend((name.len() as u16).to_le_bytes());
        v.extend(0u16.to_le_bytes());
    };
    z.extend(0x0403_4b50u32.to_le_bytes());
    header(&mut z);
    z.extend(name);
    z.extend(&comp);
    let cd = z.len() as u32;
    let mut c = 0x0201_4b50u32.to_le_bytes().to_vec();
    c.extend(20u16.to_le_bytes());
    header(&mut c);
    c.extend([0u8; 10]); // comment len, disk, internal and external attrs
    c.extend(0u32.to_le_bytes()); // local header offset
    c.extend(name);
    z.extend(&c);
    z.extend(0x0605_4b50u32.to_le_bytes());
    z.extend([0u8; 4]);
    z.extend(1u16.to_le_bytes());
    z.extend(1u16.to_le_bytes());
    z.extend((c.len() as u32).to_le_bytes());
    z.extend(cd.to_le_bytes());
    z.extend(0u16.to_le_bytes());
    let p = parse_palette("Blues.swatches", &z).unwrap();
    assert_eq!(p[0].name, "Blues", "empty name falls back to the file name");
    assert_eq!(p[0].colors, ["#0000ff"]);
}

#[test]
fn zip_round_trip_and_unknown_formats() {
    let z = write_zip(&[("a.txt", b"hello"), ("dir/b.bin", &[1, 2, 3])]);
    let e = read_zip(&z).unwrap();
    assert_eq!(e, vec![("a.txt".to_string(), b"hello".to_vec()), ("dir/b.bin".to_string(), vec![1, 2, 3])]);
    assert!(parse_palette("x.pal", b"").unwrap_err().to_string().contains(".swatches"));
}
