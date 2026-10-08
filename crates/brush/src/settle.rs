//! Watercolour settle (spec 35 D): a one-shot pass over a finished stroke
//! buffer, before it flattens onto the layer. Ported from happypaint
//! `engine/wet/settle-renderer.ts`.
//!
//! - **Edge darkening:** an unsharp mask on the stroke's alpha (8 taps on
//!   rings of 4 and 14 px) finds the rim, where pigment gathers as a wash
//!   dries. Soft brushes also lift the mid-skirt toward the local maximum.
//! - **Granulation:** the brush grain, fixed to the canvas, moves density up
//!   in the paper's valleys and down on its ridges.
//!
//! Both scale alpha and premultiplied colour together, so the hue never
//! shifts. Deterministic, so recordings replay identically.

use crate::TipMask;
use onepaint_raster::{Rect, Surface, TILE, Tile};
use rayon::prelude::*;
use serde::Deserialize;
use std::sync::Arc;

const EDGE_RADIUS: f32 = 4.0;
const EDGE_RADIUS_WIDE: f32 = 14.0;
const EDGE_MAX_BOOST: f32 = 0.9;
const GRAIN_MAX_SWING: f32 = 0.55;
/// Taps reach this far outside the pixel.
const REACH: i32 = EDGE_RADIUS_WIDE as i32 + 1;

/// A brush's `settle` block (both 0..1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settle {
    /// Dried-rim strength.
    pub edge_darken: f64,
    /// Pigment granulation into the brush grain.
    pub granulate: f64,
}

impl Settle {
    pub fn is_active(&self) -> bool {
        self.edge_darken > 0.0 || self.granulate > 0.0
    }
}

/// The paper the pigment settles into: the brush grain at `scale` doc px per tile.
pub struct SettleGrain {
    pub tex: Arc<TipMask>,
    pub scale: f64,
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Bilinear alpha at pixel-centre coordinates, clamped to the window (which
/// is clamped to the document, as the doc-sized stroke texture is).
struct Window {
    r: Rect,
    a: Vec<f32>,
}

impl Window {
    #[inline]
    fn at(&self, x: i32, y: i32) -> f32 {
        let (x, y) = (x.clamp(self.r.x, self.r.right() - 1), y.clamp(self.r.y, self.r.bottom() - 1));
        self.a[((y - self.r.y) * self.r.w + (x - self.r.x)) as usize]
    }

    /// Alpha at doc position (px, py) measured in pixel centres (x + 0.5).
    #[inline]
    fn bilinear(&self, px: f32, py: f32) -> f32 {
        let (u, v) = (px - 0.5, py - 0.5);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let top = self.at(x0, y0) + (self.at(x0 + 1, y0) - self.at(x0, y0)) * fx;
        let bot = self.at(x0, y0 + 1) + (self.at(x0 + 1, y0 + 1) - self.at(x0, y0 + 1)) * fx;
        top + (bot - top) * fy
    }
}

/// Settle the stroke `buf` in place (`hardness` gates the soft-halo term).
pub fn apply(buf: &mut Surface, doc: Rect, fx: Settle, grain: Option<&SettleGrain>, hardness: f64) {
    if !fx.is_active() {
        return;
    }
    let grain = grain.filter(|_| fx.granulate > 0.0);
    let keys = buf.tile_keys();
    let src = &*buf;
    let out: Vec<_> = keys
        .par_iter()
        .filter_map(|&(tx, ty)| {
            let inner = Rect::from_tile(tx, ty).intersect(&doc)?;
            let wr = Rect::new(inner.x - REACH, inner.y - REACH, inner.w + 2 * REACH, inner.h + 2 * REACH).intersect(&doc)?;
            let a = src.read_rect(wr).chunks_exact(4).map(|p| p[3]).collect();
            let win = Window { r: wr, a };
            let mut t = src.tile(tx, ty)?.to_f32();
            for y in inner.y..inner.bottom() {
                for x in inner.x..inner.right() {
                    let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
                    let c = [t[i], t[i + 1], t[i + 2], t[i + 3]];
                    if let Some(na) = settle_px(&win, x, y, c[3], fx, grain, hardness as f32) {
                        let k = na / c[3];
                        for ch in 0..3 {
                            t[i + ch] = c[ch] * k;
                        }
                        t[i + 3] = na;
                    }
                }
            }
            Some(((tx, ty), Tile::from_f32(&t)))
        })
        .collect();
    for ((tx, ty), t) in out {
        buf.set_tile(tx, ty, Some(Arc::new(t)));
    }
}

/// The settled alpha of one pixel, or `None` where it is (nearly) empty.
fn settle_px(win: &Window, x: i32, y: i32, a: f32, fx: Settle, grain: Option<&SettleGrain>, hardness: f32) -> Option<f32> {
    if a <= 0.0005 {
        return None;
    }
    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
    let mut rim = 0.0f32;
    let mut mx = a;
    for r in [EDGE_RADIUS, EDGE_RADIUS_WIDE] {
        let d = r * std::f32::consts::FRAC_1_SQRT_2;
        let taps = [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r), (d, d), (-d, d), (d, -d), (-d, -d)];
        let mut ring = 0.0;
        for (ox, oy) in taps {
            let t = win.bilinear(cx + ox, cy + oy);
            ring += t;
            mx = mx.max(t);
        }
        ring *= 0.125;
        rim = rim.max((a - ring) / a * 2.0);
    }
    let rim = rim.clamp(0.0, 1.0);
    let edge = fx.edge_darken as f32;
    let mut na = a * (1.0 + edge * EDGE_MAX_BOOST * rim);
    // Soft-brush halo: lift the mid-skirt toward a ring denser than the body.
    let soft = smoothstep(0.5, 0.15, hardness);
    if soft > 0.0 && edge > 0.0 {
        let r = a / mx.max(0.0005);
        let band = smoothstep(0.95, 0.65, r) * smoothstep(0.03, 0.18, r) * soft;
        let target = mx * (1.0 + 0.9 * edge);
        na = na.max(a + (target - a) * band);
    }
    if let Some(g) = grain {
        let h = g.tex.sample_repeat(cx as f64 / g.scale, cy as f64 / g.scale);
        na *= 1.0 + fx.granulate as f32 * GRAIN_MAX_SWING * (h - 0.5) * 2.0;
    }
    Some(na.clamp(a * 0.45, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A disc of radius 30 at (64, 64): a flat body with a soft 8 px edge.
    fn disc(color: [f32; 3], body: f32) -> Surface {
        let mut s = Surface::new();
        for y in 0..128 {
            for x in 0..128 {
                let d = (((x as f32 + 0.5 - 64.0).powi(2) + (y as f32 + 0.5 - 64.0).powi(2)).sqrt() - 22.0) / 8.0;
                let a = body * (1.0 - d.clamp(0.0, 1.0));
                if a > 0.0 {
                    s.set(x, y, [color[0] * a, color[1] * a, color[2] * a, a]);
                }
            }
        }
        s
    }

    const DOC: Rect = Rect::new(0, 0, 128, 128);

    #[test]
    fn edges_darken_and_the_body_stays_put() {
        let before = disc([0.2, 0.4, 0.8], 0.5);
        let mut s = before.clone();
        apply(&mut s, DOC, Settle { edge_darken: 0.55, granulate: 0.0 }, None, 1.0);
        // Deep inside: every tap sees the same body, so nothing changes.
        assert!((s.get(64, 64)[3] - 0.5).abs() < 1e-3, "{:?}", s.get(64, 64));
        // Just inside the edge the rim gathers pigment.
        let (b, a) = (before.get(64, 64 + 25)[3], s.get(64, 64 + 25)[3]);
        assert!(a > b * 1.1, "rim: {b} → {a}");
        // Density, not hue: straight colour is unchanged.
        let p = s.get(64, 64 + 25);
        assert!((p[0] / p[3] - 0.2).abs() < 2e-3 && (p[2] / p[3] - 0.8).abs() < 2e-3, "{p:?}");
        // Outside stays clear.
        assert_eq!(s.get(5, 5), [0.0; 4]);
    }

    #[test]
    fn soft_brushes_lift_the_skirt_hard_ones_do_not() {
        let before = disc([1.0, 0.0, 0.0], 0.6);
        let (mut soft, mut hard) = (before.clone(), before.clone());
        let fx = Settle { edge_darken: 0.5, granulate: 0.0 };
        apply(&mut soft, DOC, fx, None, 0.1);
        apply(&mut hard, DOC, fx, None, 0.9);
        // Mid-skirt (alpha about half the body).
        let y = 64 + 22 + 4;
        assert!(soft.get(64, y)[3] > hard.get(64, y)[3] + 0.02, "soft {:?} hard {:?}", soft.get(64, y), hard.get(64, y));
    }

    #[test]
    fn granulation_follows_the_grain_and_is_bounded() {
        let tex = crate::grain_for("grain").unwrap();
        let g = SettleGrain { tex, scale: 40.0 };
        let before = disc([0.0, 0.5, 0.0], 0.8);
        let mut s = before.clone();
        apply(&mut s, DOC, Settle { edge_darken: 0.0, granulate: 1.0 }, Some(&g), 1.0);
        let mut ratios = Vec::new();
        for y in 50..78 {
            for x in 50..78 {
                let (b, a) = (before.get(x, y)[3], s.get(x, y)[3]);
                ratios.push(a / b);
                assert!(a >= b * 0.45 - 1e-3 && a <= 1.0 + 1e-6);
            }
        }
        let (lo, hi) = ratios.iter().fold((f32::MAX, f32::MIN), |(l, h), &r| (l.min(r), h.max(r)));
        assert!(lo < 0.9 && hi > 1.1, "granulation swings density: {lo}..{hi}");
        // Deterministic.
        let mut again = before.clone();
        apply(&mut again, DOC, Settle { edge_darken: 0.0, granulate: 1.0 }, Some(&g), 1.0);
        assert_eq!(again.read_rect(DOC), s.read_rect(DOC));
    }

    #[test]
    fn inactive_settle_is_a_no_op() {
        let before = disc([1.0, 1.0, 1.0], 1.0);
        let mut s = before.clone();
        apply(&mut s, DOC, Settle::default(), None, 0.5);
        assert_eq!(s.read_rect(DOC), before.read_rect(DOC));
    }
}
