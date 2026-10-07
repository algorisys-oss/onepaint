//! Shape tool geometry and quick-shape recognition (happypaint
//! `engine/stroke/shape-tool.ts`, `quick-shape.ts`).

use std::f64::consts::{PI, TAU};

pub type Pt = [f64; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShapeKind {
    #[default]
    Line,
    Ellipse,
    Rect,
    Polygon,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Line { a: Pt, b: Pt },
    Rect { cx: f64, cy: f64, hw: f64, hh: f64 },
    Ellipse { cx: f64, cy: f64, rx: f64, ry: f64 },
    Polygon { pts: Vec<Pt> },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragOpts {
    /// Shift: square/circle, 45° lines, 15° polygon rotation.
    pub constrain: bool,
    /// Alt: grow from the press point.
    pub from_center: bool,
    pub sides: u32,
}

impl Default for DragOpts {
    fn default() -> Self {
        Self { constrain: false, from_center: false, sides: 6 }
    }
}

const MIN_DRAG: f64 = 2.0;
const ANGLE_STEP: f64 = PI / 12.0;

pub fn clamp_sides(n: f64) -> u32 {
    if !n.is_finite() { 3 } else { n.round().clamp(3.0, 12.0) as u32 }
}

pub fn snap45(from: Pt, to: Pt) -> Pt {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let len = dx.hypot(dy);
    let a = (dy.atan2(dx) / (PI / 4.0)).round() * (PI / 4.0);
    [from[0] + len * a.cos(), from[1] + len * a.sin()]
}

/// The shape from a drag `a → b` (`dragShape`); `None` for a tap.
pub fn drag_shape(kind: ShapeKind, a: Pt, b: Pt, o: DragOpts) -> Option<Shape> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    if dx.hypot(dy) < MIN_DRAG {
        return None;
    }
    Some(match kind {
        ShapeKind::Line => {
            let from = if o.from_center { [a[0] - dx, a[1] - dy] } else { a };
            Shape::Line { a: from, b: if o.constrain { snap45(from, b) } else { b } }
        }
        ShapeKind::Polygon => {
            let n = o.sides.clamp(3, 12);
            let r = dx.hypot(dy);
            let mut rot = dy.atan2(dx);
            if o.constrain {
                rot = (rot / ANGLE_STEP).round() * ANGLE_STEP;
            }
            Shape::Polygon {
                pts: (0..n)
                    .map(|i| {
                        let t = rot + i as f64 * TAU / n as f64;
                        [a[0] + r * t.cos(), a[1] + r * t.sin()]
                    })
                    .collect(),
            }
        }
        ShapeKind::Rect | ShapeKind::Ellipse => {
            let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
            let (cx, cy, hw, hh) = if o.constrain {
                let s = dx.abs().max(dy.abs());
                let h = if o.from_center { s } else { s / 2.0 };
                if o.from_center { (a[0], a[1], h, h) } else { (a[0] + sign(dx) * h, a[1] + sign(dy) * h, h, h) }
            } else if o.from_center {
                (a[0], a[1], dx.abs(), dy.abs())
            } else {
                (a[0] + dx / 2.0, a[1] + dy / 2.0, dx.abs() / 2.0, dy.abs() / 2.0)
            };
            let (hw, hh) = (hw.max(1.0), hh.max(1.0));
            if kind == ShapeKind::Rect { Shape::Rect { cx, cy, hw, hh } } else { Shape::Ellipse { cx, cy, rx: hw, ry: hh } }
        }
    })
}

fn sample_edges(corners: &[Pt], step: f64, closed: bool) -> Vec<Pt> {
    let mut out = Vec::new();
    let n = corners.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    for i in 0..segs {
        let (a, b) = (corners[i], corners[(i + 1) % n]);
        let k = ((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil().max(1.0) as usize;
        for j in 0..k {
            let f = j as f64 / k as f64;
            out.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
        }
    }
    if let Some(&last) = if closed { corners.first() } else { corners.last() } {
        out.push(last);
    }
    out
}

/// Points along the outline, at most `step` apart; closed shapes repeat the start.
pub fn sample_path(s: &Shape, step: f64) -> Vec<Pt> {
    match s {
        Shape::Line { a, b } => sample_edges(&[*a, *b], step, false),
        Shape::Rect { cx, cy, hw, hh } => {
            sample_edges(&[[cx - hw, cy - hh], [cx + hw, cy - hh], [cx + hw, cy + hh], [cx - hw, cy + hh]], step, true)
        }
        Shape::Polygon { pts } => sample_edges(pts, step, true),
        Shape::Ellipse { cx, cy, rx, ry } => {
            let p = PI * (3.0 * (rx + ry) - ((3.0 * rx + ry) * (rx + 3.0 * ry)).sqrt());
            let n = ((p / step).ceil() as usize).max(16);
            (0..=n)
                .map(|i| {
                    let t = i as f64 / n as f64 * TAU;
                    [cx + rx * t.cos(), cy + ry * t.sin()]
                })
                .collect()
        }
    }
}

/// The closed outline as a polygon (for fills); lines have none.
pub fn outline(s: &Shape) -> Option<Vec<Pt>> {
    match s {
        Shape::Line { .. } => None,
        other => Some(sample_path(other, 2.0)),
    }
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (vx, vy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = vx * vx + vy * vy;
    if l2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p[0] - a[0]) * vx + (p[1] - a[1]) * vy) / l2).clamp(0.0, 1.0);
    dist(p, [a[0] + vx * t, a[1] + vy * t])
}

fn rdp(pts: &[Pt], eps: f64) -> Vec<Pt> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let (a, b) = (pts[0], pts[pts.len() - 1]);
    let (mut idx, mut max) = (0, 0.0);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = seg_dist(*p, a, b);
        if d > max {
            (idx, max) = (i, d);
        }
    }
    if max > eps {
        let mut l = rdp(&pts[..=idx], eps);
        let r = rdp(&pts[idx..], eps);
        l.pop();
        l.extend(r);
        l
    } else {
        vec![a, b]
    }
}

/// Recognise a held stroke as a line, rect, polygon or ellipse (`recognizeShape`).
pub fn recognize(pts: &[Pt], shift: bool) -> Option<Shape> {
    if pts.len() < 8 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in pts {
        (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
    }
    let diag = (x1 - x0).hypot(y1 - y0);
    if diag < 24.0 {
        return None;
    }
    let (start, end) = (pts[0], pts[pts.len() - 1]);
    let closed = dist(start, end) < 0.22 * diag;
    if !closed {
        let len = dist(start, end).max(1e-9);
        let max_perp = pts
            .iter()
            .map(|p| ((end[0] - start[0]) * (start[1] - p[1]) - (start[0] - p[0]) * (end[1] - start[1])).abs() / len)
            .fold(0.0, f64::max);
        if max_perp < 0.1 * len {
            return Some(Shape::Line { a: start, b: if shift { snap45(start, end) } else { end } });
        }
        return None;
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (rx, ry) = (((x1 - x0) / 2.0).max(1.0), ((y1 - y0) / 2.0).max(1.0));
    let ellipse_err =
        pts.iter().map(|p| (((p[0] - cx) / rx).hypot((p[1] - cy) / ry) - 1.0).abs() * rx.min(ry)).sum::<f64>() / pts.len() as f64;
    let mut corners = rdp(pts, 0.04 * diag);
    if corners.len() > 1 && dist(corners[corners.len() - 1], corners[0]) < 0.06 * diag {
        corners.pop();
    }
    let n = corners.len();
    let poly_err = if n < 3 {
        f64::INFINITY
    } else {
        pts.iter().map(|p| (0..n).map(|i| seg_dist(*p, corners[i], corners[(i + 1) % n])).fold(f64::INFINITY, f64::min)).sum::<f64>()
            / pts.len() as f64
    };
    if poly_err < ellipse_err && (3..=8).contains(&n) {
        let bbox = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
        if n == 4 && bbox.iter().all(|b| corners.iter().any(|c| dist(*b, *c) < 0.14 * diag)) {
            let (mut hw, mut hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
            if shift {
                let m = hw.max(hh);
                (hw, hh) = (m, m);
            }
            return Some(Shape::Rect { cx, cy, hw, hh });
        }
        return Some(Shape::Polygon { pts: corners });
    }
    if shift {
        let r = (rx + ry) / 2.0;
        return Some(Shape::Ellipse { cx, cy, rx: r, ry: r });
    }
    Some(Shape::Ellipse { cx, cy, rx, ry })
}
