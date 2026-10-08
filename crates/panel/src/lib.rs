//! Comic panels (happypaint docs/manga-spec.md P6, spec 30).
//!
//! A panel is a layer group carrying a [`PanelSpec`] in its `extra["panel"]`.
//! The group's composite is clipped to the frame inset by half the gutter, and
//! the frame's border ink lives in one path layer above the panels. Only the
//! recipe is stored; the mask is derived ([`coverage`]).

pub mod coverage;
pub mod geometry;

use geometry::Pt;
use onepaint_doc::Group;
use serde::{Deserialize, Serialize};

/// Every panel field a group carries, in serialization order (panel-meta.ts:50).
pub const PANEL_META_FIELDS: [&str; 1] = ["panel"];

/// Default gutter, border width and page margin in doc px (paint-engine.ts:6621-6628).
pub const DEFAULT_GUTTER: f64 = 24.0;
pub const DEFAULT_BORDER: f64 = 4.0;
pub const DEFAULT_MARGIN: f64 = 40.0;
pub const DEFAULT_BORDER_COLOR: &str = "#000000";

/// The name of the top-level path layer holding every panel's border.
pub const BORDER_LAYER: &str = "Panel borders";

/// The brush borders are stroked with: uniform, no taper (manga-spec.md P1/P6).
pub const BORDER_BRUSH: &str = "manga-millipen";

/// happypaint `PanelSpec` (panel-meta.ts:29-42).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PanelSpec {
    /// The frame, a closed polygon in document px.
    pub outline: Vec<Pt>,
    /// Gap between neighbouring frames; each gives up half of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gutter: Option<f64>,
    /// Border stroke width; absent or 0 draws no border.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border: Option<f64>,
    /// Border colour as hex; absent is black.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_color: Option<String>,
    /// Keys onepaint does not model, kept for the round trip.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl PanelSpec {
    pub fn new(outline: Vec<Pt>, gutter: f64, border: f64) -> Self {
        Self { outline, gutter: Some(gutter), border: Some(border), border_color: Some(DEFAULT_BORDER_COLOR.into()), ..Default::default() }
    }

    pub fn gutter(&self) -> f64 {
        self.gutter.unwrap_or(0.0).max(0.0)
    }

    pub fn border(&self) -> f64 {
        self.border.unwrap_or(0.0).max(0.0)
    }

    pub fn border_color(&self) -> &str {
        self.border_color.as_deref().unwrap_or(DEFAULT_BORDER_COLOR)
    }
}

/// The panel a group carries, if any (`panelMetaOf`, panel-meta.ts:57-61).
pub fn panel_of(g: &Group) -> Option<PanelSpec> {
    g.extra.get("panel").and_then(|v| serde_json::from_value(v.clone()).ok())
}

/// Set or clear a group's panel. Absent stays absent: no key is written for `None`.
pub fn set_panel(g: &mut Group, spec: Option<PanelSpec>) {
    match spec.and_then(|s| serde_json::to_value(s).ok()) {
        Some(v) => {
            g.extra.insert("panel".into(), v);
        }
        None => {
            g.extra.remove("panel");
        }
    }
}

/// `#rrggbb` to straight RGB 0..1; anything unreadable is black.
pub fn hex_rgb(hex: &str) -> [f32; 3] {
    let h = hex.trim_start_matches('#');
    let ch = |i: usize| h.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok()).map_or(0.0, |v| v as f32 / 255.0);
    if h.len() == 6 { [ch(0), ch(2), ch(4)] } else { [0.0; 3] }
}
