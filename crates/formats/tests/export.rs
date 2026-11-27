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

#[test]
fn tiff_cmyk_separates_flattened_on_white() {
    let o = ExportOptions { format: ExportFormat::Tiff, cmyk: true, dpi: 300.0, ..Default::default() };
    let (t, w, h) = export_flat(&px(), 2, 1, &o).unwrap();
    assert_eq!((w, h), (2, 1));
    assert_eq!(&t[..4], b"II*\0");
    let strip = &t[t.len() - 8..];
    // Red: magenta and yellow, no cyan; the transparent pixel is paper (no ink).
    assert!(strip[0] < 10 && strip[1] > 200 && strip[2] > 200, "{strip:?}");
    assert_eq!(&strip[4..], &[0, 0, 0, 0]);
}

#[test]
fn exports_are_tagged_and_p3_round_trips_through_import() {
    // Saturated red-ish, which differs in P3.
    let red = [0.6f32, 0.05, 0.02, 1.0];
    for format in [ExportFormat::Png, ExportFormat::Jpeg] {
        let o = ExportOptions { format, quality: 100, display_p3: true, ..Default::default() };
        let (b, _, _) = export_flat(&red, 1, 1, &o).unwrap();
        let back = onepaint_formats::decode_image(&b).unwrap();
        let plain = onepaint_formats::decode_image(
            &export_flat(&red, 1, 1, &ExportOptions { format, quality: 100, ..Default::default() }).unwrap().0,
        )
        .unwrap();
        for (a, b) in back.rgba.iter().zip(&plain.rgba) {
            assert!(a.abs_diff(*b) <= 3, "{format:?}: P3 export reads back as the sRGB one: {:?} vs {:?}", back.rgba, plain.rgba);
        }
        assert!(b.windows(11).any(|w| w == b"ICC_PROFILE") || b.windows(4).any(|w| w == b"iCCP"), "{format:?} is tagged");
    }
}

#[test]
fn a_cmyk_tiff_embeds_a_loaded_press_profile() {
    let Ok(icc) = std::fs::read("/usr/share/color/icc/colord/FOGRA39L_coated.icc") else { return };
    let press = onepaint_color::cmyk::press_from_icc(&icc).unwrap();
    let o = ExportOptions { format: ExportFormat::Tiff, cmyk: true, ..Default::default() };
    let (t, _, _) = onepaint_formats::export::export_flat_with(&px(), 2, 1, &o, &press).unwrap();
    // Tag 34675, type UNDEFINED, pointing at the profile; the strip follows.
    let entry = t.windows(8).position(|w| w[..4] == [0x73, 0x87, 7, 0]).expect("InterColorProfile tag");
    let off = u32::from_le_bytes(t[entry + 8..entry + 12].try_into().unwrap()) as usize;
    assert_eq!(&t[off..off + icc.len()], &icc[..]);
    assert_eq!(t.len(), (off + icc.len()).next_multiple_of(2) + 2 * 4, "word-aligned strip of 2 CMYK pixels");
}

// happypaint `utils/export-preview.ts` (v0.211.12).
mod preview {
    use onepaint_formats::export::ExportFormat as F;
    use onepaint_formats::export_preview::*;

    #[test]
    fn fits_without_upscaling() {
        assert_eq!(fit(4000, 2000, 380, 220), (380, 190));
        assert_eq!(fit(100, 50, 380, 220), (100, 50));
        assert_eq!(fit(0, 10, 380, 220), (1, 1));
        assert_eq!(fit(10000, 10, 380, 220), (380, 1));
    }

    #[test]
    fn which_formats_keep_transparency() {
        assert!(keeps_transparency(F::Png, false, false));
        assert!(keeps_transparency(F::Jpeg, true, false), "PSD");
        assert!(!keeps_transparency(F::Jpeg, false, false));
        assert!(!keeps_transparency(F::Ppm, false, false));
        assert!(!keeps_transparency(F::Tiff, false, true), "CMYK TIFF is on white");
    }

    #[test]
    fn flattening_puts_straight_alpha_over_white() {
        let mut px = [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 255, 128];
        flatten_on_white(&mut px);
        assert_eq!(px, [255, 0, 0, 255, 255, 255, 255, 255, 127, 127, 255, 255]);
    }

    #[test]
    fn downscale_box_filters_premultiplied_pixels() {
        // 2×1: opaque white and clear → one pixel of half-covered white.
        let px = [1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0];
        let (w, h, out) = downscale(&px, 2, 1, 1, 1, onepaint_color::Space::Gamma);
        assert_eq!((w, h), (1, 1));
        assert_eq!(out, vec![255, 255, 255, 128]);
    }

    #[test]
    fn proofing_keeps_white_and_alpha_and_dulls_pure_rgb() {
        let press = onepaint_color::cmyk::builtin_press();
        let mut px = [255, 255, 255, 255, 0, 0, 255, 200];
        proof(press, &mut px);
        assert!(px[..3].iter().all(|&v| v >= 250), "paper white stays white: {px:?}");
        assert_eq!(px[7], 200);
        assert!(px[4] > 10 || px[5] > 10 || px[6] < 245, "pure blue is out of gamut: {px:?}");
    }
}
