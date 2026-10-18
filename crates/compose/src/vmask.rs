//! Vector mask coverage (happypaint `vector-mask.ts` `rasterizeVectorMask`),
//! one tile at a time. Each path is filled by its rule and combined with the
//! ones before it (A the result so far, B this path): combine `B + A(1 − B)`,
//! subtract `A(1 − B)`, intersect `A·B`, exclude `A(1 − B) + B(1 − A)`; the
//! result starts full when the first path subtracts or intersects, and
//! invert comes last. happypaint fills with Canvas2D; this samples 16
//! sub-scanlines a pixel with exact horizontal coverage, so edges match it
//! closely but not bit for bit.

use onepaint_doc::{FillRule, VectorMask, VectorOp};
use onepaint_raster::{TILE, TILE_PX, Tile};

/// Sub-scanlines per pixel row.
const SUB: usize = 16;
/// Flattening tolerance, doc px.
const TOL: f64 = 0.05;

struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    /// +1 going down, −1 going up.
    dir: i32,
}

struct FlatPath {
    op: VectorOp,
    rule: FillRule,
    edges: Vec<Edge>,
    /// min x, min y, max x, max y.
    bbox: [f64; 4],
}

/// A vector mask flattened to edges, ready to fill any tile.
pub struct Flat {
    paths: Vec<FlatPath>,
    invert: bool,
}

/// One tile's coverage.
pub enum Cover {
    Empty,
    Full,
    Partial(Tile),
}

fn cubic(p: [[f64; 2]; 4], out: &mut Vec<[f64; 2]>) {
    let dd = |a: [f64; 2], b: [f64; 2], c: [f64; 2]| (a[0] - 2.0 * b[0] + c[0]).hypot(a[1] - 2.0 * b[1] + c[1]);
    let m = dd(p[0], p[1], p[2]).max(dd(p[1], p[2], p[3]));
    // Wang's bound: enough steps to stay within TOL of the curve.
    let n = ((0.75 * m / TOL).sqrt().ceil() as usize).clamp(1, 4096);
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let mt = 1.0 - t;
        let (a, b, c, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
        out.push([a * p[0][0] + b * p[1][0] + c * p[2][0] + d * p[3][0], a * p[0][1] + b * p[1][1] + c * p[2][1] + d * p[3][1]]);
    }
}

/// Flatten `v`'s paths (every path closes, an open one with a straight line).
pub fn flatten(v: &VectorMask) -> Flat {
    let paths = v
        .paths
        .iter()
        .map(|p| {
            let k = &p.knots;
            let mut pts = Vec::new();
            if let Some(first) = k.first() {
                pts.push([first[2], first[3]]);
                let n = if p.open { k.len() - 1 } else { k.len() };
                for i in 0..n {
                    let (a, b) = (k[i], k[(i + 1) % k.len()]);
                    cubic([[a[2], a[3]], [a[4], a[5]], [b[0], b[1]], [b[2], b[3]]], &mut pts);
                }
            }
            let mut edges = Vec::new();
            let mut bbox = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                bbox = [bbox[0].min(a[0]), bbox[1].min(a[1]), bbox[2].max(a[0]), bbox[3].max(a[1])];
                if a[1] != b[1] {
                    edges.push(Edge { x0: a[0], y0: a[1], x1: b[0], y1: b[1], dir: if b[1] > a[1] { 1 } else { -1 } });
                }
            }
            FlatPath { op: p.op, rule: p.rule, edges, bbox }
        })
        .collect();
    Flat { paths, invert: v.invert }
}

/// Coverage of one path over the tile at `(ox, oy)`, added into `cov`.
fn fill_path(p: &FlatPath, ox: i32, oy: i32, cov: &mut [f32]) {
    // Each pixel row lists the edges crossing it, so a sub-scanline only
    // looks at those.
    let mut rows: Vec<Vec<u32>> = vec![Vec::new(); TILE as usize];
    for (i, e) in p.edges.iter().enumerate() {
        let (lo, hi) = (e.y0.min(e.y1) - oy as f64, e.y0.max(e.y1) - oy as f64);
        if hi <= 0.0 || lo >= TILE as f64 {
            continue;
        }
        let (r0, r1) = (lo.max(0.0).floor() as usize, (hi.ceil() as usize).min(TILE as usize));
        for r in &mut rows[r0..r1] {
            r.push(i as u32);
        }
    }
    let mut xs: Vec<(f64, i32)> = Vec::new();
    let w = 1.0 / SUB as f32;
    for ly in 0..TILE as usize {
        if rows[ly].len() < 2 {
            continue;
        }
        let row = &mut cov[ly * TILE as usize..(ly + 1) * TILE as usize];
        for s in 0..SUB {
            let y = oy as f64 + ly as f64 + (s as f64 + 0.5) / SUB as f64;
            xs.clear();
            for &i in &rows[ly] {
                let e = &p.edges[i as usize];
                if (e.y0 <= y) != (e.y1 <= y) {
                    xs.push((e.x0 + (y - e.y0) / (e.y1 - e.y0) * (e.x1 - e.x0), e.dir));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut wind = 0;
            for i in 0..xs.len() - 1 {
                wind += xs[i].1;
                let inside = match p.rule {
                    FillRule::EvenOdd => (i + 1) % 2 == 1,
                    FillRule::NonZero => wind != 0,
                };
                if !inside {
                    continue;
                }
                let (a, b) = ((xs[i].0 - ox as f64).max(0.0), (xs[i + 1].0 - ox as f64).min(TILE as f64));
                if b <= a {
                    continue;
                }
                let (ia, ib) = (a.floor() as usize, (b.ceil() as usize).min(TILE as usize));
                for (x, c) in row.iter_mut().enumerate().take(ib).skip(ia) {
                    let lo = a.max(x as f64);
                    let hi = b.min(x as f64 + 1.0);
                    *c += (hi - lo) as f32 * w;
                }
            }
        }
    }
}

/// `flat`'s coverage of tile `(tx, ty)`.
pub fn cover(flat: &Flat, tx: i32, ty: i32) -> Cover {
    let (ox, oy) = (tx * TILE, ty * TILE);
    let misses = |b: &[f64; 4]| b[2] <= ox as f64 || b[0] >= (ox + TILE) as f64 || b[3] <= oy as f64 || b[1] >= (oy + TILE) as f64;
    let start = matches!(flat.paths.first().map(|p| p.op), Some(VectorOp::Subtract | VectorOp::Intersect));
    // Every path missing the tile leaves the result one value.
    if flat.paths.iter().all(|p| misses(&p.bbox)) {
        let mut a = start;
        for p in &flat.paths {
            if p.op == VectorOp::Intersect {
                a = false;
            }
        }
        return if a != flat.invert { Cover::Full } else { Cover::Empty };
    }
    let mut acc = vec![if start { 1.0f32 } else { 0.0 }; TILE_PX];
    let mut one = vec![0.0f32; TILE_PX];
    for p in &flat.paths {
        one.iter_mut().for_each(|v| *v = 0.0);
        if !misses(&p.bbox) {
            fill_path(p, ox, oy, &mut one);
        }
        for (a, &b) in acc.iter_mut().zip(&one) {
            let b = b.clamp(0.0, 1.0);
            *a = match p.op {
                VectorOp::Combine => b + *a * (1.0 - b),
                VectorOp::Subtract => *a * (1.0 - b),
                VectorOp::Intersect => *a * b,
                VectorOp::Exclude => *a * (1.0 - b) + b * (1.0 - *a),
            };
        }
    }
    if flat.invert {
        acc.iter_mut().for_each(|a| *a = 1.0 - *a);
    }
    if acc.iter().all(|&a| a <= 0.0) {
        return Cover::Empty;
    }
    if acc.iter().all(|&a| a >= 1.0) {
        return Cover::Full;
    }
    let px: Vec<f32> = acc.into_iter().flat_map(|a| [a, a, a, 1.0]).collect();
    Cover::Partial(Tile::from_f32(&px))
}

#[cfg(test)]
mod tests {
    use super::*;
    use onepaint_doc::{VectorKnot, VectorPath};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<VectorKnot> {
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].iter().map(|p| [p[0], p[1], p[0], p[1], p[0], p[1]]).collect()
    }

    fn path(op: VectorOp, knots: Vec<VectorKnot>) -> VectorPath {
        VectorPath { op, knots, ..Default::default() }
    }

    fn at(v: &VectorMask, x: usize, y: usize) -> f32 {
        match cover(&flatten(v), 0, 0) {
            Cover::Empty => 0.0,
            Cover::Full => 1.0,
            Cover::Partial(t) => t.get(x, y)[0],
        }
    }

    #[test]
    fn combine_and_invert() {
        let mut v = VectorMask { paths: vec![path(VectorOp::Combine, rect(10.0, 10.0, 30.0, 30.0))], ..Default::default() };
        assert_eq!((at(&v, 20, 20), at(&v, 5, 5)), (1.0, 0.0));
        v.invert = true;
        assert_eq!((at(&v, 20, 20), at(&v, 5, 5)), (0.0, 1.0));
    }

    #[test]
    fn the_second_path_ops_match_vector_mask_test() {
        for (op, want) in
            [(VectorOp::Subtract, [1.0, 0.0, 0.0]), (VectorOp::Intersect, [0.0, 1.0, 0.0]), (VectorOp::Exclude, [1.0, 0.0, 1.0])]
        {
            let v = VectorMask {
                paths: vec![path(VectorOp::Combine, rect(0.0, 0.0, 30.0, 40.0)), path(op, rect(20.0, 0.0, 40.0, 40.0))],
                ..Default::default()
            };
            assert_eq!([10, 25, 35].map(|x| at(&v, x, 20)), want, "{op:?}");
        }
    }

    #[test]
    fn a_first_subtract_starts_full_and_far_tiles_are_constant() {
        let v = VectorMask { paths: vec![path(VectorOp::Subtract, rect(10.0, 10.0, 30.0, 30.0))], ..Default::default() };
        assert_eq!((at(&v, 20, 20), at(&v, 5, 5)), (0.0, 1.0));
        assert!(matches!(cover(&flatten(&v), 3, 3), Cover::Full));
    }

    #[test]
    fn a_triangle_and_its_antialiased_edge() {
        let k = [[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]].iter().map(|p| [p[0], p[1], p[0], p[1], p[0], p[1]]).collect();
        let v = VectorMask { paths: vec![path(VectorOp::Combine, k)], ..Default::default() };
        assert_eq!((at(&v, 5, 5), at(&v, 35, 35)), (1.0, 0.0));
        // The diagonal x + y = 40 cuts pixel (20, 19) in half.
        assert!((at(&v, 20, 19) - 0.5).abs() < 0.05, "{}", at(&v, 20, 19));
    }

    #[test]
    fn a_curve_bulges() {
        // A circle-ish closed curve of radius 20 about (32, 32) from four knots.
        let (c, r, h) = (32.0, 20.0, 20.0 * 0.5523);
        let k = vec![
            [c - h, c - r, c, c - r, c + h, c - r],
            [c + r, c - h, c + r, c, c + r, c + h],
            [c + h, c + r, c, c + r, c - h, c + r],
            [c - r, c + h, c - r, c, c - r, c - h],
        ];
        let v = VectorMask { paths: vec![path(VectorOp::Combine, k)], ..Default::default() };
        assert_eq!(at(&v, 32, 32), 1.0);
        // On the diagonal, inside the circle but outside the knots' square: x = y = 32 ± 13.
        assert!(at(&v, 18, 18) > 0.99 && at(&v, 15, 15) < 0.01, "{} {}", at(&v, 18, 18), at(&v, 15, 15));
    }
}
