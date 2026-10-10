//! Sheets: the canvas composite split into equal textures (array layers) for
//! documents larger than the GPU texture limit (spec 40).
//!
//! A document that fits is one sheet of exactly its size, as before. Past
//! the limit, each axis is cut into the fewest sheets that fit, each a
//! multiple of 256 so tiles never straddle a sheet and every level of
//! detail divides evenly. Sheet `(sx, sy)` is array layer `sy * cols + sx`.

use onepaint_raster::TILE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheets {
    /// Document size in pixels.
    pub doc: [u32; 2],
    /// Size of one sheet at level 0.
    pub sheet: [u32; 2],
    /// Sheets across and down.
    pub grid: [u32; 2],
}

impl Sheets {
    /// One sheet covering the document.
    pub fn single(doc: [u32; 2]) -> Self {
        Self { doc, sheet: doc, grid: [1, 1] }
    }

    /// The layout for `doc` within `max_dim` pixels per side and `max_layers`
    /// sheets, or `None` if it can't fit.
    pub fn plan(doc: [u32; 2], max_dim: u32, max_layers: u32) -> Option<Self> {
        if doc[0] == 0 || doc[1] == 0 {
            return None;
        }
        let cap = max_dim / TILE as u32 * TILE as u32;
        let axis = |n: u32| -> Option<(u32, u32)> {
            if n <= max_dim {
                return Some((n, 1));
            }
            if cap == 0 {
                return None;
            }
            let size = n.div_ceil(n.div_ceil(cap)).next_multiple_of(TILE as u32);
            // Rounding up can leave the last sheet empty; count what's used.
            Some((size, n.div_ceil(size)))
        };
        let ((sw, cols), (sh, rows)) = (axis(doc[0])?, axis(doc[1])?);
        (cols * rows <= max_layers).then_some(Self { doc, sheet: [sw, sh], grid: [cols, rows] })
    }

    pub fn count(&self) -> u32 {
        self.grid[0] * self.grid[1]
    }

    /// Document size at `level` (the composite pyramid's level size).
    pub fn doc_at(&self, level: u32) -> [u32; 2] {
        [(self.doc[0] >> level).max(1), (self.doc[1] >> level).max(1)]
    }

    /// Sheet size at `level`.
    pub fn sheet_at(&self, level: u32) -> [u32; 2] {
        [(self.sheet[0] >> level).max(1), (self.sheet[1] >> level).max(1)]
    }

    /// The layer holding document pixel `(x, y)` at level 0, and the pixel
    /// within it.
    pub fn locate(&self, x: u32, y: u32) -> (u32, [u32; 2]) {
        let (sx, sy) = (x / self.sheet[0], y / self.sheet[1]);
        (sy * self.grid[0] + sx, [x - sx * self.sheet[0], y - sy * self.sheet[1]])
    }

    /// The document rectangle `[x, y, w, h]` sheet `layer` covers.
    pub fn rect(&self, layer: u32) -> [u32; 4] {
        let (sx, sy) = (layer % self.grid[0], layer / self.grid[0]);
        let (x, y) = (sx * self.sheet[0], sy * self.sheet[1]);
        [x, y, self.sheet[0].min(self.doc[0] - x), self.sheet[1].min(self.doc[1] - y)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_within_the_limit_is_one_sheet_of_its_size() {
        let s = Sheets::plan([8000, 3001], 8192, 256).unwrap();
        assert_eq!(s, Sheets::single([8000, 3001]));
        assert_eq!(s.locate(7999, 3000), (0, [7999, 3000]));
    }

    #[test]
    fn past_the_limit_each_axis_takes_the_fewest_tile_aligned_sheets() {
        let s = Sheets::plan([20000, 9000], 8192, 256).unwrap();
        assert_eq!(s.grid, [3, 2]);
        assert_eq!(s.sheet, [6912, 4608]);
        assert!(s.sheet.iter().all(|&n| n <= 8192 && n % 256 == 0));
        assert!(s.sheet[0] * s.grid[0] >= 20000 && s.sheet[1] * s.grid[1] >= 9000);
        // Only the axis that overflows is cut.
        let s = Sheets::plan([5000, 9000], 8192, 256).unwrap();
        assert_eq!((s.grid, s.sheet), ([1, 2], [5000, 4608]));
    }

    #[test]
    fn locate_and_rect_agree() {
        let s = Sheets::plan([700, 600], 256, 64).unwrap();
        assert_eq!(s.grid, [3, 3]);
        for (x, y) in [(0, 0), (255, 255), (256, 0), (699, 599), (300, 512)] {
            let (layer, [lx, ly]) = s.locate(x, y);
            let [rx, ry, rw, rh] = s.rect(layer);
            assert_eq!((rx + lx, ry + ly), (x, y));
            assert!(lx < rw && ly < rh);
        }
        assert_eq!(s.rect(8), [512, 512, 188, 88]);
    }

    #[test]
    fn levels_divide_evenly_across_sheets() {
        let s = Sheets::plan([20000, 9000], 8192, 256).unwrap();
        for l in 0..=crate::mip::L_MAX {
            let [sw, _] = s.sheet_at(l);
            assert_eq!(sw << l, s.sheet[0]);
            assert_eq!(s.doc_at(l)[0], 20000 >> l);
        }
    }

    #[test]
    fn too_many_sheets_or_an_empty_document_does_not_fit() {
        assert_eq!(Sheets::plan([100_000, 100_000], 4096, 256), None);
        assert_eq!(Sheets::plan([0, 10], 4096, 256), None);
        assert_eq!(Sheets::plan([300, 10], 200, 256), None);
    }
}
