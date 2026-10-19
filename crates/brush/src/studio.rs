//! Brush studio support (spec 27): the editable parameters, the preview
//! stroke and try-it rendering.
//!
//! happypaint keeps flat override fields and folds them into a preset in
//! `effectiveBrush()` (`state/brush-store.ts:861-1040`). Here every slider
//! reads and writes the preset directly through [`Param`], building the same
//! bindings `effectiveBrush` builds.

use crate::{BrushPreset, ColorDynamics, Dynamics, Grain, StrokeEngine};
use onepaint_raster::Surface;
use onepaint_stroke::{Binding, CurvePoint, Interp, Sensor, StrokePoint};

/// A slider in the studio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Param {
    Size,
    Opacity,
    Angle,
    Roundness,
    Spacing,
    Smoothing,
    StabilizerAmount,
    EvenBuild,
    Airbrush,
    TaperIn,
    TaperOut,
    Hardness,
    Flow,
    Tilt,
    Pressure,
    Squash,
    Spin,
    SpeedSize,
    SpeedFlow,
    SizePressure,
    FlowPressure,
    Scatter,
    SizeJitter,
    AngleJitter,
    HueJitter,
    SatJitter,
    ValJitter,
    PressureDarken,
    GrainAmount,
    GrainScale,
    /// How hard the document paper's tooth bites (`paperResponse`).
    PaperResponse,
    /// Wet / Oil: how much canvas paint the brush drags.
    Wetness,
    /// Load / Pigment: paint on the brush (opacity and impasto body).
    Load,
    /// Mix: canvas vs brush colour.
    Mix,
    /// Live fluid water (bloom distance and drying time).
    FluidWater,
    /// Live fluid pigment per dab.
    FluidPigment,
    /// How far a live dab's water reaches past its pigment (`fluid.halo`, 1..3).
    FluidHalo,
    /// Random spacing variation (`spacingJitter`).
    SpacingJitter,
    /// Hue drift across the stroke (`color.hueShift`, −1..1 of the wheel).
    HueShift,
    /// How far the grain rolls with the brush (`grain.move`).
    GrainMove,
    /// Dual brush: how much the second tip carves (`maskTip.strength`).
    MaskStrength,
    /// Colour-tip recolour: hue (degrees), saturation and brightness gains.
    TipHue,
    TipSat,
    TipVal,
}

/// How a slider value reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Stored 0..1, shown as a whole percentage.
    Percent,
    Px,
    Degrees,
    /// Dabs per second; 0 reads "Off".
    Rate,
    /// The pressure firmness (Soft / Lin / Firm).
    Firmness,
    /// A multiple, shown as `1.5×`.
    Times,
}

pub struct ParamInfo {
    pub label: &'static str,
    /// Slider range in stored units (firmness for `Pressure`).
    pub min: f64,
    pub max: f64,
    pub unit: Unit,
}

const fn info(label: &'static str, min: f64, max: f64, unit: Unit) -> ParamInfo {
    ParamInfo { label, min, max, unit }
}

/// The sections in order (`toolbar.tsx:584-1383`). Toggles, pickers and
/// curves are handled by the UI around these sliders.
pub const SECTIONS: &[(&str, &[Param])] = {
    use Param::*;
    &[
        ("General", &[Size, Opacity, Angle, Roundness]),
        ("Stroke", &[Spacing, Smoothing, StabilizerAmount, EvenBuild, Airbrush, TaperIn, TaperOut]),
        ("Shape", &[Hardness, Flow, Tilt, Pressure, Squash, Spin, SpeedSize, SpeedFlow]),
        ("Pressure", &[SizePressure, FlowPressure]),
        ("Jitter", &[Scatter, SizeJitter, AngleJitter, SpacingJitter, HueJitter, SatJitter, ValJitter, PressureDarken, HueShift]),
        ("Grain", &[GrainAmount, GrainScale, GrainMove, PaperResponse]),
        ("Dual brush", &[MaskStrength]),
        ("Colour tip", &[TipHue, TipSat, TipVal]),
        ("Wet", &[Wetness, Load, Mix]),
        ("Fluid", &[FluidWater, FluidPigment, FluidHalo]),
    ]
};

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

/// happypaint's pressure slider maps an exponent to "firmness" (`toolbar.tsx:379-381`).
pub fn curve_to_firm(c: f64) -> f64 {
    (50.0 + 50.0 * c.max(1e-6).ln() / 3f64.ln()).round()
}

pub fn firm_to_curve(f: f64) -> f64 {
    3f64.powf((f - 50.0) / 50.0).clamp(1.0 / 3.0, 3.0)
}

pub fn pressure_label(c: f64) -> &'static str {
    if c < 0.92 {
        "Soft"
    } else if c > 1.08 {
        "Firm"
    } else {
        "Lin"
    }
}

fn dynamics(b: &mut BrushPreset) -> &mut Dynamics {
    b.dynamics.get_or_insert_with(Dynamics::default)
}

fn color_dyn(b: &mut BrushPreset) -> &mut ColorDynamics {
    dynamics(b).color.get_or_insert_with(ColorDynamics::default)
}

fn color_get(b: &BrushPreset, f: impl Fn(&ColorDynamics) -> Option<f64>) -> f64 {
    b.dynamics.as_ref().and_then(|d| d.color.as_ref()).and_then(f).unwrap_or(0.0)
}

/// Colour dynamics vanish when every jitter is zero (`brush-store.ts:883-900`).
fn tidy_color(b: &mut BrushPreset) {
    if let Some(d) = b.dynamics.as_mut()
        && let Some(c) = &d.color
        && [c.hue, c.sat, c.val, c.pressure_darken, c.hue_shift].iter().all(|v| v.unwrap_or(0.0) == 0.0)
        && c.to_color.is_none()
        && c.extra.is_empty()
    {
        d.color = None;
    }
}

fn speed_binding(y0: f64, y1: f64) -> Binding {
    Binding { sensor: Sensor::Speed, curve: vec![CurvePoint::new(0.0, y0), CurvePoint::new(1.0, y1)], ..Default::default() }
}

/// The slider value of a synthesised speed binding, else 0.
fn speed_amount(bs: &[Binding], y0: f64, from_y1: impl Fn(f64) -> f64) -> f64 {
    match bs {
        [b] if b.sensor == Sensor::Speed && b.curve.len() == 2 && b.curve[0] == CurvePoint::new(0.0, y0) && b.curve[1].x == 1.0 => {
            from_y1(b.curve[1].y)
        }
        _ => 0.0,
    }
}

fn is_size_jitter(b: &Binding) -> bool {
    b.sensor == Sensor::Random && b.max.unwrap_or(1.0) == 1.0 && b.curve == [CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)]
}

impl Param {
    pub fn info(self) -> ParamInfo {
        use Param::*;
        use Unit::*;
        match self {
            Size => info("Size", 1.0, 1000.0, Px),
            Opacity => info("Opacity", 0.01, 1.0, Percent),
            Angle => info("Angle", 0.0, 360.0, Degrees),
            Roundness => info("Roundness", 0.05, 1.0, Percent),
            Spacing => info("Spacing", 0.02, 1.0, Percent),
            Smoothing => info("Smooth", 0.0, 1.0, Percent),
            StabilizerAmount => info("Amount", 0.0, 1.0, Percent),
            EvenBuild => info("Even build", 0.0, 1.0, Percent),
            Airbrush => info("Airbrush", 0.0, 60.0, Rate),
            TaperIn => info("Taper in", 0.0, 200.0, Px),
            TaperOut => info("Taper out", 0.0, 200.0, Px),
            Hardness => info("Hardness", 0.0, 1.0, Percent),
            Flow => info("Flow", 0.01, 1.0, Percent),
            Tilt => info("Tilt", 0.0, 1.0, Percent),
            Pressure => info("Pressure", 0.0, 100.0, Firmness),
            Squash => info("Squash", 0.0, 1.0, Percent),
            Spin => info("Spin", 0.0, 1.0, Percent),
            SpeedSize => info("Speed→Size", 0.0, 1.0, Percent),
            SpeedFlow => info("Speed→Opacity", 0.0, 1.0, Percent),
            SizePressure => info("Size·P", 0.0, 1.0, Percent),
            FlowPressure => info("Flow·P", 0.0, 1.0, Percent),
            Scatter => info("Scatter", 0.0, 2.0, Percent),
            SizeJitter => info("Size·J", 0.0, 1.0, Percent),
            AngleJitter => info("Angle·J", 0.0, 1.0, Percent),
            HueJitter => info("Hue·J", 0.0, 1.0, Percent),
            SatJitter => info("Sat·J", 0.0, 1.0, Percent),
            ValJitter => info("Val·J", 0.0, 1.0, Percent),
            PressureDarken => info("Press→Dark", 0.0, 1.0, Percent),
            GrainAmount => info("Amount", 0.0, 1.0, Percent),
            GrainScale => info("Scale", 16.0, 400.0, Px),
            PaperResponse => info("Paper", 0.0, 1.0, Percent),
            Wetness => info("Wet", 0.0, 1.0, Percent),
            Load => info("Load", 0.0, 1.0, Percent),
            Mix => info("Mix", 0.0, 1.0, Percent),
            FluidWater => info("Water", 0.05, 2.0, Percent),
            FluidPigment => info("Pigment", 0.0, 1.0, Percent),
            FluidHalo => info("Halo", 1.0, 3.0, Times),
            SpacingJitter => info("Space·J", 0.0, 1.0, Percent),
            HueShift => info("Hue→Shift", -1.0, 1.0, Percent),
            GrainMove => info("Movement", 0.0, 1.0, Percent),
            MaskStrength => info("Strength", 0.0, 1.0, Percent),
            TipHue => info("Tip hue", -180.0, 180.0, Degrees),
            TipSat => info("Tip sat", 0.0, 2.0, Percent),
            TipVal => info("Tip bright", 0.0, 2.0, Percent),
        }
    }

    /// The label for this brush: oil and body paint read Oil / Pigment.
    pub fn label_for(self, b: &BrushPreset) -> &'static str {
        match self {
            Param::Wetness if oil_paint(b) => "Oil",
            Param::Load if oil_paint(b) => "Pigment",
            _ => self.info().label,
        }
    }

    /// The value as displayed next to the slider.
    pub fn format(self, v: f64) -> String {
        match self.info().unit {
            Unit::Percent => format!("{:.0}%", v * 100.0),
            Unit::Px => format!("{v:.0} px"),
            Unit::Degrees => format!("{v:.0}°"),
            Unit::Rate if v < 0.5 => "Off".into(),
            Unit::Rate => format!("{v:.0}/s"),
            Unit::Firmness => pressure_label(firm_to_curve(v)).into(),
            Unit::Times => format!("{v:.1}×"),
        }
    }

    /// Whether the slider applies to this brush (e.g. Amount needs the stabilizer on).
    pub fn visible(self, b: &BrushPreset) -> bool {
        match self {
            Param::StabilizerAmount => b.stabilizer.as_ref().is_some_and(|s| s.enabled),
            Param::GrainAmount | Param::GrainScale => b.grain.is_some(),
            // Pigment belongs to the paint, so body paint shows it even dry;
            // Mix is derived while blending and moot until the brush is wet.
            Param::Load => wet_num(b, "wetness", 0.0) > 0.0 || b.impasto_depth() > 0.0,
            Param::Mix => !is_blender(b) && wet_num(b, "wetness", 0.0) > 0.0,
            Param::FluidWater | Param::FluidPigment | Param::FluidHalo => b.fluid().is_some_and(|f| f.water > 0.0 || f.blow > 0.0),
            Param::GrainMove => b.grain.is_some(),
            Param::MaskStrength => b.extra.contains_key("maskTip"),
            Param::TipHue | Param::TipSat | Param::TipVal => crate::color_tip_for(&b.tip_id).is_some(),
            _ => true,
        }
    }

    pub fn get(self, b: &BrushPreset) -> f64 {
        use Param::*;
        let d = b.dynamics.as_ref();
        match self {
            Size => b.size,
            Opacity => b.opacity,
            Angle => b.tip_angle_deg.unwrap_or(0.0),
            Roundness => b.tip_roundness.unwrap_or(1.0),
            Spacing => b.spacing,
            Smoothing => b.smoothing,
            StabilizerAmount => b.stabilizer.as_ref().map_or(0.5, |s| s.strength),
            EvenBuild => b.opaque_linearize.unwrap_or(0.9),
            Airbrush => b.airbrush.as_ref().map_or(0.0, |a| a.rate),
            TaperIn => b.taper.as_ref().map_or(0.0, |t| t.entry),
            TaperOut => b.taper.as_ref().map_or(0.0, |t| t.exit),
            Hardness => b.hardness,
            Flow => b.flow,
            Tilt => b.tilt_strength,
            Pressure => curve_to_firm(b.pressure_curve),
            Squash => d.map_or(0.0, |d| speed_amount(&d.ratio, 1.0, |y| 1.0 - y)),
            Spin => d.map_or(0.0, |d| speed_amount(&d.spin, 0.0, |y| y)),
            SpeedSize => b.speed_size,
            SpeedFlow => b.speed_flow,
            SizePressure => b.size_pressure,
            FlowPressure => b.flow_pressure,
            Scatter => d.and_then(|d| d.scatter).unwrap_or(0.0),
            SizeJitter => d.and_then(|d| d.size.iter().find(|b| is_size_jitter(b))).map_or(0.0, |b| 1.0 - b.min.unwrap_or(0.0)),
            AngleJitter => d.and_then(|d| d.rotation.as_ref()).and_then(|r| r.jitter).unwrap_or(0.0),
            HueJitter => color_get(b, |c| c.hue),
            SatJitter => color_get(b, |c| c.sat),
            ValJitter => color_get(b, |c| c.val),
            PressureDarken => color_get(b, |c| c.pressure_darken),
            GrainAmount => b.grain.as_ref().map_or(0.0, |g| g.strength),
            GrainScale => b.grain.as_ref().map_or(160.0, |g| g.scale),
            PaperResponse => b.paper_response.unwrap_or(1.0),
            Wetness => wet_num(b, "wetness", 0.0),
            Load => wet_num(b, "load", 1.0),
            Mix => wet_num(b, "mix", 0.5),
            FluidWater => b.fluid().map_or(0.0, |f| f.water),
            FluidPigment => b.fluid().map_or(0.0, |f| f.pigment),
            FluidHalo => b.fluid().map_or(1.0, |f| f.halo()),
            SpacingJitter => b.spacing_jitter.unwrap_or(0.0),
            HueShift => color_get(b, |c| c.hue_shift),
            GrainMove => b.grain.as_ref().and_then(|g| g.moves).unwrap_or(0.0),
            MaskStrength => b.extra.get("maskTip").and_then(|m| m.get("strength")).and_then(|v| v.as_f64()).unwrap_or(0.7),
            TipHue => recolor_get(b, "hue", 0.0) * 360.0,
            TipSat => recolor_get(b, "sat", 1.0),
            TipVal => recolor_get(b, "val", 1.0),
        }
    }

    /// Write `v`, clamped like happypaint's setters. NaN is ignored.
    pub fn set(self, b: &mut BrushPreset, v: f64) {
        use Param::*;
        if !v.is_finite() {
            return;
        }
        match self {
            Size => b.size = v.clamp(1.0, 10000.0).round(),
            Opacity => b.opacity = v.clamp(0.01, 1.0),
            Angle => {
                let a = v.rem_euclid(360.0);
                b.tip_angle_deg = (a != 0.0).then_some(a);
            }
            Roundness => {
                let r = v.clamp(0.05, 1.0);
                b.tip_roundness = (r < 1.0).then_some(r);
            }
            Spacing => b.spacing = v.clamp(0.02, 2.0),
            Smoothing => b.smoothing = clamp01(v),
            // Amount > 0 turns the stabilizer on; 0 turns it off and keeps the strength.
            StabilizerAmount => {
                let s = b.stabilizer.get_or_insert(crate::StabilizerSettings { enabled: false, strength: 0.5 });
                if v > 0.0 {
                    s.enabled = true;
                    s.strength = v.min(1.0);
                } else {
                    s.enabled = false;
                }
            }
            EvenBuild => b.opaque_linearize = Some(clamp01(v)),
            Airbrush => {
                let r = v.clamp(0.0, 60.0).round();
                b.airbrush = (r > 0.0).then_some(crate::Airbrush { rate: r });
            }
            TaperIn | TaperOut => {
                let mut t = b.taper.clone().unwrap_or_default();
                let v = v.clamp(0.0, 300.0).round();
                if self == TaperIn {
                    t.entry = v
                } else {
                    t.exit = v
                }
                b.taper = (t.entry > 0.0 || t.exit > 0.0).then_some(t);
            }
            Hardness => b.hardness = clamp01(v),
            Flow => b.flow = v.clamp(0.01, 1.0),
            Tilt => b.tilt_strength = clamp01(v),
            Pressure => b.pressure_curve = firm_to_curve(v.clamp(0.0, 100.0)),
            Squash => {
                let v = clamp01(v);
                dynamics(b).ratio = if v > 0.0 { vec![speed_binding(1.0, 1.0 - v)] } else { Vec::new() };
            }
            Spin => {
                let v = clamp01(v);
                dynamics(b).spin = if v > 0.0 { vec![speed_binding(0.0, v)] } else { Vec::new() };
            }
            SpeedSize => b.speed_size = clamp01(v),
            SpeedFlow => b.speed_flow = clamp01(v),
            SizePressure => b.size_pressure = clamp01(v),
            FlowPressure => b.flow_pressure = clamp01(v),
            Scatter => {
                let v = v.clamp(0.0, 2.0);
                dynamics(b).scatter = (v > 0.0).then_some(v);
            }
            SizeJitter => {
                let v = clamp01(v);
                let size = &mut dynamics(b).size;
                size.retain(|b| !is_size_jitter(b));
                if v > 0.0 {
                    size.push(Binding {
                        sensor: Sensor::Random,
                        curve: vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)],
                        min: Some(1.0 - v),
                        max: Some(1.0),
                        ..Default::default()
                    });
                }
            }
            AngleJitter => {
                let v = clamp01(v);
                let d = dynamics(b);
                match &mut d.rotation {
                    Some(r) => r.jitter = Some(v),
                    None if v > 0.0 => {
                        d.rotation = Some(crate::RotationDynamics { source: "none".into(), jitter: Some(v), offset: None });
                    }
                    None => {}
                }
                if let Some(r) = &d.rotation
                    && r.source == "none"
                    && r.jitter.unwrap_or(0.0) == 0.0
                    && r.offset.unwrap_or(0.0) == 0.0
                {
                    d.rotation = None;
                }
            }
            HueJitter => {
                color_dyn(b).hue = Some(clamp01(v));
                tidy_color(b);
            }
            SatJitter => {
                color_dyn(b).sat = Some(clamp01(v));
                tidy_color(b);
            }
            ValJitter => {
                color_dyn(b).val = Some(clamp01(v));
                tidy_color(b);
            }
            PressureDarken => {
                color_dyn(b).pressure_darken = Some(clamp01(v));
                tidy_color(b);
            }
            GrainAmount => {
                if let Some(g) = &mut b.grain {
                    g.strength = clamp01(v);
                }
            }
            GrainScale => {
                if let Some(g) = &mut b.grain {
                    g.scale = v.clamp(16.0, 600.0).round();
                }
            }
            PaperResponse => b.paper_response = Some(clamp01(v)),
            Wetness => {
                let v = clamp01(v);
                set_wet(b, "wetness", v);
                // A blender levels relief by as much oil as it carries.
                if is_blender(b) {
                    set_wet(b, "blend", v);
                }
            }
            Load => {
                let v = clamp01(v);
                set_wet(b, "load", v);
                // Blending takes the rest of its colour from the canvas.
                if is_blender(b) {
                    set_wet(b, "mix", 1.0 - v);
                }
            }
            Mix => set_wet(b, "mix", clamp01(v)),
            FluidWater => set_fluid(b, "water", v.clamp(0.0, 2.0)),
            FluidPigment => set_fluid(b, "pigment", clamp01(v)),
            // No halo is stored as none (happypaint writes it only above 1).
            FluidHalo => {
                let v = v.clamp(1.0, 3.0);
                if v <= 1.0 {
                    if let Some(o) = b.extra.get_mut("fluid").and_then(|f| f.as_object_mut()) {
                        o.remove("halo");
                    }
                } else {
                    set_fluid(b, "halo", v);
                }
            }
            SpacingJitter => {
                let v = clamp01(v);
                b.spacing_jitter = (v > 0.0).then_some(v);
            }
            HueShift => {
                color_dyn(b).hue_shift = Some(v.clamp(-1.0, 1.0));
                tidy_color(b);
            }
            GrainMove => {
                if let Some(g) = &mut b.grain {
                    let v = clamp01(v);
                    g.moves = (v > 0.0).then_some(v);
                }
            }
            MaskStrength => {
                if let Some(o) = b.extra.get_mut("maskTip").and_then(|m| m.as_object_mut()) {
                    o.insert("strength".into(), serde_json::json!(clamp01(v)));
                }
            }
            TipHue => recolor_set(b, "hue", (v / 360.0).clamp(-0.5, 0.5)),
            TipSat => recolor_set(b, "sat", v.clamp(0.0, 2.0)),
            TipVal => recolor_set(b, "val", v.clamp(0.0, 2.0)),
        }
        if let Some(d) = &b.dynamics
            && *d == Dynamics::default()
        {
            b.dynamics = None;
        }
    }
}

fn recolor_get(b: &BrushPreset, key: &str, default: f64) -> f64 {
    b.extra.get("tipRecolor").and_then(|r| r.get(key)).and_then(|v| v.as_f64()).unwrap_or(default)
}

/// Colour-tip recolour; dropped when back to neutral (`brush-store.ts:968-971`).
fn recolor_set(b: &mut BrushPreset, key: &str, v: f64) {
    let r = b.extra.entry("tipRecolor").or_insert_with(|| serde_json::json!({"hue": 0.0, "sat": 1.0, "val": 1.0}));
    if let Some(o) = r.as_object_mut() {
        o.insert(key.into(), serde_json::json!(v));
    }
    if b.tip_recolor().is_none() {
        b.extra.remove("tipRecolor");
    }
}

/// Dual brush: pick the second tip (`None` removes it); new masks start at 70%.
pub fn set_mask_tip(b: &mut BrushPreset, id: Option<&str>) {
    match id {
        Some(id) => {
            let strength = b.extra.get("maskTip").and_then(|m| m.get("strength")).and_then(|v| v.as_f64()).unwrap_or(0.7);
            b.extra.insert("maskTip".into(), serde_json::json!({"id": id, "strength": strength}));
        }
        None => {
            b.extra.remove("maskTip");
        }
    }
}

/// Colour tips: paint the tip's own colours (on) or tint the brush colour.
pub fn set_use_tip_color(b: &mut BrushPreset, on: bool) {
    if on {
        b.extra.insert("useTipColor".into(), serde_json::json!(true));
    } else {
        b.extra.remove("useTipColor");
    }
}

/// Two-colour load ("Tip col"): the paint fades toward this sRGB colour.
pub fn set_to_color(b: &mut BrushPreset, c: Option<[f64; 3]>) {
    color_dyn(b).to_color = c;
    tidy_color(b);
}

fn wet_num(b: &BrushPreset, key: &str, default: f64) -> f64 {
    b.extra.get("wet").and_then(|w| w.get(key)).and_then(|v| v.as_f64()).unwrap_or(default)
}

fn set_wet(b: &mut BrushPreset, key: &str, v: f64) {
    let w = b.extra.entry("wet").or_insert_with(|| serde_json::json!({"wetness": 0.0, "load": 1.0, "mix": 0.5}));
    if let Some(o) = w.as_object_mut() {
        o.insert(key.into(), serde_json::json!(v));
    }
}

fn set_fluid(b: &mut BrushPreset, key: &str, v: f64) {
    if let Some(o) = b.extra.get_mut("fluid").and_then(|f| f.as_object_mut()) {
        o.insert(key.into(), serde_json::json!(v));
    }
}

/// A blender: the brush levels relief and takes its colour from the canvas.
pub fn is_blender(b: &BrushPreset) -> bool {
    wet_num(b, "blend", 0.0) > 0.0
}

/// Oil / Pigment naming: a blender, or any brush that lays down body paint.
pub fn oil_paint(b: &BrushPreset) -> bool {
    is_blender(b) || b.impasto_depth() > 0.0
}

/// Marks a dry brush that was wetted only to blend, so it dries again after.
const BLEND_WETTED: &str = "onepaint:blendWetted";

/// Turn the brush into a blender and back (`brush-store.ts:751-761`). A dry
/// brush gets usable blending defaults (Oil 100%, Pigment 20%) and goes back
/// dry; turning it off restores the preset's own tip and mix.
pub fn set_blender(b: &mut BrushPreset, on: bool) {
    if on == is_blender(b) {
        return;
    }
    if on {
        if wet_num(b, "wetness", 0.0) <= 0.0 {
            set_wet(b, "wetness", 1.0);
            set_wet(b, "load", 0.2);
            b.extra.insert(BLEND_WETTED.into(), serde_json::json!(true));
        }
        let (wet, load) = (wet_num(b, "wetness", 0.0), wet_num(b, "load", 1.0));
        set_wet(b, "blend", wet);
        set_wet(b, "mix", 1.0 - load);
        if let Some(o) = b.extra.get_mut("wet").and_then(|w| w.as_object_mut()) {
            o.insert("tip".into(), serde_json::json!(true));
        }
    } else {
        let base = crate::preset(&b.id);
        let base_wet = |k: &str| base.as_ref().and_then(|p| p.extra.get("wet")).and_then(|w| w.get(k)).cloned();
        if let Some(o) = b.extra.get_mut("wet").and_then(|w| w.as_object_mut()) {
            o.insert("blend".into(), serde_json::json!(0.0));
            o.insert("tip".into(), base_wet("tip").unwrap_or(serde_json::json!(false)));
            o.insert("mix".into(), base_wet("mix").unwrap_or(serde_json::json!(0.5)));
        }
        if b.extra.remove(BLEND_WETTED).is_some() {
            set_wet(b, "wetness", 0.0);
        }
    }
}

/// Pick a grain (`None` removes it); a new grain starts at scale 160, amount 50%.
pub fn set_grain(b: &mut BrushPreset, id: Option<&str>) {
    b.grain = id.map(|id| match &b.grain {
        Some(g) => Grain { id: id.into(), ..g.clone() },
        None => Grain { id: id.into(), scale: 160.0, strength: 0.5, moves: None, random: None },
    });
}

pub fn set_stabilizer(b: &mut BrushPreset, on: bool) {
    let s = b.stabilizer.get_or_insert(crate::StabilizerSettings { enabled: false, strength: 0.5 });
    s.enabled = on;
    if on && s.strength <= 0.0 {
        s.strength = 0.5;
    }
}

pub fn set_blend_mode(b: &mut BrushPreset, m: onepaint_doc::BlendMode) {
    if m == onepaint_doc::BlendMode::Normal {
        b.extra.remove("blendMode");
    } else {
        b.extra.insert("blendMode".into(), m.as_str().into());
    }
}

/// The three pressure curves the studio edits (`toolbar.tsx:953-1034`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CurveChannel {
    Size,
    Flow,
    Hardness,
}

impl CurveChannel {
    pub const ALL: [CurveChannel; 3] = [CurveChannel::Size, CurveChannel::Flow, CurveChannel::Hardness];

    pub fn label(self) -> &'static str {
        match self {
            CurveChannel::Size => "Size ← Pressure",
            CurveChannel::Flow => "Flow ← Pressure",
            CurveChannel::Hardness => "Hardness ← Pressure",
        }
    }

    /// Shown when the preset has no pressure binding (`brush-store.ts:10-23`).
    fn default_curve(self) -> Vec<CurvePoint> {
        let y0 = match self {
            CurveChannel::Size => 0.15,
            CurveChannel::Flow => 0.3,
            CurveChannel::Hardness => 1.0,
        };
        vec![CurvePoint::new(0.0, y0), CurvePoint::new(1.0, 1.0)]
    }

    fn bindings(self, d: &Dynamics) -> &Vec<Binding> {
        match self {
            CurveChannel::Size => &d.size,
            CurveChannel::Flow => &d.flow,
            CurveChannel::Hardness => &d.hardness,
        }
    }

    fn bindings_mut(self, d: &mut Dynamics) -> &mut Vec<Binding> {
        match self {
            CurveChannel::Size => &mut d.size,
            CurveChannel::Flow => &mut d.flow,
            CurveChannel::Hardness => &mut d.hardness,
        }
    }

    /// The pressure curve to show: the preset's binding or the default.
    pub fn curve(self, b: &BrushPreset) -> (Vec<CurvePoint>, Interp) {
        b.dynamics
            .as_ref()
            .and_then(|d| self.bindings(d).iter().find(|b| b.sensor == Sensor::Pressure))
            .map_or_else(|| (self.default_curve(), Interp::Linear), |b| (b.curve.clone(), b.interp.unwrap_or_default()))
    }

    /// Replace this channel's pressure binding; other sensors stay.
    pub fn set_curve(self, b: &mut BrushPreset, curve: Vec<CurvePoint>, interp: Interp) {
        let list = self.bindings_mut(dynamics(b));
        let binding =
            Binding { sensor: Sensor::Pressure, curve, interp: (interp != Interp::Linear).then_some(interp), min: None, max: None };
        match list.iter_mut().find(|b| b.sensor == Sensor::Pressure) {
            Some(slot) => *slot = binding,
            None => list.insert(0, binding),
        }
    }
}

/// The sample S-curve (`brush-preview.ts`): one sine period across the pad,
/// pressure rising and falling, a slight tilt.
pub fn preview_path(w: f64, h: f64) -> Vec<StrokePoint> {
    let n = ((w / 2.0).round() as usize).max(24);
    (0..=n)
        .map(|i| {
            let f = i as f64 / n as f64;
            StrokePoint {
                x: w * 0.08 + f * w * 0.84,
                y: h * 0.5 + (f * std::f64::consts::TAU).sin() * h * 0.26,
                pressure: (f * std::f64::consts::PI).sin().max(0.04),
                tilt_x: 0.45,
                tilt_y: 0.0,
                twist: 0.0,
                t: Some(i as f64 * 12.0),
            }
        })
        .collect()
}

/// Fit a big brush into a pad of height `h` (`forPreview`): size, taper and
/// grain scale shrink together.
pub fn for_preview(b: &BrushPreset, h: f64) -> BrushPreset {
    let mut p = b.clone();
    if p.size > h * 0.85 {
        let k = h * 0.85 / p.size;
        p.size *= k;
        if let Some(t) = &mut p.taper {
            t.entry *= k;
            t.exit *= k;
        }
        if let Some(g) = &mut p.grain {
            g.scale *= k;
        }
    }
    p
}

/// Rasterise one stroke into a fresh buffer (premultiplied, linear colour),
/// stroke opacity not yet applied.
pub fn render_stroke(b: &BrushPreset, color_linear: [f32; 3], pts: &[StrokePoint], seed: u32) -> Surface {
    let mut eng = StrokeEngine::new(b.clone(), color_linear, seed);
    let mut s = Surface::new();
    let mut dabs = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        dabs.extend(if i == 0 { eng.begin(*p) } else { eng.add(*p) });
    }
    dabs.extend(eng.end());
    for d in &dabs {
        eng.rasterize(&mut s, d, None);
    }
    s
}

/// The studio preview: the S-curve in a `w × h` pad, opacity applied.
pub fn render_preview(b: &BrushPreset, color_linear: [f32; 3], w: usize, h: usize) -> Surface {
    let p = for_preview(b, h as f64);
    let mut s = render_stroke(&p, color_linear, &preview_path(w as f64, h as f64), 7);
    scale_alpha(&mut s, p.opacity as f32);
    s
}

/// Multiply every pixel (premultiplied) by `k`.
pub fn scale_alpha(s: &mut Surface, k: f32) {
    if k >= 1.0 {
        return;
    }
    for (tx, ty) in s.tile_keys() {
        let t = s.tile_mut(tx, ty);
        for y in 0..onepaint_raster::TILE as usize {
            for x in 0..onepaint_raster::TILE as usize {
                let p = t.get(x, y);
                t.set(x, y, p.map(|c| c * k));
            }
        }
    }
}

/// `dst = src + dst·(1 − src.a)` over every tile of `src`.
pub fn composite_over(dst: &mut Surface, src: &Surface) {
    for (tx, ty) in src.tile_keys() {
        let Some(st) = src.tile(tx, ty).cloned() else { continue };
        let dt = dst.tile_mut(tx, ty);
        for y in 0..onepaint_raster::TILE as usize {
            for x in 0..onepaint_raster::TILE as usize {
                let s = st.get(x, y);
                if s[3] <= 0.0 {
                    continue;
                }
                let d = dt.get(x, y);
                let k = 1.0 - s[3];
                dt.set(x, y, [s[0] + d[0] * k, s[1] + d[1] * k, s[2] + d[2] * k, s[3] + d[3] * k]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{Taper, preset};

    fn hard() -> BrushPreset {
        preset("hard-round").unwrap()
    }

    fn coverage(s: &Surface, w: i32, h: i32) -> f64 {
        let mut a = 0.0;
        for y in 0..h {
            for x in 0..w {
                a += s.get(x, y)[3] as f64;
            }
        }
        a
    }

    #[test]
    fn preview_path_follows_the_formula() {
        let pts = preview_path(232.0, 72.0);
        assert_eq!(pts.len(), 117, "n = max(24, round(w/2)) = 116 segments");
        assert!((pts[0].x - 232.0 * 0.08).abs() < 1e-9);
        assert!((pts[116].x - 232.0 * 0.92).abs() < 1e-9);
        assert!((pts[0].y - 36.0).abs() < 1e-9);
        assert!((pts[29].y - (36.0 + (29.0 / 116.0 * std::f64::consts::TAU).sin() * 72.0 * 0.26)).abs() < 1e-9);
        assert_eq!(pts[0].pressure, 0.04);
        assert!((pts[58].pressure - 1.0).abs() < 1e-9);
        assert_eq!(pts[3].t, Some(36.0));
        assert_eq!(pts[3].tilt_x, 0.45);
        assert_eq!(preview_path(20.0, 10.0).len(), 25);
    }

    #[test]
    fn big_brushes_shrink_to_fit_the_preview() {
        let mut b = hard();
        b.size = 200.0;
        b.taper = Some(Taper { entry: 40.0, exit: 20.0 });
        crate::studio::set_grain(&mut b, Some("paper"));
        let p = for_preview(&b, 72.0);
        let k = 72.0 * 0.85 / 200.0;
        assert!((p.size - 200.0 * k).abs() < 1e-9);
        assert!((p.taper.as_ref().unwrap().entry - 40.0 * k).abs() < 1e-9);
        assert!((p.grain.as_ref().unwrap().scale - 160.0 * k).abs() < 1e-9);
        let small = for_preview(&hard(), 72.0);
        assert_eq!(small, hard());
    }

    #[test]
    fn a_param_edit_changes_the_rendered_preview() {
        let mut b = hard();
        b.size = 10.0;
        let thin = coverage(&render_preview(&b, [1.0; 3], 232, 72), 232, 72);
        assert!(thin > 100.0);
        Param::Size.set(&mut b, 40.0);
        let thick = coverage(&render_preview(&b, [1.0; 3], 232, 72), 232, 72);
        assert!(thick > thin * 2.0, "{thin} → {thick}");

        Param::Opacity.set(&mut b, 0.5);
        let half = coverage(&render_preview(&b, [1.0; 3], 232, 72), 232, 72);
        assert!((half / thick - 0.5).abs() < 0.01, "{half} vs {thick}");

        let mut g = b.clone();
        set_grain(&mut g, Some("paper"));
        Param::GrainAmount.set(&mut g, 1.0);
        let grained = coverage(&render_preview(&g, [1.0; 3], 232, 72), 232, 72);
        assert!(grained < half * 0.95, "grain eats coverage: {grained} vs {half}");
    }

    #[test]
    fn every_param_round_trips_inside_its_range() {
        for (_, params) in SECTIONS {
            for &p in *params {
                let i = p.info();
                // Fluid sliders only apply to live-fluid brushes.
                let mut b = match p {
                    Param::FluidWater | Param::FluidPigment | Param::FluidHalo => preset("watercolor-live").unwrap(),
                    Param::TipHue | Param::TipSat | Param::TipVal => preset("rgba-oil").unwrap(),
                    _ => hard(),
                };
                set_grain(&mut b, Some("paper"));
                set_mask_tip(&mut b, Some("bristle"));
                set_stabilizer(&mut b, true);
                let v = i.min + (i.max - i.min) * 0.37;
                p.set(&mut b, v);
                let got = p.get(&b);
                let tol = match i.unit {
                    Unit::Px | Unit::Rate | Unit::Firmness | Unit::Degrees => 1.0,
                    Unit::Percent | Unit::Times => 1e-9,
                };
                assert!((got - v).abs() <= tol, "{}: set {v} got {got}", i.label);
                assert!(!p.format(got).is_empty());
                // NaN never lands.
                let before = b.clone();
                p.set(&mut b, f64::NAN);
                assert_eq!(b, before, "{} NaN", i.label);
            }
        }
    }

    #[test]
    fn tip_form_clamps_and_omits_neutral_values() {
        // brush-tip-form.test.ts
        let mut b = hard();
        Param::Roundness.set(&mut b, 0.0);
        assert_eq!(b.tip_roundness, Some(0.05));
        Param::Angle.set(&mut b, -30.0);
        assert_eq!(b.tip_angle_deg, Some(330.0));
        Param::Roundness.set(&mut b, 1.0);
        Param::Angle.set(&mut b, 360.0);
        assert_eq!((b.tip_roundness, b.tip_angle_deg), (None, None));
    }

    #[test]
    fn stabilizer_amount_turns_it_on_and_off() {
        // brush-stabilizer-bar.test.ts
        let mut b = hard();
        Param::StabilizerAmount.set(&mut b, 0.7);
        assert_eq!(b.stabilizer.as_ref().map(|s| (s.enabled, s.strength)), Some((true, 0.7)));
        Param::StabilizerAmount.set(&mut b, 0.0);
        assert_eq!(b.stabilizer.as_ref().map(|s| (s.enabled, s.strength)), Some((false, 0.7)));
        Param::StabilizerAmount.set(&mut b, 3.0);
        assert_eq!(b.stabilizer.as_ref().map(|s| s.strength), Some(1.0));
        set_stabilizer(&mut b, false);
        assert!(!Param::StabilizerAmount.visible(&b));
    }

    #[test]
    fn synthesised_bindings_match_effective_brush() {
        let mut b = hard();
        Param::Squash.set(&mut b, 0.4);
        Param::Spin.set(&mut b, 0.25);
        Param::SizeJitter.set(&mut b, 0.3);
        Param::AngleJitter.set(&mut b, 0.5);
        let d = b.dynamics.clone().unwrap();
        assert_eq!(d.ratio, vec![speed_binding(1.0, 0.6)]);
        assert_eq!(d.spin, vec![speed_binding(0.0, 0.25)]);
        let j = d.size.iter().find(|b| b.sensor == Sensor::Random).unwrap();
        assert_eq!((j.min, j.max), (Some(0.7), Some(1.0)));
        assert_eq!(d.rotation.as_ref().map(|r| (r.source.as_str(), r.jitter)), Some(("none", Some(0.5))));
        for p in [Param::Squash, Param::Spin, Param::SizeJitter, Param::AngleJitter] {
            p.set(&mut b, 0.0);
        }
        assert_eq!(b.dynamics, None, "all-zero dynamics disappear");

        // A preset's own binding reads as 0 and survives until the slider moves.
        let mut q = hard();
        q.dynamics = Some(Dynamics { spin: vec![Binding { sensor: Sensor::Random, ..Default::default() }], ..Default::default() });
        assert_eq!(Param::Spin.get(&q), 0.0);
        Param::Squash.set(&mut q, 0.2);
        assert_eq!(q.dynamics.as_ref().unwrap().spin.len(), 1);
    }

    #[test]
    fn colour_jitter_vanishes_at_zero() {
        let mut b = hard();
        Param::HueJitter.set(&mut b, 0.2);
        assert_eq!(b.dynamics.as_ref().and_then(|d| d.color.as_ref()).and_then(|c| c.hue), Some(0.2));
        Param::HueJitter.set(&mut b, 0.0);
        assert_eq!(b.dynamics, None);
    }

    #[test]
    fn pressure_firmness_maps_like_happypaint() {
        assert_eq!(curve_to_firm(1.0), 50.0);
        assert_eq!(curve_to_firm(3.0), 100.0);
        assert_eq!(curve_to_firm(1.0 / 3.0), 0.0);
        assert!((firm_to_curve(100.0) - 3.0).abs() < 1e-12);
        assert!((firm_to_curve(50.0) - 1.0).abs() < 1e-12);
        assert_eq!(pressure_label(0.5), "Soft");
        assert_eq!(pressure_label(1.0), "Lin");
        assert_eq!(pressure_label(2.0), "Firm");
    }

    #[test]
    fn curves_show_defaults_and_replace_only_the_pressure_binding() {
        let mut b = hard();
        b.dynamics = None;
        assert_eq!(CurveChannel::Size.curve(&b).0, vec![CurvePoint::new(0.0, 0.15), CurvePoint::new(1.0, 1.0)]);
        assert_eq!(CurveChannel::Flow.curve(&b).0[0].y, 0.3);
        assert_eq!(CurveChannel::Hardness.curve(&b).0[0].y, 1.0);
        Param::SizeJitter.set(&mut b, 0.5);
        let c = vec![CurvePoint::new(0.0, 0.5), CurvePoint::new(0.5, 0.6), CurvePoint::new(1.0, 1.0)];
        CurveChannel::Size.set_curve(&mut b, c.clone(), Interp::Smooth);
        assert_eq!(CurveChannel::Size.curve(&b), (c, Interp::Smooth));
        assert_eq!(b.dynamics.as_ref().unwrap().size.len(), 2, "jitter kept");
        assert_eq!(Param::SizeJitter.get(&b), 0.5);
    }

    #[test]
    fn blend_mode_lives_in_extra() {
        let mut b = hard();
        set_blend_mode(&mut b, onepaint_doc::BlendMode::Multiply);
        assert_eq!(b.blend_mode(), onepaint_doc::BlendMode::Multiply);
        set_blend_mode(&mut b, onepaint_doc::BlendMode::Normal);
        assert!(!b.extra.contains_key("blendMode"));
    }

    #[test]
    fn media_sliders_follow_happypaint() {
        // A wet brush: Wet / Load / Mix edit the wet block and route the stroke.
        let mut b = crate::preset("hard-round").unwrap();
        assert!(!Param::Load.visible(&b) && !Param::Mix.visible(&b));
        Param::Wetness.set(&mut b, 0.6);
        assert!(Param::Load.visible(&b) && Param::Mix.visible(&b));
        Param::Mix.set(&mut b, 0.3);
        let w = b.wet().unwrap();
        assert_eq!((w.wetness, w.load, w.mix), (0.6, 1.0, 0.3));
        assert_eq!(Param::Wetness.label_for(&b), "Wet");

        // Blender on a dry brush: Oil 100%, Pigment 20%, mix 80%, textured;
        // Pigment then drives mix; off dries it again.
        let mut d = crate::preset("hard-round").unwrap();
        set_blender(&mut d, true);
        let w = d.wet().unwrap();
        assert_eq!((w.wetness, w.load, w.blend, w.tip), (1.0, 0.2, 1.0, true));
        assert!((w.mix - 0.8).abs() < 1e-9 && !Param::Mix.visible(&d));
        assert_eq!((Param::Wetness.label_for(&d), Param::Load.label_for(&d)), ("Oil", "Pigment"));
        Param::Load.set(&mut d, 0.4);
        Param::Wetness.set(&mut d, 0.5);
        let w = d.wet().unwrap();
        assert!((w.mix - 0.6).abs() < 1e-9 && w.blend == 0.5);
        set_blender(&mut d, false);
        assert!(d.wet().is_none() && !is_blender(&d), "back to a dry brush");

        // Body paint shows Pigment even dry; Paper and Fluid read their fields.
        let thick = crate::preset("thick-paint").unwrap();
        assert!(Param::Load.visible(&thick) && Param::Load.label_for(&thick) == "Pigment");
        let mut ink = crate::preset("ink").unwrap();
        assert_eq!(Param::PaperResponse.get(&ink), 0.0);
        Param::PaperResponse.set(&mut ink, 0.7);
        assert_eq!(ink.paper_response, Some(0.7));
        let mut live = crate::preset("watercolor-live").unwrap();
        assert!(Param::FluidWater.visible(&live) && !Param::FluidWater.visible(&ink));
        Param::FluidWater.set(&mut live, 1.6);
        Param::FluidPigment.set(&mut live, 0.3);
        let f = live.fluid().unwrap();
        assert_eq!((f.water, f.pigment), (1.6, 0.3));
    }
}
