//! Pixels as a PSD stores them (happypaint `storage/psd/channels.ts`
//! `encoder`): in the file's own colour mode and depth, so an edited layer
//! goes back into a CMYK, Lab, grey or 16/32-bit file as such.

use crate::psd::packbits;
use crate::psd_write::W;
use onepaint_color::{Space, cmyk};
use std::io::Write;

/// The mode, depth and version a file is written as.
pub(crate) struct Enc {
    /// 1 Grayscale, 3 RGB, 4 CMYK, 7 Multichannel, 8 Duotone, 9 Lab.
    pub mode: u16,
    /// 8, 16 or 32 bits a sample.
    pub depth: u16,
    pub psb: bool,
    /// CMYK: the separation (the file's profile, else the built-in press).
    press: Option<Box<cmyk::Press>>,
}

impl Enc {
    /// 8-bit RGB PSD, or PSB past 30000 px.
    pub fn rgb8(psb: bool) -> Self {
        Self { mode: 3, depth: 8, psb, press: None }
    }

    /// A file's own mode and depth; Indexed and Bitmap become 8-bit RGB.
    pub fn of(mode: u16, depth: u16, psb: bool, icc: Option<&[u8]>) -> Self {
        match mode {
            1 | 3 | 7 | 8 | 9 if matches!(depth, 8 | 16 | 32) => Self { mode, depth, psb, press: None },
            4 if matches!(depth, 8 | 16) => {
                let press = icc.and_then(|b| cmyk::press_from_icc(b).ok()).unwrap_or_else(|| {
                    let p = cmyk::builtin_press();
                    cmyk::Press {
                        sep_lut: p.sep_lut.clone(),
                        proof_lut: Vec::new(),
                        white: p.white,
                        black: p.black,
                        name: p.name.clone(),
                        icc: None,
                    }
                });
                Self { mode, depth, psb, press: Some(Box::new(press)) }
            }
            _ => Self::rgb8(psb),
        }
    }

    /// Colour channels a layer has.
    pub fn colours(&self) -> usize {
        match self.mode {
            3 | 9 => 3,
            4 => 4,
            _ => 1,
        }
    }

    /// One straight colour (gamma sRGB, 0..1) as the file's channel
    /// values, 0..1 (32-bit RGB and grey are linear light).
    pub fn colour(&self, srgb: [f32; 3]) -> [f32; 4] {
        let lin = |v: f32| onepaint_color::srgb_to_linear(v);
        match self.mode {
            3 if self.depth == 32 => [lin(srgb[0]), lin(srgb[1]), lin(srgb[2]), 0.0],
            3 => [srgb[0], srgb[1], srgb[2], 0.0],
            4 => {
                let rgb = srgb.map(f64::from);
                let ink = self.press.as_ref().map_or([0.0; 4], |p| cmyk::separate_with(p, rgb));
                // Stored inverted: 1 is no ink.
                ink.map(|v| (1.0 - v) as f32)
            }
            9 => {
                let lab = cmyk::srgb_to_lab(f64::from(srgb[0]), f64::from(srgb[1]), f64::from(srgb[2]));
                [(lab[0] / 100.0) as f32, ((lab[1] + 128.0) / 255.0) as f32, ((lab[2] + 128.0) / 255.0) as f32, 0.0]
            }
            _ => {
                let c = if self.depth == 32 { srgb.map(lin) } else { srgb };
                [0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2], 0.0, 0.0, 0.0]
            }
        }
    }

    /// Straight colour channels and alpha (`n` per pixel: colours, then
    /// alpha) of premultiplied pixels in `space`.
    pub fn planes(&self, px: &[f32], space: Space) -> Vec<Vec<f32>> {
        let n = self.colours();
        let count = px.len() / 4;
        let mut planes = vec![Vec::with_capacity(count); n + 1];
        for p in px.chunks_exact(4) {
            if self.mode == 3 && self.depth == 8 {
                // 8-bit RGB as it always was written: the document's own 8-bit rounding.
                let c = space.to_srgb8([p[0], p[1], p[2], p[3]]);
                for (k, plane) in planes.iter_mut().enumerate() {
                    plane.push(f32::from(c[k]) / 255.0);
                }
                continue;
            }
            let a = p[3].clamp(0.0, 1.0);
            let straight = if a > 0.0 { [p[0] / a, p[1] / a, p[2] / a] } else { [0.0; 3] };
            let c = self.colour(straight.map(|v| space.to_srgb(v).clamp(0.0, 1.0)));
            for k in 0..n {
                planes[k].push(c[k]);
            }
            planes[n].push(a);
        }
        planes
    }

    /// One row of samples at the depth (big-endian; 32-bit as floats).
    pub fn row(&self, v: &[f32]) -> Vec<u8> {
        match self.depth {
            16 => v.iter().flat_map(|x| ((x.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes()).collect(),
            32 => v.iter().flat_map(|x| x.to_be_bytes()).collect(),
            _ => v.iter().map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8).collect(),
        }
    }

    /// A layer channel's image data (`w` × `h` samples): RLE at 8 bits,
    /// ZIP with prediction at 16 and 32 (as Photoshop and happypaint).
    pub fn channel(&self, v: &[f32], w: usize, h: usize) -> Vec<u8> {
        let rows: Vec<Vec<u8>> = (0..h).map(|y| self.row(&v[y * w..(y + 1) * w])).collect();
        let mut o = W(Vec::new());
        if self.depth == 8 {
            o.u16(1);
            o.bytes(&self.rle_rows(&rows));
            return o.0;
        }
        o.u16(3);
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        for mut r in rows {
            predict(&mut r, w, self.depth);
            // Writing to a Vec can't fail.
            let _ = z.write_all(&r);
        }
        o.bytes(&z.finish().unwrap_or_default());
        o.0
    }

    /// PackBits rows: their byte counts (u16, u32 in a PSB), then the data.
    pub fn rle_rows(&self, rows: &[Vec<u8>]) -> Vec<u8> {
        let enc: Vec<Vec<u8>> = rows.iter().map(|r| packbits(r)).collect();
        let mut o = W(Vec::new());
        for e in &enc {
            self.count(&mut o, e.len());
        }
        for e in enc {
            o.bytes(&e);
        }
        o.0
    }

    /// An RLE row count: u16, or u32 in a PSB.
    pub fn count(&self, o: &mut W, n: usize) {
        if self.psb { o.u32(n as u32) } else { o.u16(n as u16) }
    }

    /// A length field of a PSB's long sections: u64 there, else u32.
    pub fn len(&self, o: &mut W, n: usize) {
        if self.psb { o.bytes(&(n as u64).to_be_bytes()) } else { o.u32(n as u32) }
    }
}

/// ZIP prediction on one row (the reader's `unpredict` undone): byte
/// deltas at 8-bit, big-endian u16 deltas at 16-bit; at 32-bit the floats
/// go byte-planar, then bytewise deltas.
pub(crate) fn predict(r: &mut [u8], w: usize, depth: u16) {
    match depth {
        16 => {
            for x in (1..w).rev() {
                let prev = u16::from_be_bytes([r[(x - 1) * 2], r[(x - 1) * 2 + 1]]);
                let cur = u16::from_be_bytes([r[x * 2], r[x * 2 + 1]]).wrapping_sub(prev);
                r[x * 2..x * 2 + 2].copy_from_slice(&cur.to_be_bytes());
            }
        }
        32 => {
            let packed = r.to_vec();
            for x in 0..w {
                for b in 0..4 {
                    r[b * w + x] = packed[x * 4 + b];
                }
            }
            for x in (1..r.len()).rev() {
                r[x] = r[x].wrapping_sub(r[x - 1]);
            }
        }
        _ => {
            for x in (1..r.len()).rev() {
                r[x] = r[x].wrapping_sub(r[x - 1]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prediction_inverts_at_every_depth() {
        for (depth, w) in [(8u16, 7usize), (16, 5), (32, 3)] {
            let row: Vec<u8> = (0..w * depth as usize / 8).map(|i| (i * 37 + 11) as u8).collect();
            let mut r = row.clone();
            predict(&mut r, w, depth);
            assert_ne!(r, row);
            crate::psd::unpredict(&mut r, w, depth);
            assert_eq!(r, row, "depth {depth}");
        }
    }

    #[test]
    fn colours_by_mode() {
        let white = [1.0f32; 3];
        assert_eq!(Enc::of(1, 8, false, None).colour(white)[0], 1.0);
        let lab = Enc::of(9, 8, false, None).colour([0.5; 3]);
        assert!((lab[0] - 0.534).abs() < 0.01 && (lab[1] - 128.0 / 255.0).abs() < 0.01, "{lab:?}");
        let k = Enc::of(4, 8, false, None).colour([0.0; 3]);
        assert!(k[3] < 0.2, "black takes K ink: {k:?}");
        let paper = Enc::of(4, 8, false, None).colour(white);
        assert!(paper.iter().all(|v| *v > 0.97), "white is no ink: {paper:?}");
        assert_eq!(Enc::of(3, 32, false, None).colour([0.5; 3])[0], onepaint_color::srgb_to_linear(0.5));
        // Indexed and Bitmap fall back to 8-bit RGB.
        let e = Enc::of(2, 8, false, None);
        assert_eq!((e.mode, e.depth), (3, 8));
    }
}
