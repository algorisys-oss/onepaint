//! Cubic-bezier path geometry for the pen tool and path layers
//! (happypaint `engine/stroke/bezier-path.ts`, spec 22). Pure; document space.

use serde::{Deserialize, Serialize};

pub type Pt = [f64; 2];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnchorKind {
    #[default]
    Corner,
    Smooth,
}

/// An editable anchor; handles are offsets from it (absent = straight side).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathAnchor {
    pub x: f64,
    pub y: f64,
    #[serde(rename = "inX", default, skip_serializing_if = "Option::is_none")]
    pub in_x: Option<f64>,
    #[serde(rename = "inY", default, skip_serializing_if = "Option::is_none")]
    pub in_y: Option<f64>,
    #[serde(rename = "outX", default, skip_serializing_if = "Option::is_none")]
    pub out_x: Option<f64>,
    #[serde(rename = "outY", default, skip_serializing_if = "Option::is_none")]
    pub out_y: Option<f64>,
    #[serde(default)]
    pub kind: AnchorKind,
}

impl PathAnchor {
    pub fn corner(x: f64, y: f64) -> Self {
        Self { x, y, ..Default::default() }
    }

    pub fn has_in(&self) -> bool {
        self.in_x.is_some() || self.in_y.is_some()
    }

    pub fn has_out(&self) -> bool {
        self.out_x.is_some() || self.out_y.is_some()
    }

    pub fn in_handle(&self) -> Pt {
        [self.in_x.unwrap_or(0.0), self.in_y.unwrap_or(0.0)]
    }

    pub fn out_handle(&self) -> Pt {
        [self.out_x.unwrap_or(0.0), self.out_y.unwrap_or(0.0)]
    }

    fn clear_handles(&mut self) {
        (self.in_x, self.in_y, self.out_x, self.out_y) = (None, None, None, None);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Subpath {
    pub anchors: Vec<PathAnchor>,
    #[serde(default)]
    pub closed: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathStroke {
    pub brush_id: String,
    pub size: f64,
    /// Straight RGB 0..1.
    pub color: [f32; 3],
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathFill {
    pub color: [f32; 3],
    pub opacity: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PathSpec {
    pub subpaths: Vec<Subpath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<PathStroke>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<PathFill>,
}

// ── Segments ──

pub fn segment_controls(a: &PathAnchor, b: &PathAnchor) -> [Pt; 4] {
    let (o, i) = (a.out_handle(), b.in_handle());
    [[a.x, a.y], [a.x + o[0], a.y + o[1]], [b.x + i[0], b.y + i[1]], [b.x, b.y]]
}

pub fn segment_is_curved(a: &PathAnchor, b: &PathAnchor) -> bool {
    a.has_out() || b.has_in()
}

pub fn cubic_at(p: &[Pt; 4], t: f64) -> Pt {
    let mt = 1.0 - t;
    let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
    [a * p[0][0] + b * p[1][0] + c * p[2][0] + d * p[3][0], a * p[0][1] + b * p[1][1] + c * p[2][1] + d * p[3][1]]
}

fn dist(a: Pt, b: Pt) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Mean of the chord and the control-net length.
pub fn segment_length(p: &[Pt; 4]) -> f64 {
    (dist(p[0], p[3]) + dist(p[0], p[1]) + dist(p[1], p[2]) + dist(p[2], p[3])) / 2.0
}

/// A subpath as a polyline (curved segments sampled every ~`step` px).
pub fn flatten_subpath(sub: &Subpath, step: f64) -> Vec<Pt> {
    let a = &sub.anchors;
    let n = a.len();
    if n == 0 {
        return Vec::new();
    }
    let mut out = vec![[a[0].x, a[0].y]];
    if n == 1 {
        return out;
    }
    let segs = if sub.closed { n } else { n - 1 };
    for s in 0..segs {
        let (p, q) = (&a[s], &a[(s + 1) % n]);
        if !segment_is_curved(p, q) {
            out.push([q.x, q.y]);
            continue;
        }
        let c = segment_controls(p, q);
        let steps = ((segment_length(&c) / step).ceil() as usize).clamp(2, 96);
        out.extend((1..=steps).map(|k| cubic_at(&c, k as f64 / steps as f64)));
    }
    out
}

// ── Editing ──

/// Quantise a handle to the nearest 45°, keeping its length.
pub fn constrain_handle(dx: f64, dy: f64) -> Pt {
    let len = dx.hypot(dy);
    if len < 1e-6 {
        return [dx, dy];
    }
    let step = std::f64::consts::FRAC_PI_4;
    let ang = (dy.atan2(dx) / step).round() * step;
    [ang.cos() * len, ang.sin() * len]
}

/// Smooth anchors through `points` (Catmull-Rom → bezier, handles = chord / 6).
pub fn catmull_rom_anchors(points: &[Pt], closed: bool) -> Vec<PathAnchor> {
    let n = points.len();
    if n < 2 {
        return points.iter().map(|p| PathAnchor::corner(p[0], p[1])).collect();
    }
    let at = |i: i64| -> Pt { if closed { points[i.rem_euclid(n as i64) as usize] } else { points[i.clamp(0, n as i64 - 1) as usize] } };
    (0..n)
        .map(|i| {
            let (p, prev, next) = (points[i], at(i as i64 - 1), at(i as i64 + 1));
            let (tx, ty) = ((next[0] - prev[0]) / 6.0, (next[1] - prev[1]) / 6.0);
            let is_end = !closed && (i == 0 || i == n - 1);
            PathAnchor {
                x: p[0],
                y: p[1],
                in_x: Some(-tx),
                in_y: Some(-ty),
                out_x: Some(tx),
                out_y: Some(ty),
                kind: if is_end && tx == 0.0 && ty == 0.0 { AnchorKind::Corner } else { AnchorKind::Smooth },
            }
        })
        .collect()
}

pub fn move_anchor(anchors: &mut [PathAnchor], i: usize, x: f64, y: f64) {
    if let Some(a) = anchors.get_mut(i) {
        (a.x, a.y) = (x, y);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleSide {
    In,
    Out,
}

/// Set a handle; a smooth anchor mirrors the partner.
pub fn set_handle(anchors: &mut [PathAnchor], i: usize, side: HandleSide, h: Pt) {
    let Some(a) = anchors.get_mut(i) else { return };
    let smooth = a.kind == AnchorKind::Smooth;
    match side {
        HandleSide::Out => {
            (a.out_x, a.out_y) = (Some(h[0]), Some(h[1]));
            if smooth {
                (a.in_x, a.in_y) = (Some(-h[0]), Some(-h[1]));
            }
        }
        HandleSide::In => {
            (a.in_x, a.in_y) = (Some(h[0]), Some(h[1]));
            if smooth {
                (a.out_x, a.out_y) = (Some(-h[0]), Some(-h[1]));
            }
        }
    }
}

/// Drop anchor `i`'s out-handle and make it a corner, keeping the in-handle:
/// Photoshop's (Alt+)click on the anchor just drawn, so the next segment
/// leaves it straight instead of carrying the curve on (happypaint v0.204.13).
pub fn cut_out_handle(anchors: &mut [PathAnchor], i: usize) {
    let Some(a) = anchors.get_mut(i) else { return };
    (a.out_x, a.out_y) = (None, None);
    a.kind = AnchorKind::Corner;
}

/// Set anchor `i`'s out-handle alone, as a corner (the in-handle is not
/// mirrored): Photoshop's Alt+drag, aiming the next segment without bending
/// the last.
pub fn set_out_handle_only(anchors: &mut [PathAnchor], i: usize, h: Pt) {
    let Some(a) = anchors.get_mut(i) else { return };
    (a.out_x, a.out_y) = (Some(h[0]), Some(h[1]));
    a.kind = AnchorKind::Corner;
}

/// Corner ↔ smooth (smoothing derives ⅓-chord handles along the neighbours' tangent).
pub fn toggle_anchor_kind(anchors: &mut [PathAnchor], i: usize) {
    let n = anchors.len();
    if i >= n {
        return;
    }
    if anchors[i].kind == AnchorKind::Smooth {
        anchors[i].clear_handles();
        anchors[i].kind = AnchorKind::Corner;
        return;
    }
    let (prev, next, a) = (anchors[(i + n - 1) % n], anchors[(i + 1) % n], anchors[i]);
    let (mut tx, mut ty) = (next.x - prev.x, next.y - prev.y);
    let len = match tx.hypot(ty) {
        0.0 => 1.0,
        l => l,
    };
    (tx, ty) = (tx / len, ty / len);
    let or20 = |d: f64| if d == 0.0 { 20.0 } else { d };
    let d_out = or20((next.x - a.x).hypot(next.y - a.y) / 3.0);
    let d_in = or20((a.x - prev.x).hypot(a.y - prev.y) / 3.0);
    let a = &mut anchors[i];
    (a.out_x, a.out_y, a.in_x, a.in_y) = (Some(tx * d_out), Some(ty * d_out), Some(-tx * d_in), Some(-ty * d_in));
    a.kind = AnchorKind::Smooth;
}

/// Delete anchor `i` unless that would leave fewer than 2.
pub fn delete_anchor(sub: &mut Subpath, i: usize) {
    if sub.anchors.len() > 2 && i < sub.anchors.len() {
        sub.anchors.remove(i);
    }
}

/// Nearest (segment, t, distance) to `p`, within `tol`.
pub fn closest_segment(sub: &Subpath, p: Pt, tol: f64) -> Option<(usize, f64, f64)> {
    let a = &sub.anchors;
    let n = a.len();
    if n < 2 {
        return None;
    }
    let segs = if sub.closed { n } else { n - 1 };
    let mut best: Option<(usize, f64, f64)> = None;
    for s in 0..segs {
        let c = segment_controls(&a[s], &a[(s + 1) % n]);
        for k in 0..=24 {
            let t = k as f64 / 24.0;
            let d = dist(cubic_at(&c, t), p);
            if best.is_none_or(|b| d < b.2) {
                best = Some((s, t, d));
            }
        }
    }
    best.filter(|b| b.2 <= tol)
}

/// Insert an anchor on segment `seg` at `t`, keeping the curve (de Casteljau).
pub fn insert_anchor_at(sub: &mut Subpath, seg: usize, t: f64) {
    let n = sub.anchors.len();
    if n < 2 || seg >= n {
        return;
    }
    let (ia, ib) = (seg, (seg + 1) % n);
    let (a, b) = (sub.anchors[ia], sub.anchors[ib]);
    let lerp = |p: Pt, q: Pt| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
    let na = if !segment_is_curved(&a, &b) {
        let m = lerp([a.x, a.y], [b.x, b.y]);
        PathAnchor::corner(m[0], m[1])
    } else {
        let c = segment_controls(&a, &b);
        let (a1, b1, c1) = (lerp(c[0], c[1]), lerp(c[1], c[2]), lerp(c[2], c[3]));
        let (d1, e1) = (lerp(a1, b1), lerp(b1, c1));
        let f = lerp(d1, e1);
        let pa = &mut sub.anchors[ia];
        (pa.out_x, pa.out_y) = (Some(a1[0] - a.x), Some(a1[1] - a.y));
        let pb = &mut sub.anchors[ib];
        (pb.in_x, pb.in_y) = (Some(c1[0] - b.x), Some(c1[1] - b.y));
        PathAnchor {
            x: f[0],
            y: f[1],
            in_x: Some(d1[0] - f[0]),
            in_y: Some(d1[1] - f[1]),
            out_x: Some(e1[0] - f[0]),
            out_y: Some(e1[1] - f[1]),
            kind: AnchorKind::Smooth,
        }
    };
    sub.anchors.insert(seg + 1, na);
}

// ── Whole-path operations ──

/// Bounds of every subpath (flattened), padded by half the stroke width.
pub fn bounds(specs: &[PathSpec]) -> Option<[f64; 4]> {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for s in specs {
        let pad = s.stroke.as_ref().map_or(0.0, |st| st.size / 2.0);
        for sub in &s.subpaths {
            for p in flatten_subpath(sub, 4.0) {
                (x0, y0, x1, y1) = (x0.min(p[0] - pad), y0.min(p[1] - pad), x1.max(p[0] + pad), y1.max(p[1] + pad));
            }
        }
    }
    (x1 >= x0).then_some([x0, y0, x1 - x0, y1 - y0])
}

/// x' = a·x + c·y + e, y' = b·x + d·y + f (canvas `setTransform` order).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

/// Map a path through an affine transform exactly; the stroke width scales by √|det|.
pub fn affine(spec: &PathSpec, m: &Affine) -> PathSpec {
    let lin = |x: f64, y: f64| [m.a * x + m.c * y, m.b * x + m.d * y];
    let subpaths = spec
        .subpaths
        .iter()
        .map(|sub| Subpath {
            closed: sub.closed,
            anchors: sub
                .anchors
                .iter()
                .map(|a| {
                    let p = lin(a.x, a.y);
                    let mut o = PathAnchor { x: p[0] + m.e, y: p[1] + m.f, kind: a.kind, ..Default::default() };
                    if a.has_in() {
                        let h = lin(a.in_handle()[0], a.in_handle()[1]);
                        (o.in_x, o.in_y) = (Some(h[0]), Some(h[1]));
                    }
                    if a.has_out() {
                        let h = lin(a.out_handle()[0], a.out_handle()[1]);
                        (o.out_x, o.out_y) = (Some(h[0]), Some(h[1]));
                    }
                    o
                })
                .collect(),
        })
        .collect();
    let k = (m.a * m.d - m.b * m.c).abs().sqrt();
    PathSpec { subpaths, stroke: spec.stroke.clone().map(|s| PathStroke { size: s.size * k, ..s }), fill: spec.fill.clone() }
}

/// Every subpath as a closed contour (for selection, fill and masks).
pub fn area_contours(specs: &[PathSpec]) -> Vec<Vec<Pt>> {
    let mut out = Vec::new();
    for s in specs {
        for sub in &s.subpaths {
            if sub.anchors.len() < 2 {
                continue;
            }
            let poly = flatten_subpath(&Subpath { anchors: sub.anchors.clone(), closed: true }, 2.0);
            if poly.len() >= 3 {
                out.push(poly);
            }
        }
    }
    out
}

/// Photoshop's Simulate Pressure: a smoothstep taper at both ends.
pub fn simulated_pressure(poly: &[Pt]) -> Vec<f64> {
    let (fraction, max_taper, floor) = (0.3, 400.0, 0.05);
    if poly.is_empty() {
        return Vec::new();
    }
    let mut cum = vec![0.0];
    for w in poly.windows(2) {
        cum.push(cum.last().copied().unwrap_or(0.0) + dist(w[0], w[1]));
    }
    let len = cum.last().copied().unwrap_or(0.0);
    if len <= 0.0 {
        return vec![1.0; poly.len()];
    }
    let taper = (len * fraction).min(max_taper);
    let ease = |t: f64| {
        let c = t.clamp(0.0, 1.0);
        c * c * (3.0 - 2.0 * c)
    };
    cum.iter().map(|&s| floor + (1.0 - floor) * ease(s / taper).min(ease((len - s) / taper))).collect()
}
