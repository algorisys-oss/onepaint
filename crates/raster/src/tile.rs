//! One 256² tile of premultiplied RGBA pixels, row-major, row 0 at the top.
//!
//! Float documents store linear `f16`. 8-bit documents store bytes the way
//! happypaint's `RGBA8` textures do (spec 49): colour as premultiplied sRGB,
//! masks as raw coverage. Every read decodes to premultiplied linear `f32`,
//! and a write encodes into the tile's own store.

use crate::{Px, TILE, TILE_PX};
use half::f16;
use std::borrow::Cow;
use std::sync::OnceLock;

/// How a tile keeps its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    /// Premultiplied linear light, 8 bytes a pixel.
    F16,
    /// Premultiplied sRGB bytes (`c8 = round(srgb(c / a) · a · 255)`), 4 bytes a pixel.
    Srgb8,
    /// Raw bytes `round(v · 255)` (masks), 4 bytes a pixel.
    Unorm8,
}

#[derive(Clone, PartialEq)]
enum Data {
    F16(Vec<f16>),
    Srgb8(Vec<u8>),
    Unorm8(Vec<u8>),
}

#[derive(Clone, PartialEq)]
pub struct Tile {
    data: Data,
}

impl Default for Tile {
    fn default() -> Self {
        Self { data: Data::F16(vec![f16::ZERO; TILE_PX * 4]) }
    }
}

impl std::fmt::Debug for Tile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tile({:?})", self.store())
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// `srgb_to_linear(c8 / a8) · a8 / 255` for every byte pair, indexed
/// `a8 · 256 + c8` (a constant table, built once).
fn srgb8_table() -> &'static [f32] {
    static T: OnceLock<Vec<f32>> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = vec![0.0; 256 * 256];
        for a in 1..256usize {
            for c in 0..256usize {
                let straight = (c as f32 / a as f32).min(1.0);
                t[a * 256 + c] = srgb_to_linear(straight) * a as f32 / 255.0;
            }
        }
        t
    })
}

#[inline]
fn decode_srgb8(p: &[u8]) -> Px {
    let t = srgb8_table();
    let a = p[3] as usize * 256;
    [t[a + p[0] as usize], t[a + p[1] as usize], t[a + p[2] as usize], p[3] as f32 / 255.0]
}

#[inline]
fn encode_srgb8(p: Px) -> [u8; 4] {
    let a = p[3].clamp(0.0, 1.0);
    let a8 = (a * 255.0).round();
    if a8 <= 0.0 {
        return [0; 4];
    }
    let c = |v: f32| ((linear_to_srgb(v / a).min(1.0) * a * 255.0).round().min(a8)) as u8;
    [c(p[0]), c(p[1]), c(p[2]), a8 as u8]
}

#[inline]
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

impl Tile {
    pub fn filled(p: Px) -> Self {
        let px = p.map(f16::from_f32);
        let mut data = Vec::with_capacity(TILE_PX * 4);
        for _ in 0..TILE_PX {
            data.extend_from_slice(&px);
        }
        Self { data: Data::F16(data) }
    }

    pub fn store(&self) -> Store {
        match self.data {
            Data::F16(_) => Store::F16,
            Data::Srgb8(_) => Store::Srgb8,
            Data::Unorm8(_) => Store::Unorm8,
        }
    }

    /// Bytes the pixels take.
    pub fn byte_size(&self) -> usize {
        match &self.data {
            Data::F16(d) => d.len() * 2,
            Data::Srgb8(d) | Data::Unorm8(d) => d.len(),
        }
    }

    #[inline]
    pub fn get(&self, lx: usize, ly: usize) -> Px {
        let i = (ly * TILE as usize + lx) * 4;
        match &self.data {
            Data::F16(d) => {
                let d = &d[i..i + 4];
                [d[0].to_f32(), d[1].to_f32(), d[2].to_f32(), d[3].to_f32()]
            }
            Data::Srgb8(d) => decode_srgb8(&d[i..i + 4]),
            Data::Unorm8(d) => [0, 1, 2, 3].map(|k| d[i + k] as f32 / 255.0),
        }
    }

    /// Write one pixel, encoded into the tile's store.
    #[inline]
    pub fn set(&mut self, lx: usize, ly: usize, p: Px) {
        let i = (ly * TILE as usize + lx) * 4;
        match &mut self.data {
            Data::F16(d) => {
                for c in 0..4 {
                    d[i + c] = f16::from_f32(p[c]);
                }
            }
            Data::Srgb8(d) => d[i..i + 4].copy_from_slice(&encode_srgb8(p)),
            Data::Unorm8(d) => {
                for c in 0..4 {
                    d[i + c] = byte(p[c]);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.data {
            Data::F16(d) => d.chunks_exact(4).all(|p| p[3].to_f32() <= 0.0),
            Data::Srgb8(d) | Data::Unorm8(d) => d.chunks_exact(4).all(|p| p[3] == 0),
        }
    }

    /// Whether every pixel's alpha is exactly 1.
    pub fn is_opaque(&self) -> bool {
        match &self.data {
            Data::F16(d) => d.chunks_exact(4).all(|p| p[3] == f16::ONE),
            Data::Srgb8(d) | Data::Unorm8(d) => d.chunks_exact(4).all(|p| p[3] == 255),
        }
    }

    /// The tile's pixels as premultiplied linear f32, for bulk processing.
    pub fn to_f32(&self) -> Vec<f32> {
        match &self.data {
            Data::F16(d) => {
                let mut out = vec![0.0; d.len()];
                half::slice::HalfFloatSliceExt::convert_to_f32_slice(&d[..], &mut out);
                out
            }
            Data::Srgb8(d) => d.chunks_exact(4).flat_map(decode_srgb8).collect(),
            Data::Unorm8(d) => d.iter().map(|&v| v as f32 / 255.0).collect(),
        }
    }

    /// A tile from its raw f16 pixels (`TILE_PX * 4` values).
    pub fn from_f16(data: Vec<f16>) -> Self {
        assert_eq!(data.len(), TILE_PX * 4, "a tile holds TILE_PX RGBA pixels");
        Self { data: Data::F16(data) }
    }

    /// The pixels as premultiplied linear f16, row-major (GPU uploads,
    /// recordings); 8-bit tiles are decoded.
    pub fn f16_data(&self) -> Cow<'_, [f16]> {
        match &self.data {
            Data::F16(d) => Cow::Borrowed(d),
            _ => {
                let mut out = vec![f16::ZERO; TILE_PX * 4];
                half::slice::HalfFloatSliceExt::convert_from_f32_slice(&mut out[..], &self.to_f32());
                Cow::Owned(out)
            }
        }
    }

    pub fn from_f32(data: &[f32]) -> Self {
        let mut out = vec![f16::ZERO; TILE_PX * 4];
        half::slice::HalfFloatSliceExt::convert_from_f32_slice(&mut out[..], data);
        Self { data: Data::F16(out) }
    }

    /// The tile in `store` (itself, cloned, when it already is).
    pub fn to_store(&self, store: Store) -> Tile {
        if self.store() == store {
            return self.clone();
        }
        match store {
            Store::F16 => Tile::from_f32(&self.to_f32()),
            Store::Srgb8 => {
                Self { data: Data::Srgb8(self.to_f32().chunks_exact(4).flat_map(|p| encode_srgb8([p[0], p[1], p[2], p[3]])).collect()) }
            }
            Store::Unorm8 => Self { data: Data::Unorm8(self.to_f32().into_iter().map(byte).collect()) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(t: &Tile) -> Vec<u8> {
        match &t.data {
            Data::Srgb8(d) | Data::Unorm8(d) => d.clone(),
            Data::F16(_) => unreachable!("an 8-bit tile"),
        }
    }

    /// Every premultiplied byte pair (c8 ≤ a8) across a tile.
    fn every_srgb8() -> Tile {
        let mut d = Vec::with_capacity(TILE_PX * 4);
        for i in 0..TILE_PX {
            let a = (i % 256) as u8;
            let c = ((i / 256) % 256) as u8;
            let c = c.min(a);
            d.extend_from_slice(&[c, c / 2, a - c / 3, a]);
        }
        Tile { data: Data::Srgb8(d) }
    }

    #[test]
    fn eight_bit_stores_decode_and_encode_back_to_the_same_bytes() {
        let t = every_srgb8();
        let again = Tile::from_f32(&t.to_f32()).to_store(Store::Srgb8);
        assert_eq!(bytes(&again), bytes(&t));
        let m = Tile { data: Data::Unorm8((0..TILE_PX * 4).map(|i| (i * 7 % 256) as u8).collect()) };
        assert_eq!(bytes(&Tile::from_f32(&m.to_f32()).to_store(Store::Unorm8)), bytes(&m));
    }

    #[test]
    fn srgb8_errs_by_under_half_a_step_in_srgb() {
        let mut t = Tile::default();
        for y in 0..256 {
            for x in 0..256 {
                let a = 0.2 + 0.8 * y as f32 / 255.0;
                let s = x as f32 / 255.0 * 0.999 + 0.0004;
                t.set(x, y, [srgb_to_linear(s) * a, srgb_to_linear(1.0 - s) * a, 0.0, a]);
            }
        }
        let q = t.to_store(Store::Srgb8);
        assert_eq!(q.store(), Store::Srgb8);
        for y in (0..256).step_by(5) {
            for x in (0..256).step_by(3) {
                let (want, got) = (t.get(x, y), q.get(x, y));
                assert!((want[3] - got[3]).abs() <= 0.5 / 255.0 + 1e-3);
                // In premultiplied sRGB the error is half a byte (plus alpha's own rounding).
                let enc = |p: Px, c: usize| linear_to_srgb(p[c] / p[3]) * p[3];
                for c in 0..2 {
                    assert!((enc(want, c) - enc(got, c)).abs() <= 1.0 / 255.0 + 2e-3, "{x},{y} ch {c}: {want:?} vs {got:?}");
                }
            }
        }
    }

    #[test]
    fn writes_encode_into_the_tile_store_and_reads_agree() {
        let mut t = Tile::default().to_store(Store::Srgb8);
        t.set(3, 4, [0.2, 0.1, 0.05, 0.5]);
        assert_eq!(t.store(), Store::Srgb8);
        let p = t.get(3, 4);
        assert!((p[3] - 128.0 / 255.0).abs() < 1e-6);
        let f = t.to_f32();
        let i = (4 * 256 + 3) * 4;
        assert_eq!(&f[i..i + 4], &p);
        let h = t.f16_data();
        assert!((h[i].to_f32() - p[0]).abs() < 1e-3);
        assert_eq!(t.byte_size() * 2, Tile::default().byte_size());
        assert!(!t.is_empty() && !t.is_opaque());
        let mut m = Tile::default().to_store(Store::Unorm8);
        m.set(0, 0, [0.5; 4]);
        assert_eq!(m.get(0, 0), [128.0 / 255.0; 4]);
    }
}
