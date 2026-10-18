//! Color math: sRGB transfer curves, premultiplication, UI colors.
//!
//! Matches happypaint's `engine/color/color.ts` (exact IEC 61966-2-1 curves).

use serde::{Deserialize, Serialize};

/// A premultiplied RGBA pixel in f32.
pub mod cmyk;
pub mod icc;
pub mod median_cut;
pub mod mix_grid;
pub mod pigment;

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

const LUT_N: usize = 4096;

fn to_lin_table() -> &'static [f32; LUT_N + 1] {
    static T: std::sync::OnceLock<Box<[f32; LUT_N + 1]>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0.0; LUT_N + 1]);
        for (i, v) in t.iter_mut().enumerate() {
            *v = srgb_to_linear(i as f32 / LUT_N as f32);
        }
        t
    })
}

/// `linear_to_srgb` tabulated over √x, which keeps the steep start accurate.
fn to_srgb_table() -> &'static [f32; LUT_N + 1] {
    static T: std::sync::OnceLock<Box<[f32; LUT_N + 1]>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        let mut t = Box::new([0.0; LUT_N + 1]);
        for (i, v) in t.iter_mut().enumerate() {
            let s = i as f32 / LUT_N as f32;
            *v = linear_to_srgb(s * s);
        }
        t
    })
}

#[inline]
fn lerp_table(t: &[f32; LUT_N + 1], u: f32) -> f32 {
    let x = u * LUT_N as f32;
    let i = (x as usize).min(LUT_N - 1);
    let f = x - i as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

/// Table-driven `srgb_to_linear` for hot loops (|error| < 2e-5 on 0..1).
#[inline]
pub fn fast_srgb_to_linear(c: f32) -> f32 {
    if (0.0..=1.0).contains(&c) { lerp_table(to_lin_table(), c) } else { srgb_to_linear(c) }
}

/// Table-driven `linear_to_srgb` for hot loops (|error| < 1e-4 on 0..1).
#[inline]
pub fn fast_linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        return linear_to_srgb(c);
    }
    if c <= 1.0 { lerp_table(to_srgb_table(), c.sqrt()) } else { linear_to_srgb(c) }
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
    [fast_linear_to_srgb(s(p[0])) * a, fast_linear_to_srgb(s(p[1])) * a, fast_linear_to_srgb(s(p[2])) * a, a]
}

/// Premultiplied sRGB → premultiplied linear (happypaint `premSrgb2Lin`).
#[inline]
pub fn prem_srgb_to_lin(p: Px) -> Px {
    let a = p[3];
    let s = |c: f32| if a > 0.0 { c / a } else { c };
    [fast_srgb_to_linear(s(p[0])) * a, fast_srgb_to_linear(s(p[1])) * a, fast_srgb_to_linear(s(p[2])) * a, a]
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

/// How a document's pixel values relate to colour (specs 49, 50): float
/// documents hold premultiplied linear light, 8-bit documents hold
/// premultiplied sRGB values and blend them as they are, like happypaint.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Space {
    #[default]
    Linear,
    Gamma,
}

impl Space {
    /// A straight sRGB colour as a premultiplied pixel in this space.
    pub fn pixel(self, c: Rgba) -> Px {
        match self {
            Space::Linear => c.to_linear_premul(),
            Space::Gamma => premultiply([c.r, c.g, c.b, c.a]),
        }
    }

    /// A premultiplied pixel in this space as a straight sRGB colour.
    pub fn color(self, p: Px) -> Rgba {
        match self {
            Space::Linear => Rgba::from_linear_premul(p),
            Space::Gamma => {
                let u = unpremultiply(p);
                Rgba::new(u[0], u[1], u[2], u[3])
            }
        }
    }

    /// One channel of a straight pixel value as sRGB, exactly.
    pub fn to_srgb(self, v: f32) -> f32 {
        match self {
            Space::Linear => linear_to_srgb(v),
            Space::Gamma => v,
        }
    }

    /// One sRGB channel as a straight pixel value, exactly.
    pub fn from_srgb(self, v: f32) -> f32 {
        match self {
            Space::Linear => srgb_to_linear(v),
            Space::Gamma => v,
        }
    }

    /// One channel of a straight pixel value as sRGB (colour filters' "work" space).
    pub fn to_work(self, v: f32) -> f32 {
        match self {
            Space::Linear => fast_linear_to_srgb(v),
            Space::Gamma => v,
        }
    }

    pub fn from_work(self, v: f32) -> f32 {
        match self {
            Space::Linear => fast_srgb_to_linear(v),
            Space::Gamma => v,
        }
    }

    /// Premultiplied pixels in this space (RGBA runs) to `to`, in place.
    pub fn convert(self, px: &mut [f32], to: Space) {
        if self == to {
            return;
        }
        for p in px.chunks_exact_mut(4) {
            let q = [p[0], p[1], p[2], p[3]];
            let c = if to == Space::Linear { prem_srgb_to_lin(q) } else { prem_lin_to_srgb(q) };
            p.copy_from_slice(&c);
        }
    }

    /// A premultiplied pixel as straight sRGB bytes (PNG tiles, exports).
    pub fn to_srgb8(self, p: Px) -> [u8; 4] {
        let a = p[3].clamp(0.0, 1.0);
        if a <= 0.0 {
            return [0; 4];
        }
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let enc = |v: f32| match self {
            Space::Linear => linear_to_srgb(v / a),
            Space::Gamma => v / a,
        };
        [q(enc(p[0])), q(enc(p[1])), q(enc(p[2])), q(a)]
    }

    /// Straight sRGB bytes as a premultiplied pixel in this space.
    pub fn from_srgb8(self, c: [u8; 4]) -> Px {
        let a = c[3] as f32 / 255.0;
        let dec = |v: u8| match self {
            Space::Linear => srgb_to_linear(v as f32 / 255.0),
            Space::Gamma => v as f32 / 255.0,
        };
        [dec(c[0]) * a, dec(c[1]) * a, dec(c[2]) * a, a]
    }
}

#[cfg(test)]
mod space_tests {
    use super::*;

    #[test]
    fn gamma_keeps_srgb_values_and_linear_is_unchanged() {
        let c = Rgba::new(0.5, 0.25, 1.0, 0.5);
        assert_eq!(Space::Gamma.pixel(c), [0.25, 0.125, 0.5, 0.5]);
        assert_eq!(Space::Linear.pixel(c), c.to_linear_premul());
        let back = Space::Gamma.color(Space::Gamma.pixel(c));
        assert!((back.r - 0.5).abs() < 1e-6 && (back.g - 0.25).abs() < 1e-6);
        assert_eq!(Space::Gamma.to_work(0.3), 0.3);
        // 8-bit straight bytes survive a gamma round trip exactly.
        for a in [255u8, 200, 128, 17, 1] {
            for v in 0..=255u8 {
                let px = [v, 255 - v, v / 2, a];
                assert_eq!(Space::Gamma.to_srgb8(Space::Gamma.from_srgb8(px)), px, "{px:?}");
            }
        }
    }
}
