//! Speech balloons around text objects (spec 28; happypaint
//! `engine/text/text-renderer.ts:33-60, 411-584`).
//!
//! Geometry is in the object's local block space: the block is
//! `[0, blockW] × [0, blockH]`, and the object's translate and rotate apply on
//! top. Paths are flattened to polygons. The body, cloud lobes and tail are
//! filled and stroked as their union, through a signed distance field.

use crate::TextSpec;
use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BalloonShape {
    Rounded,
    /// "Shout".
    Burst,
    /// "Thought".
    Cloud,
    /// "Speech"; also what an unknown shape draws as (TR's default branch).
    #[default]
    #[serde(other)]
    Oval,
}

/// Tail tip in local block coordinates (so it rotates with the object).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Tail {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
}

/// happypaint `BalloonSpec` (TR:41-58).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Balloon {
    #[serde(default)]
    pub shape: BalloonShape,
    #[serde(default = "white")]
    pub fill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f64>,
    /// Padding as a multiple of the font size (default 0.55).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail: Option<Tail>,
}

fn white() -> String {
    "#ffffff".into()
}

pub const DEFAULT_PAD: f64 = 0.55;
pub const DEFAULT_STROKE_WIDTH: f64 = 2.0;
pub const DEFAULT_TAIL_WIDTH: f64 = 24.0;

impl Balloon {
    /// The drawn outline width (0 when there is no stroke colour).
    pub fn stroke_px(&self) -> f64 {
        if self.stroke.is_some() { self.stroke_width.unwrap_or(DEFAULT_STROKE_WIDTH).max(0.0) } else { 0.0 }
    }
}

/// Tail reach as a multiple of the font size (TP:85).
pub const TAIL_REACH: f64 = 2.2;

/// A tail aimed at `deg` (0 = right, 90 = down) from the block origin, at a
/// readable fixed reach (TP:91-97).
pub fn tail_from_angle(font_size: f64, deg: f64) -> Tail {
    let r = deg.to_radians();
    let reach = font_size * TAIL_REACH;
    Tail { x: r.cos() * reach, y: r.sin() * reach, width: Some((font_size * 0.5).max(8.0)) }
}

/// The tail's direction in degrees [0, 360); 90 when there is none (TP:86-90).
pub fn tail_angle(tail: Option<&Tail>) -> f64 {
    tail.map_or(90.0, |t| (t.y.atan2(t.x).to_degrees() + 360.0) % 360.0)
}

/// The balloon a shape pick creates when there is none yet (TS:138-139).
pub fn default_balloon(shape: BalloonShape) -> Balloon {
    Balloon { shape, fill: white(), stroke: Some("#000000".into()), stroke_width: Some(3.0), pad: None, tail: None }
}

/// Float AABB in document space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

/// The largest font size the spec draws at (TR:442).
pub fn spec_font_size(spec: &TextSpec) -> f64 {
    match &spec.runs {
        Some(r) if !r.is_empty() => r.iter().map(|r| r.font_size_px.unwrap_or(spec.font_size_px)).fold(f64::NEG_INFINITY, f64::max),
        _ => spec.font_size_px,
    }
}

/// Padding between the block and the balloon outline, doc px (TR:452).
pub fn balloon_pad(spec: &TextSpec, font_size: f64) -> f64 {
    let Some(b) = &spec.balloon else { return 0.0 };
    font_size * b.pad.unwrap_or(DEFAULT_PAD) + if b.stroke.is_some() { b.stroke_width.unwrap_or(DEFAULT_STROKE_WIDTH) / 2.0 } else { 0.0 }
}

/// Local points beyond the padded block: the tail tip ± its half-width (TR:467).
pub fn balloon_extra_local_points(spec: &TextSpec) -> Vec<[f64; 2]> {
    let Some(t) = spec.balloon.as_ref().and_then(|b| b.tail.as_ref()) else { return Vec::new() };
    let half = (t.width.unwrap_or(DEFAULT_TAIL_WIDTH) / 2.0).max(0.0);
    vec![[t.x, t.y], [t.x - half, t.y], [t.x + half, t.y], [t.x, t.y - half], [t.x, t.y + half]]
}

/// The padded block plus `extra` local points, rotated about (x, y) (TR:411).
pub fn spec_aabb(spec: &TextSpec, block_w: f64, block_h: f64, pad: f64, extra: &[[f64; 2]]) -> Aabb {
    let (c, s) = (spec.rotation.cos(), spec.rotation.sin());
    let corners = [[-pad, -pad], [block_w + pad, -pad], [-pad, block_h + pad], [block_w + pad, block_h + pad]];
    let mut a = Aabb { min_x: f64::INFINITY, min_y: f64::INFINITY, max_x: f64::NEG_INFINITY, max_y: f64::NEG_INFINITY };
    for [lx, ly] in corners.iter().chain(extra) {
        let (x, y) = (spec.x + lx * c - ly * s, spec.y + lx * s + ly * c);
        a = Aabb { min_x: a.min_x.min(x), min_y: a.min_y.min(y), max_x: a.max_x.max(x), max_y: a.max_y.max(y) };
    }
    a
}

/// Max chord error when flattening arcs, px.
const TOLERANCE: f64 = 0.1;

/// Segments for an arc of `sweep` radians at radius `r`, a multiple of 4 for a full turn.
fn segments(r: f64, sweep: f64) -> usize {
    let step = if r > TOLERANCE { 2.0 * (1.0 - TOLERANCE / r).acos() } else { TAU };
    let n = (sweep / step.max(1e-3)).ceil().max(4.0) as usize;
    n.div_ceil(4) * 4
}

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> Vec<[f64; 2]> {
    let n = segments(rx.max(ry), TAU);
    (0..n).map(|i| i as f64 / n as f64 * TAU).map(|t| [cx + t.cos() * rx, cy + t.sin() * ry]).collect()
}

/// Closed sub-paths (local space) of a spec's balloon: body, then the tail (TR:510-584).
pub fn balloon_paths(spec: &TextSpec, block_w: f64, block_h: f64) -> Vec<Vec<[f64; 2]>> {
    let Some(b) = &spec.balloon else { return Vec::new() };
    let pad = spec_font_size(spec) * b.pad.unwrap_or(DEFAULT_PAD);
    let (x0, y0, w, h) = (-pad, -pad, block_w + pad * 2.0, block_h + pad * 2.0);
    let (cx, cy) = (x0 + w / 2.0, y0 + h / 2.0);
    let mut paths = Vec::new();
    match b.shape {
        BalloonShape::Rounded => {
            let r = w.min(h) * 0.28;
            let mut p = Vec::new();
            // Corner centres clockwise from top-right, each a quarter arc.
            let corners = [(x0 + w - r, y0 + r, -0.25), (x0 + w - r, y0 + h - r, 0.0), (x0 + r, y0 + h - r, 0.25), (x0 + r, y0 + r, 0.5)];
            let n = segments(r, TAU / 4.0);
            for (ccx, ccy, start) in corners {
                for i in 0..=n {
                    let t = (start + 0.25 * i as f64 / n as f64) * TAU;
                    p.push([ccx + t.cos() * r, ccy + t.sin() * r]);
                }
            }
            paths.push(p);
        }
        BalloonShape::Burst => {
            let spikes = 16;
            paths.push(
                (0..spikes * 2)
                    .map(|i| {
                        let t = i as f64 / (spikes * 2) as f64 * TAU;
                        let k = if i % 2 == 0 { 1.22 } else { 0.86 };
                        [cx + t.cos() * (w / 2.0) * k, cy + t.sin() * (h / 2.0) * k]
                    })
                    .collect(),
            );
        }
        BalloonShape::Cloud => {
            let lobes = 12;
            let rr = w.min(h) * 0.19;
            for i in 0..lobes {
                let t = i as f64 / lobes as f64 * TAU;
                paths.push(ellipse(cx + t.cos() * (w / 2.0), cy + t.sin() * (h / 2.0), rr, rr));
            }
            paths.push(ellipse(cx, cy, w / 2.0, h / 2.0));
        }
        BalloonShape::Oval => paths.push(ellipse(cx, cy, w / 2.0, h / 2.0)),
    }
    if let Some(t) = &b.tail {
        let tw = t.width.unwrap_or(DEFAULT_TAIL_WIDTH).max(1.0) / 2.0;
        let (dx, dy) = (t.x - cx, t.y - cy);
        let len = match dx.hypot(dy) {
            l if l > 0.0 => l,
            _ => 1.0,
        };
        let (px, py) = (-dy / len * tw, dx / len * tw);
        paths.push(vec![[cx + px, cy + py], [t.x, t.y], [cx - px, cy - py]]);
    }
    paths
}

/// A polygon with its bounds, for the distance field.
struct Poly<'a> {
    pts: &'a [[f64; 2]],
    lo: [f64; 2],
    hi: [f64; 2],
}

impl Poly<'_> {
    /// Signed distance (negative inside) of `p`.
    fn sdf(&self, p: [f64; 2]) -> f64 {
        let n = self.pts.len();
        let mut d2 = f64::INFINITY;
        let mut inside = false;
        for i in 0..n {
            let a = self.pts[i];
            let b = self.pts[(i + 1) % n];
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let (wx, wy) = (p[0] - a[0], p[1] - a[1]);
            let l2 = ex * ex + ey * ey;
            let t = if l2 > 0.0 { ((wx * ex + wy * ey) / l2).clamp(0.0, 1.0) } else { 0.0 };
            let (qx, qy) = (wx - ex * t, wy - ey * t);
            d2 = d2.min(qx * qx + qy * qy);
            if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * ex {
                inside = !inside;
            }
        }
        if inside { -d2.sqrt() } else { d2.sqrt() }
    }

    /// Lower bound of the distance from `p` to the polygon (0 inside its box).
    fn box_dist(&self, p: [f64; 2]) -> f64 {
        let dx = (self.lo[0] - p[0]).max(p[0] - self.hi[0]).max(0.0);
        let dy = (self.lo[1] - p[1]).max(p[1] - self.hi[1]).max(0.0);
        dx.hypot(dy)
    }
}

/// Fill and stroke coverage of a balloon on a `w × h` local canvas whose pixel
/// (0, 0) sits at block coordinate `(-ox, -oy)`.
pub(crate) struct Coverage {
    pub fill: Vec<f32>,
    pub stroke: Vec<f32>,
}

pub(crate) fn coverage(paths: &[Vec<[f64; 2]>], stroke_w: f64, w: usize, h: usize, ox: f64, oy: f64) -> Coverage {
    let polys: Vec<Poly> = paths
        .iter()
        .filter(|p| p.len() >= 3)
        .map(|pts| {
            let mut lo = [f64::INFINITY; 2];
            let mut hi = [f64::NEG_INFINITY; 2];
            for p in pts.iter() {
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
            Poly { pts, lo, hi }
        })
        .collect();
    let mut fill = vec![0.0f32; w * h];
    let mut stroke = vec![0.0f32; w * h];
    let half = stroke_w / 2.0;
    let reach = half + 1.0;
    use rayon::prelude::*;
    fill.par_chunks_mut(w.max(1)).zip(stroke.par_chunks_mut(w.max(1))).enumerate().for_each(|(y, (frow, srow))| {
        let py = y as f64 + 0.5 - oy;
        for x in 0..w {
            let p = [x as f64 + 0.5 - ox, py];
            // Union SDF = min over sub-paths. Outside a sub-path's box its SDF is
            // at least the box distance, so a box farther than the best can't lower it.
            let mut best = f64::INFINITY;
            for poly in &polys {
                let d = poly.box_dist(p);
                if d > 0.0 && d >= best {
                    continue;
                }
                best = best.min(poly.sdf(p));
            }
            if best > reach {
                continue;
            }
            frow[x] = (0.5 - best).clamp(0.0, 1.0) as f32;
            if half > 0.0 {
                srow[x] = (half + 0.5 - best.abs()).clamp(0.0, 1.0) as f32;
            }
        }
    });
    Coverage { fill, stroke }
}
