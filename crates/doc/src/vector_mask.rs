//! A layer's vector mask (happypaint `engine/vector-mask.ts`, v0.202.1):
//! closed bezier paths, each filled by its own rule and combined with the
//! ones before it; the result multiplies the layer after its pixel mask.
//! Kept in the layer's `extra` under `"vectorMask"` in happypaint's form.

use crate::Layer;
use serde::{Deserialize, Serialize};

pub const VECTOR_MASK_KEY: &str = "vectorMask";

/// How a path combines with the paths before it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorOp {
    #[default]
    Combine,
    Subtract,
    Intersect,
    Exclude,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FillRule {
    #[default]
    #[serde(rename = "even-odd")]
    EvenOdd,
    #[serde(rename = "non-zero")]
    NonZero,
}

/// One knot: in-handle x, y, anchor x, y, out-handle x, y (document px).
pub type VectorKnot = [f64; 6];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VectorPath {
    /// Photoshop's open paths; they fill as if closed by a straight line.
    #[serde(default, skip_serializing_if = "is_false")]
    pub open: bool,
    pub op: VectorOp,
    pub rule: FillRule,
    pub knots: Vec<VectorKnot>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VectorMask {
    pub paths: Vec<VectorPath>,
    /// Show outside the paths instead of inside.
    #[serde(default, skip_serializing_if = "is_false")]
    pub invert: bool,
    /// Kept, but not hiding anything (the Properties "Apply" box unticked).
    #[serde(default, skip_serializing_if = "is_false")]
    pub disabled: bool,
    /// Doesn't follow Free Transform (Photoshop's unlinked mask).
    #[serde(default, skip_serializing_if = "is_false")]
    pub unlinked: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl VectorMask {
    /// Whether it hides anything (`vectorMaskActive`).
    pub fn active(&self) -> bool {
        !self.disabled && !self.paths.is_empty()
    }

    /// Every knot point through the affine map `[a, b, c, d, e, f]`
    /// (`x' = a·x + c·y + e`, `y' = b·x + d·y + f`; `transformVectorMask`).
    pub fn transformed(&self, [a, b, c, d, e, f]: [f64; 6]) -> Self {
        let mut out = self.clone();
        for k in out.paths.iter_mut().flat_map(|p| p.knots.iter_mut()) {
            for i in [0, 2, 4] {
                let (x, y) = (k[i], k[i + 1]);
                k[i] = a * x + c * y + e;
                k[i + 1] = b * x + d * y + f;
            }
        }
        out
    }

    /// A stable key for caches: the mask's JSON.
    pub fn key(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

impl Layer {
    /// The layer's vector mask, active or not.
    pub fn vector_mask(&self) -> Option<VectorMask> {
        self.extra.get(VECTOR_MASK_KEY).and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Set or remove the vector mask.
    pub fn set_vector_mask(&mut self, v: Option<VectorMask>) {
        match v.and_then(|v| serde_json::to_value(v).ok()) {
            Some(v) => {
                self.extra.insert(VECTOR_MASK_KEY.into(), v);
            }
            None => {
                self.extra.remove(VECTOR_MASK_KEY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<VectorKnot> {
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].iter().map(|p| [p[0], p[1], p[0], p[1], p[0], p[1]]).collect()
    }

    #[test]
    fn json_matches_happypaint_and_false_flags_are_omitted() {
        let v = VectorMask {
            paths: vec![VectorPath { knots: rect(0.0, 0.0, 1.0, 1.0), ..Default::default() }],
            invert: true,
            ..Default::default()
        };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["paths"][0]["op"], "combine");
        assert_eq!(j["paths"][0]["rule"], "even-odd");
        assert_eq!(j["invert"], true);
        assert!(j.get("disabled").is_none() && j["paths"][0].get("open").is_none());
        let back: VectorMask = serde_json::from_value(json!({
            "paths": [{"op": "subtract", "rule": "non-zero", "open": true, "knots": [[1, 2, 3, 4, 5, 6]]}]
        }))
        .unwrap();
        assert_eq!((back.paths[0].op, back.paths[0].rule, back.paths[0].open), (VectorOp::Subtract, FillRule::NonZero, true));
    }

    #[test]
    fn transform_maps_every_point_and_leaves_the_original() {
        // vector-mask.test.ts: translate (5, 7).
        let v = VectorMask { paths: vec![VectorPath { knots: rect(0.0, 0.0, 10.0, 10.0), ..Default::default() }], ..Default::default() };
        let t = v.transformed([1.0, 0.0, 0.0, 1.0, 5.0, 7.0]);
        assert_eq!(t.paths[0].knots[2], [15.0, 17.0, 15.0, 17.0, 15.0, 17.0]);
        assert_eq!(v.paths[0].knots[2], [10.0; 6]);
        let mut l = Layer::new("l");
        l.set_vector_mask(Some(t.clone()));
        assert_eq!(l.vector_mask(), Some(t));
        l.set_vector_mask(None);
        assert!(l.extra.is_empty());
    }
}
