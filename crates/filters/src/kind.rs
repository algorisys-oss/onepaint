//! The adjustment kinds, their defaults (AR `defaultParams`, AR:341-497) and
//! identity rules (AR `isIdentity`, AR:499-592).

use crate::params::{BlurPin, ColorSample, FillStop, Params, Stop};
use onepaint_stroke::CurvePoint;

macro_rules! kinds {
    ($( $v:ident = $s:literal, $label:literal, $section:literal; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Kind { $($v),* }

        impl Kind {
            pub const ALL: [Kind; kinds!(@count $($v)*)] = [$(Kind::$v),*];

            pub fn as_str(self) -> &'static str {
                match self { $(Kind::$v => $s),* }
            }

            pub fn label(self) -> &'static str {
                match self { $(Kind::$v => $label),* }
            }

            /// happypaint's menu section (`ADJUSTMENT_SECTIONS`, plus Fill).
            pub fn section(self) -> &'static str {
                match self { $(Kind::$v => $section),* }
            }
        }
    };
    (@count) => { 0 };
    (@count $h:ident $($t:ident)*) => { 1 + kinds!(@count $($t)*) };
}

kinds! {
    BrightnessContrast = "brightness-contrast", "Brightness / Contrast", "Color";
    HueSaturation = "hue-saturation", "Hue / Saturation", "Color";
    BlackWhite = "black-white", "Black & White", "Color";
    Invert = "invert", "Invert", "Color";
    Threshold = "threshold", "Threshold", "Color";
    Posterize = "posterize", "Posterize", "Color";
    Levels = "levels", "Levels", "Color";
    Curves = "curves", "Curves", "Color";
    Exposure = "exposure", "Exposure", "Color";
    Vibrance = "vibrance", "Vibrance", "Color";
    ColorBalance = "color-balance", "Color Balance", "Color";
    PhotoFilter = "photo-filter", "Photo Filter", "Color";
    ChannelMixer = "channel-mixer", "Channel Mixer", "Color";
    GradientMap = "gradient-map", "Gradient Map", "Color";
    ColorToAlpha = "color-to-alpha", "Color to Alpha", "Color";
    ReplaceColor = "replace-color", "Replace Color", "Color";
    SelectiveColor = "selective-color", "Selective Color+", "Color";
    CmykSelectiveColor = "cmyk-selective-color", "Selective Color", "Color";
    ColorLookup = "color-lookup", "Color Lookup", "Color";
    GaussianBlur = "gaussian-blur", "Gaussian Blur", "Blur";
    MotionBlur = "motion-blur", "Motion Blur", "Blur";
    RadialBlur = "radial-blur", "Radial Blur", "Blur";
    TiltShift = "tilt-shift", "Tilt-Shift", "Blur";
    IrisBlur = "iris-blur", "Iris Blur", "Blur";
    FieldBlur = "field-blur", "Field Blur", "Blur";
    SpinBlur = "spin-blur", "Spin Blur", "Blur";
    Sharpen = "sharpen", "Sharpen", "Sharpen & Noise";
    Noise = "noise", "Noise", "Sharpen & Noise";
    Twirl = "twirl", "Twirl", "Distort";
    Pinch = "pinch", "Pinch", "Distort";
    Ripple = "ripple", "Ripple", "Distort";
    PolarCoordinates = "polar-coordinates", "Polar Coordinates", "Distort";
    Mosaic = "mosaic", "Mosaic", "Pixelate";
    Crystallize = "crystallize", "Crystallize", "Pixelate";
    Halftone = "halftone", "Halftone", "Pixelate";
    Tone = "tone", "Screentone", "Pixelate";
    Emboss = "emboss", "Emboss", "Stylize";
    OilPaint = "oil-paint", "Oil Paint", "Stylize";
    Wind = "wind", "Wind", "Stylize";
    Bloom = "bloom", "Bloom", "Stylize";
    ChromaticAberration = "chromatic-aberration", "Chromatic Aberration", "Stylize";
    Glitch = "glitch", "Glitch", "Stylize";
    LineArt = "line-art", "Line Art", "Stylize";
    LightRays = "light-rays", "Light Rays", "Render";
    LensFlare = "lens-flare", "Lens Flare", "Render";
    Clouds = "clouds", "Clouds", "Render";
    Fog = "fog", "Fog", "Render";
    Smoke = "smoke", "Smoke", "Render";
    Nebula = "nebula", "Nebula", "Render";
    Starfield = "starfield", "Starfield", "Render";
    Lightning = "lightning", "Lightning", "Render";
    GradientFill = "gradient-fill", "Gradient Fill", "Fill";
    SolidFill = "solid-fill", "Solid Color Fill", "Fill";
    PatternFill = "pattern-fill", "Pattern Fill", "Fill";
}

/// Menu sections in display order.
pub const SECTIONS: [&str; 8] = ["Color", "Blur", "Sharpen & Noise", "Distort", "Pixelate", "Stylize", "Render", "Fill"];

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// Fill layers: generated pictures, never filters (AR `isFillKind`).
    pub fn is_fill(self) -> bool {
        matches!(self, Kind::GradientFill | Kind::SolidFill | Kind::PatternFill)
    }

    /// Kinds that render onto a fresh layer (PE `NEW_LAYER_KINDS`, PE:8125).
    pub fn is_generator(self) -> bool {
        matches!(
            self,
            Kind::LineArt | Kind::Clouds | Kind::Fog | Kind::Smoke | Kind::Nebula | Kind::Starfield | Kind::Lightning | Kind::LensFlare
        )
    }

    /// Per-pixel colour and fill kinds: usable as live adjustment layers per tile.
    pub fn is_color(self) -> bool {
        matches!(
            self,
            Kind::BrightnessContrast
                | Kind::HueSaturation
                | Kind::BlackWhite
                | Kind::Invert
                | Kind::Threshold
                | Kind::Posterize
                | Kind::Levels
                | Kind::Curves
                | Kind::ColorBalance
                | Kind::Exposure
                | Kind::Vibrance
                | Kind::PhotoFilter
                | Kind::ChannelMixer
                | Kind::GradientMap
                | Kind::Noise
                | Kind::ColorToAlpha
                | Kind::ReplaceColor
                | Kind::SelectiveColor
                | Kind::CmykSelectiveColor
                | Kind::ColorLookup
                | Kind::Tone
        ) || self.is_fill()
    }

    /// `Prepared::color_px` can produce the pixel on its own (colour, fill and
    /// the per-pixel generators).
    pub fn is_pixelwise(self) -> bool {
        self.is_color() || matches!(self, Kind::Clouds | Kind::Fog | Kind::Smoke | Kind::Nebula | Kind::Starfield | Kind::LensFlare)
    }

    /// Kinds the menu adds as a layer rather than running as a filter (fills and
    /// screentone; AR `isFillKind`, `adjustmentSectionOf('tone')`).
    pub fn is_layer_only(self) -> bool {
        self.is_fill() || self == Kind::Tone
    }
}

fn generator(scale: f32, detail: f32, density: f32, soft: f32, warp: f32, a: [f32; 3], b: [f32; 3]) -> Params {
    Params {
        gen_scale: Some(scale),
        gen_detail: Some(detail),
        gen_density: Some(density),
        gen_softness: Some(soft),
        gen_warp: Some(warp),
        gen_color_a: Some(a),
        gen_color_b: Some(b),
        gen_seed: Some(1.0),
        ..Default::default()
    }
}

fn centred(x: f32, y: f32) -> Params {
    Params { center_x: Some(x), center_y: Some(y), ..Default::default() }
}

/// happypaint `defaultParams` (AR:341-497).
pub fn default_params(k: Kind) -> Params {
    let mut p = Params::default();
    match k {
        Kind::BrightnessContrast => (p.brightness, p.contrast) = (Some(0.0), Some(0.0)),
        Kind::HueSaturation => (p.hue, p.saturation, p.brightness) = (Some(0.0), Some(0.0), Some(0.0)),
        Kind::BlackWhite => {
            p.amount = Some(1.0);
            let [r, y, g, c, b, m] = BW_DEFAULT_WEIGHTS;
            (p.bw_reds, p.bw_yellows, p.bw_greens, p.bw_cyans, p.bw_blues, p.bw_magentas) =
                (Some(r), Some(y), Some(g), Some(c), Some(b), Some(m));
            (p.bw_tint, p.bw_tint_color) = (Some(false), Some(BW_DEFAULT_TINT));
        }
        Kind::Invert => {}
        Kind::Threshold => p.threshold = Some(0.5),
        Kind::Posterize => p.levels = Some(4.0),
        Kind::Levels => {
            (p.in_black, p.in_white, p.gamma, p.out_black, p.out_white) = (Some(0.0), Some(1.0), Some(1.0), Some(0.0), Some(1.0))
        }
        Kind::Curves => {
            p.curve = Some(vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)]);
            p.curve_mode = Some("smooth".into());
        }
        Kind::ColorBalance => {
            (p.cyan_red, p.magenta_green, p.yellow_blue) = (Some(0.0), Some(0.0), Some(0.0));
            p.preserve_lum = Some(true);
        }
        Kind::CmykSelectiveColor => (p.sc_cmyk, p.sc_method) = (Some(vec![0.0; 4 * SC_RANGES.len()]), Some("relative".into())),
        // No table: a no-op until one is loaded.
        Kind::ColorLookup => {}
        Kind::Exposure => (p.exposure, p.exp_offset, p.exp_gamma) = (Some(0.0), Some(0.0), Some(1.0)),
        Kind::Vibrance => (p.vibrance, p.vib_saturation) = (Some(0.0), Some(0.0)),
        // Photoshop's default: Warming Filter (85), 25% density, luminosity kept.
        Kind::PhotoFilter => {
            (p.filter_color, p.filter_density, p.preserve_lum) = (Some([236.0 / 255.0, 138.0 / 255.0, 0.0]), Some(0.25), Some(true))
        }
        Kind::ChannelMixer => {
            let rows = channel_mixer_identity();
            (p.mix_rr, p.mix_rg, p.mix_rb, p.mix_rc) = (Some(rows[0][0]), Some(rows[0][1]), Some(rows[0][2]), Some(rows[0][3]));
            (p.mix_gr, p.mix_gg, p.mix_gb, p.mix_gc) = (Some(rows[1][0]), Some(rows[1][1]), Some(rows[1][2]), Some(rows[1][3]));
            (p.mix_br, p.mix_bg, p.mix_bb, p.mix_bc) = (Some(rows[2][0]), Some(rows[2][1]), Some(rows[2][2]), Some(rows[2][3]));
            p.mix_mono = Some(false);
            (p.mix_kr, p.mix_kg, p.mix_kb, p.mix_kc) = (Some(0.4), Some(0.4), Some(0.2), Some(0.0));
        }
        Kind::GradientMap => p.gradient_stops = Some(vec![Stop { pos: 0.0, color: [0.0; 3] }, Stop { pos: 1.0, color: [1.0; 3] }]),
        Kind::ColorToAlpha => (p.target, p.threshold) = (Some([1.0; 3]), Some(0.0)),
        Kind::ReplaceColor => (p.target, p.replace, p.threshold) = (Some([1.0; 3]), Some([0.2, 0.45, 1.0]), Some(0.3)),
        Kind::SelectiveColor => {
            p.color_samples =
                Some(vec![ColorSample { target: [1.0, 0.0, 0.0], reach: 0.25, falloff: 0.15, dh: 0.0, ds: 0.0, dv: 0.0, enabled: None }]);
            p.show_range = Some(false);
        }
        Kind::Noise => (p.amount, p.mono) = (Some(0.0), Some(true)),
        Kind::GaussianBlur => p.radius = Some(0.0),
        Kind::MotionBlur => (p.angle, p.distance) = (Some(0.0), Some(0.0)),
        Kind::Sharpen => p.amount = Some(0.0),
        Kind::Bloom => (p.threshold, p.amount, p.radius) = (Some(0.6), Some(0.5), Some(15.0)),
        Kind::ChromaticAberration => p.amount = Some(0.3),
        Kind::Halftone => (p.cell_size, p.angle, p.mono) = (Some(6.0), Some(15.0), Some(true)),
        Kind::Tone => {
            p.lpi = Some(60.0);
            p.angle = Some(15.0);
            p.tone_shape = Some("dot".into());
            p.tone_density = Some("flat".into());
            p.tone_level = Some(0.5);
            p.hard_dots = Some(false);
        }
        Kind::Glitch => p.amount = Some(0.5),
        Kind::RadialBlur => {
            p = centred(0.5, 0.5);
            (p.radial_mode, p.amount) = (Some("zoom".into()), Some(0.3));
        }
        Kind::Twirl => {
            p = centred(0.5, 0.5);
            (p.angle, p.extent) = (Some(90.0), Some(0.8));
        }
        Kind::Pinch => {
            p = centred(0.5, 0.5);
            (p.amount, p.extent) = (Some(0.5), Some(0.8));
        }
        Kind::Ripple => (p.amplitude, p.wavelength, p.angle) = (Some(8.0), Some(60.0), Some(0.0)),
        Kind::PolarCoordinates => {
            p = centred(0.5, 0.5);
            p.polar_mode = Some("to-polar".into());
        }
        Kind::Mosaic => p.cell_size = Some(16.0),
        Kind::Crystallize => p.cell_size = Some(20.0),
        Kind::Emboss => (p.angle, p.distance, p.amount, p.mono) = (Some(135.0), Some(3.0), Some(0.5), Some(true)),
        Kind::OilPaint => (p.radius, p.amount) = (Some(8.0), Some(0.5)),
        Kind::Wind => (p.angle, p.distance, p.amount) = (Some(0.0), Some(40.0), Some(0.6)),
        Kind::TiltShift => {
            p = centred(0.5, 0.5);
            (p.angle, p.focus_size, p.falloff_size, p.radius) = (Some(0.0), Some(0.1), Some(0.2), Some(20.0));
        }
        Kind::IrisBlur => {
            p = centred(0.5, 0.5);
            (p.extent, p.aspect, p.angle, p.focus, p.radius) = (Some(0.6), Some(0.7), Some(0.0), Some(0.5), Some(20.0));
        }
        Kind::SpinBlur => {
            p = centred(0.5, 0.5);
            (p.extent, p.aspect, p.angle, p.focus, p.spin) = (Some(0.4), Some(1.0), Some(0.0), Some(0.8), Some(30.0));
        }
        Kind::FieldBlur => p.pins = Some(vec![BlurPin { x: 0.5, y: 0.75, blur: 0.0 }, BlurPin { x: 0.5, y: 0.2, blur: 20.0 }]),
        Kind::LightRays => {
            p = centred(0.5, 0.3);
            (p.threshold, p.amount, p.extent, p.tint) = (Some(0.6), Some(0.6), Some(0.5), Some([1.0, 0.96, 0.88]));
        }
        Kind::LensFlare => {
            p = centred(0.3, 0.3);
            (p.amount, p.extent, p.tint, p.flare_streak) = (Some(0.8), Some(0.5), Some([1.0, 0.9, 0.75]), Some(true));
        }
        Kind::SolidFill => p.fill_color = Some([0.0; 3]),
        Kind::PatternFill => {
            p.pattern_id = Some("checker".into());
            p.pattern_color_a = Some([0.0; 3]);
            p.pattern_color_b = Some([1.0; 3]);
            p.pattern_scale = Some(1.0);
            p.pattern_angle = Some(0.0);
            p.pattern_off_x = Some(0.5);
            p.pattern_off_y = Some(0.5);
        }
        Kind::GradientFill => {
            p.grad_shape = Some("linear".into());
            p.fill_stops = Some(vec![
                FillStop { pos: 0.0, color: [0.0; 3], opacity: 1.0, mid: None },
                FillStop { pos: 1.0, color: [1.0; 3], opacity: 1.0, mid: None },
            ]);
            (p.grad_ax, p.grad_ay, p.grad_bx, p.grad_by, p.grad_reverse) = (Some(0.5), Some(0.0), Some(0.5), Some(1.0), Some(false));
        }
        Kind::LineArt => {
            (p.threshold, p.amount, p.radius, p.mono, p.line_mode) = (Some(0.5), Some(0.5), Some(3.0), Some(false), Some("xdog".into()))
        }
        Kind::Clouds => p = generator(220.0, 5.0, 0.5, 0.5, 0.25, [0.55, 0.6, 0.72], [1.0, 1.0, 1.0]),
        Kind::Fog => p = generator(380.0, 3.0, 0.6, 0.85, 0.1, [0.7, 0.72, 0.75], [0.92, 0.93, 0.95]),
        Kind::Smoke => p = generator(200.0, 6.0, 0.42, 0.45, 0.7, [0.12, 0.12, 0.14], [0.5, 0.5, 0.54]),
        Kind::Nebula => p = generator(260.0, 7.0, 0.55, 0.6, 0.6, [0.7, 0.1, 0.55], [0.15, 0.75, 0.85]),
        Kind::Starfield => p = generator(14.0, 1.0, 0.5, 0.4, 0.0, [0.0; 3], [1.0; 3]),
        Kind::Lightning => {
            p.lgt_start_x = Some(0.5);
            p.lgt_end_x = Some(0.5);
            p.lgt_jag = Some(0.5);
            p.lgt_branch = Some(0.5);
            p.lgt_glow = Some(0.5);
            p.lgt_width = Some(3.0);
            p.gen_color_b = Some([0.7, 0.85, 1.0]);
            p.gen_seed = Some(1.0);
        }
    }
    p
}

fn channel_mixer_identity() -> [[f32; 4]; 3] {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]
}

/// The mixer's rows `[r, g, b, constant]` for red, green and blue, then the
/// grey row; monochrome uses the grey row for all three (`channelMixerMatrix`).
pub fn channel_mixer_rows(p: &Params) -> [[f32; 4]; 4] {
    let o = |v: Option<f32>, d: f32| v.unwrap_or(d);
    let grey = [o(p.mix_kr, 0.4), o(p.mix_kg, 0.4), o(p.mix_kb, 0.2), o(p.mix_kc, 0.0)];
    if p.mix_mono.unwrap_or(false) {
        return [grey; 4];
    }
    [
        [o(p.mix_rr, 1.0), o(p.mix_rg, 0.0), o(p.mix_rb, 0.0), o(p.mix_rc, 0.0)],
        [o(p.mix_gr, 0.0), o(p.mix_gg, 1.0), o(p.mix_gb, 0.0), o(p.mix_gc, 0.0)],
        [o(p.mix_br, 0.0), o(p.mix_bg, 0.0), o(p.mix_bb, 1.0), o(p.mix_bc, 0.0)],
        grey,
    ]
}

fn curve_is_identity(c: &Option<Vec<CurvePoint>>) -> bool {
    c.as_ref().is_none_or(|c| c.len() < 2 || c.iter().all(|p| (p.y - p.x).abs() < 1e-3))
}

/// Field Blur's strongest pin (AR `fieldBlurMax`).
/// Selective Color's ranges, in the order of `scCmyk`'s groups of (C, M, Y, K).
pub const SC_RANGES: [&str; 9] = ["reds", "yellows", "greens", "cyans", "blues", "magentas", "whites", "neutrals", "blacks"];

/// Black & White's default hue weights, Photoshop's "Default" preset
/// (happypaint `BW_DEFAULT_WEIGHTS`): reds, yellows, greens, cyans, blues,
/// magentas. A missing weight reads as its default.
pub const BW_DEFAULT_WEIGHTS: [f32; 6] = [0.4, 0.6, 0.4, 0.6, 0.2, 0.8];

/// Black & White's default tint, a warm sepia (straight sRGB).
pub const BW_DEFAULT_TINT: [f32; 3] = [225.0 / 255.0, 211.0 / 255.0, 179.0 / 255.0];

pub fn field_blur_max(p: &Params) -> f32 {
    p.pins.as_deref().unwrap_or(&[]).iter().take(crate::MAX_BLUR_PINS).map(|q| q.blur).fold(0.0, f32::max)
}

/// The destructive preview skips identity params (AR `isIdentity`).
pub fn is_identity(k: Kind, p: &Params) -> bool {
    let z = |v: Option<f32>| v.unwrap_or(0.0) == 0.0;
    let le0 = |v: Option<f32>| v.unwrap_or(0.0) <= 0.0;
    match k {
        Kind::BrightnessContrast => z(p.brightness) && z(p.contrast),
        Kind::HueSaturation => z(p.hue) && z(p.saturation) && z(p.brightness),
        Kind::BlackWhite => p.amount.unwrap_or(1.0) <= 0.0,
        Kind::Levels => {
            z(p.in_black)
                && p.in_white.unwrap_or(1.0) == 1.0
                && p.gamma.unwrap_or(1.0) == 1.0
                && z(p.out_black)
                && p.out_white.unwrap_or(1.0) == 1.0
        }
        Kind::Curves => [&p.curve, &p.curve_r, &p.curve_g, &p.curve_b].into_iter().all(curve_is_identity),
        Kind::CmykSelectiveColor => p.sc_cmyk.as_deref().is_none_or(|v| v.iter().all(|x| *x == 0.0)),
        Kind::ColorLookup => crate::lut3d::lut_of(p.lut.as_ref()).is_none(),
        Kind::ColorBalance => [
            p.cyan_red,
            p.magenta_green,
            p.yellow_blue,
            p.cb_shadow_cr,
            p.cb_shadow_mg,
            p.cb_shadow_yb,
            p.cb_high_cr,
            p.cb_high_mg,
            p.cb_high_yb,
        ]
        .into_iter()
        .all(z),
        Kind::Exposure => z(p.exposure) && z(p.exp_offset) && p.exp_gamma.unwrap_or(1.0) == 1.0,
        Kind::Vibrance => z(p.vibrance) && z(p.vib_saturation),
        Kind::PhotoFilter => le0(p.filter_density),
        Kind::ChannelMixer => !p.mix_mono.unwrap_or(false) && channel_mixer_rows(p)[..3] == channel_mixer_identity()[..],
        Kind::SelectiveColor => {
            !p.show_range.unwrap_or(false)
                && !p
                    .color_samples
                    .as_deref()
                    .unwrap_or(&[])
                    .iter()
                    .any(|c| c.enabled != Some(false) && (c.dh != 0.0 || c.ds != 0.0 || c.dv != 0.0))
        }
        Kind::GaussianBlur | Kind::OilPaint | Kind::TiltShift | Kind::IrisBlur => le0(p.radius),
        Kind::MotionBlur => le0(p.distance),
        Kind::Sharpen | Kind::Noise | Kind::Bloom | Kind::ChromaticAberration | Kind::Glitch | Kind::LightRays => le0(p.amount),
        Kind::RadialBlur | Kind::Pinch => z(p.amount),
        Kind::Twirl => z(p.angle),
        Kind::Ripple => le0(p.amplitude),
        Kind::Mosaic | Kind::Crystallize => p.cell_size.unwrap_or(0.0) <= 1.0,
        Kind::Emboss => !p.mono.unwrap_or(true) && le0(p.amount),
        Kind::Wind => le0(p.distance) || le0(p.amount),
        Kind::FieldBlur => field_blur_max(p) <= 0.0,
        Kind::SpinBlur => le0(p.spin),
        Kind::Tone => {
            let d = p.tone_density.as_deref().unwrap_or("flat");
            let start = p.tone_level.unwrap_or(0.5);
            match d {
                "luminance" => false,
                "gradient" => start <= 0.0 && p.tone_level_end.unwrap_or(0.0) <= 0.0,
                _ => start <= 0.0,
            }
        }
        // Always remap or always generate.
        Kind::Invert
        | Kind::Threshold
        | Kind::Posterize
        | Kind::GradientMap
        | Kind::ColorToAlpha
        | Kind::ReplaceColor
        | Kind::Halftone
        | Kind::PolarCoordinates
        | Kind::LineArt
        | Kind::LensFlare
        | Kind::Clouds
        | Kind::Fog
        | Kind::Smoke
        | Kind::Nebula
        | Kind::Starfield
        | Kind::Lightning
        | Kind::GradientFill
        | Kind::SolidFill
        | Kind::PatternFill => false,
    }
}
