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

/// A paper choice in the UI (happypaint `PaperPreset`): id, label, hint,
/// the paper (`None` = smooth), the sheet colour a new document's
/// Background takes, and whether it is a classic paper kept for old
/// documents only.
pub struct PaperPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub grain_id: Option<&'static str>,
    pub strength: f64,
    pub scale: f64,
    pub tint: Option<&'static str>,
    pub legacy: bool,
}

const fn physical(
    id: &'static str,
    label: &'static str,
    hint: &'static str,
    grain: &'static str,
    strength: f64,
    tint: &'static str,
) -> PaperPreset {
    PaperPreset { id, label, hint, grain_id: Some(grain), strength, scale: 1.0, tint: Some(tint), legacy: false }
}

const fn classic(id: &'static str, label: &'static str, hint: &'static str, grain: &'static str, strength: f64, scale: f64) -> PaperPreset {
    PaperPreset { id, label, hint, grain_id: Some(grain), strength, scale, tint: None, legacy: true }
}

/// The papers (`utils/paper-presets.ts`; spec 57): the procedural ones are
/// sized in millimetres, so their scale starts at 1.
pub const PAPER_PRESETS: [PaperPreset; 15] = [
    PaperPreset {
        id: "smooth",
        label: "Smooth",
        hint: "No tooth at all: a perfectly flat surface (the default)",
        grain_id: None,
        strength: 0.0,
        scale: 1.0,
        tint: None,
        legacy: false,
    },
    physical("cold", "Cold Press", "Medium tooth, natural white: the all-round watercolour sheet", "paper-cold", 0.8, "#f6f3ec"),
    physical("hot", "Hot Press", "Pressed smooth and bright: crisp edges, little granulation", "paper-hot", 0.8, "#f8f7f3"),
    physical("rough", "Rough", "Deep, knobbly tooth: dry brush breaks up, pigment pools in the hollows", "paper-rough", 0.85, "#f5f1e8"),
    physical("khadi", "Khadi", "Handmade cotton rag: lumpy and cream, with raised fibres", "paper-khadi", 0.85, "#efe7d6"),
    physical("washi", "Washi", "Japanese kozo: smooth and cloudy, with long silky fibres", "paper-washi", 0.8, "#f1ece0"),
    physical("laid", "Laid", "Ingres-style laid lines, with chain lines every inch", "paper-laid", 0.8, "#f2ebdb"),
    physical("canvas", "Canvas", "Primed cotton canvas board: a fine plain weave", "paper-canvas", 0.3, "#f3efe6"),
    physical("toned", "Toned Sand", "Mi-Teintes-style honeycomb tooth on a warm tan sheet", "paper-toned", 0.55, "#d9c8a6"),
    physical("kraft", "Kraft", "Recycled brown kraft: smooth and streaky", "paper-kraft", 0.8, "#b99872"),
    physical("student", "Student", "Wood-pulp student paper: a regular machine-pressed texture", "paper-student", 0.45, "#f7f7f4"),
    PaperPreset {
        id: "linen",
        label: "Linen",
        hint: "Diagonal weave — finer than canvas",
        grain_id: Some("linen"),
        strength: 0.7,
        scale: 2.6,
        tint: None,
        legacy: false,
    },
    // Classic papers: no longer offered, kept so older documents load as they were.
    classic("rough-classic", "Rough (classic)", "The pre-0.197 cold-press tooth", "rough-tooth", 0.85, 1.0),
    classic("fine-classic", "Fine grain (classic)", "Light tooth — subtle, tight texture in washes", "paper", 0.6, 0.8),
    classic("canvas-classic", "Canvas (classic)", "The pre-0.197 woven cloth texture", "canvas", 0.75, 1.3),
];

impl PaperPreset {
    pub fn paper(&self) -> Option<Paper> {
        self.grain_id.map(|g| Paper { grain_id: g.to_string(), scale: Some(self.scale), strength: self.strength })
    }
}

/// The picker's choices: every current paper, plus a classic one only while
/// the document is on it (`pickablePapers`).
pub fn pickable_papers(current: &str) -> Vec<&'static PaperPreset> {
    PAPER_PRESETS.iter().filter(|p| !p.legacy || p.id == current).collect()
}

/// The preset a paper came from, by grain (`paperPresetId`); the oldest
/// documents' `rough` grain is Rough (classic), anything else Smooth.
pub fn paper_preset_id(p: Option<&Paper>) -> &'static str {
    let Some(p) = p else { return "smooth" };
    if let Some(q) = PAPER_PRESETS.iter().find(|q| q.grain_id == Some(p.grain_id.as_str())) {
        return q.id;
    }
    if p.grain_id == "rough" { "rough-classic" } else { "smooth" }
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

    // happypaint paper-presets.test.ts.
    #[test]
    fn presets_and_the_doc_key() {
        let ids: Vec<_> = pickable_papers("smooth").iter().map(|p| p.id).collect();
        assert_eq!(ids, ["smooth", "cold", "hot", "rough", "khadi", "washi", "laid", "canvas", "toned", "kraft", "student", "linen"]);
        let on: Vec<_> = pickable_papers("rough-classic").iter().map(|p| p.id).collect();
        assert!(on.contains(&"rough-classic") && !on.contains(&"fine-classic"));
        let id = |g: &str| paper_preset_id(Some(&Paper { grain_id: g.into(), scale: None, strength: 0.5 }));
        assert_eq!(
            [id("rough-tooth"), id("paper"), id("canvas"), id("rough"), id("linen"), id("paper-khadi")],
            ["rough-classic", "fine-classic", "canvas-classic", "rough-classic", "linen", "khadi"]
        );
        assert_eq!(paper_preset_id(None), "smooth");
        let canvas = PAPER_PRESETS.iter().find(|p| p.id == "canvas-classic").and_then(PaperPreset::paper).unwrap();
        assert_eq!((canvas.grain_id.as_str(), canvas.scale(), canvas.strength), ("canvas", 1.3, 0.75));
        let mut d = crate::Document::new(10, 10, None);
        assert!(d.paper().is_none());
        d.set_paper(Some(canvas.clone()));
        assert_eq!(d.extra.get("paper"), Some(&serde_json::json!({"grainId": "canvas", "scale": 1.3, "strength": 0.75})));
        assert_eq!(d.paper(), Some(canvas));
        d.set_paper(None);
        assert!(d.extra.get("paper").is_none());
    }
}
