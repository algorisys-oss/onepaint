#![allow(clippy::unwrap_used)]
use onepaint_formats::export::*;

/// 2×1: opaque red, then a fully transparent pixel.
fn px() -> Vec<f32> {
    vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]
}

#[test]
fn png_carries_dpi() {
    let (b, w, h) = export_flat(&px(), 2, 1, &ExportOptions { dpi: 300.0, ..Default::default() }).unwrap();
    assert_eq!((w, h), (2, 1));
    let dec = png::Decoder::new(std::io::Cursor::new(&b)).read_info().unwrap();
    let d = dec.info().pixel_dims.unwrap();
    assert_eq!((d.xppu, d.unit), (11811, png::Unit::Meter));
}

#[test]
fn jpeg_is_flattened_on_white_with_density() {
    let opts = ExportOptions { format: ExportFormat::Jpeg, dpi: 144.0, quality: 100, ..Default::default() };
    let (b, _, _) = export_flat(&px(), 2, 1, &opts).unwrap();
    // JFIF APP0: "JFIF\0", version, units (1 = dpi), x and y density.
    let j = b.windows(5).position(|w| w == b"JFIF\0").unwrap();
    assert_eq!(b[j + 7], 1);
    assert_eq!(u16::from_be_bytes([b[j + 8], b[j + 9]]), 144);
    let img = image::load_from_memory(&b).unwrap().to_rgb8();
    assert!(img.get_pixel(1, 0)[0] > 240 && img.get_pixel(1, 0)[2] > 240, "transparent became white");
}

#[test]
fn ppm_binary_and_ascii() {
    let (b, _, _) = export_flat(&px(), 2, 1, &ExportOptions { format: ExportFormat::Ppm, ..Default::default() }).unwrap();
    assert_eq!(b, [b"P6\n2 1\n255\n".as_slice(), &[255, 0, 0, 255, 255, 255]].concat());
    let (a, _, _) = export_flat(&px(), 2, 1, &ExportOptions { format: ExportFormat::Ppm, ascii: true, ..Default::default() }).unwrap();
    assert_eq!(String::from_utf8(a).unwrap(), "P3\n2 1\n255\n255 0 0 255 255 255\n");
}

#[test]
fn scaling_and_tiff() {
    // 4×2: left half opaque white, right half transparent.
    let mut p = Vec::new();
    for _ in 0..2 {
        p.extend([1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }
    let (b, w, h) = export_flat(&p, 4, 2, &ExportOptions { scale: 0.5, ..Default::default() }).unwrap();
    assert_eq!((w, h), (2, 1));
    let img = image::load_from_memory(&b).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]);
    let (t, _, _) = export_flat(&px(), 2, 1, &ExportOptions { format: ExportFormat::Tiff, ..Default::default() }).unwrap();
    let back = image::load_from_memory(&t).unwrap().to_rgba8();
    assert_eq!(back.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(back.get_pixel(1, 0)[3], 0);
    assert_eq!(output_size(1000, 10, 0.001), (100, 1), "clamped to 10%");
}
