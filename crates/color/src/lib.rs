//! Color math: sRGB transfer curves, premultiplication, UI colors.
//!
//! Matches happypaint's `engine/color/color.ts` (exact IEC 61966-2-1 curves).

use serde::{Deserialize, Serialize};

/// A premultiplied RGBA pixel in f32.
pub type Px = [f32; 4];

#[inline]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

#[inline]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

#[inline]
pub fn premultiply(c: Px) -> Px {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// Straight color from premultiplied; alpha 0 gives all zeros.
#[inline]
pub fn unpremultiply(p: Px) -> Px {
    if p[3] > 0.0 { [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3]] } else { [0.0; 4] }
}

/// Premultiplied linear → premultiplied sRGB (happypaint `premLin2Srgb`).
#[inline]
pub fn prem_lin_to_srgb(p: Px) -> Px {
    let a = p[3];
    let s = |c: f32| if a > 0.0 { c / a } else { c };
    [linear_to_srgb(s(p[0])) * a, linear_to_srgb(s(p[1])) * a, linear_to_srgb(s(p[2])) * a, a]
}

/// Premultiplied sRGB → premultiplied linear (happypaint `premSrgb2Lin`).
#[inline]
pub fn prem_srgb_to_lin(p: Px) -> Px {
    let a = p[3];
    let s = |c: f32| if a > 0.0 { c / a } else { c };
    [srgb_to_linear(s(p[0])) * a, srgb_to_linear(s(p[1])) * a, srgb_to_linear(s(p[2])) * a, a]
}

/// A straight-alpha sRGB color, the type the UI works in.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Default for Rgba {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0, 1.0)
    }
}

impl Rgba {
    pub const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Premultiplied linear-light pixel, the document's storage form.
    pub fn to_linear_premul(self) -> Px {
        premultiply([srgb_to_linear(self.r), srgb_to_linear(self.g), srgb_to_linear(self.b), self.a])
    }

    pub fn from_linear_premul(p: Px) -> Self {
        let u = unpremultiply(p);
        Self::new(linear_to_srgb(u[0]), linear_to_srgb(u[1]), linear_to_srgb(u[2]), u[3])
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let ch = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|v| v as f32 / 255.0);
        Some(Self::new(ch(0)?, ch(2)?, ch(4)?, 1.0))
    }

    pub fn to_hex(self) -> String {
        let b = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", b(self.r), b(self.g), b(self.b))
    }

    /// `h` in degrees [0, 360), `s` and `v` in [0, 1].
    pub fn from_hsv(h: f32, s: f32, v: f32, a: f32) -> Self {
        let h = h.rem_euclid(360.0) / 60.0;
        let c = v * s;
        let x = c * (1.0 - (h % 2.0 - 1.0).abs());
        let (r, g, b) = match h as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        let m = v - c;
        Self::new(r + m, g + m, b + m, a)
    }

    /// Returns `(h degrees, s, v)`; hue is 0 for greys.
    pub fn to_hsv(self) -> (f32, f32, f32) {
        let mx = self.r.max(self.g).max(self.b);
        let mn = self.r.min(self.g).min(self.b);
        let d = mx - mn;
        let s = if mx > 0.0 { d / mx } else { 0.0 };
        let h = if d <= 0.0 {
            0.0
        } else if mx == self.r {
            60.0 * ((self.g - self.b) / d).rem_euclid(6.0)
        } else if mx == self.g {
            60.0 * ((self.b - self.r) / d + 2.0)
        } else {
            60.0 * ((self.r - self.g) / d + 4.0)
        };
        (h, s, mx)
    }
}
