//! Progressive compositing: never block a frame on a big recomposite.
//!
//! Dirty tiles queue up; each frame composites batches (one rayon batch at a
//! time) until a time budget is spent, visible tiles first and nearest the
//! view centre first. The canvas keeps refining over the next frames.

use onepaint_raster::{Rect, TILE};
use std::collections::BTreeSet;

pub const BUDGET_MS: f64 = 10.0;

/// The GPU's share of a frame, in level-0 program ops (one op over one
/// tile). An op took about 19 µs on Iris Xe (`gpu/benches/view.rs`), so
/// this is about 10 ms; a one-layer page still does 256 tiles a frame.
pub const GPU_OPS_PER_FRAME: usize = 512;

/// How many of `costs` (in order) fit `budget`; at least one, so a
/// single tile dearer than the budget still makes progress.
pub fn fit(costs: impl IntoIterator<Item = usize>, budget: usize) -> usize {
    let mut spent = 0;
    let mut n = 0;
    for c in costs {
        spent += c;
        if n > 0 && spent > budget {
            break;
        }
        n += 1;
    }
    n
}

/// Order tiles: inside `visible` first, then by distance from `centre` (doc px).
pub fn prioritize(tiles: &BTreeSet<(i32, i32)>, visible: Rect, centre: [f64; 2]) -> Vec<(i32, i32)> {
    let mut v: Vec<_> = tiles.iter().copied().collect();
    let key = |&(tx, ty): &(i32, i32)| {
        let r = Rect::from_tile(tx, ty);
        let inside = visible.intersect(&r).is_some();
        let (cx, cy) = ((tx * TILE + TILE / 2) as f64, (ty * TILE + TILE / 2) as f64);
        let d = (cx - centre[0]).hypot(cy - centre[1]);
        (!inside, (d * 16.0) as i64)
    };
    v.sort_by_key(key);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_tiles_come_first_then_nearest() {
        let tiles: BTreeSet<_> = [(0, 0), (5, 5), (2, 2), (3, 2), (9, 9)].into_iter().collect();
        let visible = Rect::new(2 * TILE, 2 * TILE, 2 * TILE, TILE);
        let order = prioritize(&tiles, visible, [2.5 * TILE as f64, 2.5 * TILE as f64]);
        assert_eq!(&order[..2], &[(2, 2), (3, 2)]);
        assert_eq!(order[2], (0, 0), "nearest of the rest");
        assert_eq!(order.last(), Some(&(9, 9)));
    }

    #[test]
    fn fit_takes_the_prefix_within_budget() {
        assert_eq!(fit([2; 300], 512), 256, "a one-layer page: 256 tiles");
        assert_eq!(fit([52; 63], 512), 9, "100 layers: about ten tiles");
        assert_eq!(fit([600, 1], 512), 1, "an over-budget tile still runs");
        assert_eq!(fit([], 512), 0);
    }
}
