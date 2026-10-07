#![allow(clippy::unwrap_used)]
use onepaint_formats::*;

fn gradient(w: u32, h: u32) -> Vec<u8> {
    (0..w * h).flat_map(|i| [(i % w * 255 / w) as u8, (i / w * 255 / h) as u8, 128, 255]).collect()
}

#[test]
fn decodes_png_exactly() {
    let px = gradient(40, 30);
    let png = encode_png_rgba8(40, 30, &px).unwrap();
    let img = decode_image(&png).unwrap();
    assert_eq!((img.width, img.height), (40, 30));
    assert_eq!(img.rgba, px);
}

#[test]
fn decodes_jpeg_approximately() {
    let rgba = gradient(64, 48);
    let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let jpg = encode_jpeg_rgb8(64, 48, &rgb, 95).unwrap();
    let img = decode_image(&jpg).unwrap();
    assert_eq!((img.width, img.height), (64, 48));
    let err: f64 = img.rgba.iter().zip(&rgba).map(|(a, b)| (*a as f64 - *b as f64).abs()).sum::<f64>() / rgba.len() as f64;
    assert!(err < 3.0, "mean error {err}");
    assert!(img.rgba.chunks_exact(4).all(|p| p[3] == 255));
}

#[test]
fn rejects_garbage() {
    assert!(decode_image(b"not an image").is_err());
}

#[test]
fn image_to_surface_premultiplies_into_linear() {
    let img = DecodedImage { width: 2, height: 1, rgba: vec![255, 255, 255, 255, 255, 0, 0, 128] };
    let s = image_to_surface(&img, 10, 20);
    assert_eq!(s.get(10, 20), [1.0, 1.0, 1.0, 1.0]);
    let p = s.get(11, 20);
    assert!((p[3] - 128.0 / 255.0).abs() < 1e-3 && (p[0] - p[3]).abs() < 1e-3 && p[1] == 0.0);
    assert_eq!(s.get(12, 20), [0.0; 4]);
}
