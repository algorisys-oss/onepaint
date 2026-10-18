//! Mask feather (happypaint `compositor.ts` `lookMask`): the mask blurred
//! with happypaint's separable Gaussian before density applies. Its blur
//! (`adjustment-renderer.ts` gaussian-blur) takes 65 linearly filtered taps
//! across ±r with σ = r / 2, and `r = 2 · feather`, so σ is the feather in
//! doc px. Taps clamp to the document's edge; the mask is zero where it has
//! no tile. The cost per pixel doesn't grow with the feather.

use onepaint_raster::{Surface, TILE, TILE_PX, Tile};
use std::sync::Arc;

/// Taps on each side of the centre.
const HALF: i32 = 32;

/// How far (doc px) a feathered pixel reads the mask.
pub fn reach(feather: f32) -> i32 {
    (2.0 * feather).max(1.0).ceil() as i32 + 1
}

/// The mask tiles feathering one tile reads.
pub struct Window {
    /// The first tile's column and row.
    pub x0: i32,
    pub y0: i32,
    pub cols: usize,
    /// Row by row; `None`: no tile, coverage 0.
    pub tiles: Vec<Option<Arc<Tile>>>,
}

/// The mask tiles feathering tile `(tx, ty)` reads.
pub fn window(mask: &Surface, feather: f32, tx: i32, ty: i32) -> Window {
    let r = reach(feather);
    let span = |t: i32| ((t * TILE - r).div_euclid(TILE), (t * TILE + TILE - 1 + r).div_euclid(TILE));
    let ((x0, x1), (y0, y1)) = (span(tx), span(ty));
    let tiles = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| (x, y))).map(|(x, y)| mask.tile(x, y).cloned()).collect();
    Window { x0, y0, cols: (x1 - x0 + 1) as usize, tiles }
}

/// The feathered coverage of tile `(tx, ty)`, `TILE × TILE` row-major, or
/// `None` when the mask has no tile anywhere it reads (coverage 0 there).
pub fn feathered_tile(mask: &Surface, feather: f32, tx: i32, ty: i32, doc: [u32; 2]) -> Option<Vec<f32>> {
    let Window { x0: wx0, y0: wy0, cols, tiles } = window(mask, feather, tx, ty);
    if tiles.iter().all(Option::is_none) {
        return None;
    }
    let (dw, dh) = (doc[0] as i32, doc[1] as i32);
    if dw <= 0 || dh <= 0 {
        return None;
    }
    // The mask's R channel over the window, one tile at a time.
    let planes: Vec<Option<Vec<f32>>> =
        tiles.iter().map(|t| t.as_ref().map(|t| t.to_f32().chunks_exact(4).map(|p| p[0]).collect())).collect();
    let m = |x: i32, y: i32| -> f32 {
        let (x, y) = (x.clamp(0, dw - 1), y.clamp(0, dh - 1));
        let (i, j) = ((x.div_euclid(TILE) - wx0) as usize, (y.div_euclid(TILE) - wy0) as usize);
        planes.get(j * cols + i).and_then(Option::as_ref).map_or(0.0, |p| p[(y.rem_euclid(TILE) * TILE + x.rem_euclid(TILE)) as usize])
    };
    let r = (2.0 * feather).max(1.0);
    let spread = r / HALF as f32;
    let sigma = HALF as f32 * 0.5;
    let taps: Vec<(f32, f32)> = (-HALF..=HALF).map(|i| (i as f32 * spread, (-0.5 * (i * i) as f32 / (sigma * sigma)).exp())).collect();
    let wsum: f32 = taps.iter().map(|t| t.1).sum();
    // Linear filtering between texel centres, clamped at the edge.
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let (ox, oy) = (tx * TILE, ty * TILE);
    let reach = reach(feather);
    // Pass 1, across: the rows pass 2 reads (clamped into the document).
    let (ry0, ry1) = ((oy - reach).max(0), (oy + TILE - 1 + reach).min(dh - 1));
    let rows = (ry1 - ry0 + 1).max(0) as usize;
    let mut h = vec![0.0f32; rows * TILE as usize];
    for (k, y) in (ry0..=ry1).enumerate() {
        for lx in 0..TILE {
            let x = (ox + lx) as f32;
            let mut s = 0.0;
            for &(off, w) in &taps {
                let u = x + off;
                let j = u.floor();
                let j0 = j as i32;
                s += w * lerp(m(j0, y), m(j0 + 1, y), u - j);
            }
            h[k * TILE as usize + lx as usize] = s / wsum;
        }
    }
    let hv = |x: usize, y: i32| -> f32 {
        if rows == 0 {
            return 0.0;
        }
        let k = (y.clamp(ry0, ry1) - ry0) as usize;
        h[k * TILE as usize + x]
    };
    // Pass 2, down.
    let mut out = vec![0.0f32; TILE_PX];
    for ly in 0..TILE {
        let y = (oy + ly) as f32;
        for lx in 0..TILE as usize {
            let mut s = 0.0;
            for &(off, w) in &taps {
                let v = y + off;
                let j = v.floor();
                let j0 = j as i32;
                s += w * lerp(hv(lx, j0), hv(lx, j0 + 1), v - j);
            }
            out[ly as usize * TILE as usize + lx] = s / wsum;
        }
    }
    Some(out)
}

/// [`feathered_tile`] as a mask tile (coverage in every colour channel),
/// stored like any other mask tile; the CPU and GPU compositors both read it.
pub fn feathered_mask_tile(mask: &Surface, feather: f32, tx: i32, ty: i32, doc: [u32; 2]) -> Option<Tile> {
    let v = feathered_tile(mask, feather, tx, ty, doc)?;
    let px: Vec<f32> = v.into_iter().flat_map(|a| [a, a, a, 1.0]).collect();
    Some(Tile::from_f32(&px))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn half_mask(w: i32) -> Surface {
        // White left of x = w / 2, black right of it, over a w × 64 document.
        let mut s = Surface::new();
        for y in 0..64 {
            for x in 0..w {
                let v = if x < w / 2 { 1.0 } else { 0.0 };
                s.set(x, y, [v, v, v, 1.0]);
            }
        }
        s
    }

    #[test]
    fn a_feathered_edge_ramps_symmetrically_and_flat_areas_stay() {
        let s = half_mask(256);
        let f = feathered_tile(&s, 6.0, 0, 0, [256, 64]).unwrap();
        let at = |x: usize, y: usize| f[y * 256 + x];
        // Far from the edge nothing changes; at the edge it's half.
        assert!((at(10, 30) - 1.0).abs() < 1e-5 && at(250, 30).abs() < 1e-5);
        assert!((at(127, 30) + at(128, 30) - 1.0).abs() < 1e-3, "{} {}", at(127, 30), at(128, 30));
        // Pixel 121 is 6.5 px (1.083 σ) inside the edge at 127.5, and the
        // kernel stops at ±2σ: (Φ(1.083) − Φ(−2)) / (Φ(2) − Φ(−2)) ≈ 0.8778.
        let near = at(121, 30);
        assert!((near - 0.8778).abs() < 2e-3, "{near}");
        // Rows are alike (the mask is constant down, the edge clamps).
        assert!((at(120, 0) - at(120, 63)).abs() < 1e-5);
    }

    #[test]
    fn it_reaches_across_tiles() {
        let mut s = Surface::new();
        for y in 0..512 {
            for x in 0..256 {
                s.set(x, y, [1.0, 1.0, 1.0, 1.0]);
            }
        }
        // Tile (1, 0) has no mask tile but sits next to a white one.
        let f = feathered_tile(&s, 4.0, 1, 0, [512, 512]).unwrap();
        assert!(f[10 * 256] > 0.4 && f[10 * 256] < 0.6, "{}", f[10 * 256]);
        assert!(f[10 * 256 + 40] < 1e-4);
        assert!(feathered_tile(&Surface::new(), 4.0, 1, 0, [512, 512]).is_none());
    }
}
