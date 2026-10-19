//! The geometry of a wash fill (happypaint `engine/wet/wash-field.ts`,
//! v0.198.0; spec 58 C): how far every pixel of the picked region is from
//! the tap, along paths inside the region, and from the region's edge. The
//! wash is laid band by band along that distance, so it spreads out from
//! where you tapped.
//!
//! After the Watercolor Playground's bucket fill (MIT): Dial's algorithm
//! over an integer chamfer metric with knight's moves. The wobble that keeps
//! the front organic is a hash of the document position, so a recorded fill
//! replays identically.

/// Chamfer weights ×AX: orthogonal 1, diagonal √2, knight √5.
const AX: i32 = 5;
const DI: i32 = 7;
const KN: i32 = 11;

/// A wash's field over the region's bounding box.
#[derive(Clone, Debug, PartialEq)]
pub struct WashField {
    /// Half-open doc-px rect the region occupies.
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
    /// Region pixels (connected to the tap).
    pub count: usize,
    /// Farthest wobbled distance from the tap (px).
    pub max_dist: f32,
    /// Per bbox pixel, row-major: distance from the tap (px; −1 outside the
    /// region), distance to the region's edge (px, capped), weight 0..1
    /// (selection coverage; 1 without one), 0.
    pub data: Vec<[f32; 4]>,
}

impl WashField {
    pub fn width(&self) -> i32 {
        self.x1 - self.x0
    }

    /// The field at doc pixel (x, y), if it lies in the box.
    pub fn at(&self, x: i32, y: i32) -> Option<[f32; 4]> {
        (x >= self.x0 && y >= self.y0 && x < self.x1 && y < self.y1)
            .then(|| self.data[((y - self.y0) * self.width() + x - self.x0) as usize])
    }
}

/// Integer chamfer distances (×5) from `sources` within `mask` (−1 =
/// unreached); stops expanding past `cap` px.
pub fn chamfer(mask: &[bool], w: usize, h: usize, sources: &[usize], cap: f64) -> Vec<i32> {
    let mut dist = vec![-1i32; w * h];
    let mut buckets: Vec<Vec<usize>> = Vec::new();
    let push = |buckets: &mut Vec<Vec<usize>>, d: i32, i: usize| {
        let d = d as usize;
        if buckets.len() <= d {
            buckets.resize_with(d + 1, Vec::new);
        }
        buckets[d].push(i);
    };
    for &s in sources {
        dist[s] = 0;
        push(&mut buckets, 0, s);
    }
    let limit = cap * f64::from(AX);
    let mut d = 0usize;
    while d < buckets.len() && d as f64 <= limit {
        let b = std::mem::take(&mut buckets[d]);
        let di = d as i32;
        for i in b {
            if dist[i] != di {
                continue; // a shorter path got here first
            }
            let (x, y) = (i % w, i / w);
            let (l, r, u, dn) = (x > 0, x + 1 < w, y > 0, y + 1 < h);
            let (l2, r2, u2, dn2) = (x > 1, x + 2 < w, y > 1, y + 2 < h);
            let mut relax = |j: usize, nd: i32| {
                if !mask[j] {
                    return;
                }
                if dist[j] != -1 && dist[j] <= nd {
                    return;
                }
                dist[j] = nd;
                push(&mut buckets, nd, j);
            };
            if l {
                relax(i - 1, di + AX);
            }
            if r {
                relax(i + 1, di + AX);
            }
            if u {
                relax(i - w, di + AX);
            }
            if dn {
                relax(i + w, di + AX);
            }
            // Diagonal and knight's moves only through the region's pixels,
            // so the front never hops a 1 px line of line art.
            let m = |j: usize| mask[j];
            let (cl, cr, cu, cd) = (l && m(i - 1), r && m(i + 1), u && m(i - w), dn && m(i + w));
            if cl && cu {
                relax(i - w - 1, di + DI);
            }
            if cr && cu {
                relax(i - w + 1, di + DI);
            }
            if cl && cd {
                relax(i + w - 1, di + DI);
            }
            if cr && cd {
                relax(i + w + 1, di + DI);
            }
            // Knight's moves (2, 1) round off the octagon a 5-7 chamfer gives.
            if u2 && cu && l && m(i - w - 1) {
                relax(i - 2 * w - 1, di + KN);
            }
            if u2 && cu && r && m(i - w + 1) {
                relax(i - 2 * w + 1, di + KN);
            }
            if dn2 && cd && l && m(i + w - 1) {
                relax(i + 2 * w - 1, di + KN);
            }
            if dn2 && cd && r && m(i + w + 1) {
                relax(i + 2 * w + 1, di + KN);
            }
            if l2 && cl && u && m(i - w - 1) {
                relax(i - w - 2, di + KN);
            }
            if l2 && cl && dn && m(i + w - 1) {
                relax(i + w - 2, di + KN);
            }
            if r2 && cr && u && m(i - w + 1) {
                relax(i - w + 2, di + KN);
            }
            if r2 && cr && dn && m(i + w + 1) {
                relax(i + w + 2, di + KN);
            }
        }
        d += 1;
    }
    dist
}

/// Hash of an integer lattice point → [0, 1).
fn hash2(x: i32, y: i32) -> f64 {
    let mut h = x.wrapping_mul(374761393).wrapping_add(y.wrapping_mul(668265263)) as u32;
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    f64::from(h ^ (h >> 16)) / 4294967296.0
}

/// Smooth doc-anchored value noise in 0..1, features ~`cell` px (2 octaves).
fn wobble_at(x: i32, y: i32, cell: f64) -> f64 {
    let (mut v, mut amp, mut total, mut cell) = (0.0, 0.5, 0.0, cell);
    for o in 0..2 {
        let (fx, fy) = (f64::from(x) / cell, f64::from(y) / cell);
        let (gx, gy) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - gx, fy - gy);
        let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
        let (gx, gy, s) = (gx as i32, gy as i32, o * 101);
        let a = hash2(gx + s, gy);
        let b = hash2(gx + 1 + s, gy);
        let c = hash2(gx + s, gy + 1);
        let d = hash2(gx + 1 + s, gy + 1);
        v += amp * (a + (b - a) * sx + (c + (d - c) * sx - (a + (b - a) * sx)) * sy);
        total += amp;
        cell /= 2.0;
        amp /= 2.0;
    }
    v / total
}

/// Options for [`wash_field`]: the edge distance cap (px), the wobble
/// amplitude (px) and a per-pixel weight (0..255 over the document).
pub struct WashOpts<'a> {
    pub edge_cap: f32,
    pub wobble: f64,
    pub weight: Option<&'a [u8]>,
}

impl Default for WashOpts<'_> {
    fn default() -> Self {
        WashOpts { edge_cap: 48.0, wobble: 8.0, weight: None }
    }
}

/// The field of the region of `mask` (over the whole document) connected
/// to the tap (sx, sy); `None` when the tap isn't in it.
pub fn wash_field(mask: &[bool], w: usize, h: usize, sx: i32, sy: i32, opts: &WashOpts) -> Option<WashField> {
    if sx < 0 || sy < 0 || sx as usize >= w || sy as usize >= h {
        return None;
    }
    let seed = sy as usize * w + sx as usize;
    if !mask[seed] {
        return None;
    }
    let from_seed = chamfer(mask, w, h, &[seed], f64::INFINITY);
    let reached: Vec<bool> = from_seed.iter().map(|d| *d >= 0).collect();
    let (mut x0, mut y0, mut x1, mut y1, mut count) = (w, h, 0, 0, 0);
    for (i, _) in reached.iter().enumerate().filter(|(_, r)| **r) {
        let (x, y) = (i % w, i / w);
        count += 1;
        x0 = x0.min(x);
        x1 = x1.max(x + 1);
        y0 = y0.min(y);
        y1 = y1.max(y + 1);
    }
    // Edge pixels: a 4-neighbour outside the region. The document border is
    // not an edge: a wash runs off the page cleanly.
    let mut edges = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * w + x;
            if reached[i]
                && ((x > 0 && !reached[i - 1])
                    || (x + 1 < w && !reached[i + 1])
                    || (y > 0 && !reached[i - w])
                    || (y + 1 < h && !reached[i + w]))
            {
                edges.push(i);
            }
        }
    }
    let from_edge = chamfer(&reached, w, h, &edges, f64::from(opts.edge_cap));
    let bw = x1 - x0;
    let mut data = vec![[0.0f32; 4]; bw * (y1 - y0)];
    let mut max_dist = 0.0f32;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * w + x;
            let o = &mut data[(y - y0) * bw + x - x0];
            let d = from_seed[i];
            if d < 0 {
                o[0] = -1.0;
                continue;
            }
            // A little low-frequency wobble keeps the front organic.
            let r = (f64::from(d) / f64::from(AX) + (wobble_at(x as i32, y as i32, 48.0) - 0.5) * 2.0 * opts.wobble).max(0.0) as f32;
            max_dist = max_dist.max(r);
            let e = from_edge[i];
            *o = [r, if e < 0 { opts.edge_cap } else { e as f32 / AX as f32 }, opts.weight.map_or(1.0, |wt| f32::from(wt[i]) / 255.0), 0.0];
        }
    }
    Some(WashField { x0: x0 as i32, y0: y0 as i32, x1: x1 as i32, y1: y1 as i32, count, max_dist, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    // happypaint wash-field.test.ts.
    #[test]
    fn chamfer_is_close_to_euclidean_and_respects_walls() {
        let n = 201;
        let d = chamfer(&vec![true; n * n], n, n, &[100 * n + 100], f64::INFINITY);
        for (x, y) in [(200, 100), (200, 200), (150, 190), (100, 0)] {
            let e = ((x as f64 - 100.0).powi(2) + (y as f64 - 100.0).powi(2)).sqrt();
            let c = f64::from(d[y * n + x]) / 5.0;
            assert!((c - e).abs() / e < 0.04, "({x},{y}): {c} vs {e}");
        }
        let n = 50;
        let mut mask = vec![true; n * n];
        for y in 0..45 {
            mask[y * n + 25] = false;
        }
        let d = chamfer(&mask, n, n, &[10 * n + 10], f64::INFINITY);
        assert!(f64::from(d[10 * n + 40]) / 5.0 > 60.0, "round the wall");
        assert_eq!(d[10 * n + 25], -1);
    }

    #[test]
    fn a_field_of_a_region() {
        assert!(wash_field(&[false; 100], 10, 10, 5, 5, &WashOpts::default()).is_none());
        let n = 40;
        let mut mask = vec![false; n * n];
        for y in 5..25 {
            for x in 5..25 {
                mask[y * n + x] = true;
            }
        }
        mask[35 * n + 35] = true;
        let f = wash_field(&mask, n, n, 10, 10, &WashOpts { wobble: 0.0, ..Default::default() }).unwrap();
        assert_eq!((f.x0, f.y0, f.x1, f.y1, f.count), (5, 5, 25, 25, 400));
        let at = |x, y| f.at(x, y).unwrap();
        assert_eq!(at(10, 10)[0], 0.0);
        assert_eq!(at(5, 15)[1], 0.0);
        assert!(at(15, 15)[1] > 8.0);
        assert_eq!(at(10, 10)[2], 1.0);
        assert!(f.max_dist > 19.0);
        // The document border is not an edge.
        let full = wash_field(&[true; 400], 20, 20, 10, 10, &WashOpts { wobble: 0.0, ..Default::default() }).unwrap();
        assert_eq!(full.at(0, 0).unwrap()[1], 48.0);
        // The wobble is a function of position: two runs agree.
        let a = wash_field(&[true; 64 * 64], 64, 64, 30, 30, &WashOpts::default()).unwrap();
        assert_eq!(a, wash_field(&[true; 64 * 64], 64, 64, 30, 30, &WashOpts::default()).unwrap());
        // A soft selection weighs the wash.
        let wt = vec![128u8; 400];
        let soft = wash_field(&[true; 400], 20, 20, 10, 10, &WashOpts { weight: Some(&wt), ..Default::default() }).unwrap();
        assert!((soft.data[0][2] - 128.0 / 255.0).abs() < 1e-5);
    }
}
