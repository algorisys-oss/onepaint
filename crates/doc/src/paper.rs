//! Document paper (happypaint `PaperSettings`, `utils/paper-presets.ts`): a
//! grain tile the dry brushes bite into, the wet sim soaks into, and the
//! canvas shows as a lit tooth. Stored as `doc.paper` in `.happypaint`;
//! absent means smooth.

use serde::{Deserialize, Serialize};

pub const PAPER_SCALE_MIN: f64 = 0.25;
pub const PAPER_SCALE_MAX: f64 = 4.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paper {
    /// A brush grain id (`rough-tooth`, `paper`, `canvas`, `linen`, or an imported grain).
    pub grain_id: String,
    /// Tile multiplier (0.25..4; absent = 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// 0..1.
    pub strength: f64,
}

impl Paper {
    pub fn scale(&self) -> f64 {
        self.scale.unwrap_or(1.0).clamp(PAPER_SCALE_MIN, PAPER_SCALE_MAX)
    }

    pub fn strength(&self) -> f64 {
        self.strength.clamp(0.0, 1.0)
    }
}

/// A paper choice in the UI: id, label, and the paper (`None` = smooth).
pub struct PaperPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub grain_id: Option<&'static str>,
    pub strength: f64,
    pub scale: f64,
}

pub const PAPER_PRESETS: [PaperPreset; 5] = [
    PaperPreset { id: "smooth", label: "Smooth", grain_id: None, strength: 0.0, scale: 1.0 },
    PaperPreset { id: "rough", label: "Rough", grain_id: Some("rough-tooth"), strength: 0.85, scale: 1.0 },
    PaperPreset { id: "paper", label: "Fine grain", grain_id: Some("paper"), strength: 0.6, scale: 0.8 },
    PaperPreset { id: "canvas", label: "Canvas", grain_id: Some("canvas"), strength: 0.75, scale: 1.3 },
    PaperPreset { id: "linen", label: "Linen", grain_id: Some("linen"), strength: 0.7, scale: 2.6 },
];

impl PaperPreset {
    pub fn paper(&self) -> Option<Paper> {
        self.grain_id.map(|g| Paper { grain_id: g.to_string(), scale: Some(self.scale), strength: self.strength })
    }
}

/// The preset a paper came from, by grain (`paperPresetId`); legacy `rough`
/// maps to Rough, anything else to Smooth.
pub fn paper_preset_id(p: Option<&Paper>) -> &'static str {
    let Some(p) = p else { return "smooth" };
    if p.grain_id == "rough" {
        return "rough";
    }
    PAPER_PRESETS.iter().find(|q| q.grain_id == Some(p.grain_id.as_str())).map_or("smooth", |q| q.id)
}

impl crate::Document {
    /// The document's paper, if any (`doc.paper`).
    pub fn paper(&self) -> Option<Paper> {
        serde_json::from_value(self.extra.get("paper")?.clone()).ok()
    }

    pub fn set_paper(&mut self, p: Option<Paper>) {
        match p.and_then(|p| serde_json::to_value(p).ok()) {
            Some(v) => {
                self.extra.insert("paper".into(), v);
            }
            None => {
                self.extra.remove("paper");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_the_doc_key() {
        assert_eq!(paper_preset_id(None), "smooth");
        let canvas = PAPER_PRESETS[3].paper().unwrap();
        assert_eq!((canvas.grain_id.as_str(), canvas.scale(), canvas.strength), ("canvas", 1.3, 0.75));
        assert_eq!(paper_preset_id(Some(&canvas)), "canvas");
        let legacy = Paper { grain_id: "rough".into(), scale: None, strength: 0.8 };
        assert_eq!((paper_preset_id(Some(&legacy)), legacy.scale()), ("rough", 1.0));
        let mut d = crate::Document::new(10, 10, None);
        assert!(d.paper().is_none());
        d.set_paper(Some(canvas.clone()));
        assert_eq!(d.extra.get("paper"), Some(&serde_json::json!({"grainId": "canvas", "scale": 1.3, "strength": 0.75})));
        assert_eq!(d.paper(), Some(canvas));
        d.set_paper(None);
        assert!(d.extra.get("paper").is_none());
    }
}
