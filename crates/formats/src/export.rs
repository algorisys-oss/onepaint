//! Flat image export (spec 25): PNG, JPEG, TIFF and Netpbm, with scale and DPI.

use crate::icc_embed::embed_icc;
use crate::{FormatError, premul_linear_to_srgb8};
use onepaint_color::cmyk::Press;
use onepaint_color::icc::{RgbSpace, build_icc_profile};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    #[default]
    Png,
    Jpeg,
    Tiff,
    Ppm,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tif",
            Self::Ppm => "ppm",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportOptions {
    pub format: ExportFormat,
    /// Output scale, 0.1..=4.
    pub scale: f64,
    /// JPEG quality 1..=100.
    pub quality: u8,
    /// PPM: P3 (ASCII) instead of P6.
    pub ascii: bool,
    /// TIFF: separate to CMYK through the built-in press (flattened on white).
    pub cmyk: bool,
    pub dpi: f64,
    /// Encode Display-P3 values and tag the profile (a P3 document);
    /// otherwise sRGB. PNG and JPEG carry the profile.
    pub display_p3: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { format: ExportFormat::Png, scale: 1.0, quality: 92, ascii: false, cmyk: false, dpi: 300.0, display_p3: false }
    }
}

pub fn output_size(w: u32, h: u32, scale: f64) -> (u32, u32) {
    let s = scale.clamp(0.1, 4.0);
    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

/// Bilinear resample of premultiplied sRGB floats (no dark fringes).
fn resample(src: &[[f32; 4]], w: u32, h: u32, nw: u32, nh: u32) -> Vec<[f32; 4]> {
    if (nw, nh) == (w, h) {
        return src.to_vec();
    }
    let (sx, sy) = (w as f64 / nw as f64, h as f64 / nh as f64);
    let at = |x: i64, y: i64| src[(y.clamp(0, h as i64 - 1) * w as i64 + x.clamp(0, w as i64 - 1)) as usize];
    let mut out = Vec::with_capacity((nw * nh) as usize);
    for y in 0..nh {
        let fy = (y as f64 + 0.5) * sy - 0.5;
        let (y0, ty) = (fy.floor() as i64, (fy - fy.floor()) as f32);
        for x in 0..nw {
            let fx = (x as f64 + 0.5) * sx - 0.5;
            let (x0, tx) = (fx.floor() as i64, (fx - fx.floor()) as f32);
            let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            out.push(std::array::from_fn(|k| {
                let top = a[k] + (b[k] - a[k]) * tx;
                let bot = c[k] + (d[k] - c[k]) * tx;
                top + (bot - top) * ty
            }));
        }
    }
    out
}

/// Straight 8-bit RGBA at exactly `nw`×`nh` (bilinear) from a flattened
/// canvas; `opaque` flattens onto white (video frames, JPEG).
pub fn flat_to_rgba8(px: &[f32], w: u32, h: u32, nw: u32, nh: u32, opaque: bool) -> Vec<u8> {
    to_rgba8(px, (w, h), (nw, nh), opaque, false)
}

fn to_rgba8(px: &[f32], (w, h): (u32, u32), (nw, nh): (u32, u32), opaque: bool, p3: bool) -> Vec<u8> {
    let src: Vec<[f32; 4]> = px
        .chunks_exact(4)
        .map(|p| {
            let q = if p3 { std::array::from_fn(|i| SRGB_TO_P3[i].iter().zip(p).map(|(m, v)| m * v).sum()) } else { [p[0], p[1], p[2]] };
            let c = premul_linear_to_srgb8([q[0], q[1], q[2], p[3]]);
            let a = c[3] as f32 / 255.0;
            [c[0] as f32 * a, c[1] as f32 * a, c[2] as f32 * a, c[3] as f32]
        })
        .collect();
    resample(&src, w, h, nw, nh)
        .iter()
        .flat_map(|p| {
            let a = p[3] / 255.0;
            let b = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            if opaque {
                // Flatten onto white.
                [b(p[0] + 255.0 * (1.0 - a)), b(p[1] + 255.0 * (1.0 - a)), b(p[2] + 255.0 * (1.0 - a)), 255]
            } else if a > 0.0 {
                [b(p[0] / a), b(p[1] / a), b(p[2] / a), b(p[3])]
            } else {
                [0, 0, 0, 0]
            }
        })
        .collect()
}

/// Linear sRGB → linear Display P3 (happypaint `srgb2displayP3`).
const SRGB_TO_P3: [[f32; 3]; 3] = [[0.822_462_1, 0.177_538, 0.0], [0.033_194_1, 0.966_805_8, 0.0], [0.017_082_7, 0.072_397_4, 0.910_519_9]];

/// Encode a flattened canvas (premultiplied linear f32 RGBA). Returns the
/// bytes and the output size. CMYK TIFF separates through the built-in press.
pub fn export_flat(px: &[f32], w: u32, h: u32, o: &ExportOptions) -> Result<(Vec<u8>, u32, u32), FormatError> {
    export_flat_with(px, w, h, o, onepaint_color::cmyk::builtin_press())
}

/// As `export_flat`, with CMYK TIFF separated through `press`.
pub fn export_flat_with(px: &[f32], w: u32, h: u32, o: &ExportOptions, press: &Press) -> Result<(Vec<u8>, u32, u32), FormatError> {
    let (nw, nh) = output_size(w, h, o.scale);
    // Print separation works from sRGB whatever the document's space.
    let p3 = o.display_p3 && !(o.format == ExportFormat::Tiff && o.cmyk);
    let space = if p3 { RgbSpace::DisplayP3 } else { RgbSpace::Srgb };
    let opaque = matches!(o.format, ExportFormat::Jpeg | ExportFormat::Ppm) || (o.format == ExportFormat::Tiff && o.cmyk);
    let rgba = to_rgba8(px, (w, h), (nw, nh), opaque, p3);
    let bytes = match o.format {
        ExportFormat::Png => {
            let err = |e: png::EncodingError| FormatError::Image(e.to_string());
            let mut out = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, nw, nh);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let ppm = (o.dpi / 0.0254).round() as u32;
                enc.set_pixel_dims(Some(png::PixelDimensions { xppu: ppm, yppu: ppm, unit: png::Unit::Meter }));
                let mut wr = enc.write_header().map_err(err)?;
                wr.write_image_data(&rgba).map_err(err)?;
            }
            embed_icc(&out, &build_icc_profile(space), space.name())
        }
        ExportFormat::Jpeg => {
            use image::codecs::jpeg::{JpegEncoder, PixelDensity};
            let rgb: Vec<u8> = rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
            let mut out = Vec::new();
            let mut enc = JpegEncoder::new_with_quality(&mut out, o.quality.clamp(1, 100));
            enc.set_pixel_density(PixelDensity::dpi(o.dpi.round().clamp(1.0, 65535.0) as u16));
            enc.encode(&rgb, nw, nh, image::ExtendedColorType::Rgb8).map_err(|e| FormatError::Image(e.to_string()))?;
            embed_icc(&out, &build_icc_profile(space), space.name())
        }
        ExportFormat::Tiff if o.cmyk => crate::tiff_cmyk::encode_cmyk_tiff_with(press, &rgba, nw, nh, o.dpi),
        ExportFormat::Tiff => {
            use image::ImageEncoder;
            let mut out = std::io::Cursor::new(Vec::new());
            image::codecs::tiff::TiffEncoder::new(&mut out)
                .write_image(&rgba, nw, nh, image::ExtendedColorType::Rgba8)
                .map_err(|e| FormatError::Image(e.to_string()))?;
            out.into_inner()
        }
        ExportFormat::Ppm => {
            if o.ascii {
                // One "r g b" group per pixel, at most 70 characters a line (netpbm.ts:200-211).
                let mut s = format!("P3\n{nw} {nh}\n255\n");
                let mut line = String::new();
                for p in rgba.chunks_exact(4) {
                    let px = format!("{} {} {}", p[0], p[1], p[2]);
                    if line.len() + px.len() + 1 > 70 {
                        s.push_str(&line);
                        s.push('\n');
                        line.clear();
                    }
                    if !line.is_empty() {
                        line.push(' ');
                    }
                    line.push_str(&px);
                }
                if !line.is_empty() {
                    s.push_str(&line);
                    s.push('\n');
                }
                s.into_bytes()
            } else {
                let mut out = format!("P6\n{nw} {nh}\n255\n").into_bytes();
                out.extend(rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]));
                out
            }
        }
    };
    Ok((bytes, nw, nh))
}
