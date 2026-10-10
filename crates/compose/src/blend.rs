//! Per-pixel blend math, ported from happypaint `engine/compositor.ts:316-411`.
//!
//! All pixels are premultiplied, linear light (the float document path).
//! Normal and add composite in linear; every other mode is computed on
//! premultiplied sRGB and converted back, exactly as happypaint's shader does.

use onepaint_color::{Px, Space, prem_lin_to_srgb, prem_srgb_to_lin};
use onepaint_doc::BlendMode;

/// Separable per-channel blend `B(b, s)` on straight colour, with happypaint's
/// non-W3C edge cases for dodge, burn and divide.
#[inline]
pub fn bc(m: BlendMode, b: f32, s: f32) -> f32 {
    use BlendMode::*;
    match m {
        Multiply => b * s,
        Screen => b + s - b * s,
        Overlay => {
            if b <= 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        Darken => b.min(s),
        Lighten => b.max(s),
        ColorDodge => {
            if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s)).min(1.0)
            }
        }
        ColorBurn => {
            if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s).min(1.0)
            }
        }
        HardLight => {
            if s <= 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        SoftLight => {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 { ((16.0 * b - 12.0) * b + 4.0) * b } else { b.sqrt() };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
        Difference => (b - s).abs(),
        Exclusion => b + s - 2.0 * b * s,
        LinearBurn => (b + s - 1.0).max(0.0),
        Divide => {
            if s <= 0.0 {
                1.0
            } else {
                (b / s).min(1.0)
            }
        }
        Add => b + s,
        _ => s,
    }
}

#[inline]
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

#[inline]
fn clip_color(mut c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    if n < 0.0 {
        c = c.map(|v| l + (v - l) * l / (l - n).max(1e-6));
    }
    if x > 1.0 {
        c = c.map(|v| l + (v - l) * (1.0 - l) / (x - l).max(1e-6));
    }
    c
}

#[inline]
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

#[inline]
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

#[inline]
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mn = c[0].min(c[1]).min(c[2]);
    let mx = c[0].max(c[1]).max(c[2]);
    if mx > mn { c.map(|v| (v - mn) * s / (mx - mn)) } else { [0.0; 3] }
}

/// `B(Cb, Cs)` on straight colour, including the non-separable modes.
#[inline]
pub fn blend_color(m: BlendMode, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    use BlendMode::*;
    match m {
        Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        Color => set_lum(s, lum(b)),
        Luminosity => set_lum(b, lum(s)),
        // Subtractive glaze: Kubelka–Munk 50/50 of backdrop and source (compositor.ts:376).
        Pigment => onepaint_color::pigment::pigment_mix(b, s, 0.5),
        _ => [bc(m, b[0], s[0]), bc(m, b[1], s[1]), bc(m, b[2], s[2])],
    }
}

#[inline]
fn straight(p: Px) -> [f32; 3] {
    if p[3] > 0.0 { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] } else { [0.0; 3] }
}

/// Composite `src · opacity` over backdrop `bp` with mode `m`.
#[inline]
pub fn blend_px(m: BlendMode, bp: Px, src: Px, opacity: f32) -> Px {
    let sp = src.map(|v| v * opacity);
    if sp[3] <= 0.0 && sp[0] == 0.0 && sp[1] == 0.0 && sp[2] == 0.0 {
        return bp;
    }
    match m {
        BlendMode::Normal => {
            let k = 1.0 - sp[3];
            [sp[0] + bp[0] * k, sp[1] + bp[1] * k, sp[2] + bp[2] * k, sp[3] + bp[3] * k]
        }
        BlendMode::Add => [sp[0] + bp[0], sp[1] + bp[1], sp[2] + bp[2], sp[3] + bp[3] * (1.0 - sp[3])],
        _ => {
            let s = prem_lin_to_srgb(sp);
            let b = prem_lin_to_srgb(bp);
            let (a_s, ab) = (s[3], b[3]);
            let cs = straight(s);
            let cb = straight(b);
            let bl = blend_color(m, cb, cs);
            let ao = a_s + ab * (1.0 - a_s);
            let mut o = [0.0, 0.0, 0.0, ao];
            for i in 0..3 {
                o[i] = a_s * (1.0 - ab) * cs[i] + a_s * ab * bl[i] + (1.0 - a_s) * ab * cb[i];
            }
            prem_srgb_to_lin(o)
        }
    }
}

/// [`blend_px`] in `space` (spec 50). In gamma space (8-bit documents) every
/// mode blends the stored sRGB values as they are; multiply, screen and add
/// are happypaint's fixed-function GL blends there, and an 8-bit target
/// clamps the result.
#[inline]
pub fn blend_px_in(space: Space, m: BlendMode, bp: Px, src: Px, opacity: f32) -> Px {
    if space == Space::Linear {
        return blend_px(m, bp, src, opacity);
    }
    let sp = src.map(|v| v * opacity);
    if sp[3] <= 0.0 && sp[0] == 0.0 && sp[1] == 0.0 && sp[2] == 0.0 {
        return bp;
    }
    let o = match m {
        BlendMode::Normal => {
            let k = 1.0 - sp[3];
            [sp[0] + bp[0] * k, sp[1] + bp[1] * k, sp[2] + bp[2] * k, sp[3] + bp[3] * k]
        }
        // ONE, ONE (colour); ONE, ONE_MINUS_SRC_ALPHA (alpha).
        BlendMode::Add => [sp[0] + bp[0], sp[1] + bp[1], sp[2] + bp[2], sp[3] + bp[3] * (1.0 - sp[3])],
        // DST_COLOR, ONE_MINUS_SRC_ALPHA: alpha stays the backdrop's, and the
        // source adds nothing over transparency (happypaint's multiply).
        BlendMode::Multiply => [0, 1, 2, 3].map(|i| sp[i] * bp[i] + bp[i] * (1.0 - sp[3])),
        // ONE, ONE_MINUS_SRC_COLOR, alpha included.
        BlendMode::Screen => [0, 1, 2, 3].map(|i| sp[i] + bp[i] * (1.0 - sp[i])),
        _ => {
            let (a_s, ab) = (sp[3], bp[3]);
            let (cs, cb) = (straight(sp), straight(bp));
            let bl = blend_color(m, cb, cs);
            let mut o = [0.0, 0.0, 0.0, a_s + ab * (1.0 - a_s)];
            for i in 0..3 {
                o[i] = a_s * (1.0 - ab) * cs[i] + a_s * ab * bl[i] + (1.0 - a_s) * ab * cb[i];
            }
            o
        }
    };
    o.map(|v| v.clamp(0.0, 1.0))
}

/// [`blend_px_alpha_lock`] in `space` (spec 50): gamma blends the stored values.
#[inline]
pub fn blend_px_alpha_lock_in(space: Space, m: BlendMode, bp: Px, src: Px, opacity: f32) -> Px {
    if space == Space::Linear || m == BlendMode::Normal {
        return blend_px_alpha_lock(m, bp, src, opacity);
    }
    let sp = src.map(|v| v * opacity);
    if sp[3] <= 0.0 {
        return bp;
    }
    let (a_s, ab) = (sp[3], bp[3]);
    let (cs, cb) = (straight(sp), straight(bp));
    let bl = blend_color(m, cb, cs);
    let mut o = [0.0, 0.0, 0.0, ab];
    for i in 0..3 {
        o[i] = ((a_s * bl[i] + (1.0 - a_s) * cb[i]) * ab).clamp(0.0, 1.0);
    }
    o
}

/// Alpha-locked recolour with mode `m`: colour changes, backdrop alpha is kept.
#[inline]
pub fn blend_px_alpha_lock(m: BlendMode, bp: Px, src: Px, opacity: f32) -> Px {
    let sp = src.map(|v| v * opacity);
    if sp[3] <= 0.0 {
        return bp;
    }
    if m == BlendMode::Normal {
        let k = 1.0 - sp[3];
        return [sp[0] * bp[3] + bp[0] * k, sp[1] * bp[3] + bp[1] * k, sp[2] * bp[3] + bp[2] * k, bp[3]];
    }
    let s = prem_lin_to_srgb(sp);
    let b = prem_lin_to_srgb(bp);
    let (a_s, ab) = (s[3], b[3]);
    let cs = straight(s);
    let cb = straight(b);
    let bl = blend_color(m, cb, cs);
    let mut o = [0.0, 0.0, 0.0, ab];
    for i in 0..3 {
        o[i] = (a_s * bl[i] + (1.0 - a_s) * cb[i]) * ab;
    }
    prem_srgb_to_lin(o)
}

/// Clipped-layer draw: `o = s·op·d.a + d·(1 − s.a·op)` (GL `DST_ALPHA, ONE_MINUS_SRC_ALPHA`).
#[inline]
pub fn clip_over(d: Px, src: Px, opacity: f32) -> Px {
    let k = 1.0 - src[3] * opacity;
    let da = d[3];
    [src[0] * opacity * da + d[0] * k, src[1] * opacity * da + d[1] * k, src[2] * opacity * da + d[2] * k, src[3] * opacity * da + d[3] * k]
}
