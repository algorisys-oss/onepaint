//! The Mixer's paint reservoir (happypaint `smudge-renderer.ts`): one flat
//! colour, or a 64² patch loaded from the canvas with Alt-click. Texels are
//! straight colour plus how much paint that part of the brush carries, laid
//! over the dab's square (axis-aligned; it doesn't turn with the tip) and
//! stretched with the dab's radius.

use std::sync::Arc;

/// Patch texels a side (`RESERVOIR_SIZE`).
pub const RESERVOIR_SIZE: usize = 64;

/// Straight colour (the document's pixel space) and amount, 0..1.
pub type Texel = [f32; 4];

#[derive(Clone, Debug, PartialEq)]
pub enum Reservoir {
    Flat(Texel),
    /// Row-major, top row first, [`RESERVOIR_SIZE`]² texels.
    Patch(Arc<Vec<Texel>>),
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear premultiplied sample at document position (x, y) of a region
/// read from the canvas; outside the region is clear.
pub(crate) fn bilinear(buf: &[f32], r: onepaint_raster::Rect, x: f64, y: f64) -> [f32; 4] {
    let (u, v) = (x - 0.5 - r.x as f64, y - 0.5 - r.y as f64);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = ((u - x0) as f32, (v - y0) as f32);
    let (x0, y0) = (x0 as i32, y0 as i32);
    let px = |xi: i32, yi: i32| -> [f32; 4] {
        if xi < 0 || yi < 0 || xi >= r.w || yi >= r.h {
            return [0.0; 4];
        }
        let i = ((yi * r.w + xi) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    let (a, b, c, d) = (px(x0, y0), px(x0 + 1, y0), px(x0, y0 + 1), px(x0 + 1, y0 + 1));
    std::array::from_fn(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        top + (bot - top) * fy
    })
}

impl Reservoir {
    /// A brush loaded with one colour, full of paint.
    pub fn flat(c: [f32; 3]) -> Self {
        Self::Flat([c[0], c[1], c[2], 1.0])
    }

    /// Load the square `2·radius` wide around (cx, cy) from `buf`, the
    /// premultiplied canvas over `r` (`loadPatch`): each texel averages 4 × 4
    /// taps of its footprint and is un-premultiplied. `None` when the square
    /// holds almost no paint (mean coverage under 0.001).
    pub fn from_canvas(buf: &[f32], r: onepaint_raster::Rect, cx: f64, cy: f64, radius: f64) -> Option<Self> {
        let n = RESERVOIR_SIZE;
        let radius = radius.max(1.0);
        let step = 2.0 * radius / n as f64;
        let mut texels = Vec::with_capacity(n * n);
        let mut coverage = 0.0f64;
        for j in 0..n {
            for i in 0..n {
                let (x0, y0) = (cx - radius + i as f64 * step, cy - radius + j as f64 * step);
                let mut p = [0.0f32; 4];
                for b in 0..4 {
                    for a in 0..4 {
                        let s = bilinear(buf, r, x0 + (a as f64 + 0.5) * step / 4.0, y0 + (b as f64 + 0.5) * step / 4.0);
                        for k in 0..4 {
                            p[k] += s[k] / 16.0;
                        }
                    }
                }
                coverage += f64::from(p[3]);
                texels.push(if p[3] > 0.002 {
                    let a = p[3];
                    [(p[0] / a).clamp(0.0, 1.0), (p[1] / a).clamp(0.0, 1.0), (p[2] / a).clamp(0.0, 1.0), a.min(1.0)]
                } else {
                    [0.0; 4]
                });
            }
        }
        (coverage / (n * n) as f64 >= 0.001).then(|| Self::Patch(Arc::new(texels)))
    }

    /// The texel under dab-local (u, v) in 0..1, filtered like a LINEAR,
    /// clamp-to-edge texture.
    pub fn sample(&self, u: f64, v: f64) -> Texel {
        let t = match self {
            Self::Flat(c) => return *c,
            Self::Patch(t) => t,
        };
        let n = RESERVOIR_SIZE as i32;
        let (x, y) = (u * n as f64 - 0.5, v * n as f64 - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = ((x - x0) as f32, (y - y0) as f32);
        let at = |xi: i32, yi: i32| t[(yi.clamp(0, n - 1) * n + xi.clamp(0, n - 1)) as usize];
        let (x0, y0) = (x0 as i32, y0 as i32);
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bot = c[k] + (d[k] - c[k]) * fx;
            top + (bot - top) * fy
        })
    }

    /// An `n`² preview, each cell the coverage-weighted colour of its block
    /// and its mean amount (`patchSwatch`). A flat reservoir is one colour.
    pub fn swatch(&self, n: usize) -> Vec<Texel> {
        let t = match self {
            Self::Flat(c) => return vec![*c; n * n],
            Self::Patch(t) => t,
        };
        let s = RESERVOIR_SIZE;
        let mut out = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let (xs, ys) = (i * s / n..((i + 1) * s / n).max(i * s / n + 1), j * s / n..((j + 1) * s / n).max(j * s / n + 1));
                let (mut c, mut a, mut count) = ([0.0f32; 3], 0.0f32, 0.0f32);
                for y in ys {
                    for x in xs.clone() {
                        let p = t[y * s + x];
                        for k in 0..3 {
                            c[k] += p[k] * p[3];
                        }
                        a += p[3];
                        count += 1.0;
                    }
                }
                out.push(if a > 0.0 { [c[0] / a, c[1] / a, c[2] / a, a / count] } else { [0.0; 4] });
            }
        }
        out
    }

    /// Drift every texel toward the pickup's average `pa` (premultiplied)
    /// at `rate` (`RESERVOIR_FRAGMENT_SRC`): an empty texel takes the pickup
    /// colour outright, and picking up paint fills the empty parts.
    pub(crate) fn contaminate(&mut self, pa: [f32; 4], rate: f32, mixer: &(dyn Fn([f32; 3], [f32; 3], f32) -> [f32; 3] + Sync)) {
        let got = pa[3] > 0.001;
        let step = |pv: Texel| -> Texel {
            let prev = [pv[0], pv[1], pv[2]];
            let avg = if got { [pa[0] / pa[3], pa[1] / pa[3], pa[2] / pa[3]] } else { prev };
            let r = 1.0 + (rate - 1.0) * smoothstep(0.0, 0.05, pv[3]);
            let col = mixer(prev, avg, r);
            let a = if got { pv[3] + (1.0 - pv[3]) * (rate * pa[3].min(1.0)) } else { pv[3] };
            [col[0], col[1], col[2], a]
        };
        match self {
            Self::Flat(c) => *c = step(*c),
            Self::Patch(t) => {
                if !got {
                    return;
                }
                use rayon::prelude::*;
                Arc::make_mut(t).par_chunks_mut(RESERVOIR_SIZE).for_each(|row| {
                    for p in row {
                        *p = step(*p);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onepaint_raster::Rect;

    /// A `w`×`h` premultiplied canvas from a pixel function.
    fn canvas(w: i32, h: i32, f: impl Fn(i32, i32) -> [f32; 4]) -> (Vec<f32>, Rect) {
        let mut b = Vec::new();
        for y in 0..h {
            for x in 0..w {
                b.extend(f(x, y));
            }
        }
        (b, Rect::new(0, 0, w, h))
    }

    // mixer-patch.test.ts: the top of the patch is the top of the swatch,
    // and a horizontal gradient stays left to right.
    #[test]
    fn patch_keeps_its_orientation() {
        let (b, r) = canvas(64, 64, |x, y| if y < 32 { [x as f32 / 64.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] });
        let p = Reservoir::from_canvas(&b, r, 32.0, 32.0, 32.0).unwrap();
        let s = p.swatch(2);
        assert!(s[0][0] < s[1][0], "left to right: {s:?}");
        assert!(s[2][2] > 0.99, "the bottom row is the blue half");
        assert!(s[0][2] < 0.01);
    }

    #[test]
    fn patch_unpremultiplies_and_weights_by_coverage() {
        // Half-covered red at 50%: straight red, half the paint.
        let (b, r) = canvas(32, 32, |_, _| [0.5, 0.0, 0.0, 0.5]);
        let p = Reservoir::from_canvas(&b, r, 16.0, 16.0, 16.0).unwrap();
        let t = p.sample(0.5, 0.5);
        assert!((t[0] - 1.0).abs() < 1e-5 && (t[3] - 0.5).abs() < 1e-5, "{t:?}");
        // Half white, half clear: the swatch stays white at half amount.
        let (b, r) = canvas(32, 32, |x, _| if x < 16 { [1.0; 4] } else { [0.0; 4] });
        let s = Reservoir::from_canvas(&b, r, 16.0, 16.0, 16.0).unwrap().swatch(1);
        assert!((s[0][0] - 1.0).abs() < 1e-5 && (s[0][3] - 0.5).abs() < 0.02, "{s:?}");
    }

    #[test]
    fn an_empty_square_loads_nothing() {
        let (b, r) = canvas(32, 32, |_, _| [0.0; 4]);
        assert!(Reservoir::from_canvas(&b, r, 16.0, 16.0, 16.0).is_none());
        // Past the canvas edge is clear too.
        let (b, r) = canvas(32, 32, |_, _| [1.0; 4]);
        assert!(Reservoir::from_canvas(&b, r, 500.0, 500.0, 16.0).is_none());
        let half = Reservoir::from_canvas(&b, r, 32.0, 16.0, 16.0).unwrap();
        assert_eq!(half.sample(0.1, 0.5)[3], 1.0);
        assert_eq!(half.sample(0.9, 0.5)[3], 0.0, "the part past the edge holds no paint");
    }

    #[test]
    fn contamination_drifts_every_texel_and_fills_the_empty_ones() {
        let (b, r) = canvas(32, 32, |x, _| if x < 16 { [1.0, 0.0, 0.0, 1.0] } else { [0.0; 4] });
        let mut p = Reservoir::from_canvas(&b, r, 16.0, 16.0, 16.0).unwrap();
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] { std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t) };
        p.contaminate([0.0, 0.0, 1.0, 1.0], 0.1, &lerp);
        let (full, empty) = (p.sample(0.1, 0.5), p.sample(0.9, 0.5));
        assert!((full[0] - 0.9).abs() < 1e-5 && (full[2] - 0.1).abs() < 1e-5, "a full texel drifts by the rate: {full:?}");
        assert_eq!([empty[0], empty[2]], [0.0, 1.0], "an empty texel takes the pickup colour");
        assert!((empty[3] - 0.1).abs() < 1e-5, "and starts to fill: {empty:?}");
        // Nothing picked up: the patch stays as it was.
        let before = p.clone();
        p.contaminate([0.0; 4], 0.1, &lerp);
        assert_eq!(p, before);
    }
}
