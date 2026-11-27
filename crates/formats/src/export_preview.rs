//! The pixel rules behind the Export dialog's preview (happypaint
//! `utils/export-preview.ts`, v0.211.12). The preview shows what the file
//! will hold, so it applies the per-format treatment the exporters do: PNG and
//! PSD keep transparency (shown over a checkerboard), JPEG and PPM flatten onto
//! white, and a CMYK TIFF flattens onto white and is soft-proofed through the
//! press.

use onepaint_color::Space;

/// Fit `w × h` inside `max_w × max_h`, never upscaling, at least 1 px each way.
pub fn fit(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    let s = (max_w as f64 / w as f64).min(max_h as f64 / h as f64).min(1.0);
    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

/// A box-filtered, straight-alpha sRGB8 copy of premultiplied `px` (in
/// `space`), fitted inside `max_w × max_h`: the preview's base, made once.
pub fn downscale(px: &[f32], w: u32, h: u32, max_w: u32, max_h: u32, space: Space) -> (u32, u32, Vec<u8>) {
    let (tw, th) = fit(w, h, max_w, max_h);
    if w == 0 || h == 0 || px.len() < (w * h * 4) as usize {
        return (tw, th, vec![0; (tw * th * 4) as usize]);
    }
    let (sx, sy) = (w as f64 / tw as f64, h as f64 / th as f64);
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        let (y0, y1) = ((y as f64 * sy) as u32, (((y + 1) as f64 * sy) as u32).clamp(y + 1, h));
        for x in 0..tw {
            let (x0, x1) = ((x as f64 * sx) as u32, (((x + 1) as f64 * sx) as u32).clamp(x + 1, w));
            let (x0, y0) = (x0.min(w - 1), y0.min(h - 1));
            let mut acc = [0.0f32; 4];
            let mut n = 0.0;
            for yy in y0..y1.max(y0 + 1) {
                let row = (yy * w) as usize;
                for xx in x0..x1.max(x0 + 1) {
                    let i = (row + xx as usize) * 4;
                    for c in 0..4 {
                        acc[c] += px[i + c];
                    }
                    n += 1.0;
                }
            }
            out.extend_from_slice(&space.to_srgb8(acc.map(|v| v / n)));
        }
    }
    (tw, th, out)
}

/// Does this export keep the drawing's transparency? PSD (`psd`) and PNG do;
/// so does an RGB TIFF here (happypaint's TIFF is always CMYK on white).
pub fn keeps_transparency(format: crate::export::ExportFormat, psd: bool, cmyk: bool) -> bool {
    use crate::export::ExportFormat as F;
    psd || format == F::Png || (format == F::Tiff && !cmyk)
}

/// Flatten straight-alpha RGBA8 onto opaque white, in place: what the JPEG,
/// PPM and TIFF exporters get.
pub fn flatten_on_white(rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let a = p[3] as f32 / 255.0;
        for c in &mut p[..3] {
            *c = (*c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
        }
        p[3] = 255;
    }
}

/// Soft-proof RGBA8 (already on white) through `press`, in place: each
/// pixel becomes what the press prints, media-relative, as Print preview
/// shows it. Alpha is left alone.
pub fn proof(press: &onepaint_color::cmyk::Press, rgba: &mut [u8]) {
    for p in rgba.chunks_exact_mut(4) {
        let rgb = [p[0], p[1], p[2]].map(|v| v as f64 / 255.0);
        let (out, _) = onepaint_color::cmyk::proof_with(press, rgb, false, false);
        for c in 0..3 {
            p[c] = (out[c].clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}
