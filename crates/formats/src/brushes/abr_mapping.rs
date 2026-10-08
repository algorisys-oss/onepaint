//! ABR preset descriptor → brush preset (happypaint
//! `storage/brush-import/abr-mapping.ts`). Present-but-unmapped features are
//! reported in `unmapped`, never dropped silently.

use onepaint_brush::{Airbrush, BrushPreset, Dynamics, Grain, RotationDynamics};
use onepaint_stroke::{Binding, CurvePoint, Sensor};
use serde_json::{Value, json};

/// Photoshop's control dropdown, in `bVTy` order, and the sensor each maps to.
const CONTROLS: [(&str, Option<Sensor>); 8] = [
    ("off", None),
    ("fade", Some(Sensor::Fade)),
    ("pressure", Some(Sensor::Pressure)),
    ("tilt", Some(Sensor::Tilt)),
    ("wheel", Some(Sensor::Twist)),
    ("rotation", Some(Sensor::Twist)),
    ("initialDirection", Some(Sensor::Angle)),
    ("direction", Some(Sensor::Angle)),
];

/// A one-time shape transform baked into the imported tip bitmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TipTransform {
    pub angle_deg: f64,
    pub roundness_pct: f64,
    pub flip_x: bool,
    pub flip_y: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MappedAbr {
    pub tip_transform: Option<TipTransform>,
    /// `sampledData` UUID of the dual-brush tip (becomes `maskTip`).
    pub dual_tip_uuid: Option<String>,
    pub unmapped: Vec<String>,
}

fn num(v: Option<&Value>, fallback: f64) -> f64 {
    v.and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(fallback)
}

fn unit(v: Option<&Value>, fallback: f64) -> f64 {
    num(v.and_then(|u| u.get("value")), fallback)
}

fn control(block: Option<&Value>) -> &'static str {
    let i = num(block.and_then(|b| b.get("bVTy")), 0.0);
    if i >= 0.0 && i.fract() == 0.0 { CONTROLS.get(i as usize).map_or("off", |c| c.0) } else { "off" }
}

fn jitter_pct(block: Option<&Value>) -> f64 {
    unit(block.and_then(|b| b.get("jitter")), 0.0).max(0.0) / 100.0
}

fn linear() -> Vec<CurvePoint> {
    vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)]
}

fn binding(sensor: Sensor, min: f64) -> Binding {
    Binding { sensor, curve: linear(), interp: None, min: Some(min), max: Some(1.0) }
}

/// The control's binding scaled onto `[min, 1]`; none when off or when
/// `min ≥ 1` (no effect in Photoshop either).
fn control_binding(block: Option<&Value>, min: f64) -> Option<Binding> {
    let name = control(block);
    let sensor = CONTROLS.iter().find(|c| c.0 == name).and_then(|c| c.1)?;
    let lo = min.clamp(0.0, 1.0);
    (lo < 1.0).then(|| binding(sensor, lo))
}

/// Random jitter: the parameter times `[max(floor, 1 − jitter), 1]`.
fn jitter_binding(pct: f64, floor: f64) -> Option<Binding> {
    (pct > 0.0).then(|| binding(Sensor::Random, floor.max(1.0 - pct)))
}

fn is_true(v: Option<&Value>) -> bool {
    v == Some(&Value::Bool(true))
}

/// Map one preset descriptor onto `p` (already holding the import defaults).
pub fn map_abr_descriptor(desc: &Value, p: &mut BrushPreset) -> MappedAbr {
    let mut m = MappedAbr::default();
    let mut dyn_ = Dynamics::default();
    let mut any_dyn = false;
    let mut spacing_set = false;

    // Tip block: size, spacing, one-time shape transform.
    if let Some(tip) = desc.get("Brsh").filter(|t| t.is_object()) {
        let dmtr = unit(tip.get("Dmtr"), 0.0);
        if dmtr > 0.0 {
            p.size = dmtr.round().clamp(1.0, 512.0);
        }
        let on = tip.get("Intr") != Some(&Value::Bool(false));
        let spcn = unit(tip.get("Spcn"), 25.0);
        // Spacing off in Photoshop paints at the event rate; approximate with our floor.
        p.spacing = if on { (spcn / 100.0).clamp(0.02, 4.0) } else { 0.05 };
        spacing_set = true;
        let t = TipTransform {
            angle_deg: unit(tip.get("Angl"), 0.0),
            roundness_pct: unit(tip.get("Rndn"), 100.0),
            flip_x: is_true(tip.get("flipX")),
            flip_y: is_true(tip.get("flipY")),
        };
        if t.angle_deg != 0.0 || t.roundness_pct < 100.0 || t.flip_x || t.flip_y {
            m.tip_transform = Some(t);
        }
    }

    // Tip dynamics: size, angle and roundness variance.
    if is_true(desc.get("useTipDynamics")) {
        let sz = desc.get("szVr");
        let min_diam = unit(desc.get("minimumDiameter"), 0.0) / 100.0;
        let size: Vec<Binding> = [control_binding(sz, min_diam), jitter_binding(jitter_pct(sz), min_diam)].into_iter().flatten().collect();
        if !size.is_empty() {
            dyn_.size = size;
            any_dyn = true;
        }
        if control(sz) == "pressure" {
            p.size_pressure = 1.0 - min_diam;
        }
        if sz.is_some() && control(sz) == "wheel" {
            m.unmapped.push("szVr control: stylus wheel (mapped to twist)".into());
        }

        let angle = desc.get("angleDynamics");
        let jit = jitter_pct(angle);
        if matches!(control(angle), "direction" | "initialDirection") {
            dyn_.rotation = Some(RotationDynamics { source: "angle".into(), jitter: Some(jit), offset: None });
            any_dyn = true;
        } else if jit > 0.0 {
            dyn_.rotation = Some(RotationDynamics { source: "none".into(), jitter: Some(jit), offset: None });
            any_dyn = true;
        }

        let round = desc.get("roundnessDynamics");
        let min_round = unit(desc.get("minimumRoundness"), 0.0) / 100.0;
        if let Some(b) = jitter_binding(jitter_pct(round), min_round.max(0.05)) {
            dyn_.ratio = vec![b];
            any_dyn = true;
        }

        let tilt = unit(desc.get("tiltScale"), 0.0);
        if tilt > 0.0 {
            p.tilt_strength = (tilt / 200.0).min(1.0);
        }
    }

    // Paint dynamics (Photoshop "Transfer"): opacity and flow both drive flow.
    if is_true(desc.get("usePaintDynamics")) {
        let mut flow: Vec<Binding> = Vec::new();
        for key in ["opVr", "prVr"] {
            let block = desc.get(key);
            if let Some(b) = control_binding(block, 0.02)
                && !flow.iter().any(|x| x.sensor == b.sensor)
            {
                flow.push(b);
            }
            if let Some(j) = jitter_binding(jitter_pct(block), 0.02)
                && !flow.iter().any(|x| x.sensor == Sensor::Random)
            {
                flow.push(j);
            }
        }
        if !flow.is_empty() {
            if flow.iter().any(|b| b.sensor == Sensor::Pressure) {
                p.flow_pressure = 1.0;
            }
            dyn_.flow = flow;
            any_dyn = true;
        }
    }

    // Scatter: % of diameter → a Gaussian fraction of size (0.5× reads closest).
    if is_true(desc.get("useScatter")) {
        let amount = jitter_pct(desc.get("scatterDynamics")) * 0.5;
        if amount > 0.0 {
            dyn_.scatter = Some(amount.min(2.0));
            any_dyn = true;
        }
        let count = num(desc.get("Cnt"), 1.0);
        if count > 1.0 && spacing_set {
            // No per-stamp count: approximate the density with tighter spacing.
            p.spacing = (p.spacing / count).max(0.02);
        }
    }

    if is_true(desc.get("Wtdg")) {
        p.extra.insert("settle".into(), json!({ "edgeDarken": 0.6, "granulate": 0 }));
    }
    if is_true(desc.get("Rpt")) {
        p.airbrush = Some(Airbrush { rate: 15.0 });
        p.wash = false;
    }
    if is_true(desc.get("Nose")) {
        p.grain = Some(Grain { id: "speckle".into(), scale: 40.0, strength: 0.3, moves: Some(1.0), random: None });
    }

    if let Some(dual) = desc.get("dualBrush")
        && is_true(dual.get("useDualBrush"))
    {
        let sd = dual.get("Brsh").and_then(|b| b.get("sampledData")).and_then(Value::as_str).map(str::trim).unwrap_or("");
        if sd.is_empty() {
            m.unmapped.push("dualBrush without sampledData".into());
        } else {
            m.dual_tip_uuid = Some(sd.to_string());
        }
    }

    if is_true(desc.get("useTexture")) {
        m.unmapped.push("useTexture (texture block unmapped — no observed corpus yet)".into());
    }
    if is_true(desc.get("useColorDynamics")) {
        m.unmapped.push("useColorDynamics".into());
    }
    if is_true(desc.get("useBrushPose")) {
        m.unmapped.push("useBrushPose".into());
    }

    if any_dyn {
        p.dynamics = Some(dyn_);
    }
    m
}
