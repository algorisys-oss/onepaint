//! Generic panel controls for the spec-18 kinds, so the app can build their
//! panels from data. Ranges follow happypaint's `components/adjustments/adjustments.tsx`.
//! A slider shows `stored / scale`.

use crate::Kind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Slider {
        key: &'static str,
        label: &'static str,
        min: f32,
        max: f32,
        scale: f32,
        unit: &'static str,
        default: f32,
    },
    Toggle {
        key: &'static str,
        label: &'static str,
        default: bool,
    },
    Choice {
        key: &'static str,
        label: &'static str,
        options: &'static [(&'static str, &'static str)],
        default: &'static str,
    },
    /// A straight sRGB colour; the panel offers "use the current colour".
    Color {
        key: &'static str,
        label: &'static str,
        default: [f32; 3],
    },
    /// A re-roll button that bumps an integer seed.
    Seed {
        key: &'static str,
        label: &'static str,
    },
    /// A section heading (Color Balance's tone ranges).
    Heading {
        label: &'static str,
    },
}

const fn pct(key: &'static str, label: &'static str, default: f32) -> Control {
    Control::Slider { key, label, min: 0.0, max: 100.0, scale: 0.01, unit: "%", default }
}

const fn spct(key: &'static str, label: &'static str, default: f32) -> Control {
    Control::Slider { key, label, min: -100.0, max: 100.0, scale: 0.01, unit: "", default }
}

const fn px(key: &'static str, label: &'static str, min: f32, max: f32, default: f32) -> Control {
    Control::Slider { key, label, min, max, scale: 1.0, unit: " px", default }
}

const fn deg(key: &'static str, label: &'static str, min: f32, max: f32, default: f32) -> Control {
    Control::Slider { key, label, min, max, scale: 1.0, unit: "°", default }
}

const fn num(key: &'static str, label: &'static str, min: f32, max: f32, default: f32) -> Control {
    Control::Slider { key, label, min, max, scale: 1.0, unit: "", default }
}

const fn colour(key: &'static str, label: &'static str, default: [f32; 3]) -> Control {
    Control::Color { key, label, default }
}

const fn toggle(key: &'static str, label: &'static str, default: bool) -> Control {
    Control::Toggle { key, label, default }
}

/// A Black & White hue weight: −200..300 %.
const fn weight(key: &'static str, label: &'static str, default: f32) -> Control {
    Control::Slider { key, label, min: -200.0, max: 300.0, scale: 0.01, unit: "%", default }
}

const BLACK_WHITE: &[Control] = &[
    weight("bwReds", "Reds", 0.4),
    weight("bwYellows", "Yellows", 0.6),
    weight("bwGreens", "Greens", 0.4),
    weight("bwCyans", "Cyans", 0.6),
    weight("bwBlues", "Blues", 0.2),
    weight("bwMagentas", "Magentas", 0.8),
    toggle("bwTint", "Tint", false),
    colour("bwTintColor", "Tint colour", crate::BW_DEFAULT_TINT),
    pct("amount", "Amount", 1.0),
];

const COLOR_BALANCE: &[Control] = &[
    Control::Heading { label: "Shadows" },
    spct("cbShadowCR", "Cyan – Red", 0.0),
    spct("cbShadowMG", "Magenta – Green", 0.0),
    spct("cbShadowYB", "Yellow – Blue", 0.0),
    Control::Heading { label: "Midtones" },
    spct("cyanRed", "Cyan – Red", 0.0),
    spct("magentaGreen", "Magenta – Green", 0.0),
    spct("yellowBlue", "Yellow – Blue", 0.0),
    Control::Heading { label: "Highlights" },
    spct("cbHighCR", "Cyan – Red", 0.0),
    spct("cbHighMG", "Magenta – Green", 0.0),
    spct("cbHighYB", "Yellow – Blue", 0.0),
    toggle("preserveLum", "Preserve luminosity", true),
];

const CX: Control = pct("centerX", "Centre X", 0.5);
const CY: Control = pct("centerY", "Centre Y", 0.5);
const BOKEH: Control = pct("bokeh", "Light bokeh", 0.0);
const BOKEH_RANGE: Control = pct("bokehRange", "Bokeh range", 0.75);

const SHAPES: &[(&str, &str)] = &[("dot", "Dot"), ("line", "Line"), ("cross", "Cross"), ("sand", "Sand")];
const DENSITIES: &[(&str, &str)] = &[("flat", "Flat"), ("gradient", "Gradient"), ("luminance", "Luminance")];
const RADIAL: &[(&str, &str)] = &[("zoom", "Zoom"), ("spin", "Spin")];
const POLAR: &[(&str, &str)] = &[("to-polar", "Rectangular to polar"), ("to-rect", "Polar to rectangular")];
const LINE: &[(&str, &str)] = &[("xdog", "Ink (XDoG)"), ("sobel", "Edges (Sobel)")];
const GRAD_SHAPES: &[(&str, &str)] =
    &[("linear", "Linear"), ("radial", "Radial"), ("angle", "Angle"), ("reflected", "Reflected"), ("diamond", "Diamond")];
const PATTERN_IDS: &[(&str, &str)] = &crate::pattern::PATTERNS;

const GEN: &[Control] = &[
    num("genScale", "Scale", 20.0, 600.0, 220.0),
    num("genDetail", "Detail", 1.0, 8.0, 5.0),
    pct("genDensity", "Density", 0.5),
    pct("genSoftness", "Softness", 0.5),
    pct("genWarp", "Swirl", 0.25),
    colour("genColorA", "Low colour", [1.0; 3]),
    colour("genColorB", "High colour", [1.0; 3]),
    Control::Seed { key: "genSeed", label: "Re-roll" },
];

const fn mix(key: &'static str, label: &'static str, default: f32) -> Control {
    Control::Slider { key, label, min: -200.0, max: 200.0, scale: 0.01, unit: "%", default }
}

const MIXER: &[Control] = &[
    toggle("mixMono", "Monochrome", false),
    mix("mixRR", "Red ← Red", 1.0),
    mix("mixRG", "Red ← Green", 0.0),
    mix("mixRB", "Red ← Blue", 0.0),
    mix("mixRC", "Red constant", 0.0),
    mix("mixGR", "Green ← Red", 0.0),
    mix("mixGG", "Green ← Green", 1.0),
    mix("mixGB", "Green ← Blue", 0.0),
    mix("mixGC", "Green constant", 0.0),
    mix("mixBR", "Blue ← Red", 0.0),
    mix("mixBG", "Blue ← Green", 0.0),
    mix("mixBB", "Blue ← Blue", 1.0),
    mix("mixBC", "Blue constant", 0.0),
    mix("mixKR", "Grey ← Red", 0.4),
    mix("mixKG", "Grey ← Green", 0.4),
    mix("mixKB", "Grey ← Blue", 0.2),
    mix("mixKC", "Grey constant", 0.0),
];

/// Whether `c` shows for these params: the Channel Mixer shows its colour
/// rows, or only the grey row when Monochrome is on; Black & White shows
/// its tint colour while Tint is on.
pub fn visible(kind: Kind, c: &Control, p: &crate::Params) -> bool {
    match (kind, c) {
        (Kind::ChannelMixer, Control::Slider { key, .. }) => key.starts_with("mixK") == p.mix_mono.unwrap_or(false),
        (Kind::BlackWhite, Control::Color { key: "bwTintColor", .. }) => p.bw_tint.unwrap_or(false),
        _ => true,
    }
}

/// The panel for a spec-18 kind. Empty for spec-10 kinds (the app has bespoke
/// panels for those), selective colour and field blur (their lists are edited
/// by dedicated widgets).
pub fn controls(kind: Kind) -> &'static [Control] {
    match kind {
        Kind::Exposure => {
            const {
                &[
                    num("exposure", "Exposure", -20.0, 20.0, 0.0),
                    num("expOffset", "Offset", -0.5, 0.5, 0.0),
                    num("expGamma", "Gamma", 0.01, 9.99, 1.0),
                ]
            }
        }
        Kind::Vibrance => const { &[spct("vibrance", "Vibrance", 0.0), spct("vibSaturation", "Saturation", 0.0)] },
        Kind::PhotoFilter => {
            const {
                &[
                    colour("filterColor", "Colour", [236.0 / 255.0, 138.0 / 255.0, 0.0]),
                    pct("filterDensity", "Density", 0.25),
                    toggle("preserveLum", "Preserve luminosity", true),
                ]
            }
        }
        Kind::ChannelMixer => MIXER,
        Kind::BlackWhite => BLACK_WHITE,
        Kind::ColorBalance => COLOR_BALANCE,
        Kind::ColorToAlpha => const { &[colour("target", "Colour", [1.0; 3]), pct("threshold", "Threshold", 0.0)] },
        Kind::ReplaceColor => {
            const { &[colour("target", "Find", [1.0; 3]), colour("replace", "Replace with", [0.2, 0.45, 1.0]), pct("threshold", "Range", 0.3)] }
        }
        Kind::Tone => {
            const {
                &[
                    num("lpi", "Lines per inch", 5.0, 150.0, 60.0),
                    deg("angle", "Angle", 0.0, 180.0, 15.0),
                    Control::Choice { key: "toneShape", label: "Shape", options: SHAPES, default: "dot" },
                    Control::Choice { key: "toneDensity", label: "Density", options: DENSITIES, default: "flat" },
                    pct("toneLevel", "Level", 0.5),
                    pct("toneLevelEnd", "End level", 0.0),
                    deg("toneGradientAngle", "Ramp angle", 0.0, 360.0, 0.0),
                    toggle("hardDots", "Hard dots (1-bit)", false),
                    colour("target", "Ink", [0.0; 3]),
                ]
            }
        }
        Kind::RadialBlur => {
            const {
                &[
                    Control::Choice { key: "radialMode", label: "Mode", options: RADIAL, default: "zoom" },
                    pct("amount", "Amount", 0.3),
                    CX,
                    CY,
                ]
            }
        }
        Kind::TiltShift => {
            const {
                &[
                    px("radius", "Blur", 0.0, 100.0, 20.0),
                    pct("focusSize", "Focus", 0.1),
                    pct("falloffSize", "Transition", 0.2),
                    deg("angle", "Angle", -180.0, 180.0, 0.0),
                    CX,
                    CY,
                    BOKEH,
                    BOKEH_RANGE,
                ]
            }
        }
        Kind::IrisBlur => {
            const {
                &[
                    px("radius", "Blur", 0.0, 100.0, 20.0),
                    Control::Slider { key: "extent", label: "Size", min: 5.0, max: 150.0, scale: 0.01, unit: "%", default: 0.6 },
                    Control::Slider { key: "aspect", label: "Roundness", min: 5.0, max: 300.0, scale: 0.01, unit: "%", default: 0.7 },
                    Control::Slider { key: "focus", label: "Sharp core", min: 0.0, max: 95.0, scale: 0.01, unit: "%", default: 0.5 },
                    deg("angle", "Angle", -180.0, 180.0, 0.0),
                    CX,
                    CY,
                    BOKEH,
                    BOKEH_RANGE,
                ]
            }
        }
        Kind::FieldBlur => const { &[BOKEH, BOKEH_RANGE] },
        Kind::SpinBlur => {
            const {
                &[
                    deg("spin", "Spin", 0.0, 360.0, 30.0),
                    Control::Slider { key: "extent", label: "Size", min: 5.0, max: 150.0, scale: 0.01, unit: "%", default: 0.4 },
                    Control::Slider { key: "aspect", label: "Roundness", min: 5.0, max: 300.0, scale: 0.01, unit: "%", default: 1.0 },
                    Control::Slider { key: "focus", label: "Feather from", min: 0.0, max: 95.0, scale: 0.01, unit: "%", default: 0.8 },
                    deg("angle", "Angle", -180.0, 180.0, 0.0),
                    CX,
                    CY,
                ]
            }
        }
        Kind::Twirl => {
            const {
                &[
                    deg("angle", "Angle", -720.0, 720.0, 90.0),
                    Control::Slider { key: "extent", label: "Radius", min: 5.0, max: 150.0, scale: 0.01, unit: "%", default: 0.8 },
                    CX,
                    CY,
                ]
            }
        }
        Kind::Pinch => {
            const {
                &[
                    spct("amount", "Amount", 0.5),
                    Control::Slider { key: "extent", label: "Radius", min: 5.0, max: 150.0, scale: 0.01, unit: "%", default: 0.8 },
                    CX,
                    CY,
                ]
            }
        }
        Kind::Ripple => {
            const {
                &[
                    px("amplitude", "Height", 0.0, 100.0, 8.0),
                    px("wavelength", "Wavelength", 4.0, 400.0, 60.0),
                    deg("angle", "Angle", 0.0, 360.0, 0.0),
                ]
            }
        }
        Kind::PolarCoordinates => {
            const { &[Control::Choice { key: "polarMode", label: "Direction", options: POLAR, default: "to-polar" }, CX, CY] }
        }
        Kind::Mosaic => const { &[px("cellSize", "Cell size", 2.0, 200.0, 16.0)] },
        Kind::Crystallize => const { &[px("cellSize", "Cell size", 2.0, 200.0, 20.0)] },
        Kind::Halftone => {
            const { &[px("cellSize", "Cell size", 2.0, 40.0, 6.0), deg("angle", "Angle", 0.0, 360.0, 15.0), toggle("mono", "Black ink", true)] }
        }
        Kind::Emboss => {
            const {
                &[
                    deg("angle", "Light angle", 0.0, 360.0, 135.0),
                    px("distance", "Height", 1.0, 20.0, 3.0),
                    pct("amount", "Amount", 0.5),
                    toggle("mono", "Grey relief", true),
                ]
            }
        }
        Kind::OilPaint => const { &[px("radius", "Brush size", 1.0, 40.0, 8.0), pct("amount", "Sharpness", 0.5)] },
        Kind::Wind => {
            const { &[deg("angle", "Direction", 0.0, 360.0, 0.0), px("distance", "Length", 0.0, 200.0, 40.0), pct("amount", "Amount", 0.6)] }
        }
        Kind::Bloom => {
            const { &[pct("threshold", "Threshold", 0.6), pct("amount", "Intensity", 0.5), px("radius", "Radius", 1.0, 50.0, 15.0)] }
        }
        Kind::ChromaticAberration => const { &[pct("amount", "Amount", 0.3)] },
        Kind::Glitch => const { &[pct("amount", "Amount", 0.5)] },
        Kind::LineArt => {
            const {
                &[
                    Control::Choice { key: "lineMode", label: "Method", options: LINE, default: "xdog" },
                    pct("threshold", "Threshold", 0.5),
                    pct("amount", "Strength", 0.5),
                    num("radius", "Thickness", 1.0, 6.0, 3.0),
                    toggle("mono", "White lines", false),
                ]
            }
        }
        Kind::LightRays => {
            const {
                &[
                    pct("threshold", "Threshold", 0.6),
                    pct("amount", "Intensity", 0.6),
                    pct("extent", "Length", 0.5),
                    colour("tint", "Light colour", [1.0, 0.96, 0.88]),
                    CX,
                    Control::Slider { key: "centerY", label: "Centre Y", min: 0.0, max: 100.0, scale: 0.01, unit: "%", default: 0.3 },
                ]
            }
        }
        Kind::LensFlare => {
            const {
                &[
                    pct("amount", "Brightness", 0.8),
                    Control::Slider { key: "extent", label: "Size", min: 5.0, max: 150.0, scale: 0.01, unit: "%", default: 0.5 },
                    colour("tint", "Flare colour", [1.0, 0.9, 0.75]),
                    toggle("flareStreak", "Anamorphic streak", true),
                    Control::Slider { key: "centerX", label: "Centre X", min: 0.0, max: 100.0, scale: 0.01, unit: "%", default: 0.3 },
                    Control::Slider { key: "centerY", label: "Centre Y", min: 0.0, max: 100.0, scale: 0.01, unit: "%", default: 0.3 },
                ]
            }
        }
        Kind::Clouds | Kind::Fog | Kind::Smoke | Kind::Nebula | Kind::Starfield => GEN,
        Kind::Lightning => {
            const {
                &[
                    pct("lgtStartX", "Start X", 0.5),
                    pct("lgtEndX", "End X", 0.5),
                    pct("lgtJag", "Jaggedness", 0.5),
                    pct("lgtBranch", "Branches", 0.5),
                    pct("lgtGlow", "Glow", 0.5),
                    px("lgtWidth", "Width", 1.0, 12.0, 3.0),
                    colour("genColorB", "Bolt colour", [0.7, 0.85, 1.0]),
                    Control::Seed { key: "genSeed", label: "Re-roll" },
                ]
            }
        }
        Kind::SolidFill => const { &[colour("fillColor", "Colour", [0.0; 3])] },
        Kind::GradientFill => {
            const {
                &[
                    Control::Choice { key: "gradShape", label: "Shape", options: GRAD_SHAPES, default: "linear" },
                    toggle("gradReverse", "Reverse", false),
                    pct("gradAX", "Start X", 0.5),
                    pct("gradAY", "Start Y", 0.0),
                    pct("gradBX", "End X", 0.5),
                    pct("gradBY", "End Y", 1.0),
                ]
            }
        }
        Kind::PatternFill => {
            const {
                &[
                    Control::Choice { key: "patternId", label: "Pattern", options: PATTERN_IDS, default: "checker" },
                    colour("patternColorA", "Colour A", [0.0; 3]),
                    colour("patternColorB", "Colour B", [1.0; 3]),
                    Control::Slider { key: "patternScale", label: "Scale", min: 5.0, max: 800.0, scale: 0.01, unit: "%", default: 1.0 },
                    deg("patternAngle", "Angle", -180.0, 180.0, 0.0),
                    pct("patternOffX", "Offset X", 0.5),
                    pct("patternOffY", "Offset Y", 0.5),
                ]
            }
        }
        _ => const { &[] },
    }
}
