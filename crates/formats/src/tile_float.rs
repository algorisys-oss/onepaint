//! Exact float tiles for 16- and 32-bit drawings (happypaint
//! `storage/tile-float.ts`, review F07, v0.207.0).
//!
//! PNG holds 8 bits a channel, so saving a float drawing through it merged
//! nearby values and clipped anything brighter than white. A float tile keeps
//! the samples as they are (premultiplied, linear):
//!
//! ```text
//! header (16 bytes, little-endian)
//!   0  "HPFT"        magic
//!   4  u8  format    1 = f16, 2 = f32
//!   5  u8×3          reserved, 0
//!   8  u32 width
//!   12 u32 height
//! body: deflate-raw of the RGBA samples, rows top to bottom, as little-endian
//!       bytes split into byte planes (every sample's first byte, then its
//!       second, …), each byte stored as the difference from the one before.
//! ```

use crate::{FormatError, deflate_raw};
use half::f16;
use std::io::Read;

pub const MAGIC: &[u8; 4] = b"HPFT";
pub const HEADER: usize = 16;
/// Largest side accepted on decode (the container's canvas limit).
const MAX_SIDE: u32 = 32_768;
/// Largest decoded body accepted (an overflow strip can be big): 1 GiB.
const MAX_BODY: usize = 1 << 30;

/// How a float tile stores its samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatFormat {
    F16,
    F32,
}

impl FloatFormat {
    /// The manifest's `fmt` value.
    pub fn as_str(self) -> &'static str {
        match self {
            FloatFormat::F16 => "f16",
            FloatFormat::F32 => "f32",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "f16" => Some(FloatFormat::F16),
            "f32" => Some(FloatFormat::F32),
            _ => None,
        }
    }

    fn bytes(self) -> usize {
        match self {
            FloatFormat::F16 => 2,
            FloatFormat::F32 => 4,
        }
    }
}

fn bad(msg: &str) -> FormatError {
    FormatError::Corrupt(format!("float tile: {msg}"))
}

fn to_planes(bytes: &[u8], width: usize) -> Vec<u8> {
    let n = bytes.len() / width;
    let mut out = Vec::with_capacity(bytes.len());
    let mut prev = 0u8;
    for k in 0..width {
        for i in 0..n {
            let b = bytes[i * width + k];
            out.push(b.wrapping_sub(prev));
            prev = b;
        }
    }
    out
}

fn from_planes(planes: &[u8], width: usize) -> Vec<u8> {
    let n = planes.len() / width;
    let mut out = vec![0u8; planes.len()];
    let mut prev = 0u8;
    let mut o = 0;
    for k in 0..width {
        for i in 0..n {
            prev = prev.wrapping_add(planes[o]);
            o += 1;
            out[i * width + k] = prev;
        }
    }
    out
}

/// Encode `width × height` RGBA samples. f16 rounds to nearest even.
pub fn encode(px: &[f32], width: u32, height: u32, format: FloatFormat) -> Result<Vec<u8>, FormatError> {
    if px.len() != width as usize * height as usize * 4 {
        return Err(bad("size mismatch"));
    }
    let bytes: Vec<u8> = match format {
        FloatFormat::F16 => px.iter().flat_map(|&v| f16::from_f32(v).to_le_bytes()).collect(),
        FloatFormat::F32 => px.iter().flat_map(|v| v.to_le_bytes()).collect(),
    };
    let mut out = Vec::with_capacity(HEADER + bytes.len() / 2);
    out.extend_from_slice(MAGIC);
    out.push(match format {
        FloatFormat::F16 => 1,
        FloatFormat::F32 => 2,
    });
    out.extend_from_slice(&[0; 3]);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&deflate_raw(&to_planes(&bytes, format.bytes())));
    Ok(out)
}

/// A decoded float tile: its size, format and samples widened to f32.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatTile {
    pub width: u32,
    pub height: u32,
    pub format: FloatFormat,
    pub pixels: Vec<f32>,
}

/// Whether `bytes` start like a float tile.
pub fn is_float_tile(bytes: &[u8]) -> bool {
    bytes.len() >= HEADER && &bytes[..4] == MAGIC
}

/// Decode a float tile; a bad header, size or body is an error.
pub fn decode(bytes: &[u8]) -> Result<FloatTile, FormatError> {
    if !is_float_tile(bytes) {
        return Err(bad("not a float tile"));
    }
    let format = match bytes[4] {
        1 => FloatFormat::F16,
        2 => FloatFormat::F32,
        _ => return Err(bad("unknown format")),
    };
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let (width, height) = (u32_at(8), u32_at(12));
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(bad("size out of range"));
    }
    let bps = format.bytes();
    let expect = width as usize * height as usize * 4 * bps;
    if expect > MAX_BODY {
        return Err(bad("too large"));
    }
    let mut planes = Vec::with_capacity(expect);
    flate2::read::DeflateDecoder::new(&bytes[HEADER..])
        .take(expect as u64 + 1)
        .read_to_end(&mut planes)
        .map_err(|_| bad("body would not unpack"))?;
    if planes.len() != expect {
        return Err(bad(if planes.len() > expect { "body past its stated size" } else { "truncated" }));
    }
    let raw = from_planes(&planes, bps);
    let pixels = match format {
        FloatFormat::F16 => raw.chunks_exact(2).map(|b| f16::from_le_bytes([b[0], b[1]]).to_f32()).collect(),
        FloatFormat::F32 => raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
    };
    Ok(FloatTile { width, height, format, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    // tile-float.test.ts
    #[test]
    fn every_half_value_round_trips_exactly() {
        let all: Vec<f32> = (0..=u16::MAX).map(|h| f16::from_bits(h).to_f32()).filter(|v| !v.is_nan()).collect();
        let n = all.len() / 4;
        let px = &all[..n * 4];
        let back = decode(&encode(px, n as u32, 1, FloatFormat::F16).unwrap()).unwrap();
        assert_eq!((back.width, back.height, back.format), (n as u32, 1, FloatFormat::F16));
        for (a, b) in px.iter().zip(&back.pixels) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }

    #[test]
    fn f32_is_bit_exact_and_keeps_values_past_white() {
        let px: Vec<f32> = (0..64 * 4).map(|i| (i as f32 * 0.123_456_7).sin() * 3.0 + 0.5001).collect();
        let back = decode(&encode(&px, 8, 8, FloatFormat::F32).unwrap()).unwrap();
        assert_eq!(back.pixels.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), px.iter().map(|v| v.to_bits()).collect::<Vec<_>>());
        assert!(back.pixels.iter().any(|&v| v > 1.0));
    }

    #[test]
    fn nearby_values_stay_apart() {
        // 8-bit storage made linear 0.5 and 0.5001 the same byte (review F07).
        let px = [0.5, 0.5, 0.5, 1.0, 0.5001, 0.5001, 0.5001, 1.0];
        let back = decode(&encode(&px, 2, 1, FloatFormat::F32).unwrap()).unwrap();
        assert_ne!(back.pixels[0], back.pixels[4]);
    }

    #[test]
    fn header_layout() {
        let t = encode(&[0.0; 4 * 6], 3, 2, FloatFormat::F16).unwrap();
        assert_eq!(&t[..8], b"HPFT\x01\0\0\0");
        assert_eq!(&t[8..16], &[3, 0, 0, 0, 2, 0, 0, 0]);
    }

    #[test]
    fn corrupt_tiles_are_refused() {
        let good = encode(&[0.25; 16], 2, 2, FloatFormat::F16).unwrap();
        assert!(decode(&good[..10]).is_err(), "short header");
        let mut fmt = good.clone();
        fmt[4] = 9;
        assert!(decode(&fmt).is_err(), "unknown format");
        let mut big = good.clone();
        big[8..12].copy_from_slice(&40_000u32.to_le_bytes());
        assert!(decode(&big).is_err(), "side past the limit");
        let mut wrong = good.clone();
        wrong[8] = 3;
        assert!(decode(&wrong).is_err(), "body doesn't match the stated size");
        assert!(decode(&good[..good.len() - 4]).is_err(), "truncated body");
        assert!(encode(&[0.0; 3], 1, 1, FloatFormat::F16).is_err());
    }

    #[test]
    fn planes_round_trip() {
        let b: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        assert_eq!(from_planes(&to_planes(&b, 4), 4), b);
        assert_eq!(from_planes(&to_planes(&b, 2), 2), b);
    }
}
