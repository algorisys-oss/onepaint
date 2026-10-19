//! Impasto: the per-layer paint height field (spec 35 C).
//!
//! Ported from happypaint `engine/wet/height-renderer.ts` (stamp, smooth and
//! erase passes), `engine/wet/bevel-field.ts` (relief from alpha) and
//! `engine/brush/paint-medium.ts` (the pigment curves). The field is a
//! [`Surface`] holding the height in every channel, so empty tiles prune like
//! paint and undo reuses the tile machinery. Missing tiles are flat.

use onepaint_raster::{Rect, Surface, TILE};

/// Height the `.happypaint` codec can store (`height-codec.ts`).
pub const HEIGHT_STORE_MAX: f32 = 16.0;

/// Dry stamp: height per dab = `body · DRY_RATE · dab alpha` (`brush-engine.ts:501`).
pub const DRY_RATE: f64 = 0.1;
/// Wet mixer: height per dab = `body · WET_RATE · deposit` (PE:3020).
pub const WET_RATE: f64 = 0.14;
/// Blender: how much of the relief a dab takes off at full coverage (PE:3040).
pub const BLEND_FLATTEN: f64 = 0.12;

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

/// Pigment → stroke opacity cap: a 5% floor, then linear (`paint-medium.ts:39`).
pub fn pigment_opacity(pigment: f64) -> f64 {
    0.05 + 0.95 * clamp01(pigment)
}

/// Pigment → the impasto depth actually laid down (`paint-medium.ts:48`).
pub fn pigment_body(depth: f64, pigment: f64) -> f64 {
    depth.max(0.0) * clamp01(pigment)
}

/// One height stamp in document px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightDab {
    pub x: f64,
    pub y: f64,
    pub radius: f64,
    pub amount: f64,
}

fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The height at a pixel (flat where there is no tile).
pub fn sample(field: &Surface, x: i32, y: i32) -> f32 {
    field.get(x, y)[0]
}

fn write(field: &mut Surface, x: i32, y: i32, h: f32) {
    let h = h.max(0.0);
    field.set(x, y, [h; 4]);
}

/// Pixel-centre bounds of a dab, clipped to `doc`.
fn dab_rect(d: &HeightDab, doc: Rect) -> Option<Rect> {
    let r = d.radius.max(0.5);
    Rect::covering((d.x - r) as f32, (d.y - r) as f32, (d.x + r) as f32, (d.y + r) as f32).intersect(&doc)
}

/// Additive dome per dab: `(1 - smoothstep(.5, 1, d)) · amount` (HEIGHT_FRAG).
pub fn stamp(field: &mut Surface, dabs: &[HeightDab], doc: Rect) -> Rect {
    let mut dirty = Rect::default();
    for d in dabs {
        if d.amount <= 0.0 {
            continue;
        }
        let Some(r) = dab_rect(d, doc) else { continue };
        let rad = d.radius.max(0.5);
        let mut row = Vec::new();
        for (tx, ty) in r.tiles() {
            let Some(ix) = r.intersect(&Rect::from_tile(tx, ty)) else { continue };
            let t = field.tile_mut(tx, ty);
            for y in ix.y..ix.bottom() {
                let ly = (y as f64 + 0.5 - d.y) / rad;
                if ly.abs() >= 1.0 {
                    continue;
                }
                // The row's span inside the dome, padded a pixel; the rest is untouched.
                let half = (1.0 - ly * ly).sqrt() * rad;
                let (x0, x1) = (ix.x.max((d.x - half - 0.5).floor() as i32 - 1), ix.right().min((d.x + half - 0.5).ceil() as i32 + 2));
                if x0 >= x1 {
                    continue;
                }
                let (l0, lyi) = ((x0 - tx * TILE) as usize, (y - ty * TILE) as usize);
                t.read_row(lyi, l0, (x1 - tx * TILE) as usize, &mut row);
                for x in x0..x1 {
                    let lx = (x as f64 + 0.5 - d.x) / rad;
                    let dist = (lx * lx + ly * ly).sqrt();
                    if dist >= 1.0 {
                        continue;
                    }
                    let cov = ((1.0 - smoothstep(0.5, 1.0, dist)) * d.amount) as f32;
                    let k = (x - x0) as usize * 4;
                    let h = row[k] + cov;
                    row[k..k + 4].fill(h);
                }
                t.write_row(lyi, l0, &row);
            }
        }
        dirty = dirty.union(&r);
    }
    dirty
}

/// Blender dabs level the relief they cross: each pixel moves toward its
/// 5-tap average less `flatten` of it, by the dab's coverage (SMOOTH_FRAG).
/// Reads a copy of the region, so the source is stable within a dab.
pub fn smooth(field: &mut Surface, dabs: &[HeightDab], flatten: f64, doc: Rect) -> Rect {
    let mut dirty = Rect::default();
    for d in dabs {
        let Some(r) = dab_rect(d, doc) else { continue };
        // One pixel of margin for the taps; clamp-to-edge at the document.
        let Some(src_r) = Rect::new(r.x - 1, r.y - 1, r.w + 2, r.h + 2).intersect(&doc) else { continue };
        let src = field.read_rect(src_r);
        let at = |x: i32, y: i32| -> f32 {
            let (x, y) = (x.clamp(src_r.x, src_r.right() - 1), y.clamp(src_r.y, src_r.bottom() - 1));
            src[(((y - src_r.y) * src_r.w + (x - src_r.x)) * 4) as usize]
        };
        let rad = d.radius.max(0.5);
        for y in r.y..r.bottom() {
            let ly = (y as f64 + 0.5 - d.y) / rad;
            for x in r.x..r.right() {
                let lx = (x as f64 + 0.5 - d.x) / rad;
                let k = ((1.0 - smoothstep(0.35, 1.0, (lx * lx + ly * ly).sqrt())) * d.amount).clamp(0.0, 1.0) as f32;
                if k <= 0.0 {
                    continue;
                }
                let c = at(x, y);
                let avg = 0.2 * (c + at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1));
                let target = avg * (1.0 - flatten as f32);
                write(field, x, y, c + (target - c) * k);
            }
        }
        dirty = dirty.union(&r);
    }
    dirty
}

/// The eraser carves relief by the stroke's coverage:
/// `h *= 1 - clamp(alpha · opacity)` (`height-renderer.ts:151`).
pub fn carve(field: &mut Surface, stroke: &Surface, opacity: f32) -> Rect {
    let mut dirty = Rect::default();
    for (tx, ty) in stroke.tile_keys() {
        let (Some(st), true) = (stroke.tile(tx, ty), field.tile(tx, ty).is_some()) else { continue };
        let s = st.to_f32();
        let t = field.tile_mut(tx, ty);
        for i in 0..(TILE * TILE) as usize {
            let k = (s[i * 4 + 3] * opacity).clamp(0.0, 1.0);
            if k > 0.0 {
                let (lx, ly) = (i % TILE as usize, i / TILE as usize);
                let h = t.get(lx, ly)[0] * (1.0 - k);
                t.set(lx, ly, [h; 4]);
            }
        }
        field.prune_tile(tx, ty);
        dirty = dirty.union(&Rect::from_tile(tx, ty));
    }
    dirty
}

/// Layer mask applied: relief goes where the paint went (`height-renderer.ts:137`).
pub fn apply_mask(field: &mut Surface, mask: &Surface) {
    for (tx, ty) in field.tile_keys() {
        let m = mask.tile(tx, ty).map(|t| t.to_f32());
        let t = field.tile_mut(tx, ty);
        for i in 0..(TILE * TILE) as usize {
            let k = m.as_ref().map_or(0.0, |m| m[i * 4]);
            let (lx, ly) = (i % TILE as usize, i / TILE as usize);
            let h = t.get(lx, ly)[0] * k;
            t.set(lx, ly, [h; 4]);
        }
        field.prune_tile(tx, ty);
    }
}

/// Merge-down: the upper layer's relief adds onto the lower one's.
pub fn add_into(dst: &mut Surface, src: &Surface) {
    for ((tx, ty), st) in src.tiles() {
        let s = st.to_f32();
        let t = dst.tile_mut(tx, ty);
        for i in 0..(TILE * TILE) as usize {
            let (lx, ly) = (i % TILE as usize, i / TILE as usize);
            let h = t.get(lx, ly)[0] + s[i * 4];
            t.set(lx, ly, [h; 4]);
        }
    }
}

// ── Bevel: relief derived from the layer's own alpha (`bevel-field.ts`) ──

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BevelOptions {
    /// Dome height as a multiple of the painted region's inscribed radius; 0 = off.
    pub depth: f64,
    /// 0..1 extra smoothing: rounder and softer, losing fine relief.
    pub softness: f64,
    /// Alpha (0..255) at or above which a pixel counts as painted.
    pub threshold: f64,
}

impl Default for BevelOptions {
    fn default() -> Self {
        Self { depth: 0.6, softness: 0.25, threshold: 8.0 }
    }
}

/// Chamfer (1, √2) distance to the nearest clear pixel; outside the buffer is clear.
pub fn distance_transform(mask: &[bool], w: usize, h: usize) -> Vec<f32> {
    let far = (w + h) as f32;
    let mut d: Vec<f32> = mask.iter().map(|&m| if m { far } else { 0.0 }).collect();
    let (d1, d2) = (1.0f32, std::f32::consts::SQRT_2);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut b = d[i];
            b = b.min(if x > 0 { d[i - 1] + d1 } else { d1 });
            b = b.min(if y > 0 { d[i - w] + d1 } else { d1 });
            if x > 0 && y > 0 {
                b = b.min(d[i - w - 1] + d2);
            }
            if x + 1 < w && y > 0 {
                b = b.min(d[i - w + 1] + d2);
            }
            d[i] = b;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            if d[i] == 0.0 {
                continue;
            }
            let mut b = d[i];
            b = b.min(if x + 1 < w { d[i + 1] + d1 } else { d1 });
            b = b.min(if y + 1 < h { d[i + w] + d1 } else { d1 });
            if x + 1 < w && y + 1 < h {
                b = b.min(d[i + w + 1] + d2);
            }
            if x > 0 && y + 1 < h {
                b = b.min(d[i + w - 1] + d2);
            }
            d[i] = b;
        }
    }
    d
}

/// Distance → dome height `R·√(1 − (1 − d/dmax)²)`, `R = dmax · depth`.
pub fn dome(dist: &[f32], depth: f64) -> Vec<f32> {
    let dmax = dist.iter().copied().fold(0.0f32, f32::max);
    if dmax <= 0.0 {
        return vec![0.0; dist.len()];
    }
    let r = dmax * depth as f32;
    dist.iter()
        .map(|&d| {
            let u = 1.0 - (d / dmax).min(1.0);
            r * (1.0 - u * u).max(0.0).sqrt()
        })
        .collect()
}

/// Separable box blur, `passes` times, clamp-to-edge (≈ Gaussian at 3 passes).
pub fn blur_field(src: Vec<f32>, w: usize, h: usize, radius: f64, passes: usize) -> Vec<f32> {
    let r = radius.round().max(0.0) as i64;
    if r == 0 || passes == 0 {
        return src;
    }
    let n = (2 * r + 1) as f32;
    let (wi, hi) = (w as i64, h as i64);
    let cx = |x: i64| x.clamp(0, wi - 1) as usize;
    let cy = |y: i64| y.clamp(0, hi - 1) as usize;
    let mut cur = src;
    let mut tmp = vec![0.0f32; w * h];
    for _ in 0..passes {
        for y in 0..h {
            let row = y * w;
            let mut acc: f32 = (-r..=r).map(|k| cur[row + cx(k)]).sum();
            for x in 0..wi {
                tmp[row + x as usize] = acc / n;
                acc += cur[row + cx(x + r + 1)] - cur[row + cx(x - r)];
            }
        }
        let mut out = vec![0.0f32; w * h];
        for x in 0..w {
            let mut acc: f32 = (-r..=r).map(|k| tmp[cy(k) * w + x]).sum();
            for y in 0..hi {
                out[y as usize * w + x] = acc / n;
                acc += tmp[cy(y + r + 1) * w + x] - tmp[cy(y - r) * w + x];
            }
        }
        cur = out;
    }
    cur
}

/// Painted alpha (0..1, row-major, `w × h`) → bevel height, or `None` for an
/// empty layer or zero depth (the caller leaves the field alone).
pub fn bevel_field(alpha: &[f32], w: usize, h: usize, o: BevelOptions) -> Option<Vec<f32>> {
    if w == 0 || h == 0 || o.depth <= 0.0 {
        return None;
    }
    let mask: Vec<bool> = alpha.iter().map(|&a| a * 255.0 >= o.threshold as f32).collect();
    if !mask.iter().any(|&m| m) {
        return None;
    }
    let field = dome(&distance_transform(&mask, w, h), o.depth);
    // Radius scales with the canvas so softness means the same at any size.
    let radius = ((w.min(h) as f64) * 0.01 * (1.0 + o.softness * 6.0)).round().max(1.0);
    Some(blur_field(field, w, h, radius, 3))
}

/// A row-major field as a height [`Surface`] (zeros stay sparse).
pub fn field_to_surface(field: &[f32], doc: Rect) -> Surface {
    let mut s = Surface::new();
    for y in 0..doc.h {
        for x in 0..doc.w {
            let v = field[(y * doc.w + x) as usize];
            if v > 0.0 {
                write(&mut s, doc.x + x, doc.y + y, v);
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_mask(w: usize, h: usize, iw: usize, ih: usize) -> Vec<bool> {
        let (x0, y0) = ((w - iw) / 2, (h - ih) / 2);
        (0..w * h).map(|i| (x0..x0 + iw).contains(&(i % w)) && (y0..y0 + ih).contains(&(i / w))).collect()
    }
    fn alpha_of(m: &[bool]) -> Vec<f32> {
        m.iter().map(|&b| if b { 1.0 } else { 0.0 }).collect()
    }

    #[test]
    fn pigment_curves() {
        assert_eq!(pigment_opacity(1.0), 1.0);
        assert!((pigment_opacity(0.0) - 0.05).abs() < 1e-12);
        assert!((pigment_opacity(0.5) - 0.525).abs() < 1e-12);
        assert_eq!(pigment_opacity(2.0), 1.0);
        assert!((pigment_body(0.8, 0.25) - 0.2).abs() < 1e-12);
        assert_eq!(pigment_body(-1.0, 1.0), 0.0);
    }

    #[test]
    fn distance_transform_matches_happypaint() {
        let d = distance_transform(&box_mask(21, 21, 11, 11), 21, 21);
        assert_eq!((d[0], d[10 * 21 + 5], d[10 * 21 + 10]), (0.0, 1.0, 6.0));
        let full = distance_transform(&[true; 225], 15, 15);
        assert_eq!(full[7 * 15], 1.0);
        assert!((full[7 * 15 + 7] - 8.0).abs() < 1e-5);
        let d = distance_transform(&box_mask(41, 41, 31, 31), 41, 41);
        assert!(d[20 * 41 + 20] > 15.0 && d[20 * 41 + 20] < 17.0);
    }

    #[test]
    fn dome_is_steep_at_the_rim_and_scales_with_depth() {
        let w = 31;
        let dist = distance_transform(&box_mask(31, 31, 21, 21), 31, 31);
        let f = dome(&dist, 1.0);
        let at = |f: &[f32], x: usize, y: usize| f[y * w + x];
        assert_eq!(at(&f, 0, 0), 0.0);
        assert!(at(&f, 6, 15) - at(&f, 5, 15) > (at(&f, 15, 15) - at(&f, 14, 15)) * 3.0);
        let half = dome(&dist, 0.5);
        assert!((at(&f, 15, 15) - 2.0 * at(&half, 15, 15)).abs() < 1e-4);
        assert!(dome(&dist, 0.0).iter().all(|&v| v == 0.0));
        assert!(dome(&[0.0; 9], 1.0).iter().all(|&v| v == 0.0));
    }

    #[test]
    fn blur_conserves_and_does_not_alias() {
        let (w, h) = (25, 25);
        let mut spike = vec![0.0f32; w * h];
        spike[12 * w + 12] = 100.0;
        let out = blur_field(spike.clone(), w, h, 2.0, 3);
        assert!(out[12 * w + 12] < 100.0 && out[12 * w + 10] > 0.0);
        assert!((out[12 * w + 11] - out[12 * w + 13]).abs() < 1e-4);
        assert!((out.iter().sum::<f32>() - 100.0).abs() < 0.5);
        let one = blur_field(spike.clone(), w, h, 2.0, 1);
        assert!(out[12 * w + 12] < one[12 * w + 12] && out[12 * w + 12] > 0.0);
        assert_eq!(blur_field(vec![1.0, 2.0, 3.0, 4.0], 2, 2, 0.0, 3), vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn bevel_field_from_alpha() {
        let (w, h) = (41, 41);
        let px = alpha_of(&box_mask(w, h, 25, 25));
        let at = |f: &[f32], x: usize, y: usize| f[y * w + x];
        let f = bevel_field(&px, w, h, BevelOptions::default()).unwrap();
        assert!(at(&f, 20, 20) > 0.0 && at(&f, 1, 1).abs() < 1e-3 && at(&f, 20, 20) > at(&f, 10, 20));
        let worst = (9..20).map(|k| (at(&f, k + 1, k + 1) - at(&f, k, k)).abs()).fold(0.0, f32::max);
        assert!(worst < at(&f, 20, 20) * 0.25, "no medial-axis ridge");
        assert!(bevel_field(&vec![0.0; w * h], w, h, BevelOptions::default()).is_none());
        assert!(bevel_field(&px, w, h, BevelOptions { depth: 0.0, ..Default::default() }).is_none());
        let o = BevelOptions::default();
        let shallow = bevel_field(&px, w, h, BevelOptions { depth: 0.3, ..o }).unwrap();
        let deep = bevel_field(&px, w, h, BevelOptions { depth: 1.2, ..o }).unwrap();
        assert!(at(&deep, 20, 20) > at(&shallow, 20, 20));
        let crisp = bevel_field(&px, w, h, BevelOptions { softness: 0.0, ..o }).unwrap();
        let soft = bevel_field(&px, w, h, BevelOptions { softness: 1.0, ..o }).unwrap();
        assert!(at(&soft, 20, 20) < at(&crisp, 20, 20));
        let steepest = |f: &[f32]| (1..w - 1).map(|x| (at(f, x + 1, 20) - at(f, x, 20)).abs()).fold(0.0, f32::max);
        assert!(steepest(&soft) < steepest(&crisp));
        // Threshold is on 0..255 alpha.
        let faint = [0.0, 7.0 / 255.0, 8.0 / 255.0, 1.0];
        let m: Vec<bool> = faint.iter().map(|&a| a * 255.0 >= 8.0).collect();
        assert_eq!(m, [false, false, true, true]);
    }

    #[test]
    fn stamp_smooth_and_carve() {
        let doc = Rect::new(0, 0, 64, 64);
        let mut f = Surface::new();
        let d = HeightDab { x: 32.0, y: 32.0, radius: 10.0, amount: 0.5 };
        stamp(&mut f, &[d, d], doc);
        assert!((sample(&f, 32, 32) - 1.0).abs() < 1e-2, "additive: {}", sample(&f, 32, 32));
        assert_eq!(sample(&f, 32, 50), 0.0);
        assert!(sample(&f, 39, 32) < sample(&f, 32, 32), "domed toward the rim");
        // Smoothing levels the field and takes `flatten` off the top.
        let peak = sample(&f, 32, 32);
        smooth(&mut f, &[HeightDab { amount: 1.0, ..d }], 0.12, doc);
        assert!(sample(&f, 32, 32) < peak * 0.95);
        // The eraser carves by coverage; empty tiles prune.
        let mut stroke = Surface::new();
        for y in 0..64 {
            for x in 0..64 {
                stroke.set(x, y, [0.0, 0.0, 0.0, 1.0]);
            }
        }
        carve(&mut f, &stroke, 0.5);
        assert!(sample(&f, 32, 32) > 0.0);
        carve(&mut f, &stroke, 1.0);
        assert_eq!(f.tile_count(), 0);
    }
}
