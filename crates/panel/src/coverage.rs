//! The derived clip mask of a panel (panel-geometry.ts:185-198,
//! panel-renderer.ts). Never stored: it is computed from the outline per tile.

use crate::PanelSpec;
use crate::geometry::{Pt, is_convex, panel_border_outline, point_in_polygon};
use onepaint_raster::TILE;

/// Sub-scanlines per pixel row (selection.ts:138).
const SUBSAMPLES: usize = 4;

/// Anti-aliased even-odd coverage of `contours` over the window
/// `[x0, x0 + w) × [y0, y0 + h)` of the page, one byte per pixel.
///
/// A port of `rasterizeContoursCoverage` (selection.ts:276-345), including its
/// f32 row accumulator and its rounding, so any window of the page gets the
/// bytes the whole page would.
pub fn rasterize_coverage(contours: &[Vec<Pt>], x0: i32, y0: i32, w: u32, h: u32) -> Vec<u8> {
    let mut cov = vec![0u8; (w * h) as usize];
    let closed: Vec<&Vec<Pt>> = contours.iter().filter(|c| c.len() >= 3).collect();
    if closed.is_empty() || w == 0 || h == 0 {
        return cov;
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for p in closed.iter().flat_map(|c| c.iter()) {
        (min_x, max_x, min_y, max_y) = (min_x.min(p.x), max_x.max(p.x), min_y.min(p.y), max_y.max(p.y));
    }
    let bx0 = (min_x.floor() as i64).max(x0 as i64);
    let bx1 = (max_x.ceil() as i64).min(x0 as i64 + w as i64 - 1);
    let by0 = (min_y.floor() as i64).max(y0 as i64);
    let by1 = (max_y.ceil() as i64).min(y0 as i64 + h as i64 - 1);
    if bx1 < bx0 || by1 < by0 {
        return cov;
    }
    let mut row = vec![0f32; (bx1 - bx0 + 1) as usize];
    let share = 1.0 / SUBSAMPLES as f64;
    let mut xs: Vec<f64> = Vec::new();
    for y in by0..=by1 {
        row.fill(0.0);
        for s in 0..SUBSAMPLES {
            let cy = y as f64 + (s as f64 + 0.5) * share;
            xs.clear();
            for pts in &closed {
                let mut j = pts.len() - 1;
                for i in 0..pts.len() {
                    let (a, b) = (pts[j], pts[i]);
                    j = i;
                    if (a.y <= cy) == (b.y <= cy) {
                        continue;
                    }
                    xs.push(a.x + (cy - a.y) / (b.y - a.y) * (b.x - a.x));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(f64::total_cmp);
            for pair in xs.chunks_exact(2) {
                let sx = (bx0 as f64).max(pair[0]);
                let ex = (bx1 as f64 + 1.0).min(pair[1]);
                if ex <= sx {
                    continue;
                }
                let px_end = ex.ceil() as i64 - 1;
                for px in (sx.floor() as i64)..=px_end {
                    let overlap = (px as f64 + 1.0).min(ex) - (px as f64).max(sx);
                    if overlap > 0.0 {
                        let k = (px - bx0) as usize;
                        row[k] = (row[k] as f64 + overlap * share) as f32;
                    }
                }
            }
        }
        let base = ((y - y0 as i64) * w as i64) as usize;
        for x in bx0..=bx1 {
            let v = row[(x - bx0) as usize];
            if v <= 0.0 {
                continue;
            }
            let a = if v >= 1.0 { 255 } else { (v as f64 * 255.0).round() as u8 };
            let i = base + (x - x0 as i64) as usize;
            cov[i] = cov[i].max(a);
        }
    }
    cov
}

/// The whole-page mask: the outline inset by half the gutter, or `None` when
/// the input is degenerate or the panel has no interior (G:185-198).
pub fn panel_mask_coverage(outline: &[Pt], gutter: f64, width: u32, height: u32) -> Option<Vec<u8>> {
    if outline.len() < 3 || width == 0 || height == 0 {
        return None;
    }
    let inner = panel_border_outline(outline, gutter)?;
    Some(rasterize_coverage(&[inner], 0, 0, width, height))
}

/// A panel's clip over one tile.
#[derive(Clone, Debug, PartialEq)]
pub enum TileCoverage {
    /// Nothing is clipped on this tile.
    Full,
    /// The tile is entirely outside the frame.
    Empty,
    /// Per-pixel coverage, 0..1, `TILE × TILE`.
    Partial(Vec<f32>),
}

/// The clip of `spec` over tile `(tx, ty)`. A panel with no interior does not
/// clip, like having no panel at all (panel-renderer.ts:396-412).
pub fn tile_coverage(spec: &PanelSpec, tx: i32, ty: i32) -> TileCoverage {
    let Some(inner) = panel_border_outline(&spec.outline, spec.gutter()) else {
        return TileCoverage::Full;
    };
    let (x0, y0) = ((tx * TILE) as f64, (ty * TILE) as f64);
    let (x1, y1) = (x0 + TILE as f64, y0 + TILE as f64);
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for p in &inner {
        (min_x, max_x, min_y, max_y) = (min_x.min(p.x), max_x.max(p.x), min_y.min(p.y), max_y.max(p.y));
    }
    if max_x <= x0 || min_x >= x1 || max_y <= y0 || min_y >= y1 {
        return TileCoverage::Empty;
    }
    // A convex ring holding all four corners holds the whole tile.
    if is_convex(&inner) && [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].iter().all(|&(x, y)| point_in_polygon(Pt::new(x, y), &inner)) {
        return TileCoverage::Full;
    }
    let cov = rasterize_coverage(&[inner], tx * TILE, ty * TILE, TILE as u32, TILE as u32);
    if cov.iter().all(|&c| c == 0) {
        TileCoverage::Empty
    } else if cov.iter().all(|&c| c == 255) {
        TileCoverage::Full
    } else {
        TileCoverage::Partial(cov.into_iter().map(|c| c as f32 / 255.0).collect())
    }
}
