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
    /// A procedural paper's sheet look (spec 57).
    look: Option<Arc<crate::PaperLook>>,
}

impl PaperTexture {
    /// The paper at a document's `dpi`: a procedural paper's tile is sized
    /// in millimetres through it, a classic grain's in doc px (`PE:3342`).
    pub fn new(p: &Paper, dpi: f64) -> Option<Self> {
        let tex = crate::grain_for(&p.grain_id)?;
        let physical = crate::grain_unit_mm(&p.grain_id).map_or(1.0, |mm| dpi * mm / 25.4);
        let tile_px = (tex.size as f64 * physical * p.scale()).max(1.0);
        Some(Self { tex, tile_px, look: crate::paper_look(&p.grain_id) })
    }

    pub fn look(&self) -> Option<&crate::PaperLook> {
        self.look.as_deref()
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

    /// The grain tile's handle: a GPU cache holds it, so its address can't
    /// be reused while the cached texture lives.
    pub fn mask_arc(&self) -> &Arc<TipMask> {
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
    pub fn new(p: &Paper, dpi: f64, response: f64, surface: Option<&'a Surface>, height: Option<&'a Surface>) -> Option<Self> {
        let strength = (p.strength() * DRY_PAPER_SCALE * response.clamp(0.0, 1.0)) as f32;
        if strength <= 0.0 {
            return None;
        }
        Some(Self { paper: PaperTexture::new(p, dpi)?, strength, surface, height })
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

/// The visible tooth's amount for a paper strength (0 = none).
pub fn visible_amount(strength: f64) -> f32 {
    strength.clamp(0.0, 1.0) as f32 * VISIBLE_PAPER_SCALE
}

/// The factors the visible tooth multiplies rgb by at doc position (x, y)
/// (pixel centres), for amount `amt` (`compositor.ts:257-285`): the lit
/// tooth, and a procedural paper's mottle and flecks. Linear documents
/// raise it to 2.2; 8-bit documents use it as it is (`u_paperLinear`, spec 50).
pub fn shade_at(paper: &PaperTexture, amt: f32, x: f64, y: f64, space: onepaint_color::Space) -> [f32; 3] {
    let l = {
        let v = [-0.45f32, -0.6, 0.66];
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        v.map(|c| c / n)
    };
    let (pl, pr) = (paper.height(x - 1.0, y), paper.height(x + 1.0, y));
    let (pu, pd) = (paper.height(x, y - 1.0), paper.height(x, y + 1.0));
    let (nx, ny) = ((pl - pr) * 2.0, (pu - pd) * 2.0);
    let len = (nx * nx + ny * ny + 1.0).sqrt();
    let lambert = ((nx * l[0] + ny * l[1] + l[2]) / len).clamp(0.0, 1.0);
    let h = paper.height(x, y);
    let shade = 1.0 + (0.55 + 0.45 * (0.35 + 0.65 * lambert) * (0.4 + 0.6 * h) - 1.0) * amt;
    let mut look = [shade; 3];
    if let Some(l) = paper.look() {
        let (u, v) = (x / paper.tile_px, y / paper.tile_px);
        let (m, f) = (l.mottle_tile.sample_repeat(u, v), l.fleck_tile.sample_repeat(u, v));
        // The paper's strength again (`u_paperAmt / 0.42`).
        let k = amt / VISIBLE_PAPER_SCALE;
        let mottle = 1.0 + (m - 0.5) * l.mottle * 2.0 * k;
        let fl = (f - 0.5) * 2.0 * k;
        for (v, fleck) in look.iter_mut().zip(l.fleck) {
            *v *= mottle * if fl < 0.0 { 1.0 + (fleck - 1.0) * -fl } else { 1.0 + 0.12 * fl };
        }
    }
    if space == onepaint_color::Space::Gamma { look } else { look.map(|v| v.powf(2.2)) }
}

/// The visible tooth over a composited tile (premultiplied linear): rgb ×
/// the shade, kept within alpha, alpha untouched (`compositor.ts:257-285`).
pub fn shade_tile(buf: &mut [f32], paper: &PaperTexture, strength: f64, tx: i32, ty: i32, space: onepaint_color::Space) {
    let amt = visible_amount(strength);
    if amt <= 0.0 {
        return;
    }
    for ly in 0..TILE {
        for lx in 0..TILE {
            let i = ((ly * TILE + lx) * 4) as usize;
            if buf[i] == 0.0 && buf[i + 1] == 0.0 && buf[i + 2] == 0.0 {
                continue;
            }
            let k = shade_at(paper, amt, (tx * TILE + lx) as f64 + 0.5, (ty * TILE + ly) as f64 + 0.5, space);
            for c in 0..3 {
                buf[i + c] = (buf[i + c] * k[c]).min(buf[i + 3]);
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
        let bare = PaperTooth::new(&p, 300.0, 1.0, None, None).unwrap();
        let t = bare.tile(0, 0);
        let f: Vec<f32> = (0..256).map(|x| bare.factor(&t, x, 7, x as usize, 7)).collect();
        let (lo, hi) = f.iter().fold((1.0f32, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(lo < 0.5 && hi > 0.95 && lo >= 0.12, "valleys skip, ridges take paint: {lo}..{hi}");
        // Opaque paint under the dab fills the tooth: the bite eases off.
        let mut full = Surface::new();
        for x in 0..256 {
            full.set(x, 7, [1.0; 4]);
        }
        let filled = PaperTooth::new(&p, 300.0, 1.0, Some(&full), None).unwrap();
        let t2 = filled.tile(0, 0);
        let g: Vec<f32> = (0..256).map(|x| filled.factor(&t2, x, 7, x as usize, 7)).collect();
        assert!(g.iter().zip(&f).all(|(a, b)| a >= b), "filled paper bites less");
        // A brush that ignores paper, or smooth paper, skips the pass.
        assert!(PaperTooth::new(&p, 300.0, 0.0, None, None).is_none());
        assert!(PaperTooth::new(&Paper { strength: 0.0, ..rough() }, 300.0, 1.0, None, None).is_none());
    }

    // happypaint paper-presets.test.ts: every preset's grain resolves, and
    // is sized in millimetres exactly when it is a procedural paper.
    #[test]
    fn every_preset_paper_resolves() {
        for p in onepaint_doc::PAPER_PRESETS.iter().filter_map(|p| p.paper()) {
            assert!(crate::grain_for(&p.grain_id).is_some(), "{}", p.grain_id);
            assert_eq!(crate::grain_unit_mm(&p.grain_id).is_some(), p.grain_id.starts_with("paper-"), "{}", p.grain_id);
        }
        // 0.18 mm units at 300 dpi: a 560-unit tile is ~1191 px.
        let khadi = Paper { grain_id: "paper-khadi".into(), scale: Some(1.0), strength: 0.85 };
        let t = PaperTexture::new(&khadi, 300.0).unwrap();
        assert!((t.tile_px() - 560.0 * 300.0 * 0.18 / 25.4).abs() < 1e-9);
        assert!(t.look().is_some_and(|l| l.mottle == 0.12));
        assert_eq!(PaperTexture::new(&khadi, 150.0).unwrap().tile_px(), t.tile_px() / 2.0);
    }

    #[test]
    fn mottle_and_flecks_tint_the_sheet_within_alpha() {
        let kraft = Paper { grain_id: "paper-kraft".into(), scale: Some(1.0), strength: 0.8 };
        let tex = PaperTexture::new(&kraft, 300.0).unwrap();
        let mut buf = [0.6f32, 0.6, 0.6, 1.0].repeat((TILE * TILE) as usize);
        shade_tile(&mut buf, &tex, 0.8, 0, 0, onepaint_color::Space::Linear);
        // Flecks shift the hue (the fleck colour isn't grey), and nothing passes alpha.
        assert!(buf.chunks_exact(4).any(|p| (p[0] - p[2]).abs() > 1e-3), "flecks colour the sheet");
        assert!(buf.chunks_exact(4).all(|p| p[..3].iter().all(|v| *v <= p[3])));
        let mut opaque_white = [1.0f32; 4].repeat((TILE * TILE) as usize);
        shade_tile(&mut opaque_white, &tex, 0.8, 0, 0, onepaint_color::Space::Linear);
        assert!(opaque_white.iter().all(|v| *v <= 1.0));
    }

    #[test]
    fn the_visible_tooth_darkens_only_and_keeps_alpha() {
        let tex = PaperTexture::new(&rough(), 300.0).unwrap();
        let mut buf = [0.8f32, 0.8, 0.8, 1.0].repeat((TILE * TILE) as usize);
        shade_tile(&mut buf, &tex, 0.85, 0, 0, onepaint_color::Space::Linear);
        let rs: Vec<f32> = buf.chunks_exact(4).map(|p| p[0]).collect();
        let (lo, hi) = rs.iter().fold((1.0f32, 0.0f32), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(hi <= 0.8 + 1e-6 && lo < 0.75 && lo > 0.3, "darken only, a few %: {lo}..{hi}");
        assert!(buf.chunks_exact(4).all(|p| p[3] == 1.0));
        // Transparent pixels stay transparent.
        let mut clear = vec![0.0f32; (TILE * TILE * 4) as usize];
        shade_tile(&mut clear, &tex, 0.85, 0, 0, onepaint_color::Space::Linear);
        assert!(clear.iter().all(|v| *v == 0.0));
    }
}
