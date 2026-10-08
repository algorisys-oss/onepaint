//! Manga effect lines (happypaint `engine/stroke/speed-lines.ts`): focus lines
//! radiating from a point and parallel speed lines, as path subpaths that the
//! caller strokes with the brush. Deterministic from a seed.

use crate::Mulberry32;
use crate::bezier::{PathAnchor, Subpath};

pub type Pt = [f64; 2];

fn segment(a: Pt, b: Pt) -> Subpath {
    Subpath { anchors: vec![PathAnchor::corner(a[0], a[1]), PathAnchor::corner(b[0], b[1])], closed: false }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusLines {
    pub center: Pt,
    pub count: usize,
    pub inner_radius: f64,
    pub outer_radius: f64,
    pub jitter: f64,
    pub angle_jitter: f64,
    pub start_angle: f64,
    pub sweep: f64,
    pub seed: u32,
}

/// Lines from the inner radius out to the outer one, leaving the centre clear.
pub fn focus_lines(o: &FocusLines) -> Vec<Subpath> {
    if o.count == 0 || o.outer_radius <= 0.0 || o.outer_radius.is_nan() {
        return Vec::new();
    }
    let inner = o.inner_radius.clamp(0.0, o.outer_radius);
    let (jitter, aj) = (o.jitter.clamp(0.0, 1.0), o.angle_jitter.clamp(0.0, 1.0));
    let mut rand = Mulberry32::new(if o.seed == 0 { 1 } else { o.seed });
    let step = o.sweep / o.count as f64;
    let mut out = Vec::with_capacity(o.count);
    for i in 0..o.count {
        let a = o.start_angle + step * (i as f64 + (rand.next_f64() - 0.5) * aj);
        let (c, s) = (a.cos(), a.sin());
        let r0 = inner + (o.outer_radius - inner) * jitter * rand.next_f64();
        if r0 >= o.outer_radius {
            continue;
        }
        out.push(segment(
            [o.center[0] + c * r0, o.center[1] + s * r0],
            [o.center[0] + c * o.outer_radius, o.center[1] + s * o.outer_radius],
        ));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedLines {
    pub angle: f64,
    pub count: usize,
    pub width: f64,
    pub height: f64,
    pub jitter: f64,
    pub spacing_jitter: f64,
    pub seed: u32,
}

/// Parallel lines crossing the whole region at `angle`.
pub fn speed_lines(o: &SpeedLines) -> Vec<Subpath> {
    if o.count == 0 || o.width <= 0.0 || o.height <= 0.0 {
        return Vec::new();
    }
    let (jitter, sj) = (o.jitter.clamp(0.0, 1.0), o.spacing_jitter.clamp(0.0, 1.0));
    let mut rand = Mulberry32::new(if o.seed == 0 { 1 } else { o.seed });
    let dir = [o.angle.cos(), o.angle.sin()];
    let perp = [-dir[1], dir[0]];
    let (cx, cy) = (o.width / 2.0, o.height / 2.0);
    let half_along = (dir[0].abs() * o.width + dir[1].abs() * o.height) / 2.0;
    let half_across = (perp[0].abs() * o.width + perp[1].abs() * o.height) / 2.0;
    let lane = half_across * 2.0 / o.count as f64;
    (0..o.count)
        .map(|i| {
            let t = -half_across + lane * (i as f64 + 0.5 + (rand.next_f64() - 0.5) * sj);
            let (mx, my) = (cx + perp[0] * t, cy + perp[1] * t);
            let shrink = half_along * jitter * rand.next_f64();
            segment(
                [mx - dir[0] * (half_along - shrink), my - dir[1] * (half_along - shrink)],
                [mx + dir[0] * half_along, my + dir[1] * half_along],
            )
        })
        .collect()
}
