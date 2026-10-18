//! A layer's "look" (happypaint `look-meta.ts`): mask density and feather,
//! and Blend If. Kept in the layer's `extra` under happypaint's keys, and
//! omitted at their defaults, so `.happypaint` files round-trip as they are.
//! Groups have none.

use crate::Layer;
use serde_json::{Value, json};

pub const MASK_DENSITY_KEY: &str = "maskDensity";
pub const MASK_FEATHER_KEY: &str = "maskFeather";
pub const BLEND_IF_KEY: &str = "blendIf";

/// A Blend If slider pair: black low, black high, white low, white high, in
/// 0..=255 tone values. Tones outside `[0] ..= [3]` hide the layer; between
/// the halves of a split slider they fade.
pub type BlendRange = [u8; 4];

/// Every tone passes.
pub const FULL_RANGE: BlendRange = [0, 0, 255, 255];

/// One channel's ranges: "This Layer" (`src`) and "Underlying Layer" (`dst`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChannelRanges {
    pub src: BlendRange,
    pub dst: BlendRange,
}

impl Default for ChannelRanges {
    fn default() -> Self {
        Self { src: FULL_RANGE, dst: FULL_RANGE }
    }
}

/// Blend If for gray, red, green and blue, in that order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct BlendIf(pub [ChannelRanges; 4]);

impl BlendIf {
    /// Every slider at its end: the layer blends as if there were none.
    pub fn is_off(&self) -> bool {
        self.0.iter().all(|c| c.src == FULL_RANGE && c.dst == FULL_RANGE)
    }

    /// Read happypaint's JSON (`[{src, dst}, …]`); missing channels are full.
    pub fn from_json(v: &Value) -> Option<Self> {
        let mut out = Self::default();
        for (i, c) in v.as_array()?.iter().take(4).enumerate() {
            let range = |k: &str| -> Option<BlendRange> {
                let a = c.get(k)?.as_array()?;
                let mut r = FULL_RANGE;
                for (j, x) in a.iter().take(4).enumerate() {
                    r[j] = x.as_f64()?.round().clamp(0.0, 255.0) as u8;
                }
                Some(r)
            };
            out.0[i] = ChannelRanges { src: range("src").unwrap_or(FULL_RANGE), dst: range("dst").unwrap_or(FULL_RANGE) };
        }
        Some(out)
    }

    pub fn to_json(&self) -> Value {
        Value::Array(self.0.iter().map(|c| json!({ "src": c.src, "dst": c.dst })).collect())
    }
}

impl serde::Serialize for BlendIf {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for BlendIf {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        Self::from_json(&v).ok_or_else(|| serde::de::Error::custom("Blend If is a list of {src, dst}"))
    }
}

impl Layer {
    /// How strongly the mask hides, 0..=1 (1 when unset).
    pub fn mask_density(&self) -> f32 {
        self.extra.get(MASK_DENSITY_KEY).and_then(Value::as_f64).map_or(1.0, |d| d.clamp(0.0, 1.0) as f32)
    }

    /// Density 1 or more drops the key, as happypaint does.
    pub fn set_mask_density(&mut self, d: f32) {
        if d >= 1.0 || d.is_nan() {
            self.extra.remove(MASK_DENSITY_KEY);
        } else {
            self.extra.insert(MASK_DENSITY_KEY.into(), json!(d.max(0.0)));
        }
    }

    /// The mask's feather, a Gaussian sigma in document px (0 when unset).
    pub fn mask_feather(&self) -> f32 {
        self.extra.get(MASK_FEATHER_KEY).and_then(Value::as_f64).map_or(0.0, |f| f.max(0.0) as f32)
    }

    pub fn set_mask_feather(&mut self, f: f32) {
        if f > 0.0 {
            self.extra.insert(MASK_FEATHER_KEY.into(), json!(f));
        } else {
            self.extra.remove(MASK_FEATHER_KEY);
        }
    }

    /// The layer's Blend If, unless every slider is at its end.
    pub fn blend_if(&self) -> Option<BlendIf> {
        self.extra.get(BLEND_IF_KEY).and_then(BlendIf::from_json).filter(|b| !b.is_off())
    }

    /// Set Blend If; `None`, or one that lets every tone through, drops the key.
    pub fn set_blend_if(&mut self, b: Option<BlendIf>) {
        match b.filter(|b| !b.is_off()) {
            Some(b) => {
                self.extra.insert(BLEND_IF_KEY.into(), b.to_json());
            }
            None => {
                self.extra.remove(BLEND_IF_KEY);
            }
        }
    }
}

/// The mask factor for raw coverage `m`: `1 − density · (1 − m)`
/// (happypaint `compositor.ts` MASKMUL).
#[inline]
pub fn mask_k(density: f32, m: f32) -> f32 {
    1.0 - density * (1.0 - m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_omitted_like_look_meta_of() {
        let mut l = Layer::new("a");
        l.set_mask_density(1.0);
        l.set_mask_feather(0.0);
        l.set_blend_if(Some(BlendIf::default()));
        assert!(l.extra.is_empty());
        assert_eq!((l.mask_density(), l.mask_feather(), l.blend_if()), (1.0, 0.0, None));
        l.set_mask_density(-0.5);
        assert_eq!(l.mask_density(), 0.0);
    }

    #[test]
    fn blend_if_json_matches_happypaint() {
        let mut b = BlendIf::default();
        b.0[0].src = [0, 0, 200, 240];
        let mut l = Layer::new("a");
        l.set_mask_density(0.5);
        l.set_mask_feather(4.0);
        l.set_blend_if(Some(b));
        assert_eq!(l.extra[BLEND_IF_KEY][0], json!({ "src": [0, 0, 200, 240], "dst": [0, 0, 255, 255] }));
        assert_eq!(l.extra[BLEND_IF_KEY].as_array().map(Vec::len), Some(4));
        assert_eq!((l.mask_density(), l.mask_feather(), l.blend_if()), (0.5, 4.0, Some(b)));
        // A short list fills the rest with full ranges.
        let short = BlendIf::from_json(&json!([{ "src": [0, 0, 255, 255], "dst": [30, 30, 255, 255] }])).unwrap();
        assert_eq!(short.0[0].dst, [30, 30, 255, 255]);
        assert_eq!(short.0[3], ChannelRanges::default());
    }
}
