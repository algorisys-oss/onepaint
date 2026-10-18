//! happypaint's flat `AdjustmentParams` bag (AR:142-330), camelCase JSON.
//! Unknown fields are kept in `extra` so files round-trip.

use onepaint_stroke::CurvePoint;
use serde::{Deserialize, Serialize};

/// A gradient-map stop: position along the tonal axis, straight sRGB colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stop {
    pub pos: f32,
    pub color: [f32; 3],
}

/// One Selective Colour+ range (SC `ColorSample`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorSample {
    pub target: [f32; 3],
    #[serde(default)]
    pub reach: f32,
    #[serde(default)]
    pub falloff: f32,
    #[serde(default)]
    pub dh: f32,
    #[serde(default)]
    pub ds: f32,
    #[serde(default)]
    pub dv: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// A Field Blur pin: doc fractions and a blur in doc px.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlurPin {
    pub x: f32,
    pub y: f32,
    pub blur: f32,
}

/// A gradient-fill stop (happypaint `ResolvedStop`): straight sRGB, opacity, optional midpoint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FillStop {
    pub pos: f32,
    pub color: [f32; 3],
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid: Option<f32>,
}

fn one() -> f32 {
    1.0
}

macro_rules! params {
    ($( $(#[$m:meta])* $f:ident : $t:ty ),* $(,)?) => {
        /// happypaint's flat `AdjustmentParams` bag (camelCase JSON); unknown fields are kept.
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub struct Params {
            $(
                $(#[$m])*
                #[serde(default, skip_serializing_if = "Option::is_none")]
                pub $f: Option<$t>,
            )*
            #[serde(flatten)]
            pub extra: serde_json::Map<String, serde_json::Value>,
        }
    };
}

params! {
    brightness: f32,
    contrast: f32,
    hue: f32,
    saturation: f32,
    amount: f32,
    threshold: f32,
    levels: f32,
    in_black: f32,
    in_white: f32,
    gamma: f32,
    out_black: f32,
    out_white: f32,
    curve: Vec<CurvePoint>,
    #[serde(rename = "curveR")]
    curve_r: Vec<CurvePoint>,
    #[serde(rename = "curveG")]
    curve_g: Vec<CurvePoint>,
    #[serde(rename = "curveB")]
    curve_b: Vec<CurvePoint>,
    curve_mode: String,
    cyan_red: f32,
    magenta_green: f32,
    yellow_blue: f32,
    // exposure: stops, offset (−0.5..0.5), gamma (0.01..9.99)
    exposure: f32,
    exp_offset: f32,
    exp_gamma: f32,
    // vibrance (both −1..1)
    vibrance: f32,
    vib_saturation: f32,
    // photo-filter: straight sRGB 0..1, density 0..1
    filter_color: [f32; 3],
    filter_density: f32,
    preserve_lum: bool,
    // channel-mixer: rows R, G, B and the grey (K) row; C is the constant (−2..2)
    #[serde(rename = "mixRR")]
    mix_rr: f32,
    #[serde(rename = "mixRG")]
    mix_rg: f32,
    #[serde(rename = "mixRB")]
    mix_rb: f32,
    #[serde(rename = "mixRC")]
    mix_rc: f32,
    #[serde(rename = "mixGR")]
    mix_gr: f32,
    #[serde(rename = "mixGG")]
    mix_gg: f32,
    #[serde(rename = "mixGB")]
    mix_gb: f32,
    #[serde(rename = "mixGC")]
    mix_gc: f32,
    #[serde(rename = "mixBR")]
    mix_br: f32,
    #[serde(rename = "mixBG")]
    mix_bg: f32,
    #[serde(rename = "mixBB")]
    mix_bb: f32,
    #[serde(rename = "mixBC")]
    mix_bc: f32,
    #[serde(rename = "mixKR")]
    mix_kr: f32,
    #[serde(rename = "mixKG")]
    mix_kg: f32,
    #[serde(rename = "mixKB")]
    mix_kb: f32,
    #[serde(rename = "mixKC")]
    mix_kc: f32,
    mix_mono: bool,
    gradient_stops: Vec<Stop>,
    shadow: [f32; 3],
    highlight: [f32; 3],
    radius: f32,
    angle: f32,
    distance: f32,
    mono: bool,
    // color-to-alpha · replace-color · tone ink
    target: [f32; 3],
    replace: [f32; 3],
    // selective-color
    color_samples: Vec<ColorSample>,
    show_range: bool,
    // tone
    lpi: f32,
    tone_shape: String,
    tone_density: String,
    tone_level: f32,
    tone_level_end: f32,
    tone_gradient_angle: f32,
    hard_dots: bool,
    // pixelate
    cell_size: f32,
    // centred filters (doc fractions)
    center_x: f32,
    center_y: f32,
    radial_mode: String,
    extent: f32,
    amplitude: f32,
    wavelength: f32,
    polar_mode: String,
    // blur gallery
    focus_size: f32,
    falloff_size: f32,
    aspect: f32,
    focus: f32,
    pins: Vec<BlurPin>,
    bokeh: f32,
    bokeh_range: f32,
    spin: f32,
    // render
    tint: [f32; 3],
    flare_streak: bool,
    line_mode: String,
    gen_scale: f32,
    gen_detail: f32,
    gen_density: f32,
    gen_softness: f32,
    gen_warp: f32,
    gen_color_a: [f32; 3],
    gen_color_b: [f32; 3],
    gen_seed: f32,
    lgt_start_x: f32,
    lgt_end_x: f32,
    lgt_jag: f32,
    lgt_branch: f32,
    lgt_glow: f32,
    lgt_width: f32,
    // fills
    grad_shape: String,
    fill_stops: Vec<FillStop>,
    #[serde(rename = "gradAX")]
    grad_ax: f32,
    #[serde(rename = "gradAY")]
    grad_ay: f32,
    #[serde(rename = "gradBX")]
    grad_bx: f32,
    #[serde(rename = "gradBY")]
    grad_by: f32,
    grad_reverse: bool,
    fill_color: [f32; 3],
    pattern_id: String,
    pattern_image: String,
    pattern_color_a: [f32; 3],
    pattern_color_b: [f32; 3],
    pattern_scale: f32,
    pattern_angle: f32,
    pattern_off_x: f32,
    pattern_off_y: f32,
}

impl Params {
    /// The params as a JSON object (for generic editors).
    pub fn to_json(&self) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        }
    }

    /// Params from a JSON object; malformed values fall back to the defaults.
    pub fn from_json(m: serde_json::Map<String, serde_json::Value>) -> Params {
        serde_json::from_value(serde_json::Value::Object(m)).unwrap_or_default()
    }
}
