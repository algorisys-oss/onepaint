//! The document paper as the brushes and the canvas see it (spec 36):
//! dry dabs skip the tooth's valleys ("the tooth bites"), and the composite
//! shows the tooth lit from the upper left. Happypaint `stamp-renderer.ts:183-225`
//! and `compositor.ts:214-233`.

use crate::TipMask;
use onepaint_doc::Paper;
use onepaint_raster::{Surface, TILE, Tile};
use std::sync::Arc;

/// Dry-stamp strength per unit of paper strength (`DRY_PAPER_SCALE`).
pub const DRY_PAPER_SCALE: f64 = 0.55;
/// Visible-tooth strength per unit of paper strength (`VISIBLE_PAPER_SCALE`).
pub const VISIBLE_PAPER_SCALE: f32 = 0.42;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A paper grain anchored at doc (0, 0): no offset, no rotation.
#[derive(Clone)]
pub struct PaperTexture {
    tex: Arc<TipMask>,
    /// Doc px per tile.
    tile_px: f64,
}

impl PaperTexture {
    pub fn new(p: &Paper) -> Option<Self> {
        let tex = crate::grain_for(&p.grain_id)?;
        let tile_px = (tex.size as f64 * p.scale()).max(1.0);
        Some(Self { tex, tile_px })
    }

    /// Paper height (0 valley … 1 ridge) at doc position (x, y), repeat-sampled.
    #[inline]
    pub fn height(&self, x: f64, y: f64) -> f32 {
        self.tex.sample_repeat(x / self.tile_px, y / self.tile_px)
    }

    /// Doc px per tile (for the wet sim).
    pub fn tile_px(&self) -> f64 {
        self.tile_px
    }

    /// The grain tile itself (GPU upload).
    pub fn mask(&self) -> &TipMask {
        &self.tex
    }
}

/// How hard a dry stroke bites into the paper, and what it bites through:
/// paint already on the layer (and its relief) fills the tooth.
pub struct PaperTooth<'a> {
    pub paper: PaperTexture,
    /// Paper strength × 0.55 × the brush's paper response.
    strength: f32,
    surface: Option<&'a Surface>,
    height: Option<&'a Surface>,
}

/// The surface tiles under one stroke tile.
pub struct ToothTile<'a> {
    paint: Option<&'a Tile>,
    height: Option<&'a Tile>,
}

impl<'a> PaperTooth<'a> {
    /// `None` when the brush ignores paper (response 0) or nothing bites.
    pub fn new(p: &Paper, response: f64, surface: Option<&'a Surface>, height: Option<&'a Surface>) -> Option<Self> {
        let strength = (p.strength() * DRY_PAPER_SCALE * response.clamp(0.0, 1.0)) as f32;
        if strength <= 0.0 {
            return None;
        }
        Some(Self { paper: PaperTexture::new(p)?, strength, surface, height })
    }

    pub fn tile(&self, tx: i32, ty: i32) -> ToothTile<'_> {
        ToothTile {
            paint: self.surface.and_then(|s| s.tile(tx, ty)).map(|t| &**t),
            height: self.height.and_then(|s| s.tile(tx, ty)).map(|t| &**t),
        }
    }

    /// Coverage multiplier at doc pixel (x, y), local (lx, ly) in its tile.
    #[inline]
    pub fn factor(&self, t: &ToothTile, x: i32, y: i32, lx: usize, ly: usize) -> f32 {
        let mut fill = t.paint.map_or(0.0, |p| p.get(lx, ly)[3] * 0.55);
        if let Some(h) = t.height {
            fill = fill.max(smoothstep(0.02, 0.25, h.get(lx, ly)[0]));
        }
        let bite = self.strength * (1.0 - fill);
        let p = self.paper.height(x as f64 + 0.5, y as f64 + 0.5);
        smoothstep(bite - 0.25, bite + 0.05, p).max(0.12)
    }
}

/// The visible tooth over a composited tile (premultiplied linear): rgb ×
/// shade^2.2, darken-only, alpha untouched (`compositor.ts:214-233`).
pub fn shade_tile(buf: &mut [f32], paper: &PaperTexture, strength: f64, tx: i32, ty: i32) {
    let amt = strength.clamp(0.0, 1.0) as f32 * VISIBLE_PAPER_SCALE;
    if amt <= 0.0 {
        return;
    }
    let l = {
        let v = [-0.45f32, -0.6, 0.66];
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|c| c / n)
    };
    for ly in 0..TILE {
        for lx in 0..TILE {
            let i = ((ly * TILE + lx) * 4) as usize;
            if buf[i] == 0.0 && buf[i + 1] == 0.0 && buf[i + 2] == 0.0 {
                continue;
            }
            let (x, y) = ((tx * TILE + lx) as f64 + 0.5, (ty * TILE + ly) as f64 + 0.5);
            let (pl, pr) = (paper.height(x - 1.0, y), paper.height(x + 1.0, y));
            let (pu, pd) = (paper.height(x, y - 1.0), paper.height(x, y + 1.0));
            let (nx, ny) = ((pl - pr) * 2.0, (pu - pd) * 2.0);
            let len = (nx * nx + ny * ny + 1.0).sqrt();
            let lambert = ((nx * l[0] + ny * l[1] + l[2]) / len).clamp(0.0, 1.0);
            let h = paper.height(x, y);
            let shade = 1.0 + (0.55 + 0.45 * (0.35 + 0.65 * lambert) * (0.4 + 0.6 * h) - 1.0) * amt;
            let k = shade.powf(2.2);
            for c in 0..3 {
                buf[i + c] *= k;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rough() -> Paper {
        Paper { grain_id: "rough-tooth".into(), scale: None, strength: 0.85 }
    }

    #[test]
    fn the_tooth_bites_bare_paper_and_spares_filled_paint() {
        let p = rough();
        let bare = PaperTooth::new(&p, 1.0, None, None).unwrap();
        let t = bare.tile(0, 0);
        let f: Vec<f32> = (0..256).map(|x| bare.factor(&t, x, 7, x as usize, 7)).collect();
        let (lo, hi) = f.iter().fold((1.0f32, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(lo < 0.5 && hi > 0.95 && lo >= 0.12, "valleys skip, ridges take paint: {lo}..{hi}");
        // Opaque paint under the dab fills the tooth: the bite eases off.
        let mut full = Surface::new();
        for x in 0..256 {
            full.set(x, 7, [1.0; 4]);
        }
        let filled = PaperTooth::new(&p, 1.0, Some(&full), None).unwrap();
        let t2 = filled.tile(0, 0);
        let g: Vec<f32> = (0..256).map(|x| filled.factor(&t2, x, 7, x as usize, 7)).collect();
        assert!(g.iter().zip(&f).all(|(a, b)| a >= b), "filled paper bites less");
        // A brush that ignores paper, or smooth paper, skips the pass.
        assert!(PaperTooth::new(&p, 0.0, None, None).is_none());
        assert!(PaperTooth::new(&Paper { strength: 0.0, ..rough() }, 1.0, None, None).is_none());
    }

    #[test]
    fn the_visible_tooth_darkens_only_and_keeps_alpha() {
        let tex = PaperTexture::new(&rough()).unwrap();
        let mut buf = [0.8f32, 0.8, 0.8, 1.0].repeat((TILE * TILE) as usize);
        shade_tile(&mut buf, &tex, 0.85, 0, 0);
        let rs: Vec<f32> = buf.chunks_exact(4).map(|p| p[0]).collect();
        let (lo, hi) = rs.iter().fold((1.0f32, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(hi <= 0.8 + 1e-6 && lo < 0.75 && lo > 0.3, "darken only, a few %: {lo}..{hi}");
        assert!(buf.chunks_exact(4).all(|p| p[3] == 1.0));
        // Transparent pixels stay transparent.
        let mut clear = vec![0.0f32; (TILE * TILE * 4) as usize];
        shade_tile(&mut clear, &tex, 0.85, 0, 0);
        assert!(clear.iter().all(|v| *v == 0.0));
    }
}
