//! Trace a brush tip's coverage into an outline, so the hover cursor shows
//! the shape the brush stamps (Photoshop's "Normal Brush Tip" cursor).
//! Port of happypaint `engine/brush/tip-outline.ts`.
//!
//! The trace walks texel boundaries at half the peak coverage, links the
//! edges into closed loops, drops specks and simplifies the staircase.
//! Points come out in the tip's 0..1 box.

use crate::{Tip, TipMask, tip_for};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub type Pt = [f64; 2];
/// Closed loops in a tip's 0..1 box.
pub type Outline = Arc<Vec<Vec<Pt>>>;

/// Photoshop's half-coverage contour.
pub const OUTLINE_COVERAGE: f64 = 0.5;
/// Mask px: kills the staircase, keeps the shape.
pub const OUTLINE_EPSILON: f64 = 0.9;
/// Mask px²: smaller specks vanish.
pub const OUTLINE_MIN_AREA: f64 = 4.0;
/// Past this many points the cursor is noise, not a shape.
pub const OUTLINE_MAX_POINTS: usize = 1500;

/// Boundary loops of `data >= threshold` (u8 coverage) in texel-corner
/// coordinates (0..size), first point not repeated. Outer loops wind
/// clockwise in screen space (y down), holes counter-clockwise.
pub fn trace_contours(data: &[u8], size: usize, threshold: u8) -> Vec<Vec<Pt>> {
    let n = size as i64;
    let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < n && y < n && data[(y * n + x) as usize] >= threshold;
    let stride = n + 1;
    // Directed edges with the covered side on the left, keyed by start vertex
    // (insertion-ordered, as happypaint's Map).
    let mut order: Vec<i64> = Vec::new();
    let mut out: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut push = |sx: i64, sy: i64, ex: i64, ey: i64| {
        let k = sy * stride + sx;
        out.entry(k)
            .or_insert_with(|| {
                order.push(k);
                Vec::new()
            })
            .push(ey * stride + ex);
    };
    for y in 0..n {
        for x in 0..n {
            if !inside(x, y) {
                continue;
            }
            if !inside(x, y - 1) {
                push(x, y, x + 1, y);
            }
            if !inside(x + 1, y) {
                push(x + 1, y, x + 1, y + 1);
            }
            if !inside(x, y + 1) {
                push(x + 1, y + 1, x, y + 1);
            }
            if !inside(x - 1, y) {
                push(x, y + 1, x, y);
            }
        }
    }
    let mut loops = Vec::new();
    for start in order {
        while out.get(&start).is_some_and(|v| !v.is_empty()) {
            let mut lp = Vec::new();
            let mut cur = start;
            for _ in 0..=(n * n * 4 + 4) {
                let Some(next) = out.get_mut(&cur).and_then(|v| v.pop()) else { break };
                lp.push([(cur % stride) as f64, (cur / stride) as f64]);
                cur = next;
                if cur == start {
                    break;
                }
            }
            if lp.len() >= 4 {
                loops.push(lp);
            }
        }
    }
    loops
}

/// Signed area of a closed loop (positive = clockwise in screen space).
pub fn loop_area(lp: &[Pt]) -> f64 {
    let mut a = 0.0;
    for i in 0..lp.len() {
        let (p, q) = (lp[i], lp[(i + 1) % lp.len()]);
        a += p[0] * q[1] - q[0] * p[1];
    }
    a / 2.0
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p[0] - a[0]).hypot(p[1] - a[1]);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
    (p[0] - (a[0] + t * dx)).hypot(p[1] - (a[1] + t * dy))
}

/// Ramer–Douglas–Peucker on an open polyline.
fn rdp(pts: &[Pt], eps: f64) -> Vec<Pt> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let last = pts.len() - 1;
    let (mut far, mut max_d) = (0, -1.0);
    for (i, p) in pts.iter().enumerate().take(last).skip(1) {
        let d = seg_dist(*p, pts[0], pts[last]);
        if d > max_d {
            (max_d, far) = (d, i);
        }
    }
    if max_d <= eps {
        return vec![pts[0], pts[last]];
    }
    let mut left = rdp(&pts[..=far], eps);
    left.pop();
    left.extend(rdp(&pts[far..], eps));
    left
}

/// Drop the staircase: RDP anchored at the loop's start.
pub fn simplify_loop(lp: &[Pt], eps: f64) -> Vec<Pt> {
    if lp.len() < 4 {
        return lp.to_vec();
    }
    let mut open = lp.to_vec();
    open.push(lp[0]);
    let mut closed = rdp(&open, eps);
    closed.pop();
    if closed.len() >= 3 { closed } else { lp.to_vec() }
}

/// Outline loops of a coverage mask in its 0..1 box, or `None` when empty
/// or too busy to read.
pub fn mask_outline(data: &[u8], size: usize) -> Option<Vec<Vec<Pt>>> {
    let max = *data.iter().max()?;
    if max == 0 {
        return None;
    }
    let threshold = ((max as f64 * OUTLINE_COVERAGE).round() as u8).max(1);
    let loops: Vec<Vec<Pt>> = trace_contours(data, size, threshold)
        .into_iter()
        .filter(|l| loop_area(l).abs() >= OUTLINE_MIN_AREA)
        .map(|l| simplify_loop(&l, OUTLINE_EPSILON))
        .collect();
    if loops.is_empty() || loops.iter().map(Vec::len).sum::<usize>() > OUTLINE_MAX_POINTS {
        return None;
    }
    let s = size as f64;
    Some(loops.into_iter().map(|l| l.into_iter().map(|p| [p[0] / s, p[1] / s]).collect()).collect())
}

/// The outline of the tip `tip_id` stamps, memoised; `None` for procedural
/// tips (their cursor is exact already) or an unreadable mask.
pub fn tip_outline(tip_id: &str) -> Option<Outline> {
    static C: OnceLock<Mutex<HashMap<String, Option<Outline>>>> = OnceLock::new();
    let cache = C.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok()?.get(tip_id) {
        return hit.clone();
    }
    let path = match tip_for(tip_id) {
        Tip::Image(m) => outline_of(&m),
        _ => None,
    };
    cache.lock().ok()?.insert(tip_id.to_string(), path.clone());
    path
}

fn outline_of(m: &TipMask) -> Option<Outline> {
    mask_outline(&m.to_u8(), m.size).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(size: usize, on: impl Fn(usize, usize) -> bool, v: u8) -> Vec<u8> {
        (0..size * size).map(|i| if on(i % size, i / size) { v } else { 0 }).collect()
    }

    #[test]
    fn contours_match_happypaint() {
        assert!(trace_contours(&mask(8, |_, _| false, 255), 8, 128).is_empty());
        let one = trace_contours(&mask(8, |x, y| x == 3 && y == 2, 255), 8, 128);
        assert_eq!(one.len(), 1);
        let mut c: Vec<_> = one[0].iter().map(|p| (p[0] as i32, p[1] as i32)).collect();
        c.sort();
        assert_eq!(c, [(3, 2), (3, 3), (4, 2), (4, 3)]);
        let block = trace_contours(&mask(8, |x, y| (2..6).contains(&x) && (1..4).contains(&y), 255), 8, 128);
        assert_eq!((block.len(), loop_area(&block[0])), (1, 12.0));
        assert_eq!(trace_contours(&mask(9, |x, y| y == 4 && (x == 1 || x == 7), 255), 9, 128).len(), 2);
        let holed = trace_contours(
            &mask(9, |x, y| (1..8).contains(&x) && (1..8).contains(&y) && !((3..6).contains(&x) && (3..6).contains(&y)), 255),
            9,
            128,
        );
        let mut areas: Vec<f64> = holed.iter().map(|l| loop_area(l)).collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas, [-9.0, 49.0]);
        let mut d = mask(8, |x, y| x == 2 && y == 2, 60);
        d[3 * 8 + 3] = 250;
        let l = trace_contours(&d, 8, 128);
        assert!(l.len() == 1 && l[0].contains(&[3.0, 3.0]));
    }

    #[test]
    fn simplify_and_outline_match_happypaint() {
        let sq = &trace_contours(&mask(8, |x, y| (1..6).contains(&x) && (1..6).contains(&y), 255), 8, 128)[0];
        assert_eq!((sq.len(), simplify_loop(sq, 0.9).len()), (20, 4));
        let l = &trace_contours(&mask(8, |x, y| (2..5).contains(&x) && (2..7).contains(&y), 255), 8, 128)[0];
        assert!((loop_area(&simplify_loop(l, 0.9)) - loop_area(l)).abs() < 1e-6);
        assert!(mask_outline(&mask(8, |_, _| false, 255), 8).is_none());
        let o = mask_outline(&mask(8, |x, y| (2..6).contains(&x) && (2..6).contains(&y), 255), 8).unwrap();
        assert_eq!(o, vec![vec![[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]]]);
        let mut d = mask(16, |x, y| (4..10).contains(&x) && (4..10).contains(&y), 255);
        d[16 + 1] = 255;
        assert_eq!(mask_outline(&d, 16).unwrap().len(), 1, "specks dropped");
        let mut ramp = vec![0u8; 64];
        for y in 3..5 {
            for x in 0..8 {
                ramp[y * 8 + x] = ((x + 1) * 32 - 1) as u8;
            }
        }
        assert!(mask_outline(&ramp, 8).unwrap()[0].contains(&[0.5, 0.375]), "half coverage");
    }

    #[test]
    fn tips_trace_or_keep_the_ring() {
        assert!(tip_outline("round").is_none() && tip_outline("square").is_none() && tip_outline("no-such-tip").is_none());
        let leaf = tip_outline("leaf").unwrap();
        assert!(!leaf.is_empty() && leaf.iter().flatten().all(|p| (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1])));
        assert!(Arc::ptr_eq(&leaf, &tip_outline("leaf").unwrap()), "memoised");
    }
}
