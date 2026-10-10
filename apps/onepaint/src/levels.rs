//! Which tiles are stale at which level of detail (spec 39). Level `L` is
//! 1/2^L scale; the canvas composites only at the level it displays, and
//! compositing a tile at `L` makes it fresh at `L` and every coarser level
//! (the GPU downsamples it). A tile composited above level 0 is a preview
//! ("rough") until an idle frame composites it at level 0 and downsamples it.

use std::collections::BTreeSet;

/// Coarsest level: a 256² tile is 8 × 8 pixels there.
pub const L_MAX: usize = onepaint_gpu::L_MAX as usize;

type Key = (i32, i32);

#[derive(Clone, Debug, Default)]
pub struct Levels {
    stale: [BTreeSet<Key>; L_MAX + 1],
    /// Fresh only as a reduced-resolution preview.
    rough: BTreeSet<Key>,
}

/// The level shown at `s` document pixels per framebuffer pixel.
pub fn display_level(s: f64) -> usize {
    if s.is_nan() || s <= 0.0 || s >= 1.0 {
        return 0;
    }
    ((1.0 / s).log2().floor() as usize).min(L_MAX)
}

impl Levels {
    /// Edited tiles: stale everywhere.
    pub fn dirty(&mut self, keys: impl IntoIterator<Item = Key>) {
        for k in keys {
            for s in &mut self.stale {
                s.insert(k);
            }
        }
    }

    /// Composited at `level` (then downsampled): fresh there and coarser;
    /// above level 0 it is a preview until refined.
    pub fn composited(&mut self, k: Key, level: usize) {
        for s in &mut self.stale[level.min(L_MAX)..] {
            s.remove(&k);
        }
        if level == 0 {
            self.rough.remove(&k);
        } else {
            self.rough.insert(k);
        }
    }

    /// Previews to refine at level 0 (idle frames, visible first).
    pub fn rough(&self) -> &BTreeSet<Key> {
        &self.rough
    }

    pub fn stale_at(&self, level: usize) -> &BTreeSet<Key> {
        &self.stale[level.min(L_MAX)]
    }

    pub fn clear(&mut self) {
        for s in &mut self.stale {
            s.clear();
        }
        self.rough.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_tiles_freshen_from_their_level_up() {
        let mut l = Levels::default();
        l.dirty([(0, 0), (1, 0)]);
        for lv in 0..=L_MAX {
            assert_eq!(l.stale_at(lv).len(), 2);
        }
        l.composited((0, 0), 2);
        assert!(l.stale_at(1).contains(&(0, 0)) && l.stale_at(0).contains(&(0, 0)));
        assert!((2..=L_MAX).all(|lv| !l.stale_at(lv).contains(&(0, 0))));
        // A CPU upload lands at level 0 and is downsampled: fresh everywhere.
        l.composited((1, 0), 0);
        assert!((0..=L_MAX).all(|lv| !l.stale_at(lv).contains(&(1, 0))));
        l.dirty([(0, 0)]);
        l.clear();
        assert!((0..=L_MAX).all(|lv| l.stale_at(lv).is_empty()));
    }

    #[test]
    fn previews_stay_rough_until_refined_at_level_0() {
        let mut l = Levels::default();
        l.dirty([(2, 2)]);
        l.composited((2, 2), 3);
        assert!(l.rough().contains(&(2, 2)));
        assert!(l.stale_at(0).contains(&(2, 2)), "level 0 still needs it");
        l.composited((2, 2), 0);
        assert!(l.rough().is_empty());
        assert!((0..=L_MAX).all(|lv| l.stale_at(lv).is_empty()));
    }

    #[test]
    fn the_display_level_follows_the_scale() {
        let cases = [(2.0, 0), (1.0, 0), (0.75, 0), (0.5, 1), (0.3, 1), (0.1, 3), (0.01, L_MAX)];
        for (s, want) in cases {
            assert_eq!(display_level(s), want, "scale {s}");
        }
        assert_eq!(display_level(0.0), 0);
        assert_eq!(display_level(f64::NAN), 0);
    }
}
