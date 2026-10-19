//! The Gradient tool's stops, presets and saved state, and their meeting
//! with Gradient fill layers (happypaint `utils/gradient.ts` presets,
//! `state/gradient-store.ts`, `utils/gradient-fill.ts`; spec 59). Pure.
//!
//! A tool stop takes its colour from the foreground or background swatch,
//! or a fixed colour. A fill layer stores its stops resolved (Photoshop's
//! fill layers keep actual colours), so the swatches never repaint one.

use crate::FillStop;
use crate::gradient::{GradStop, Shape};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StopSource {
    Fg,
    Bg,
    Fixed,
}

/// An editable stop: where, which colour (straight sRGB 0..1 when fixed),
/// opacity, and the midpoint of the segment to the next stop.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorStop {
    pub pos: f64,
    pub source: StopSource,
    pub color: [f64; 3],
    pub opacity: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid: Option<f64>,
}

impl ColorStop {
    pub fn new(pos: f64, source: StopSource, color: [f64; 3], opacity: f64) -> Self {
        ColorStop { pos, source, color, opacity, mid: None }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GradientPreset {
    pub id: String,
    pub name: String,
    pub stops: Vec<ColorStop>,
}

/// Photoshop's two stock presets; not deletable.
pub fn builtin_presets() -> Vec<GradientPreset> {
    vec![
        GradientPreset {
            id: "fg-transparent".into(),
            name: "Foreground to Transparent".into(),
            stops: vec![ColorStop::new(0.0, StopSource::Fg, [0.0; 3], 1.0), ColorStop::new(1.0, StopSource::Fg, [0.0; 3], 0.0)],
        },
        GradientPreset {
            id: "fg-bg".into(),
            name: "Foreground to Background".into(),
            stops: vec![ColorStop::new(0.0, StopSource::Fg, [0.0; 3], 1.0), ColorStop::new(1.0, StopSource::Bg, [1.0; 3], 1.0)],
        },
    ]
}

impl Shape {
    pub fn id(self) -> &'static str {
        match self {
            Shape::Linear => "linear",
            Shape::Radial => "radial",
            Shape::Angle => "angle",
            Shape::Reflected => "reflected",
            Shape::Diamond => "diamond",
        }
    }

    pub fn parse(s: &str) -> Option<Shape> {
        Shape::ALL.into_iter().find(|k| k.id() == s)
    }
}

const MID_MIN: f64 = 0.05;
const MID_MAX: f64 = 0.95;

/// Each stop's colour looked up, clamped and sorted by position (`resolveStops`).
pub fn resolve_stops(stops: &[ColorStop], fg: [f64; 3], bg: [f64; 3]) -> Vec<GradStop> {
    let mut out: Vec<GradStop> = stops
        .iter()
        .map(|s| GradStop {
            pos: s.pos.clamp(0.0, 1.0),
            color: match s.source {
                StopSource::Fg => fg,
                StopSource::Bg => bg,
                StopSource::Fixed => s.color,
            },
            opacity: s.opacity.clamp(0.0, 1.0),
            mid: s.mid.map(|m| m.clamp(MID_MIN, MID_MAX)),
        })
        .collect();
    out.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    out
}

/// Stops from stored JSON: at least two, every field well formed; a junk
/// midpoint just reads as none (`sanitizeStops`).
pub fn sanitize_stops(raw: &serde_json::Value) -> Option<Vec<ColorStop>> {
    let list = raw.as_array().filter(|l| l.len() >= 2)?;
    list.iter()
        .map(|s| {
            let num = |k: &str| s.get(k).and_then(serde_json::Value::as_f64).filter(|v| v.is_finite());
            let source = serde_json::from_value::<StopSource>(s.get("source")?.clone()).ok()?;
            let c = s.get("color")?.as_array().filter(|c| c.len() == 3)?;
            let color = [c[0].as_f64()?, c[1].as_f64()?, c[2].as_f64()?];
            Some(ColorStop { pos: num("pos")?, source, color, opacity: num("opacity")?, mid: num("mid") })
        })
        .collect()
}

/// The tool's saved state: shape, stops, the user's presets and which
/// preset the stops came from (`GradientState`).
#[derive(Clone, Debug, PartialEq)]
pub struct GradientState {
    pub shape: Shape,
    pub stops: Vec<ColorStop>,
    pub presets: Vec<GradientPreset>,
    pub active_preset_id: Option<String>,
}

impl Default for GradientState {
    /// First run: linear, Foreground to Transparent.
    fn default() -> Self {
        let first = builtin_presets().remove(0);
        GradientState { shape: Shape::Linear, stops: first.stops, presets: Vec::new(), active_preset_id: Some(first.id) }
    }
}

impl GradientState {
    /// Read back field by field: a corrupt or old entry never loses the
    /// presets that are still well formed (`parseGradientState`).
    pub fn parse(raw: Option<&str>) -> Self {
        let base = Self::default();
        let Some(v) = raw.and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok()).filter(|v| v.is_object()) else {
            return base;
        };
        let shape = v.get("shape").and_then(|s| s.as_str()).and_then(Shape::parse).unwrap_or(base.shape);
        let stops = v.get("stops").and_then(sanitize_stops).unwrap_or(base.stops);
        let presets: Vec<GradientPreset> = v
            .get("presets")
            .and_then(|p| p.as_array())
            .into_iter()
            .flatten()
            .filter_map(|p| {
                Some(GradientPreset {
                    id: p.get("id")?.as_str()?.into(),
                    name: p.get("name")?.as_str()?.into(),
                    stops: sanitize_stops(p.get("stops")?)?,
                })
            })
            .collect();
        let known = |id: &str| builtin_presets().iter().any(|p| p.id == id) || presets.iter().any(|p| p.id == id);
        let active_preset_id = v.get("activePresetId").and_then(|a| a.as_str()).filter(|a| known(a)).map(String::from);
        GradientState { shape, stops, presets, active_preset_id }
    }

    pub fn to_json(&self) -> String {
        serde_json::json!({
            "shape": self.shape.id(),
            "stops": self.stops,
            "presets": self.presets,
            "activePresetId": self.active_preset_id,
        })
        .to_string()
    }

    /// Every preset to show: the built-ins, then the user's.
    pub fn all_presets(&self) -> Vec<GradientPreset> {
        builtin_presets().into_iter().chain(self.presets.iter().cloned()).collect()
    }
}

/// Where a ramp goes when the drag gave it no length (a click): top to
/// bottom across the canvas, or for the centred shapes from the middle to
/// the edge; doc fractions (ax, ay, bx, by) (`defaultEnds`).
pub fn default_ends(shape: Shape) -> [f32; 4] {
    match shape {
        Shape::Radial | Shape::Angle | Shape::Diamond => [0.5, 0.5, 0.5, 0.0],
        _ => [0.5, 0.0, 0.5, 1.0],
    }
}

/// Under this many doc px a drag is a click.
pub const MIN_RAMP_PX: f64 = 4.0;

/// A fill layer's stops as editable fixed stops (`fillStopsToEditable`).
pub fn fill_stops_to_editable(stops: &[FillStop]) -> Vec<ColorStop> {
    stops
        .iter()
        .map(|s| ColorStop {
            pos: f64::from(s.pos),
            source: StopSource::Fixed,
            color: s.color.map(f64::from),
            opacity: f64::from(s.opacity),
            mid: s.mid.map(f64::from),
        })
        .collect()
}

/// Edited stops back into a fill layer: Fg and Bg are pinned to the swatch
/// colours now (a fill layer never follows the swatches).
pub fn editable_to_fill_stops(stops: &[ColorStop], fg: [f64; 3], bg: [f64; 3]) -> Vec<FillStop> {
    resolve_stops(stops, fg, bg)
        .iter()
        .map(|s| FillStop { pos: s.pos as f32, color: s.color.map(|c| c as f32), opacity: s.opacity as f32, mid: s.mid.map(|m| m as f32) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(pos: f64, source: &str) -> serde_json::Value {
        serde_json::json!({"pos": pos, "source": source, "color": [0, 0, 0], "opacity": 1})
    }

    // happypaint gradient-store.test.ts.
    #[test]
    fn the_saved_state_degrades_field_by_field() {
        let s = GradientState::parse(None);
        assert_eq!((s.shape, s.active_preset_id.as_deref()), (Shape::Linear, Some("fg-transparent")));
        assert_eq!(s.stops.iter().map(|x| x.opacity).collect::<Vec<_>>(), [1.0, 0.0]);
        assert!(s.presets.is_empty());
        assert_eq!(GradientState::parse(Some("{nope")), GradientState::default());
        let saved = serde_json::json!({
            "shape": "diamond",
            "stops": [stop(0.0, "fg"), stop(1.0, "bg")],
            "presets": [{"id": "g1", "name": "Sunset", "stops": [stop(0.0, "fixed"), stop(1.0, "fixed")]}],
            "activePresetId": "g1",
        });
        let s = GradientState::parse(Some(&saved.to_string()));
        assert_eq!((s.shape, s.presets[0].name.as_str(), s.active_preset_id.as_deref()), (Shape::Diamond, "Sunset", Some("g1")));
        assert_eq!(GradientState::parse(Some(&s.to_json())), s);
        let raw = serde_json::json!({"presets": [
            {"id": "bad", "name": "Bad", "stops": [stop(0.0, "fg")]},
            {"id": "ok", "name": "Ok", "stops": [stop(0.0, "fg"), stop(1.0, "fg")]},
        ]});
        assert_eq!(GradientState::parse(Some(&raw.to_string())).presets.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["ok"]);
        let odd = serde_json::json!({"shape": "spiral", "activePresetId": "gone", "stops": [stop(0.0, "fg"), stop(1.0, "fg")]});
        let s = GradientState::parse(Some(&odd.to_string()));
        assert_eq!((s.shape, s.active_preset_id, s.stops.len()), (Shape::Linear, None, 2));
    }

    // happypaint gradient-fill.test.ts.
    #[test]
    fn fill_layers_freeze_the_swatches() {
        let fg_bg = builtin_presets().remove(1).stops;
        let fill = editable_to_fill_stops(&fg_bg, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        assert_eq!(fill.iter().map(|s| s.color).collect::<Vec<_>>(), [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
        assert_eq!(default_ends(Shape::Linear), [0.5, 0.0, 0.5, 1.0]);
        assert_eq!(default_ends(Shape::Reflected), default_ends(Shape::Linear));
        for s in [Shape::Radial, Shape::Angle, Shape::Diamond] {
            assert_eq!(default_ends(s), [0.5, 0.5, 0.5, 0.0]);
        }
        let resolved = vec![
            FillStop { pos: 0.0, color: [0.25, 0.5, 0.75], opacity: 1.0, mid: Some(0.25) },
            FillStop { pos: 1.0, color: [1.0; 3], opacity: 0.5, mid: None },
        ];
        let editable = fill_stops_to_editable(&resolved);
        assert!(editable.iter().all(|s| s.source == StopSource::Fixed));
        assert_eq!(editable_to_fill_stops(&editable, [0.0; 3], [1.0; 3]), resolved);
        let pinned = [ColorStop::new(0.0, StopSource::Fg, [0.0; 3], 1.0), ColorStop::new(1.0, StopSource::Fixed, [1.0; 3], 1.0)];
        assert_eq!(editable_to_fill_stops(&pinned, [0.0, 1.0, 0.0], [1.0; 3])[0].color, [0.0, 1.0, 0.0]);
        assert!(fill_stops_to_editable(&[]).is_empty());
    }
}
