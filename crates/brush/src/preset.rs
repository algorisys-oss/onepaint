//! Brush settings (happypaint `engine/brush/brush-presets.ts:11-246`).

use onepaint_stroke::{Binding, Combine};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StabilizerSettings {
    pub enabled: bool,
    pub strength: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Taper {
    pub entry: f64,
    pub exit: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Dynamics {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub size: Vec<Binding>,
    #[serde(default)]
    pub size_combine: Combine,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flow: Vec<Binding>,
    #[serde(default)]
    pub flow_combine: Combine,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hardness: Vec<Binding>,
    #[serde(default)]
    pub hardness_combine: Combine,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrushPreset {
    pub id: String,
    pub name: String,
    pub category: String,
    /// Diameter in document px at full pressure.
    pub size: f64,
    /// Gap as a fraction of the dab diameter.
    pub spacing: f64,
    pub hardness: f64,
    /// Per-dab alpha.
    pub flow: f64,
    /// Stroke-level cap applied when the stroke is flattened.
    pub opacity: f64,
    pub size_pressure: f64,
    pub flow_pressure: f64,
    #[serde(default)]
    pub speed_size: f64,
    #[serde(default)]
    pub speed_flow: f64,
    pub smoothing: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stabilizer: Option<StabilizerSettings>,
    pub wash: bool,
    pub tilt_strength: f64,
    pub pressure_curve: f64,
    pub tip_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_roundness: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_angle_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamics: Option<Dynamics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opaque_linearize: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<Taper>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_response: Option<f64>,
    /// Fields onepaint does not use yet (grain, airbrush, wet, …).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl BrushPreset {
    pub fn is_eraser(&self) -> bool {
        self.category == "Eraser"
    }
}

// Copied verbatim from brush-presets.ts (lines noted per preset).
const PRESETS_JSON: &str = r#"[
 {"id":"hard-round","name":"Hard Round","category":"Sketching","size":24,"spacing":0.1,"hardness":0.95,"flow":1,"opacity":1,
  "sizePressure":0.85,"flowPressure":0,"smoothing":0.25,"wash":false,"tiltStrength":0.4,"pressureCurve":1.0,"tipId":"round"},
 {"id":"soft-round","name":"Soft Round","category":"Paint","size":30,"spacing":0.06,"hardness":0.4,"flow":0.6,"opacity":1,
  "sizePressure":0.5,"flowPressure":0.8,"smoothing":0.25,"wash":false,"tiltStrength":0.2,"pressureCurve":1.0,"tipId":"round"},
 {"id":"soft-airbrush","name":"Soft Airbrush","category":"Airbrush","size":64,"spacing":0.08,"hardness":0.05,"flow":0.35,"opacity":1,
  "sizePressure":0.4,"flowPressure":0.8,"smoothing":0.3,"wash":false,"tiltStrength":0.6,"pressureCurve":0.7,"tipId":"round",
  "airbrush":{"rate":25}},
 {"id":"pencil","name":"Pencil","category":"Sketching","size":8,"spacing":0.15,"hardness":0.7,"flow":0.85,"opacity":1,
  "sizePressure":1,"flowPressure":0.6,"smoothing":0.15,"wash":false,"tiltStrength":1.0,"pressureCurve":1.4,"tipId":"round",
  "grain":{"id":"paper","scale":60,"strength":0.4}},
 {"id":"ink","name":"Ink","category":"Ink","size":9,"spacing":0.1,"hardness":0.98,"flow":1,"opacity":1,
  "sizePressure":1,"flowPressure":0,"smoothing":0.4,"wash":false,"tiltStrength":0.2,"pressureCurve":1.0,"tipId":"round","paperResponse":0},
 {"id":"comic-inker","name":"Comic Inker","category":"Ink","size":12,"spacing":0.05,"hardness":0.97,"flow":1,"opacity":1,
  "sizePressure":0.85,"flowPressure":0,"smoothing":0.4,"wash":false,"tiltStrength":0.2,"pressureCurve":1.1,"tipId":"round",
  "paperResponse":0,"taper":{"entry":16,"exit":26}},
 {"id":"studio-pen","name":"Studio Pen","category":"Ink","size":10,"spacing":0.05,"hardness":0.98,"flow":1,"opacity":1,
  "sizePressure":0.5,"flowPressure":0,"smoothing":0.65,"stabilizer":{"enabled":true,"strength":0.5},"wash":false,
  "tiltStrength":0,"pressureCurve":1.0,"tipId":"round","paperResponse":0,"taper":{"entry":10,"exit":12},
  "dynamics":{"size":[{"sensor":"pressure","curve":[{"x":0,"y":0.5},{"x":1,"y":1}]}]}},
 {"id":"eraser-hard","name":"Hard Eraser","category":"Eraser","size":40,"spacing":0.08,"hardness":0.95,"flow":1,"opacity":1,
  "sizePressure":0.5,"flowPressure":0,"smoothing":0.25,"wash":false,"tiltStrength":0.3,"pressureCurve":1.0,"tipId":"round"},
 {"id":"eraser-soft","name":"Soft Eraser","category":"Eraser","size":60,"spacing":0.06,"hardness":0.12,"flow":1,"opacity":1,
  "sizePressure":0.5,"flowPressure":0,"smoothing":0.3,"wash":false,"tiltStrength":0.3,"pressureCurve":1.0,"tipId":"round"}
]"#;

/// The built-in presets for slice 1.
pub fn presets() -> Vec<BrushPreset> {
    serde_json::from_str(PRESETS_JSON).unwrap_or_default()
}

pub fn preset(id: &str) -> Option<BrushPreset> {
    presets().into_iter().find(|p| p.id == id)
}

pub const DEFAULT_BRUSH: &str = "hard-round";
pub const DEFAULT_ERASER: &str = "eraser-hard";
