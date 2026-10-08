//! Brush settings (happypaint `engine/brush/brush-presets.ts:11-246`) and the
//! full happypaint preset library (`assets/presets.json`, exported verbatim).

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
pub struct Grain {
    pub id: String,
    #[serde(default = "default_grain_scale")]
    pub scale: f64,
    #[serde(default)]
    pub strength: f64,
    #[serde(default, rename = "move", skip_serializing_if = "Option::is_none")]
    pub moves: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub random: Option<f64>,
}

fn default_grain_scale() -> f64 {
    100.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Airbrush {
    pub rate: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct RotationDynamics {
    #[serde(default)]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ColorDynamics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hue: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sat: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub val: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_darken: Option<f64>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ratio: Vec<Binding>,
    #[serde(default)]
    pub ratio_combine: Combine,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spin: Vec<Binding>,
    #[serde(default)]
    pub spin_combine: Combine,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<RotationDynamics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scatter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorDynamics>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacing_jitter: Option<f64>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grain: Option<Grain>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub airbrush: Option<Airbrush>,
    /// Fields onepaint does not use yet (wet media, mask tips, …).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl BrushPreset {
    pub fn is_eraser(&self) -> bool {
        self.category == "Eraser"
    }
}

#[derive(Deserialize)]
struct Library {
    categories: Vec<String>,
    presets: Vec<BrushPreset>,
}

const LIBRARY_JSON: &str = include_str!("../assets/presets.json");

fn library() -> &'static Library {
    static L: std::sync::OnceLock<Library> = std::sync::OnceLock::new();
    L.get_or_init(|| serde_json::from_str(LIBRARY_JSON).unwrap_or(Library { categories: Vec::new(), presets: Vec::new() }))
}

/// Every built-in happypaint preset (124), in happypaint's order.
/// Imported presets are in `user_presets()`.
pub fn presets() -> Vec<BrushPreset> {
    library().presets.clone()
}

/// Category names in display order.
pub fn categories() -> Vec<String> {
    library().categories.clone()
}

impl BrushPreset {
    /// The mode the stroke composites onto its layer with (`blendMode`, default normal).
    pub fn blend_mode(&self) -> onepaint_doc::BlendMode {
        self.extra.get("blendMode").and_then(|v| v.as_str()).map(onepaint_doc::BlendMode::parse).unwrap_or_default()
    }
}

/// Wet media (`wet`, `brush-presets.ts:153-196`): a brush with `wetness > 0`
/// paints through the mixer path, picking up and dragging canvas paint.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct WetMedia {
    pub wetness: f64,
    /// Paint in the reservoir ("Pigment" for body paint).
    pub load: f64,
    /// 0 = all reservoir paint, 1 = all canvas pickup.
    pub mix: f64,
    /// Kubelka–Munk mixing.
    pub pigment: bool,
    /// The brush tip shapes each wet dab (else the classic soft disc).
    pub tip: bool,
    /// Levels relief instead of building it (blender).
    pub blend: f64,
}

impl Default for WetMedia {
    fn default() -> Self {
        Self { wetness: 0.0, load: 1.0, mix: 0.5, pigment: false, tip: false, blend: 0.0 }
    }
}

impl BrushPreset {
    /// The wet settings, when the brush paints wet (`wetness > 0`).
    pub fn wet(&self) -> Option<WetMedia> {
        let w: WetMedia = serde_json::from_value(self.extra.get("wet")?.clone()).ok()?;
        (w.wetness > 0.0).then_some(w)
    }

    /// Pigment ("Load"), which thins opacity and body even on dry brushes (default 1).
    pub fn pigment_load(&self) -> f64 {
        self.extra.get("wet").and_then(|w| w.get("load")).and_then(|v| v.as_f64()).unwrap_or(1.0)
    }

    /// Watercolour settle (`settle`), when it does anything.
    pub fn settle(&self) -> Option<crate::settle::Settle> {
        let s: crate::settle::Settle = serde_json::from_value(self.extra.get("settle")?.clone()).ok()?;
        s.is_active().then_some(s)
    }

    /// Impasto depth (`impasto.depth`, 0 = flat).
    pub fn impasto_depth(&self) -> f64 {
        self.extra.get("impasto").and_then(|w| w.get("depth")).and_then(|v| v.as_f64()).unwrap_or(0.0).max(0.0)
    }

    /// The relief each unit of dab alpha builds (`pigmentBody(depth, load)`).
    pub fn impasto_body(&self) -> f64 {
        crate::height::pigment_body(self.impasto_depth(), self.pigment_load())
    }
}

/// A built-in preset, or an imported one from the user library.
pub fn preset(id: &str) -> Option<BrushPreset> {
    library().presets.iter().find(|p| p.id == id).cloned().or_else(|| crate::user::user_preset(id))
}

pub const DEFAULT_BRUSH: &str = "hard-round";
pub const DEFAULT_ERASER: &str = "eraser-hard";
