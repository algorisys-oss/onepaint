//! Fish-eye (5-point spherical) perspective (happypaint `guides-overlay.ts`
//! `circleThrough` / `arcPointsThrough`, `paint-canvas.tsx` fish-eye snapping).
//! Poles are L, R, T, B and the centre O, in doc px.

pub type Pt = [f64; 2];

/// A snapping curve: a circle, or a straight line through `o` along unit `d`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Circle { cx: f64, cy: f64, r: f64 },
    Line { o: Pt, d: Pt },
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Circumcircle of three points; `None` when (nearly) collinear.
pub fn circle_through(a: Pt, b: Pt, c: Pt) -> Option<(f64, f64, f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-6 {
        return None;
    }
    let (a2, b2, c2) = (a[0] * a[0] + a[1] * a[1], b[0] * b[0] + b[1] * b[1], c[0] * c[0] + c[1] * c[1]);
    let cx = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let cy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    Some((cx, cy, (a[0] - cx).hypot(a[1] - cy)))
}

/// The arc from `a` to `b` passing through `thru`, sampled with `n` segments.
pub fn arc_points_through(a: Pt, b: Pt, thru: Pt, n: usize) -> Vec<Pt> {
    let Some((cx, cy, r)) = circle_through(a, b, thru) else { return vec![a, b] };
    let tau = std::f64::consts::TAU;
    let norm = |x: f64| x.rem_euclid(tau);
    let ang = |p: Pt| (p[1] - cy).atan2(p[0] - cx);
    let start = norm(ang(a));
    let mut end = norm(ang(b));
    let t = norm(ang(thru));
    let ccw_end = if end < start { end + tau } else { end };
    let tt = if t < start { t + tau } else { t };
    if tt <= ccw_end {
        if end < start {
            end += tau;
        }
    } else if end > start {
        end -= tau;
    }
    (0..=n)
        .map(|i| {
            let u = start + (end - start) * i as f64 / n as f64;
            [cx + r * u.cos(), cy + r * u.sin()]
        })
        .collect()
}

fn unit(x: f64, y: f64) -> Option<Pt> {
    let l = x.hypot(y);
    (l >= 1e-6).then(|| [x / l, y / l])
}

/// The curve through `anchor` whose tangent best matches the drag: the arc
/// through L/R, the arc through T/B, or the radial line toward O.
pub fn pick_curve(poles: &[Pt; 5], anchor: Pt, cur: Pt) -> Option<Curve> {
    let [l, r, t, b, o] = *poles;
    let d = unit(cur[0] - anchor[0], cur[1] - anchor[1])?;
    let mut cands: Vec<(Curve, Pt)> = Vec::new();
    let mut add_arc = |a: Pt, bb: Pt| match circle_through(a, bb, anchor) {
        Some((cx, cy, rr)) => {
            if let Some(tan) = unit(-(anchor[1] - cy), anchor[0] - cx) {
                cands.push((Curve::Circle { cx, cy, r: rr }, tan));
            }
        }
        None => {
            if let Some(u) = unit(bb[0] - a[0], bb[1] - a[1]) {
                cands.push((Curve::Line { o: anchor, d: u }, u));
            }
        }
    };
    add_arc(l, r);
    add_arc(t, b);
    if let Some(rad) = unit(o[0] - anchor[0], o[1] - anchor[1]) {
        cands.push((Curve::Line { o: anchor, d: rad }, rad));
    }
    let mut best = None;
    let mut score = -1.0;
    for (c, tan) in cands {
        let s = (d[0] * tan[0] + d[1] * tan[1]).abs();
        if s > score {
            score = s;
            best = Some(c);
        }
    }
    best
}

pub fn project(c: &Curve, p: Pt) -> Pt {
    match *c {
        Curve::Circle { cx, cy, r } => {
            let (dx, dy) = (p[0] - cx, p[1] - cy);
            let l = dx.hypot(dy);
            if l < 1e-6 { p } else { [cx + dx / l * r, cy + dy / l * r] }
        }
        Curve::Line { o, d } => {
            let t = (p[0] - o[0]) * d[0] + (p[1] - o[1]) * d[1];
            [o[0] + t * d[0], o[1] + t * d[1]]
        }
    }
}

/// The path from `origin` to `end` along the curve (~4 doc px steps on arcs).
pub fn sample_curve(c: &Curve, origin: Pt, end: Pt) -> Vec<Pt> {
    let Curve::Circle { cx, cy, r } = *c else { return vec![origin, end] };
    let a0 = (origin[1] - cy).atan2(origin[0] - cx);
    let a1 = (end[1] - cy).atan2(end[0] - cx);
    let pi = std::f64::consts::PI;
    let mut d = a1 - a0;
    while d > pi {
        d -= 2.0 * pi;
    }
    while d < -pi {
        d += 2.0 * pi;
    }
    let n = ((d.abs() * r) / 4.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + d * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

/// The guide polylines: arc families, radial spokes, and the equator,
/// meridian and field-of-view circle (`emphasis` = true for the last three).
pub fn guide_lines(poles: &[Pt; 5], density: u32, w: f64, h: f64) -> Vec<(Vec<Pt>, bool)> {
    let [l, r, t, b, o] = *poles;
    let rfov = (dist(o, l) + dist(o, r) + dist(o, t) + dist(o, b)) / 4.0;
    let n = density.max(2);
    let reach = 0.5 * w.hypot(h);
    let mut out = Vec::new();
    for k in 1..=n {
        let s = k as f64 / n as f64 * reach;
        out.push((arc_points_through(l, r, [o[0], o[1] - s], 24), false));
        out.push((arc_points_through(l, r, [o[0], o[1] + s], 24), false));
        out.push((arc_points_through(t, b, [o[0] - s, o[1]], 24), false));
        out.push((arc_points_through(t, b, [o[0] + s, o[1]], 24), false));
    }
    let spokes = (density * 2).max(4);
    for i in 0..spokes {
        let a = i as f64 / spokes as f64 * std::f64::consts::TAU;
        out.push((vec![o, [o[0] + a.cos() * rfov, o[1] + a.sin() * rfov]], false));
    }
    let fov: Vec<Pt> = (0..=64)
        .map(|i| {
            let a = i as f64 / 64.0 * std::f64::consts::TAU;
            [o[0] + a.cos() * rfov, o[1] + a.sin() * rfov]
        })
        .collect();
    out.push((arc_points_through(l, r, o, 24), true));
    out.push((arc_points_through(t, b, o, 24), true));
    out.push((fov, true));
    out
}
