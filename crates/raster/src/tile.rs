//! One 256² tile of premultiplied RGBA pixels, row-major, row 0 at the top.
//!
//! Float documents store `f16`. 8-bit documents store bytes the way
//! happypaint's `RGBA8` textures do (specs 49, 50): the pixel values as they
//! are (premultiplied sRGB for colour, raw coverage for masks), rounded to
//! 1/255. Every read gives f32, and a write encodes into the tile's own store.

use crate::{Px, TILE, TILE_PX};
use half::f16;
use std::borrow::Cow;

/// How a tile keeps its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    /// 8 bytes a pixel.
    F16,
    /// Bytes `round(v · 255)`, 4 bytes a pixel (8-bit documents).
    Unorm8,
}

#[derive(PartialEq)]
enum Data {
    F16(Vec<f16>),
    Unorm8(Vec<u8>),
}

impl Clone for Data {
    fn clone(&self) -> Self {
        match self {
            Data::F16(v) => Data::F16(v.clone()),
            Data::Unorm8(v) => Data::Unorm8(v.clone()),
        }
    }

    /// Reuses this tile's buffer when the stores match: an overwrite with no
    /// allocation (the live taper tail restores tiles this way every move).
    fn clone_from(&mut self, src: &Self) {
        match (self, src) {
            (Data::F16(a), Data::F16(b)) => a.clone_from(b),
            (Data::Unorm8(a), Data::Unorm8(b)) => a.clone_from(b),
            (a, b) => *a = b.clone(),
        }
    }
}

#[derive(PartialEq)]
pub struct Tile {
    data: Data,
}

impl Clone for Tile {
    fn clone(&self) -> Self {
        Self { data: self.data.clone() }
    }

    fn clone_from(&mut self, src: &Self) {
        self.data.clone_from(&src.data);
    }
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
            Data::Unorm8(_) => Store::Unorm8,
        }
    }

    /// The raw bytes of an 8-bit tile (`round(v · 255)` per channel); `None` for f16.
    pub fn unorm8_data(&self) -> Option<&[u8]> {
        match &self.data {
            Data::Unorm8(d) => Some(d),
            Data::F16(_) => None,
        }
    }

    /// Bytes the pixels take.
    pub fn byte_size(&self) -> usize {
        match &self.data {
            Data::F16(d) => d.len() * 2,
            Data::Unorm8(d) => d.len(),
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
            Data::Unorm8(d) => {
                for c in 0..4 {
                    d[i + c] = byte(p[c]);
                }
            }
        }
    }

    /// Pixels `lx0..lx1` of row `ly` as f32 (four a pixel) into `out`,
    /// converted in one batch.
    pub fn read_row(&self, ly: usize, lx0: usize, lx1: usize, out: &mut Vec<f32>) {
        let (a, b) = ((ly * TILE as usize + lx0) * 4, (ly * TILE as usize + lx1) * 4);
        out.resize(b - a, 0.0);
        match &self.data {
            Data::F16(d) => half::slice::HalfFloatSliceExt::convert_to_f32_slice(&d[a..b], out),
            Data::Unorm8(d) => {
                for (o, v) in out.iter_mut().zip(&d[a..b]) {
                    *o = *v as f32 / 255.0;
                }
            }
        }
    }

    /// Write back a row span read with [`Self::read_row`] (exact for the
    /// values it read: f16 and 8-bit round trips through f32 are lossless).
    pub fn write_row(&mut self, ly: usize, lx0: usize, src: &[f32]) {
        let a = (ly * TILE as usize + lx0) * 4;
        let b = a + src.len();
        match &mut self.data {
            Data::F16(d) => half::slice::HalfFloatSliceExt::convert_from_f32_slice(&mut d[a..b], src),
            Data::Unorm8(d) => {
                for (o, v) in d[a..b].iter_mut().zip(src) {
                    *o = byte(*v);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.data {
            Data::F16(d) => d.chunks_exact(4).all(|p| p[3].to_f32() <= 0.0),
            Data::Unorm8(d) => d.chunks_exact(4).all(|p| p[3] == 0),
        }
    }

    /// Whether every pixel's alpha is exactly 1.
    pub fn is_opaque(&self) -> bool {
        match &self.data {
            Data::F16(d) => d.chunks_exact(4).all(|p| p[3] == f16::ONE),
            Data::Unorm8(d) => d.chunks_exact(4).all(|p| p[3] == 255),
        }
    }

    /// The tile's pixels as f32, for bulk processing.
    pub fn to_f32(&self) -> Vec<f32> {
        match &self.data {
            Data::F16(d) => {
                let mut out = vec![0.0; d.len()];
                half::slice::HalfFloatSliceExt::convert_to_f32_slice(&d[..], &mut out);
                out
            }
            Data::Unorm8(d) => d.iter().map(|&v| v as f32 / 255.0).collect(),
        }
    }

    /// A tile from its raw f16 pixels (`TILE_PX * 4` values).
    pub fn from_f16(data: Vec<f16>) -> Self {
        assert_eq!(data.len(), TILE_PX * 4, "a tile holds TILE_PX RGBA pixels");
        Self { data: Data::F16(data) }
    }

    /// The pixels as f16, row-major (GPU uploads, recordings); 8-bit tiles
    /// are widened.
    pub fn f16_data(&self) -> Cow<'_, [f16]> {
        match &self.data {
            Data::F16(d) => Cow::Borrowed(d),
            Data::Unorm8(_) => {
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
            Store::Unorm8 => Self { data: Data::Unorm8(self.to_f32().into_iter().map(byte).collect()) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A row read and written back unchanged leaves the tile as it was, and
    /// matches per-pixel `get`/`set` (the dab rasteriser relies on both).
    #[test]
    fn row_round_trips_are_exact() {
        // Every finite f16 bit pattern, subnormals included.
        let vals: Vec<f16> = (0..TILE_PX as u32 * 4)
            .map(|i| f16::from_bits((i.wrapping_mul(2654435761) >> 16) as u16))
            .map(|v| if v.is_finite() { v } else { f16::ZERO })
            .collect();
        let t = Tile::from_f16(vals);
        let mut row = Vec::new();
        for (ly, a, b) in [(0usize, 0usize, 256usize), (17, 3, 200), (255, 250, 256)] {
            let mut u = t.clone();
            u.read_row(ly, a, b, &mut row);
            for (k, lx) in (a..b).enumerate() {
                assert_eq!(&row[k * 4..k * 4 + 4], &t.get(lx, ly));
            }
            u.write_row(ly, a, &row);
            assert_eq!(u, t);
        }
        let mut e = Tile::from_f32(&vec![0.0; TILE_PX * 4]).to_store(Store::Unorm8);
        for lx in 0..256 {
            e.set(lx, 9, [lx as f32 / 255.0; 4]);
        }
        let mut u = e.clone();
        u.read_row(9, 0, 256, &mut row);
        u.write_row(9, 0, &row);
        assert_eq!(u, e);
    }

    fn bytes(t: &Tile) -> Vec<u8> {
        match &t.data {
            Data::Unorm8(d) => d.clone(),
            Data::F16(_) => unreachable!("an 8-bit tile"),
        }
    }

    #[test]
    fn eight_bit_tiles_widen_and_store_back_to_the_same_bytes() {
        let m = Tile { data: Data::Unorm8((0..TILE_PX * 4).map(|i| (i * 7 % 256) as u8).collect()) };
        // Through f16 (a stroke's working copy) and back.
        assert_eq!(bytes(&Tile::from_f32(&m.to_f32()).to_store(Store::Unorm8)), bytes(&m));
        assert_eq!(bytes(&Tile::from_f16(m.f16_data().into_owned()).to_store(Store::Unorm8)), bytes(&m));
    }

    #[test]
    fn writes_encode_into_the_tile_store_and_reads_agree() {
        let mut t = Tile::default().to_store(Store::Unorm8);
        t.set(3, 4, [0.2, 0.1, 0.05, 0.5]);
        assert_eq!(t.store(), Store::Unorm8);
        let p = t.get(3, 4);
        assert_eq!(p, [51.0 / 255.0, 26.0 / 255.0, 13.0 / 255.0, 128.0 / 255.0]);
        let i = (4 * 256 + 3) * 4;
        assert_eq!(&t.to_f32()[i..i + 4], &p);
        assert!((t.f16_data()[i].to_f32() - p[0]).abs() < 1e-3);
        assert_eq!(t.byte_size() * 2, Tile::default().byte_size());
        assert!(!t.is_empty() && !t.is_opaque());
    }
}
