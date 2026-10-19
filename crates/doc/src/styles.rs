//! Layer styles (spec 56; happypaint v0.196, `engine/styles/layer-styles.ts`):
//! Photoshop-style effects on a layer or group. Kept in the node's `extra`
//! under happypaint's `styles` key, read and written as its JSON, so
//! `.happypaint` files round-trip.

use crate::BlendMode;
use serde_json::{Map, Value, json};

pub const STYLES_KEY: &str = "styles";

/// The largest effect size or distance, in px (`MAX_EFFECT_PX`).
pub const MAX_EFFECT_PX: f32 = 250.0;
/// At most this many of a repeatable kind (`MAX_INSTANCES`).
pub const MAX_INSTANCES: usize = 4;
/// Global Light when no effect sets it: angle, altitude (degrees).
pub const DEFAULT_LIGHT: (f32, f32) = (120.0, 30.0);

/// The effect kinds, in the Layer Style dialog's order (`EFFECT_KINDS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EffectKind {
    Bevel,
    Stroke,
    InnerShadow,
    InnerGlow,
    Satin,
    ColorOverlay,
    GradientOverlay,
    PatternOverlay,
    OuterGlow,
    DropShadow,
}

impl EffectKind {
    pub const ALL: [EffectKind; 10] = [
        EffectKind::Bevel,
        EffectKind::Stroke,
        EffectKind::InnerShadow,
        EffectKind::InnerGlow,
        EffectKind::Satin,
        EffectKind::ColorOverlay,
        EffectKind::GradientOverlay,
        EffectKind::PatternOverlay,
        EffectKind::OuterGlow,
        EffectKind::DropShadow,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EffectKind::Bevel => "bevel",
            EffectKind::Stroke => "stroke",
            EffectKind::InnerShadow => "inner-shadow",
            EffectKind::InnerGlow => "inner-glow",
            EffectKind::Satin => "satin",
            EffectKind::ColorOverlay => "color-overlay",
            EffectKind::GradientOverlay => "gradient-overlay",
            EffectKind::PatternOverlay => "pattern-overlay",
            EffectKind::OuterGlow => "outer-glow",
            EffectKind::DropShadow => "drop-shadow",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Kinds a style may hold up to four of (`MULTI_KINDS`).
    pub fn repeats(self) -> bool {
        matches!(
            self,
            EffectKind::Stroke | EffectKind::DropShadow | EffectKind::InnerShadow | EffectKind::ColorOverlay | EffectKind::GradientOverlay
        )
    }

    /// A new effect's blend mode: shadows and satin multiply, glows screen.
    pub fn default_mode(self) -> BlendMode {
        match self {
            EffectKind::DropShadow | EffectKind::InnerShadow | EffectKind::Satin => BlendMode::Multiply,
            EffectKind::OuterGlow | EffectKind::InnerGlow => BlendMode::Screen,
            _ => BlendMode::Normal,
        }
    }

    /// Whether the effect is lit by Global Light when linked.
    fn has_light(self) -> bool {
        matches!(self, EffectKind::DropShadow | EffectKind::InnerShadow | EffectKind::Bevel)
    }
}

/// A contour (`ContourId`): how a 0..1 ramp is shaped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Contour {
    #[default]
    Linear,
    HalfRound,
    Gaussian,
    Cone,
    ConeInverted,
    Ring,
    RollingSlope,
}

impl Contour {
    pub const ALL: [Contour; 7] = [
        Contour::Linear,
        Contour::HalfRound,
        Contour::Gaussian,
        Contour::Cone,
        Contour::ConeInverted,
        Contour::Ring,
        Contour::RollingSlope,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Contour::Linear => "linear",
            Contour::HalfRound => "half-round",
            Contour::Gaussian => "gaussian",
            Contour::Cone => "cone",
            Contour::ConeInverted => "cone-inverted",
            Contour::Ring => "ring",
            Contour::RollingSlope => "rolling-slope",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == s)
    }

    /// The shaped value at `x` in 0..1 (`contourAt`).
    pub fn at(self, x: f32) -> f32 {
        match self {
            Contour::Linear => x,
            Contour::HalfRound => (1.0 - (1.0 - x) * (1.0 - x)).max(0.0).sqrt(),
            Contour::Gaussian => x * x * (3.0 - 2.0 * x),
            Contour::Cone => 1.0 - (2.0 * x - 1.0).abs(),
            Contour::ConeInverted => (2.0 * x - 1.0).abs(),
            Contour::Ring => 0.5 - 0.5 * (4.0 * std::f32::consts::PI * x).cos(),
            Contour::RollingSlope => (x + 0.12 * (2.0 * std::f32::consts::PI * x).sin()).clamp(0.0, 1.0),
        }
    }
}

/// One effect. Every field is present; the ones a kind doesn't use keep
/// their defaults and aren't written.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub kind: EffectKind,
    pub enabled: bool,
    pub blend_mode: BlendMode,
    pub opacity: f32,
    /// Straight sRGB 0..1 (`#rrggbb` in the file).
    pub color: [f32; 3],
    /// Degrees counter-clockwise from light on the right.
    pub angle: f32,
    pub distance: f32,
    /// Spread (shadows, glows) or Choke (inner shadow), 0..1.
    pub spread: f32,
    pub size: f32,
    pub use_global_light: bool,
    /// Stroke: "outside", "inside" or "center".
    pub position: String,
    /// Gradient overlay: stops, shape, scale, reverse.
    pub stops: Vec<Value>,
    pub shape: String,
    pub scale: f32,
    pub reverse: bool,
    /// Pattern overlay: colour B and the built-in pattern id.
    pub color_b: [f32; 3],
    pub pattern: String,
    /// Satin.
    pub contour: Contour,
    pub invert: bool,
    /// Bevel & emboss.
    pub bevel: Bevel,
}

/// Bevel & emboss's own settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Bevel {
    /// "inner", "outer", "emboss" or "pillow".
    pub style: String,
    pub depth: f32,
    /// Up (true) or down.
    pub up: bool,
    pub soften: f32,
    pub altitude: f32,
    pub highlight_color: [f32; 3],
    pub highlight_opacity: f32,
    pub highlight_mode: BlendMode,
    pub shadow_color: [f32; 3],
    pub shadow_opacity: f32,
    pub shadow_mode: BlendMode,
    /// The Contour sub-effect: on, and its shape.
    pub contour_on: bool,
    pub contour: Contour,
    /// The Texture sub-effect.
    pub texture_on: bool,
    pub texture_pattern: String,
    pub texture_scale: f32,
    pub texture_depth: f32,
    pub texture_invert: bool,
}

impl Default for Bevel {
    fn default() -> Self {
        Self {
            style: "inner".into(),
            depth: 1.0,
            up: true,
            soften: 0.0,
            altitude: 30.0,
            highlight_color: [1.0; 3],
            highlight_opacity: 0.75,
            highlight_mode: BlendMode::Screen,
            shadow_color: [0.0; 3],
            shadow_opacity: 0.75,
            shadow_mode: BlendMode::Multiply,
            contour_on: false,
            contour: Contour::Linear,
            texture_on: false,
            texture_pattern: "brick".into(),
            texture_scale: 1.0,
            texture_depth: 0.5,
            texture_invert: false,
        }
    }
}

fn hex(c: [f32; 3]) -> String {
    let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

fn parse_hex(s: &str) -> Option<[f32; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    Some([v(0)?, v(2)?, v(4)?])
}

/// The effect a new `kind` starts as (`defaultEffect`).
pub fn default_effect(kind: EffectKind) -> Effect {
    let rgb = |s: &str| parse_hex(s).unwrap_or([0.0; 3]);
    let mut e = Effect {
        kind,
        enabled: true,
        blend_mode: kind.default_mode(),
        opacity: 0.75,
        color: [0.0; 3],
        angle: 120.0,
        distance: 0.0,
        spread: 0.0,
        size: 0.0,
        use_global_light: false,
        position: "outside".into(),
        stops: Vec::new(),
        shape: "linear".into(),
        scale: 1.0,
        reverse: false,
        color_b: [1.0; 3],
        pattern: "dots".into(),
        contour: Contour::Gaussian,
        invert: true,
        bevel: Bevel::default(),
    };
    match kind {
        EffectKind::DropShadow => (e.distance, e.size, e.use_global_light) = (8.0, 8.0, true),
        EffectKind::InnerShadow => (e.distance, e.size, e.use_global_light) = (5.0, 5.0, true),
        EffectKind::OuterGlow => (e.color, e.size) = (rgb("#ffffbe"), 12.0),
        EffectKind::InnerGlow => (e.color, e.size) = (rgb("#ffffbe"), 8.0),
        EffectKind::Stroke => (e.opacity, e.size) = (1.0, 3.0),
        EffectKind::ColorOverlay => (e.opacity, e.color) = (1.0, rgb("#ff0000")),
        EffectKind::GradientOverlay => {
            e.opacity = 1.0;
            e.angle = 90.0;
            e.stops = vec![json!({"pos": 0, "color": [0, 0, 0], "opacity": 1}), json!({"pos": 1, "color": [1, 1, 1], "opacity": 1})];
        }
        EffectKind::PatternOverlay => e.opacity = 1.0,
        EffectKind::Satin => (e.opacity, e.angle, e.distance, e.size) = (0.5, 19.0, 11.0, 14.0),
        EffectKind::Bevel => (e.opacity, e.color, e.size, e.use_global_light) = (1.0, [1.0; 3], 6.0, true),
    }
    e
}

/// A node's styles: Fill opacity and the effects, first drawn on top.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerStyles {
    pub fill_opacity: f32,
    pub effects: Vec<Effect>,
}

impl Default for LayerStyles {
    fn default() -> Self {
        Self { fill_opacity: 1.0, effects: Vec::new() }
    }
}

fn wrap_angle(a: f32) -> f32 {
    ((a % 360.0) + 360.0) % 360.0
}

impl Effect {
    /// Whether it draws anything (`isActive`).
    pub fn active(&self) -> bool {
        self.enabled && self.opacity > 0.0
    }

    /// Read happypaint's JSON, clamped (`sanitizeEffect`): `None` for an
    /// unknown kind. A missing blend mode is Normal, as files before
    /// v0.196.2 drew; a missing Global Light link is off.
    pub fn from_json(v: &Value) -> Option<Self> {
        let kind = EffectKind::parse(v.get("kind")?.as_str()?)?;
        let d = default_effect(kind);
        let num = |k: &str, def: f32| v.get(k).and_then(Value::as_f64).map_or(def, |x| x as f32);
        let boolean = |k: &str, def: bool| v.get(k).and_then(Value::as_bool).unwrap_or(def);
        let string = |k: &str, def: &str| v.get(k).and_then(Value::as_str).unwrap_or(def).to_string();
        let colour = |k: &str, def: [f32; 3]| v.get(k).and_then(Value::as_str).and_then(parse_hex).unwrap_or(def);
        let mode = |k: &str| v.get(k).and_then(Value::as_str).map_or(BlendMode::Normal, BlendMode::parse);
        let px = |k: &str, def: f32| num(k, def).clamp(0.0, MAX_EFFECT_PX);
        let mut e = Effect {
            kind,
            enabled: boolean("enabled", true),
            blend_mode: mode("blendMode"),
            opacity: num("opacity", d.opacity).clamp(0.0, 1.0),
            color: colour("color", d.color),
            angle: wrap_angle(num("angle", d.angle)),
            distance: px("distance", d.distance),
            spread: num("spread", d.spread).clamp(0.0, 1.0),
            size: px("size", d.size),
            use_global_light: boolean("useGlobalLight", false),
            position: d.position.clone(),
            stops: d.stops.clone(),
            shape: d.shape.clone(),
            scale: d.scale,
            reverse: false,
            color_b: d.color_b,
            pattern: d.pattern.clone(),
            contour: d.contour,
            invert: d.invert,
            bevel: d.bevel.clone(),
        };
        match kind {
            EffectKind::Stroke => {
                e.size = num("size", d.size).clamp(1.0, MAX_EFFECT_PX);
                let p = string("position", "outside");
                e.position = if matches!(p.as_str(), "outside" | "inside" | "center") { p } else { "outside".into() };
            }
            EffectKind::GradientOverlay => {
                if let Some(s) = v.get("stops").and_then(Value::as_array).filter(|s| !s.is_empty()) {
                    e.stops = s.clone();
                }
                e.shape = string("shape", "linear");
                e.scale = num("scale", 1.0).clamp(0.1, 1.5);
                e.reverse = boolean("reverse", false);
            }
            EffectKind::PatternOverlay => {
                e.color_b = colour("colorB", d.color_b);
                e.pattern = string("pattern", &d.pattern);
                e.scale = num("scale", 1.0).clamp(0.25, 10.0);
            }
            EffectKind::Satin => {
                e.contour = v.get("contour").and_then(Value::as_str).and_then(Contour::parse).unwrap_or(d.contour);
                e.invert = boolean("invert", d.invert);
            }
            EffectKind::Bevel => {
                let b = &mut e.bevel;
                b.style = string("style", "inner");
                b.depth = num("depth", 1.0).clamp(0.01, 10.0);
                b.up = v.get("direction").and_then(Value::as_str) != Some("down");
                e.size = num("size", d.size).clamp(1.0, MAX_EFFECT_PX);
                b.soften = num("soften", 0.0).clamp(0.0, 16.0);
                b.altitude = num("altitude", 30.0).clamp(0.0, 90.0);
                b.highlight_color = colour("highlightColor", [1.0; 3]);
                b.highlight_opacity = num("highlightOpacity", 0.75).clamp(0.0, 1.0);
                // Missing modes read as Normal: bevels before they were saved.
                b.highlight_mode = mode("highlightMode");
                b.shadow_color = colour("shadowColor", [0.0; 3]);
                b.shadow_opacity = num("shadowOpacity", 0.75).clamp(0.0, 1.0);
                b.shadow_mode = mode("shadowMode");
                if let Some(c) = v.get("contour") {
                    b.contour_on = c.get("on").and_then(Value::as_bool).unwrap_or(false);
                    b.contour = c.get("shape").and_then(Value::as_str).and_then(Contour::parse).unwrap_or(Contour::Linear);
                }
                if let Some(t) = v.get("texture") {
                    b.texture_on = t.get("on").and_then(Value::as_bool).unwrap_or(false);
                    b.texture_pattern = t.get("pattern").and_then(Value::as_str).unwrap_or("brick").to_string();
                    b.texture_scale = t.get("scale").and_then(Value::as_f64).map_or(1.0, |x| x as f32).clamp(0.25, 10.0);
                    b.texture_depth = t.get("depth").and_then(Value::as_f64).map_or(0.5, |x| x as f32).clamp(0.0, 1.0);
                    b.texture_invert = t.get("invert").and_then(Value::as_bool).unwrap_or(false);
                }
            }
            _ => {}
        }
        Some(e)
    }

    /// happypaint's JSON for this effect: the common fields and its kind's.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        let mut put = |k: &str, v: Value| {
            m.insert(k.into(), v);
        };
        put("kind", json!(self.kind.as_str()));
        put("enabled", json!(self.enabled));
        put("blendMode", json!(self.blend_mode.as_str()));
        put("opacity", json!(self.opacity));
        put("color", json!(hex(self.color)));
        let lit = |put: &mut dyn FnMut(&str, Value)| {
            put("angle", json!(self.angle));
            put("useGlobalLight", json!(self.use_global_light));
        };
        match self.kind {
            EffectKind::DropShadow | EffectKind::InnerShadow => {
                lit(&mut put);
                put("distance", json!(self.distance));
                put("spread", json!(self.spread));
                put("size", json!(self.size));
            }
            EffectKind::OuterGlow | EffectKind::InnerGlow => {
                put("spread", json!(self.spread));
                put("size", json!(self.size));
            }
            EffectKind::Stroke => {
                put("size", json!(self.size));
                put("position", json!(self.position));
            }
            EffectKind::ColorOverlay => {}
            EffectKind::GradientOverlay => {
                put("stops", Value::Array(self.stops.clone()));
                put("shape", json!(self.shape));
                put("angle", json!(self.angle));
                put("scale", json!(self.scale));
                put("reverse", json!(self.reverse));
            }
            EffectKind::PatternOverlay => {
                put("colorB", json!(hex(self.color_b)));
                put("pattern", json!(self.pattern));
                put("scale", json!(self.scale));
            }
            EffectKind::Satin => {
                put("angle", json!(self.angle));
                put("distance", json!(self.distance));
                put("size", json!(self.size));
                put("contour", json!(self.contour.as_str()));
                put("invert", json!(self.invert));
            }
            EffectKind::Bevel => {
                let b = &self.bevel;
                lit(&mut put);
                put("style", json!(b.style));
                put("depth", json!(b.depth));
                put("direction", json!(if b.up { "up" } else { "down" }));
                put("size", json!(self.size));
                put("soften", json!(b.soften));
                put("altitude", json!(b.altitude));
                put("highlightColor", json!(hex(b.highlight_color)));
                put("highlightOpacity", json!(b.highlight_opacity));
                put("highlightMode", json!(b.highlight_mode.as_str()));
                put("shadowColor", json!(hex(b.shadow_color)));
                put("shadowOpacity", json!(b.shadow_opacity));
                put("shadowMode", json!(b.shadow_mode.as_str()));
                put("contour", json!({"on": b.contour_on, "shape": b.contour.as_str()}));
                put(
                    "texture",
                    json!({"on": b.texture_on, "pattern": b.texture_pattern, "scale": b.texture_scale, "depth": b.texture_depth, "invert": b.texture_invert}),
                );
            }
        }
        Value::Object(m)
    }

    /// How far beyond the layer's pixels it draws, in px (`effectReach`).
    pub fn reach(&self) -> f32 {
        if !self.active() {
            return 0.0;
        }
        match self.kind {
            EffectKind::DropShadow | EffectKind::InnerShadow | EffectKind::Satin => self.distance + self.size,
            EffectKind::Bevel => self.size + self.bevel.soften + 1.0,
            EffectKind::OuterGlow | EffectKind::InnerGlow | EffectKind::Stroke => self.size,
            _ => 0.0,
        }
    }
}

/// A shadow's offset for a light at `angle` degrees and `distance` px:
/// away from the light, y down (`shadowOffset`).
pub fn shadow_offset(angle: f32, distance: f32) -> [f32; 2] {
    let a = angle.to_radians();
    [-a.cos() * distance, a.sin() * distance]
}

impl LayerStyles {
    /// Read happypaint's JSON (`sanitizeStyles`): unknown kinds dropped, at
    /// most four of a repeatable kind and one of the rest, first kept.
    pub fn from_json(v: &Value) -> Option<Self> {
        let fill_opacity = v.get("fillOpacity").and_then(Value::as_f64).map_or(1.0, |x| x as f32).clamp(0.0, 1.0);
        let mut effects: Vec<Effect> = Vec::new();
        for e in v.get("effects").and_then(Value::as_array).into_iter().flatten().filter_map(Effect::from_json) {
            let n = effects.iter().filter(|x| x.kind == e.kind).count();
            if n < if e.kind.repeats() { MAX_INSTANCES } else { 1 } {
                effects.push(e);
            }
        }
        Some(Self { fill_opacity, effects })
    }

    pub fn to_json(&self) -> Value {
        json!({"fillOpacity": self.fill_opacity, "effects": self.effects.iter().map(Effect::to_json).collect::<Vec<_>>()})
    }

    /// Whether the styles change how the node draws (`hasActiveStyles`).
    pub fn is_active(&self) -> bool {
        self.fill_opacity < 1.0 || self.effects.iter().any(Effect::active)
    }

    /// The furthest any effect reaches, whole px (`effectReach`).
    pub fn reach(&self) -> i32 {
        self.effects.iter().map(Effect::reach).fold(0.0, f32::max).ceil() as i32
    }

    /// The light of the first linked shadow or bevel, and the first linked
    /// bevel's altitude (`globalLightOf`); the defaults where there is none.
    pub fn global_light(&self) -> (f32, f32) {
        let linked = |e: &&Effect| e.kind.has_light() && e.use_global_light;
        let angle = self.effects.iter().find(linked).map_or(DEFAULT_LIGHT.0, |e| e.angle);
        let alt = self.effects.iter().filter(linked).find(|e| e.kind == EffectKind::Bevel).map_or(DEFAULT_LIGHT.1, |e| e.bevel.altitude);
        (angle, alt)
    }

    /// These styles under Global Light `(angle, altitude)`: every linked
    /// effect takes it (`withGlobalLight`); `None` if nothing is linked.
    pub fn with_global_light(&self, (angle, altitude): (f32, f32)) -> Option<Self> {
        let mut out = self.clone();
        let mut any = false;
        for e in out.effects.iter_mut().filter(|e| e.kind.has_light() && e.use_global_light) {
            e.angle = wrap_angle(angle);
            if e.kind == EffectKind::Bevel {
                e.bevel.altitude = altitude.clamp(0.0, 90.0);
            }
            any = true;
        }
        any.then_some(out)
    }
}

/// The built-in styles (happypaint `BUILTIN_STYLES`): name and styles.
pub fn builtin_presets() -> Vec<(&'static str, LayerStyles)> {
    let rgb = |s: &str| parse_hex(s).unwrap_or([0.0; 3]);
    let fx = |kind: EffectKind, f: &dyn Fn(&mut Effect)| {
        let mut e = default_effect(kind);
        f(&mut e);
        e
    };
    let styles = |effects: Vec<Effect>| LayerStyles { fill_opacity: 1.0, effects };
    vec![
        (
            "Sticker",
            styles(vec![
                fx(EffectKind::Stroke, &|e| (e.color, e.size) = (rgb("#ffffff"), 6.0)),
                fx(EffectKind::Stroke, &|e| (e.color, e.size) = (rgb("#1a1a1a"), 10.0)),
                fx(EffectKind::DropShadow, &|e| (e.opacity, e.distance, e.size) = (0.35, 6.0, 10.0)),
            ]),
        ),
        ("Ink Outline", styles(vec![fx(EffectKind::Stroke, &|e| (e.color, e.size) = (rgb("#111111"), 3.0))])),
        ("Soft Shadow", styles(vec![fx(EffectKind::DropShadow, &|e| (e.opacity, e.distance, e.size) = (0.45, 10.0, 22.0))])),
        (
            "Neon",
            styles(vec![
                fx(EffectKind::InnerGlow, &|e| (e.color, e.opacity, e.size) = ([1.0; 3], 0.8, 4.0)),
                // Normal, not Screen: a Screen glow vanishes on a white canvas.
                fx(EffectKind::OuterGlow, &|e| {
                    (e.color, e.opacity, e.size, e.spread, e.blend_mode) = (rgb("#29e6ff"), 0.9, 24.0, 0.1, BlendMode::Normal)
                }),
            ]),
        ),
        (
            "Embossed",
            styles(vec![
                fx(EffectKind::Bevel, &|e| (e.size, e.bevel.depth, e.bevel.soften) = (8.0, 1.5, 1.0)),
                fx(EffectKind::DropShadow, &|e| (e.opacity, e.distance, e.size) = (0.4, 3.0, 5.0)),
            ]),
        ),
        (
            "Gold",
            styles(vec![
                fx(EffectKind::Bevel, &|e| {
                    (e.size, e.bevel.depth, e.bevel.highlight_color, e.bevel.shadow_color) = (10.0, 2.0, rgb("#fff6d0"), rgb("#5a3a00"))
                }),
                fx(EffectKind::GradientOverlay, &|e| {
                    e.stops = vec![
                        json!({"pos": 0, "color": [0.55, 0.38, 0.08], "opacity": 1}),
                        json!({"pos": 0.5, "color": [1, 0.85, 0.4], "opacity": 1}),
                        json!({"pos": 1, "color": [0.62, 0.45, 0.12], "opacity": 1}),
                    ]
                }),
            ]),
        ),
    ]
}

impl crate::Layer {
    /// The layer's styles, if it has any.
    pub fn styles(&self) -> Option<LayerStyles> {
        self.extra.get(STYLES_KEY).and_then(LayerStyles::from_json)
    }

    /// Set the styles; `None` drops the key.
    pub fn set_styles(&mut self, s: Option<LayerStyles>) {
        match s {
            Some(s) => self.extra.insert(STYLES_KEY.into(), s.to_json()),
            None => self.extra.remove(STYLES_KEY),
        };
    }
}

impl crate::Group {
    /// The group's styles, if it has any.
    pub fn styles(&self) -> Option<LayerStyles> {
        self.extra.get(STYLES_KEY).and_then(LayerStyles::from_json)
    }

    /// Set the styles; `None` drops the key.
    pub fn set_styles(&mut self, s: Option<LayerStyles>) {
        match s {
            Some(s) => self.extra.insert(STYLES_KEY.into(), s.to_json()),
            None => self.extra.remove(STYLES_KEY),
        };
    }
}

/// A saved style (happypaint `StylePreset`): the user's own, kept in the
/// app's settings and shared across drawings.
#[derive(Clone, Debug, PartialEq)]
pub struct StylePreset {
    pub id: String,
    pub name: String,
    pub styles: LayerStyles,
}

/// The user's presets from their stored JSON (`parseStylePresets`): bad
/// JSON gives none; entries without a string id and name, with an id seen
/// before, or without an effects list are dropped.
pub fn parse_style_presets(raw: Option<&str>) -> Vec<StylePreset> {
    let Some(Value::Array(list)) = raw.and_then(|r| serde_json::from_str::<Value>(r).ok()) else {
        return Vec::new();
    };
    let mut out: Vec<StylePreset> = Vec::new();
    for r in &list {
        let (Some(id), Some(name)) = (r.get("id").and_then(Value::as_str), r.get("name").and_then(Value::as_str)) else {
            continue;
        };
        if out.iter().any(|p| p.id == id) {
            continue;
        }
        let Some(styles) = r.get("styles").filter(|s| s.get("effects").is_some_and(Value::is_array)).and_then(LayerStyles::from_json)
        else {
            continue;
        };
        out.push(StylePreset { id: id.into(), name: name.into(), styles });
    }
    out
}

/// The JSON [`parse_style_presets`] reads back.
pub fn style_presets_json(presets: &[StylePreset]) -> String {
    let v: Vec<Value> = presets.iter().map(|p| json!({"id": p.id, "name": p.name, "styles": p.styles.to_json()})).collect();
    Value::Array(v).to_string()
}

/// A new preset's name: `name` trimmed, or "Style N" after the `count`
/// already saved (`saveStylePreset`).
pub fn preset_name(name: &str, count: usize) -> String {
    let n = name.trim();
    if n.is_empty() { format!("Style {}", count + 1) } else { n.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(kind: EffectKind, f: impl FnOnce(&mut Effect)) -> Effect {
        let mut e = default_effect(kind);
        f(&mut e);
        e
    }

    // happypaint layer-styles.test.ts.
    #[test]
    fn reach_is_the_furthest_active_effect() {
        let s = |effects| LayerStyles { fill_opacity: 1.0, effects };
        let stroke = with(EffectKind::Stroke, |e| e.size = 6.0);
        let shadow = with(EffectKind::DropShadow, |e| (e.distance, e.size) = (10.0, 20.0));
        assert_eq!(s(vec![stroke, shadow]).reach(), 30);
        assert_eq!(s(vec![default_effect(EffectKind::ColorOverlay)]).reach(), 0);
        let bevel = with(EffectKind::Bevel, |e| (e.size, e.bevel.soften) = (12.0, 3.0));
        assert_eq!(s(vec![bevel]).reach(), 16);
        assert_eq!(s(vec![default_effect(EffectKind::Satin)]).reach(), 25);
        let a = with(EffectKind::Stroke, |e| e.size = 3.0);
        let b = with(EffectKind::Stroke, |e| e.size = 40.0);
        assert_eq!(s(vec![a, b]).reach(), 40);
        let off = with(EffectKind::Stroke, |e| (e.size, e.enabled) = (90.0, false));
        assert_eq!(s(vec![off]).reach(), 0, "disabled effects reach nowhere");
    }

    #[test]
    fn shadow_offsets_point_away_from_the_light() {
        let o = shadow_offset(120.0, 10.0);
        assert!((o[0] - 5.0).abs() < 1e-5 && (o[1] - 8.660254).abs() < 1e-5, "{o:?}");
        let o = shadow_offset(90.0, 4.0);
        assert!(o[0].abs() < 1e-5 && (o[1] - 4.0).abs() < 1e-5);
    }

    #[test]
    fn the_sanitizer_clamps_and_reads_legacy_files() {
        let v = json!({"fillOpacity": 0.6, "effects": [
            {"kind": "bevel", "depth": 40, "size": 900, "soften": -3, "altitude": 120, "angle": -30},
            {"kind": "drop-shadow", "distance": 9},
            {"kind": "nonsense"},
        ]});
        let s = LayerStyles::from_json(&v).unwrap();
        assert_eq!(s.fill_opacity, 0.6);
        assert_eq!(s.effects.len(), 2, "unknown kinds dropped");
        let b = &s.effects[0];
        assert_eq!((b.bevel.depth, b.size, b.bevel.soften, b.bevel.altitude, b.angle), (10.0, 250.0, 0.0, 90.0, 330.0));
        // Saved before blend modes: Normal, as it drew; unlinked from the light.
        assert_eq!((b.blend_mode, b.bevel.highlight_mode, b.use_global_light), (BlendMode::Normal, BlendMode::Normal, false));
        assert_eq!(s.effects[1].blend_mode, BlendMode::Normal);
    }

    #[test]
    fn new_effects_take_their_kinds_modes() {
        assert_eq!(default_effect(EffectKind::DropShadow).blend_mode, BlendMode::Multiply);
        assert_eq!(default_effect(EffectKind::OuterGlow).blend_mode, BlendMode::Screen);
        assert_eq!(default_effect(EffectKind::Stroke).blend_mode, BlendMode::Normal);
        assert_eq!(default_effect(EffectKind::Bevel).bevel.shadow_mode, BlendMode::Multiply);
    }

    #[test]
    fn repeatable_kinds_cap_at_four_and_the_rest_at_one() {
        let many: Vec<Value> = (0..6).map(|_| json!({"kind": "drop-shadow"})).chain((0..2).map(|_| json!({"kind": "satin"}))).collect();
        let s = LayerStyles::from_json(&json!({"effects": many})).unwrap();
        assert_eq!(s.effects.iter().filter(|e| e.kind == EffectKind::DropShadow).count(), 4);
        assert_eq!(s.effects.iter().filter(|e| e.kind == EffectKind::Satin).count(), 1);
        let multi: Vec<_> = EffectKind::ALL.into_iter().filter(|k| k.repeats()).map(EffectKind::as_str).collect();
        assert_eq!(multi, ["stroke", "inner-shadow", "color-overlay", "gradient-overlay", "drop-shadow"]);
    }

    #[test]
    fn contours_stay_in_range() {
        assert_eq!(Contour::Cone.at(0.5), 1.0);
        assert_eq!(Contour::ConeInverted.at(0.0), 1.0);
        for c in Contour::ALL {
            for i in 0..=20 {
                let v = c.at(i as f32 / 20.0);
                assert!((-1e-6..=1.0 + 1e-6).contains(&v), "{c:?} {v}");
            }
        }
    }

    #[test]
    fn global_light_follows_the_first_linked_effect() {
        assert_eq!(LayerStyles::default().global_light(), (120.0, 30.0));
        let own = with(EffectKind::DropShadow, |e| (e.angle, e.use_global_light) = (10.0, false));
        let linked = with(EffectKind::InnerShadow, |e| e.angle = 60.0);
        let bevel = with(EffectKind::Bevel, |e| (e.angle, e.bevel.altitude) = (60.0, 45.0));
        let s = LayerStyles { fill_opacity: 1.0, effects: vec![own, linked, bevel] };
        assert_eq!(s.global_light(), (60.0, 45.0));
        let lit = s.with_global_light((200.0, 50.0)).unwrap();
        assert_eq!((lit.effects[0].angle, lit.effects[1].angle, lit.effects[2].bevel.altitude), (10.0, 200.0, 50.0));
        let strokes = LayerStyles { fill_opacity: 1.0, effects: vec![default_effect(EffectKind::Stroke)] };
        assert!(strokes.with_global_light((10.0, 10.0)).is_none());
    }

    // happypaint style-presets.test.ts: stored presets are read defensively.
    #[test]
    fn stored_presets_are_read_defensively() {
        assert!(parse_style_presets(None).is_empty());
        assert!(parse_style_presets(Some("not json")).is_empty());
        let ok = json!({"id": "a", "name": "Mine", "styles": {"fillOpacity": 1, "effects": [{"kind": "stroke", "size": 4}]}});
        let raw = json!([ok, {"id": "a", "name": "dup", "styles": ok["styles"]}, {"id": "b"}, 42]).to_string();
        let got = parse_style_presets(Some(&raw));
        assert_eq!(got.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a"]);
        assert_eq!(got[0].styles.effects[0].kind, EffectKind::Stroke);
        // Missing fields come from the defaults.
        assert_eq!(got[0].styles.effects[0].color, default_effect(EffectKind::Stroke).color);
        assert_eq!(parse_style_presets(Some(&style_presets_json(&got))), got);
        assert_eq!(preset_name("  Gilt ", 3), "Gilt");
        assert_eq!(preset_name(" ", 3), "Style 4");
    }

    // happypaint style-presets.test.ts: the built-ins pass the sanitizer unchanged.
    #[test]
    fn the_built_in_presets_are_already_clean() {
        let presets = builtin_presets();
        assert!(presets.len() >= 5);
        for (name, s) in presets {
            assert_eq!(LayerStyles::from_json(&s.to_json()).as_ref(), Some(&s), "{name}");
        }
    }

    #[test]
    fn json_round_trips_through_the_layer() {
        let mut l = crate::Layer::new("a");
        let s = LayerStyles {
            fill_opacity: 0.5,
            effects: vec![
                with(EffectKind::Stroke, |e| (e.size, e.position) = (4.0, "center".into())),
                with(EffectKind::Bevel, |e| e.bevel.contour_on = true),
                default_effect(EffectKind::GradientOverlay),
            ],
        };
        l.set_styles(Some(s.clone()));
        assert_eq!(l.styles(), Some(s));
        assert_eq!(l.extra[STYLES_KEY]["effects"][0]["kind"], "stroke");
        l.set_styles(None);
        assert!(!l.extra.contains_key(STYLES_KEY));
    }
}
