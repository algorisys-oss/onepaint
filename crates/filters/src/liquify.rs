//! Liquify forward warp (happypaint `engine/liquify/liquify-renderer.ts`, spec 17).

use onepaint_raster::{Rect, Surface, TILE};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LiquifyMode {
    #[default]
    Push,
    TwirlLeft,
    TwirlRight,
    Pinch,
    Expand,
    Reconstruct,
}

pub const LIQUIFY_MODES: [(LiquifyMode, &str); 6] = [
    (LiquifyMode::Push, "Push"),
    (LiquifyMode::TwirlLeft, "Twirl left"),
    (LiquifyMode::TwirlRight, "Twirl right"),
    (LiquifyMode::Pinch, "Pinch"),
    (LiquifyMode::Expand, "Expand"),
    (LiquifyMode::Reconstruct, "Reconstruct"),
];

/// Bilinear, clamp-to-edge sample of a region read out of the layer.
fn sample(buf: &[f32], r: Rect, x: f64, y: f64) -> [f32; 4] {
    let (u, v) = (x - 0.5 - r.x as f64, y - 0.5 - r.y as f64);
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = ((u - x0) as f32, (v - y0) as f32);
    let cx = |i: f64| (i as i32).clamp(0, r.w - 1);
    let cy = |i: f64| (i as i32).clamp(0, r.h - 1);
    let px = |xi: i32, yi: i32| {
        let i = ((yi * r.w + xi) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    let (xa, xb, ya, yb) = (cx(x0), cx(x0 + 1.0), cy(y0), cy(y0 + 1.0));
    let (a, b, c, d) = (px(xa, ya), px(xb, ya), px(xa, yb), px(xb, yb));
    std::array::from_fn(|k| {
        let top = a[k] + (b[k] - a[k]) * fx;
        let bot = c[k] + (d[k] - c[k]) * fx;
        top + (bot - top) * fy
    })
}

/// One warp dab centred at `c` (coming from `prev`); returns the touched rect.
#[allow(clippy::too_many_arguments)]
pub fn dab(
    layer: &mut Surface,
    orig: &Surface,
    doc: Rect,
    mode: LiquifyMode,
    c: (f64, f64),
    prev: (f64, f64),
    radius: f64,
    strength: f64,
) -> Rect {
    let r = radius.max(1.0);
    let reach = r + (c.0 - prev.0).hypot(c.1 - prev.1) + 2.0;
    let Some(read) = Rect::covering((c.0 - reach) as f32, (c.1 - reach) as f32, (c.0 + reach) as f32, (c.1 + reach) as f32).intersect(&doc)
    else {
        return Rect::default();
    };
    let Some(out) =
        Rect::covering((c.0 - r - 1.0) as f32, (c.1 - r - 1.0) as f32, (c.0 + r + 1.0) as f32, (c.1 + r + 1.0) as f32).intersect(&doc)
    else {
        return Rect::default();
    };
    if strength <= 0.0 {
        return out;
    }
    let src = layer.read_rect(read);
    let (dw, dh) = (doc.right() as f64, doc.bottom() as f64);
    for (tx, ty) in out.tiles() {
        let Some(ix) = out.intersect(&Rect::from_tile(tx, ty)) else { continue };
        let tile = layer.tile_mut(tx, ty);
        for y in ix.y..ix.bottom() {
            let py = y as f64 + 0.5;
            for x in ix.x..ix.right() {
                let px = x as f64 + 0.5;
                let dd = (px - c.0).hypot(py - c.1) / r;
                if dd >= 1.0 {
                    continue;
                }
                let f = 1.0 - dd * dd;
                let w = f * f * strength;
                let (lx, ly) = ((x - tx * TILE) as usize, (y - ty * TILE) as usize);
                if mode == LiquifyMode::Reconstruct {
                    let cur = sample(&src, read, px, py);
                    let o = orig.get(x, y);
                    let w = w as f32;
                    tile.set(lx, ly, std::array::from_fn(|k| cur[k] + (o[k] - cur[k]) * w));
                    continue;
                }
                let (sx, sy) = match mode {
                    LiquifyMode::Push => (px - w * (c.0 - prev.0), py - w * (c.1 - prev.1)),
                    LiquifyMode::TwirlLeft | LiquifyMode::TwirlRight => {
                        let ang = w * 0.6 * if mode == LiquifyMode::TwirlLeft { 1.0 } else { -1.0 };
                        let (dx, dy) = (px - c.0, py - c.1);
                        let (cs, sn) = (ang.cos(), ang.sin());
                        (c.0 + cs * dx - sn * dy, c.1 + sn * dx + cs * dy)
                    }
                    LiquifyMode::Pinch => (px + w * 0.5 * (px - c.0), py + w * 0.5 * (py - c.1)),
                    LiquifyMode::Expand | LiquifyMode::Reconstruct => (px - w * 0.5 * (px - c.0), py - w * 0.5 * (py - c.1)),
                };
                tile.set(lx, ly, sample(&src, read, sx.clamp(0.0, dw), sy.clamp(0.0, dh)));
            }
        }
    }
    out
}

/// Split the move `last → p` into dabs (spacing `max(1, .3·r)`, at most 64).
pub fn dab_centres(last: (f64, f64), p: (f64, f64), radius: f64) -> Vec<((f64, f64), (f64, f64))> {
    let spacing = (radius * 0.3).max(1.0);
    let len = (p.0 - last.0).hypot(p.1 - last.1);
    let steps = (len / spacing).ceil().clamp(1.0, 64.0) as usize;
    let at = |i: usize| (last.0 + (p.0 - last.0) * i as f64 / steps as f64, last.1 + (p.1 - last.1) * i as f64 / steps as f64);
    (1..=steps).map(|i| (at(i), at(i - 1))).collect()
}
