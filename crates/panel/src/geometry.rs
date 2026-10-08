//! Panel polygon maths (happypaint `engine/panel/panel-geometry.ts`, G).
//!
//! A panel is a closed polygon in document px. Panels start as rectangles and
//! are made by cutting with straight lines, so they stay convex; `inset_polygon`
//! and `split_polygon_by_line` rely on that (G:12-16).

use serde::{Deserialize, Serialize};

/// One outline vertex in document px (`PanelPoint`, panel-meta.ts:24-27).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn to_array(self) -> [f64; 2] {
        [self.x, self.y]
    }
}

impl From<[f64; 2]> for Pt {
    fn from(p: [f64; 2]) -> Self {
        Self { x: p[0], y: p[1] }
    }
}

/// A rectangle from two opposite corners, clockwise from the top-left (G:25-36).
pub fn rect_outline(a: Pt, b: Pt) -> Vec<Pt> {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    vec![Pt::new(x0, y0), Pt::new(x1, y0), Pt::new(x1, y1), Pt::new(x0, y1)]
}

/// Signed shoelace area; positive is clockwise on screen (G:44-51).
pub fn polygon_area(pts: &[Pt]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        a += (pts[j].x + pts[i].x) * (pts[j].y - pts[i].y);
        j = i;
    }
    a / 2.0
}

/// Even-odd ray cast; points exactly on an edge go either way (G:58-68).
pub fn point_in_polygon(p: Pt, pts: &[Pt]) -> bool {
    let mut inside = false;
    if pts.is_empty() {
        return false;
    }
    let mut j = pts.len() - 1;
    for i in 0..pts.len() {
        let (a, b) = (pts[j], pts[i]);
        j = i;
        if (a.y > p.y) == (b.y > p.y) {
            continue;
        }
        let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
        if p.x < x {
            inside = !inside;
        }
    }
    inside
}

/// Intersection of the lines p + t·r and q + u·s, `None` when parallel (G:71-76).
fn line_intersect(p: Pt, r: Pt, q: Pt, s: Pt) -> Option<Pt> {
    let denom = r.x * s.y - r.y * s.x;
    if denom.abs() < 1e-9 {
        return None;
    }
    let t = ((q.x - p.x) * s.y - (q.y - p.y) * s.x) / denom;
    Some(Pt::new(p.x + t * r.x, p.y + t * r.y))
}

/// Pull every edge inward by `d`, keeping corners sharp; `[]` when the inset
/// consumes the polygon (G:88-132).
pub fn inset_polygon(pts: &[Pt], d: f64) -> Vec<Pt> {
    if pts.len() < 3 || d <= 0.0 {
        return pts.to_vec();
    }
    let positive = polygon_area(pts) > 0.0;
    let ring: Vec<Pt> = if positive { pts.to_vec() } else { pts.iter().rev().copied().collect() };
    let n = ring.len();
    let off = |a: Pt, b: Pt| -> (Pt, Pt) {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = dx.hypot(dy);
        let len = if len == 0.0 { 1.0 } else { len };
        let (nx, ny) = (dy / len * d, -dx / len * d);
        (Pt::new(a.x + nx, a.y + ny), Pt::new(b.x + nx, b.y + ny))
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, cur, next) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
        let (a1, a2) = off(prev, cur);
        let (b1, b2) = off(cur, next);
        let hit = line_intersect(a1, Pt::new(a2.x - a1.x, a2.y - a1.y), b1, Pt::new(b2.x - b1.x, b2.y - b1.y));
        // Parallel edges (a straight join) keep the offset point.
        out.push(hit.unwrap_or(a2));
    }
    if out.len() < 3 || polygon_area(&out).abs() < 1e-9 {
        return Vec::new();
    }
    // A folded edge runs backwards against the edge it came from: offsetting
    // can move an edge, never reverse it.
    for i in 0..n {
        let (oa, ob) = (ring[i], ring[(i + 1) % n]);
        let (na, nb) = (out[i], out[(i + 1) % n]);
        if (ob.x - oa.x) * (nb.x - na.x) + (ob.y - oa.y) * (nb.y - na.y) < 0.0 {
            return Vec::new();
        }
    }
    if !positive {
        out.reverse();
    }
    out
}

/// Cut a convex polygon with the infinite line through `a`→`b` (G:142-170).
/// Returns `(left, right)`; a half with fewer than three points is empty.
pub fn split_polygon_by_line(pts: &[Pt], a: Pt, b: Pt) -> (Vec<Pt>, Vec<Pt>) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if pts.len() < 3 || (dx == 0.0 && dy == 0.0) {
        return (pts.to_vec(), Vec::new());
    }
    let side = |p: Pt| (p.x - a.x) * dy - (p.y - a.y) * dx;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for i in 0..pts.len() {
        let (cur, next) = (pts[i], pts[(i + 1) % pts.len()]);
        let (sc, sn) = (side(cur), side(next));
        if sc <= 0.0 {
            left.push(cur);
        }
        if sc >= 0.0 {
            right.push(cur);
        }
        // Strictly crossing: both halves get the same point, so no sliver.
        if (sc < 0.0 && sn > 0.0) || (sc > 0.0 && sn < 0.0) {
            let t = sc / (sc - sn);
            let p = Pt::new(cur.x + (next.x - cur.x) * t, cur.y + (next.y - cur.y) * t);
            left.push(p);
            right.push(p);
        }
    }
    if left.len() < 3 {
        left.clear();
    }
    if right.len() < 3 {
        right.clear();
    }
    (left, right)
}

/// The ring the border is stroked along and the mask cuts at: the outline
/// inset by half the gutter (G:211-215).
pub fn panel_border_outline(outline: &[Pt], gutter: f64) -> Option<Vec<Pt>> {
    if outline.len() < 3 {
        return None;
    }
    let ring = if gutter > 0.0 { inset_polygon(outline, gutter / 2.0) } else { outline.to_vec() };
    (ring.len() >= 3).then_some(ring)
}

/// The nearest vertex within `max_dist` (inclusive) (G:226-237).
pub fn nearest_vertex(pts: &[Pt], p: Pt, max_dist: f64) -> Option<usize> {
    let mut best = None;
    let mut best_d = max_dist;
    for (i, q) in pts.iter().enumerate() {
        let d = (q.x - p.x).hypot(q.y - p.y);
        if d <= best_d {
            best_d = d;
            best = Some(i);
        }
    }
    best
}

/// Distance from `p` to the segment a→b, clamped to its ends (G:240-249).
pub fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (p.x - a.x).hypot(p.y - a.y);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// Unsigned distance to the nearest edge of the closed polygon (G:258-266).
pub fn distance_to_polygon_edge(pts: &[Pt], p: Pt) -> f64 {
    if pts.len() < 2 {
        return f64::INFINITY;
    }
    (0..pts.len()).map(|i| distance_to_segment(p, pts[i], pts[(i + 1) % pts.len()])).fold(f64::INFINITY, f64::min)
}

/// Slide a whole outline by a delta (G:269-271).
pub fn translate_outline(pts: &[Pt], dx: f64, dy: f64) -> Vec<Pt> {
    pts.iter().map(|q| Pt::new(q.x + dx, q.y + dy)).collect()
}

/// True when every turn has the same sign (collinear turns are ignored).
pub fn is_convex(pts: &[Pt]) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0;
    for i in 0..n {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross.abs() < 1e-9 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// The page split into `rows × cols` cells inside `margin`, row-major from the
/// top-left. The cells tile edge to edge; the gutter comes from the inset at
/// render time (paint-engine.ts:6621-6650).
pub fn grid_outlines(w: f64, h: f64, rows: u32, cols: u32, margin: f64) -> Vec<Vec<Pt>> {
    let (r, c) = (rows.clamp(1, 12), cols.clamp(1, 12));
    let (inner_w, inner_h) = (w - margin * 2.0, h - margin * 2.0);
    if inner_w <= 0.0 || inner_h <= 0.0 {
        return Vec::new();
    }
    let (cell_w, cell_h) = (inner_w / c as f64, inner_h / r as f64);
    let mut out = Vec::with_capacity((r * c) as usize);
    for y in 0..r {
        for x in 0..c {
            let (x0, y0) = (margin + x as f64 * cell_w, margin + y as f64 * cell_h);
            out.push(rect_outline(Pt::new(x0, y0), Pt::new(x0 + cell_w, y0 + cell_h)));
        }
    }
    out
}

/// A page-layout preset offered by the panel tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutPreset {
    pub rows: u32,
    pub cols: u32,
    pub tip: &'static str,
}

/// happypaint `PANEL_LAYOUTS` (toolbar.tsx:324-331).
pub const LAYOUT_PRESETS: [LayoutPreset; 6] = [
    LayoutPreset { rows: 1, cols: 2, tip: "Two panels across" },
    LayoutPreset { rows: 2, cols: 1, tip: "Two panels down" },
    LayoutPreset { rows: 2, cols: 2, tip: "Four panels" },
    LayoutPreset { rows: 3, cols: 1, tip: "Three tiers" },
    LayoutPreset { rows: 3, cols: 2, tip: "Six panels" },
    LayoutPreset { rows: 4, cols: 2, tip: "Eight panels" },
];
