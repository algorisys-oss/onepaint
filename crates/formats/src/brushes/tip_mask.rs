//! Pixels → 128² tip masks and 256² grains (happypaint
//! `engine/brush/tip-mask.ts`, `engine/brush/brush-grain.ts:236-254`).
//! happypaint resamples with Canvas2D; onepaint uses a triangle filter.

use super::abr_mapping::TipTransform;
use image::imageops::{FilterType, resize};
use image::{GrayImage, RgbaImage};

pub const TIP_SIZE: usize = 128;
pub const GRAIN_SIZE: usize = 256;

/// Tip polarity: which tone paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Polarity {
    /// Dark marks paint (`a·(1 − lum)`).
    DarkPaints,
    /// Light marks paint (`a·lum`).
    LightPaints,
    /// Decide per image (`tip-mask.ts:161-180`).
    Auto,
}

fn gray(w: usize, h: usize, data: &[u8]) -> Option<GrayImage> {
    GrayImage::from_raw(w as u32, h as u32, data.to_vec())
}

/// Resize an 8-bit plane.
pub fn resize_gray(data: &[u8], w: usize, h: usize, dw: usize, dh: usize) -> Vec<u8> {
    if (w, h) == (dw, dh) {
        return data.to_vec();
    }
    match gray(w, h, data) {
        Some(img) => resize(&img, dw as u32, dh as u32, FilterType::Triangle).into_raw(),
        None => vec![0; dw * dh],
    }
}

/// Tight bounding box of coverage > 8, contain-fitted and centred into 128²
/// (`tip-mask.ts:20-73`). `None` when nothing is covered.
pub fn coverage_to_tip_mask(cov: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    let mut any = false;
    for (i, &c) in cov.iter().enumerate().take(w * h) {
        if c > 8 {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            any = true;
        }
    }
    if !any {
        return None;
    }
    let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
    let crop: Vec<u8> = (y0..=y1).flat_map(|y| cov[y * w + x0..=y * w + x1].iter().copied()).collect();
    let s = TIP_SIZE as f64;
    let scale = (s / cw as f64).min(s / ch as f64);
    let dw = ((cw as f64 * scale).round() as usize).clamp(1, TIP_SIZE);
    let dh = ((ch as f64 * scale).round() as usize).clamp(1, TIP_SIZE);
    let fitted = resize_gray(&crop, cw, ch, dw, dh);
    let (ox, oy) = ((TIP_SIZE - dw) / 2, (TIP_SIZE - dh) / 2);
    let mut out = vec![0u8; TIP_SIZE * TIP_SIZE];
    for y in 0..dh {
        out[(oy + y) * TIP_SIZE + ox..(oy + y) * TIP_SIZE + ox + dw].copy_from_slice(&fitted[y * dw..(y + 1) * dw]);
    }
    Some(out)
}

/// Bake an ABR tip block's angle, roundness and flips into the coverage
/// (`tip-mask.ts:82-128`): rotate by −angle, squash the height, flip, into
/// the rotated bounding box. Bilinear, zero outside the source.
pub fn transform_coverage(cov: &[u8], w: usize, h: usize, t: &TipTransform) -> (Vec<u8>, usize, usize) {
    let rad = (-t.angle_deg).to_radians();
    let squash = (t.roundness_pct / 100.0).clamp(0.01, 1.0);
    let (c, s) = (rad.cos(), rad.sin());
    let (sw, sh) = (w as f64, h as f64 * squash);
    let ow = ((sw * c.abs() + sh * s.abs()).ceil() as usize).max(1);
    let oh = ((sw * s.abs() + sh * c.abs()).ceil() as usize).max(1);
    let (fx, fy) = (if t.flip_x { -1.0 } else { 1.0 }, (if t.flip_y { -1.0 } else { 1.0 }) * squash);
    let at = |x: i64, y: i64| -> f64 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 { 0.0 } else { cov[y as usize * w + x as usize] as f64 }
    };
    let mut out = vec![0u8; ow * oh];
    for oy in 0..oh {
        for ox in 0..ow {
            // Destination → source: undo the rotation, then the scale.
            let (dx, dy) = (ox as f64 + 0.5 - ow as f64 / 2.0, oy as f64 + 0.5 - oh as f64 / 2.0);
            let (rx, ry) = (c * dx + s * dy, -s * dx + c * dy);
            let (sx, sy) = (rx / fx + w as f64 / 2.0 - 0.5, ry / fy + h as f64 / 2.0 - 0.5);
            let (x0, y0) = (sx.floor(), sy.floor());
            let (ax, ay) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let top = at(x0, y0) * (1.0 - ax) + at(x0 + 1, y0) * ax;
            let bot = at(x0, y0 + 1) * (1.0 - ax) + at(x0 + 1, y0 + 1) * ax;
            out[oy * ow + ox] = (top * (1.0 - ay) + bot * ay).round().clamp(0.0, 255.0) as u8;
        }
    }
    (out, ow, oh)
}

fn luma(r: u8, g: u8, b: u8) -> f64 {
    0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64
}

/// An RGBA image → a tip mask (`tip-mask.ts:135-196`); long side capped at 1024.
pub fn image_to_tip_mask(rgba: &[u8], w: usize, h: usize, polarity: Polarity) -> Option<Vec<u8>> {
    let scale = (1024.0 / w.max(h) as f64).min(1.0);
    let (sw, sh) = (((w as f64 * scale).round() as usize).max(1), ((h as f64 * scale).round() as usize).max(1));
    let img = RgbaImage::from_raw(w as u32, h as u32, rgba.get(..w * h * 4)?.to_vec())?;
    let px = if (sw, sh) == (w, h) { img.into_raw() } else { resize(&img, sw as u32, sh as u32, FilterType::Triangle).into_raw() };
    let n = sw * sh;
    let light = match polarity {
        Polarity::LightPaints => true,
        Polarity::DarkPaints => false,
        Polarity::Auto => {
            // Meaningful transparency → the shape rides alpha and is usually
            // light; otherwise the majority tone is the background.
            let (mut sum, mut opaque, mut clear) = (0.0, 0usize, 0usize);
            for p in px.chunks_exact(4) {
                if p[3] < 16 {
                    clear += 1;
                } else {
                    opaque += 1;
                    sum += luma(p[0], p[1], p[2]);
                }
            }
            let mean = if opaque > 0 { sum / (opaque as f64 * 255.0) } else { 0.0 };
            clear as f64 / n as f64 > 0.15 || mean <= 0.5
        }
    };
    let cov: Vec<u8> = px
        .chunks_exact(4)
        .map(|p| {
            let a = p[3] as f64 / 255.0;
            if a <= 0.0 {
                return 0;
            }
            let lum = luma(p[0], p[1], p[2]) / 255.0;
            ((if light { a * lum } else { a * (1.0 - lum) }) * 255.0).round() as u8
        })
        .collect();
    coverage_to_tip_mask(&cov, sw, sh)
}

/// An RGBA image → a 256² grain: cover-fit, centred, luminance
/// (`brush-grain.ts:236-254`). Transparent pixels read as black.
pub fn image_to_grain(rgba: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let img = RgbaImage::from_raw(w as u32, h as u32, rgba.get(..w * h * 4)?.to_vec())?;
    let s = GRAIN_SIZE as f64;
    let scale = (s / w as f64).max(s / h as f64);
    let dw = ((w as f64 * scale).round() as usize).max(GRAIN_SIZE);
    let dh = ((h as f64 * scale).round() as usize).max(GRAIN_SIZE);
    let big = resize(&img, dw as u32, dh as u32, FilterType::Triangle);
    let (ox, oy) = ((dw - GRAIN_SIZE) / 2, (dh - GRAIN_SIZE) / 2);
    let mut out = Vec::with_capacity(GRAIN_SIZE * GRAIN_SIZE);
    for y in 0..GRAIN_SIZE {
        for x in 0..GRAIN_SIZE {
            let p = big.get_pixel((ox + x) as u32, (oy + y) as u32).0;
            out.push(if p[3] == 0 { 0 } else { luma(p[0], p[1], p[2]).round() as u8 });
        }
    }
    Some(out)
}

/// An 8-bit gray plane → a 256² grain (ABR patterns).
pub fn gray_to_grain(data: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let rgba: Vec<u8> = data.get(..w * h)?.iter().flat_map(|&v| [v, v, v, 255]).collect();
    image_to_grain(&rgba, w, h)
}

/// An image's R channel resized to `size`² (`.happybrush` `pngToBytes`).
pub fn image_red_square(rgba: &[u8], w: usize, h: usize, size: usize) -> Option<Vec<u8>> {
    let red: Vec<u8> = rgba.get(..w * h * 4)?.chunks_exact(4).map(|p| if p[3] == 0 { 0 } else { p[0] }).collect();
    Some(resize_gray(&red, w, h, size, size))
}

/// An 8-bit grayscale PNG.
pub fn encode_gray_png(size: usize, data: &[u8]) -> Result<Vec<u8>, crate::FormatError> {
    let err = |e: png::EncodingError| crate::FormatError::Image(e.to_string());
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, size as u32, size as u32);
        enc.set_color(png::ColorType::Grayscale);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(err)?;
        wr.write_image_data(data).map_err(err)?;
    }
    Ok(out)
}
