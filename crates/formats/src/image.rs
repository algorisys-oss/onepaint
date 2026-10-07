//! Pixel encoding helpers: straight sRGB 8-bit ⇄ premultiplied linear, PNG and JPEG.

use crate::FormatError;
use onepaint_color::{linear_to_srgb, srgb_to_linear};

fn q8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8
}

/// Premultiplied linear → straight sRGB 8-bit (happypaint `pixelsToCanvas`).
pub fn premul_linear_to_srgb8(p: [f32; 4]) -> [u8; 4] {
    let a = p[3].clamp(0.0, 1.0);
    if a <= 0.0 {
        return [0, 0, 0, 0];
    }
    [
        q8(linear_to_srgb(p[0] / a)),
        q8(linear_to_srgb(p[1] / a)),
        q8(linear_to_srgb(p[2] / a)),
        q8(a),
    ]
}

/// Straight sRGB 8-bit → premultiplied linear.
pub fn srgb8_to_premul_linear(c: [u8; 4]) -> [f32; 4] {
    let a = c[3] as f32 / 255.0;
    let l = |v: u8| srgb_to_linear(v as f32 / 255.0) * a;
    [l(c[0]), l(c[1]), l(c[2]), a]
}

/// Decode any PNG to straight RGBA8.
pub fn decode_png_rgba8(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), FormatError> {
    let err = |e: png::DecodingError| FormatError::Image(e.to_string());
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().map_err(err)?;
    let mut buf = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| FormatError::Image("png too large".into()))?
    ];
    let info = reader.next_frame(&mut buf).map_err(err)?;
    let (w, h) = (info.width, info.height);
    let px = &buf[..info.buffer_size()];
    let n = (w * h) as usize;
    let out = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => px
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err(FormatError::Image("unexpanded palette png".into())),
    };
    if out.len() != n * 4 {
        return Err(FormatError::Image("png size mismatch".into()));
    }
    Ok((w, h, out))
}

pub fn encode_png_rgba8(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, FormatError> {
    let err = |e: png::EncodingError| FormatError::Image(e.to_string());
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
        let mut wr = enc.write_header().map_err(err)?;
        wr.write_image_data(rgba).map_err(err)?;
    }
    Ok(out)
}

/// Encode straight RGB8 as JPEG.
pub fn encode_jpeg_rgb8(w: u16, h: u16, rgb: &[u8], quality: u8) -> Result<Vec<u8>, FormatError> {
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, quality)
        .encode(rgb, w, h, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| FormatError::Image(e.to_string()))?;
    Ok(out)
}

/// A flattened premultiplied-linear canvas as straight sRGB RGBA8.
pub fn flatten_to_srgb8(px: &[f32]) -> Vec<u8> {
    px.chunks_exact(4)
        .flat_map(|p| premul_linear_to_srgb8([p[0], p[1], p[2], p[3]]))
        .collect()
}

/// Downscaled thumbnail fitted inside `max_w × max_h` (never upscaled), over white, as JPEG q80.
pub fn thumbnail_jpeg(
    px: &[f32],
    w: u32,
    h: u32,
    max_w: u32,
    max_h: u32,
) -> Result<Vec<u8>, FormatError> {
    let scale = (max_w as f64 / w as f64)
        .min(max_h as f64 / h as f64)
        .min(1.0);
    let (tw, th) = (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    );
    let mut rgb = Vec::with_capacity((tw * th * 3) as usize);
    for y in 0..th {
        for x in 0..tw {
            // Box filter over the source footprint.
            let (x0, x1) = (
                (x as f64 / scale) as u32,
                (((x + 1) as f64 / scale) as u32).clamp(x + 1, w),
            );
            let (y0, y1) = (
                (y as f64 / scale) as u32,
                (((y + 1) as f64 / scale) as u32).clamp(y + 1, h),
            );
            let (x0, y0) = (x0.min(w - 1), y0.min(h - 1));
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for sy in y0..y1.max(y0 + 1) {
                for sx in x0..x1.max(x0 + 1) {
                    let i = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += px[i + c];
                    }
                    n += 1.0;
                }
            }
            let p = acc.map(|v| v / n);
            let over = [p[0] + 1.0 - p[3], p[1] + 1.0 - p[3], p[2] + 1.0 - p[3], 1.0];
            let s = premul_linear_to_srgb8(over);
            rgb.extend_from_slice(&s[..3]);
        }
    }
    encode_jpeg_rgb8(tw as u16, th as u16, &rgb, 80)
}
