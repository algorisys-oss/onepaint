//! Impasto relief (happypaint `compositor.ts:149-165`, spec 35 C): the layer's
//! height field lit by one fixed light, applied to its pixels before the mask,
//! opacity and blend mode. Gated by height and by the pixel's own alpha, so a
//! flat layer, and bare canvas under leftover height, are left untouched.

use onepaint_raster::{Rect, Surface, TILE};

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Light from the upper left, toward the viewer (doc space, y down):
/// normalize(-0.45, -0.6, 0.66).
fn light() -> [f32; 3] {
    let v = [-0.45f32, -0.6, 0.66];
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|c| c / n)
}

/// Whether any height lies on this tile or touches its border.
fn near(height: &Surface, tx: i32, ty: i32) -> bool {
    (-1..=1).any(|dy| (-1..=1).any(|dx| height.tile(tx + dx, ty + dy).is_some()))
}

/// Shade one premultiplied tile buffer (`TILE²·4`) at tile (tx, ty).
/// Neighbour taps clamp to the document edge, as the height texture does.
pub fn shade(buf: &mut [f32], height: &Surface, tx: i32, ty: i32, doc: Rect) {
    if !near(height, tx, ty) {
        return;
    }
    let Some(inner) = Rect::from_tile(tx, ty).intersect(&doc) else { return };
    let Some(r) = Rect::new(inner.x - 1, inner.y - 1, inner.w + 2, inner.h + 2).intersect(&doc) else { return };
    let src = height.read_rect(r);
    let l = light();
    let h = |x: i32, y: i32| -> f32 {
        let (x, y) = (x.clamp(r.x, r.right() - 1), y.clamp(r.y, r.bottom() - 1));
        src[(((y - r.y) * r.w + (x - r.x)) * 4) as usize]
    };
    for y in inner.y..inner.bottom() {
        for x in inner.x..inner.right() {
            let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
            let a = buf[i + 3];
            if a <= 0.0 {
                continue;
            }
            let (hl, hr, hu, hd) = (h(x - 1, y), h(x + 1, y), h(x, y - 1), h(x, y + 1));
            let amt = smoothstep(0.02, 0.25, hl.max(hr).max(hu).max(hd));
            if amt <= 0.0 {
                continue;
            }
            let (nx, ny) = ((hl - hr) * 9.0, (hu - hd) * 9.0);
            let len = (nx * nx + ny * ny + 1.0).sqrt();
            let n = [nx / len, ny / len, 1.0 / len];
            let ndl = n[0] * l[0] + n[1] * l[1] + n[2] * l[2];
            let diff = ndl.clamp(0.0, 1.0);
            // reflect(-L, n).z = -L.z + 2·(n·L)·n.z
            let spec = (-l[2] + 2.0 * ndl * n[2]).clamp(0.0, 1.0).powi(22);
            let k = 1.0 + (0.62 + 0.62 * diff - 1.0) * amt;
            let add = spec * 0.3 * amt * a;
            for c in 0..3 {
                buf[i + c] = buf[i + c] * k + add;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onepaint_raster::TILE_PX;

    fn grey_tile() -> Vec<f32> {
        [0.5, 0.5, 0.5, 1.0].repeat(TILE_PX)
    }

    fn px(b: &[f32], x: i32, y: i32) -> f32 {
        b[((y * TILE + x) * 4) as usize]
    }

    #[test]
    fn flat_or_empty_is_untouched() {
        let doc = Rect::new(0, 0, 256, 256);
        let mut b = grey_tile();
        shade(&mut b, &Surface::new(), 0, 0, doc);
        assert_eq!(b, grey_tile());
        // Height under transparent pixels shades nothing.
        let mut h = Surface::new();
        h.set(10, 10, [1.0; 4]);
        let mut clear = vec![0.0; TILE_PX * 4];
        shade(&mut clear, &h, 0, 0, doc);
        assert!(clear.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn a_ridge_is_lit_on_the_light_side_and_shaded_on_the_far_side() {
        let doc = Rect::new(0, 0, 256, 256);
        // A vertical ridge at x = 100: rises from x = 90, falls to x = 110.
        let mut h = Surface::new();
        for y in 0..256 {
            for x in 90..=110 {
                h.set(x, y, [1.0 - (x - 100).abs() as f32 / 10.0; 4]);
            }
        }
        let mut b = grey_tile();
        shade(&mut b, &h, 0, 0, doc);
        // The light comes from -x: the left flank faces it, the right turns away.
        let (lit, dark, top) = (px(&b, 95, 50), px(&b, 105, 50), px(&b, 50, 50));
        assert!(lit > dark + 0.05, "lit {lit}, dark {dark}");
        assert_eq!(top, 0.5, "flat canvas away from the ridge");
        // Alpha is never changed.
        assert!(b.chunks_exact(4).all(|p| p[3] == 1.0));
    }

    #[test]
    fn taps_clamp_at_the_document_edge() {
        let doc = Rect::new(0, 0, 20, 20);
        let mut h = Surface::new();
        for y in 0..20 {
            for x in 0..20 {
                h.set(x, y, [1.0; 4]);
            }
        }
        let mut b = grey_tile();
        shade(&mut b, &h, 0, 0, doc);
        // A uniform plateau has a flat normal everywhere, edges included.
        let flat = px(&b, 10, 10);
        assert!((px(&b, 0, 0) - flat).abs() < 1e-6 && (px(&b, 19, 19) - flat).abs() < 1e-6);
        assert_eq!(px(&b, 30, 30), 0.5, "outside the document is left alone");
    }
}
