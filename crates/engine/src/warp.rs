//! Distort and mesh warp maths (happypaint `compositor.ts` `quadHomography`,
//! `paint-engine.ts` `sampleWarp`, spec 29).

pub type Pt = [f64; 2];

/// Mesh warp lattice size (WARP_N × WARP_N control points).
pub const WARP_N: usize = 4;
/// Cells per side the warp is rendered with.
pub const WARP_GRID: usize = 12;

/// Column-major 3×3 mapping the unit square onto quad `c` (TL, TR, BR, BL):
/// `[a, d, g, b, e, h, x0, y0, 1]` with x = (a·u + b·v + x0)/w, w = g·u + h·v + 1.
/// Cells whose w would reach 0 degrade to the affine map (a sub-pixel change).
pub fn quad_homography(c: &[Pt; 4]) -> [f64; 9] {
    let ([x0, y0], [x1, y1], [x2, y2], [x3, y3]) = (c[0], c[1], c[2], c[3]);
    let (dx1, dx2, dx3) = (x1 - x2, x3 - x2, x0 - x1 + x2 - x3);
    let (dy1, dy2, dy3) = (y1 - y2, y3 - y2, y0 - y1 + y2 - y3);
    let (mut a, mut b, mut d, mut e, mut g, mut h);
    if dx3.abs() < 1e-9 && dy3.abs() < 1e-9 {
        (a, b, d, e, g, h) = (x1 - x0, x2 - x1, y1 - y0, y2 - y1, 0.0, 0.0);
    } else {
        let den = match dx1 * dy2 - dx2 * dy1 {
            0.0 => 1e-9,
            v => v,
        };
        g = (dx3 * dy2 - dx2 * dy3) / den;
        h = (dx1 * dy3 - dx3 * dy1) / den;
        a = x1 - x0 + g * x1;
        b = x3 - x0 + h * x3;
        d = y1 - y0 + g * y1;
        e = y3 - y0 + h * y3;
        if 1f64.min(g + 1.0).min(h + 1.0).min(g + h + 1.0) <= 1e-3 {
            (a, b, d, e, g, h) = (x1 - x0, x3 - x0, y1 - y0, y3 - y0, 0.0, 0.0);
        }
    }
    [a, d, g, b, e, h, x0, y0, 1.0]
}

pub fn apply(m: &[f64; 9], u: f64, v: f64) -> Pt {
    let w = m[2] * u + m[5] * v + m[8];
    [(m[0] * u + m[3] * v + m[6]) / w, (m[1] * u + m[4] * v + m[7]) / w]
}

/// Inverse of a column-major 3×3.
pub fn invert(m: &[f64; 9]) -> Option<[f64; 9]> {
    let (a, b, c, d, e, f, g, h, i) = (m[0], m[3], m[6], m[1], m[4], m[7], m[2], m[5], m[8]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if det.abs() < 1e-12 {
        return None;
    }
    let k = 1.0 / det;
    // Row-major inverse, then back to column-major.
    let r = [
        (e * i - f * h) * k,
        (c * h - b * i) * k,
        (b * f - c * e) * k,
        (f * g - d * i) * k,
        (a * i - c * g) * k,
        (c * d - a * f) * k,
        (d * h - e * g) * k,
        (b * g - a * h) * k,
        (a * e - b * d) * k,
    ];
    Some([r[0], r[3], r[6], r[1], r[4], r[7], r[2], r[5], r[8]])
}

/// The warp lattice (row-major, WARP_N²) at (u, v) via bicubic Catmull-Rom.
pub fn sample_warp(pts: &[Pt], u: f64, v: f64) -> Pt {
    let seg = (WARP_N - 1) as f64;
    let at = |cx: i64, cy: i64| pts[(cy.clamp(0, WARP_N as i64 - 1) as usize) * WARP_N + cx.clamp(0, WARP_N as i64 - 1) as usize];
    let cr = |a: f64, b: f64, c: f64, d: f64, t: f64| {
        let (t2, t3) = (t * t, t * t * t);
        0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    let gx = u * seg;
    let ix = (gx.floor() as i64).clamp(0, WARP_N as i64 - 2);
    let tx = gx - ix as f64;
    let gy = v * seg;
    let iy = (gy.floor() as i64).clamp(0, WARP_N as i64 - 2);
    let ty = gy - iy as f64;
    let row = |cy: i64, k: usize| cr(at(ix - 1, cy)[k], at(ix, cy)[k], at(ix + 1, cy)[k], at(ix + 2, cy)[k], tx);
    let col = |k: usize| cr(row(iy - 1, k), row(iy, k), row(iy + 1, k), row(iy + 2, k), ty);
    [col(0), col(1)]
}

/// The identity lattice over a rect (x, y, w, h).
pub fn lattice(x: f64, y: f64, w: f64, h: f64) -> Vec<Pt> {
    let n = (WARP_N - 1) as f64;
    (0..WARP_N * WARP_N).map(|i| [x + w * (i % WARP_N) as f64 / n, y + h * (i / WARP_N) as f64 / n]).collect()
}
