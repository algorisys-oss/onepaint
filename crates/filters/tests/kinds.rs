#![allow(clippy::unwrap_used)]
//! Spec 18: the kind list, defaults, identity rules, sections and the params
//! contract (ports of happypaint `filters.test.ts` and `tone.test.ts`).

use onepaint_filters::*;

fn p(json: &str) -> Params {
    serde_json::from_str(json).unwrap()
}

#[test]
fn all_52_kinds_parse_and_have_a_section() {
    assert_eq!(Kind::ALL.len(), 52);
    for k in Kind::ALL {
        assert_eq!(Kind::parse(k.as_str()), Some(k));
        assert!(SECTIONS.contains(&k.section()), "{k:?}");
    }
    assert_eq!(Kind::parse("pattern-fill-from-the-future"), None);
}

#[test]
fn sections_match_happypaints_menu() {
    for k in ["clouds", "fog", "smoke", "nebula", "starfield", "lightning", "light-rays", "lens-flare"] {
        assert_eq!(Kind::parse(k).unwrap().section(), "Render");
    }
    assert_eq!(Kind::LineArt.section(), "Stylize");
    assert_eq!(Kind::Tone.section(), "Pixelate");
    for k in [Kind::TiltShift, Kind::IrisBlur, Kind::FieldBlur, Kind::SpinBlur] {
        assert_eq!(k.section(), "Blur");
    }
    for k in [Kind::GradientFill, Kind::SolidFill, Kind::PatternFill] {
        assert!(k.is_fill() && k.is_layer_only() && k.is_color() && !k.is_generator());
    }
}

#[test]
fn generators_are_happypaints_new_layer_kinds() {
    let gens: Vec<_> = Kind::ALL.into_iter().filter(|k| k.is_generator()).map(|k| k.as_str()).collect();
    assert_eq!(gens, ["line-art", "lens-flare", "clouds", "fog", "smoke", "nebula", "starfield", "lightning"]);
    assert!(Kind::Clouds.is_pixelwise() && Kind::LensFlare.is_pixelwise());
    assert!(!Kind::Lightning.is_pixelwise() && !Kind::LineArt.is_pixelwise());
}

#[test]
fn new_filters_open_showing_their_effect() {
    for k in [
        Kind::RadialBlur,
        Kind::Twirl,
        Kind::Pinch,
        Kind::Ripple,
        Kind::PolarCoordinates,
        Kind::Mosaic,
        Kind::Crystallize,
        Kind::Emboss,
        Kind::OilPaint,
        Kind::Wind,
        Kind::TiltShift,
        Kind::IrisBlur,
        Kind::FieldBlur,
        Kind::SpinBlur,
        Kind::LightRays,
        Kind::LensFlare,
    ] {
        assert!(!is_identity(k, &default_params(k)), "{k:?}");
    }
}

#[test]
fn zero_settings_are_the_identity() {
    assert!(is_identity(Kind::RadialBlur, &p(r#"{"amount":0}"#)));
    assert!(is_identity(Kind::Twirl, &p(r#"{"angle":0}"#)));
    assert!(is_identity(Kind::Pinch, &p(r#"{"amount":0}"#)));
    assert!(is_identity(Kind::Ripple, &p(r#"{"amplitude":0}"#)));
    assert!(is_identity(Kind::Mosaic, &p(r#"{"cellSize":1}"#)));
    assert!(is_identity(Kind::OilPaint, &p(r#"{"radius":0}"#)));
    assert!(is_identity(Kind::Wind, &p(r#"{"distance":0,"amount":0.5}"#)));
    assert!(is_identity(Kind::Wind, &p(r#"{"distance":40,"amount":0}"#)));
    assert!(!is_identity(Kind::PolarCoordinates, &p(r#"{"polarMode":"to-rect"}"#)));
    assert!(!is_identity(Kind::Emboss, &p(r#"{"mono":true,"amount":0}"#)));
    assert!(is_identity(Kind::Emboss, &p(r#"{"mono":false,"amount":0}"#)));
    assert!(is_identity(Kind::TiltShift, &p(r#"{"radius":0}"#)));
    assert!(is_identity(Kind::FieldBlur, &p(r#"{"pins":[{"x":0.5,"y":0.5,"blur":0}]}"#)));
    assert!(is_identity(Kind::FieldBlur, &p(r#"{"pins":[]}"#)));
    assert!(is_identity(Kind::SpinBlur, &p(r#"{"spin":0}"#)));
    assert!(!is_identity(Kind::SpinBlur, &p(r#"{"spin":30}"#)));
    assert!(!is_identity(Kind::LensFlare, &p(r#"{"amount":0}"#)));
    for k in [Kind::GradientFill, Kind::SolidFill, Kind::PatternFill, Kind::Clouds, Kind::Lightning, Kind::LineArt] {
        assert!(!is_identity(k, &default_params(k)));
    }
}

#[test]
fn selective_colour_identity() {
    assert!(is_identity(Kind::SelectiveColor, &default_params(Kind::SelectiveColor)));
    let mut q = default_params(Kind::SelectiveColor);
    q.show_range = Some(true);
    assert!(!is_identity(Kind::SelectiveColor, &q));
    let q = p(r#"{"colorSamples":[{"target":[1,0,0],"reach":0.2,"falloff":0.1,"dh":10,"ds":0,"dv":0}]}"#);
    assert!(!is_identity(Kind::SelectiveColor, &q));
    let q = p(r#"{"colorSamples":[{"target":[1,0,0],"reach":0.2,"falloff":0.1,"dh":10,"ds":0,"dv":0,"enabled":false}]}"#);
    assert!(is_identity(Kind::SelectiveColor, &q));
}

#[test]
fn tone_contract() {
    let d = default_params(Kind::Tone);
    assert_eq!((d.lpi, d.angle, d.tone_shape.as_deref(), d.tone_density.as_deref()), (Some(60.0), Some(15.0), Some("dot"), Some("flat")));
    assert!(is_identity(Kind::Tone, &p(r#"{"toneDensity":"flat","toneLevel":0}"#)));
    assert!(!is_identity(Kind::Tone, &p(r#"{"toneDensity":"flat","toneLevel":0.5}"#)));
    assert!(!is_identity(Kind::Tone, &p(r#"{"toneDensity":"luminance","toneLevel":0}"#)));
    assert!(is_identity(Kind::Tone, &p(r#"{"toneDensity":"gradient","toneLevel":0,"toneLevelEnd":0}"#)));
    assert!(!is_identity(Kind::Tone, &p(r#"{"toneDensity":"gradient","toneLevel":0,"toneLevelEnd":0.8}"#)));
}

#[test]
fn defaults_match_happypaint() {
    for k in [Kind::RadialBlur, Kind::Twirl, Kind::Pinch, Kind::PolarCoordinates] {
        let d = default_params(k);
        assert_eq!((d.center_x, d.center_y), (Some(0.5), Some(0.5)));
    }
    for k in [Kind::TiltShift, Kind::IrisBlur, Kind::FieldBlur] {
        assert_eq!(default_params(k).bokeh.unwrap_or(0.0), 0.0);
    }
    let pins = default_params(Kind::FieldBlur).pins.unwrap();
    let near = pins.iter().max_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    let far = pins.iter().min_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
    assert_eq!(near.blur, 0.0);
    assert!(far.blur > 0.0);
    assert_eq!(field_blur_max(&p(r#"{"pins":[{"x":0,"y":0,"blur":5},{"x":1,"y":1,"blur":40}]}"#)), 40.0);
    assert_eq!(field_blur_max(&Params::default()), 0.0);
    let g = default_params(Kind::GradientFill);
    assert_eq!([g.grad_ax, g.grad_ay, g.grad_bx, g.grad_by], [Some(0.5), Some(0.0), Some(0.5), Some(1.0)]);
    assert_eq!(default_params(Kind::SolidFill).fill_color, Some([0.0; 3]));
}

#[test]
fn params_round_trip_happypaint_json() {
    let json = r#"{"gradShape":"radial","gradAX":0.1,"gradAY":0.2,"gradBX":0.3,"gradBY":0.4,"patternOffX":0.5,
        "fillStops":[{"pos":0,"color":[1,0,0],"opacity":1},{"pos":1,"color":[0,0,1],"opacity":0.5,"mid":0.3}],
        "colorSamples":[{"target":[1,0,0],"reach":0.25,"falloff":0.15,"dh":0,"ds":0,"dv":0}],
        "lgtStartX":0.3,"genColorA":[0.1,0.2,0.3],"futureKey":7}"#;
    let q = p(json);
    assert_eq!((q.grad_ax, q.grad_by, q.pattern_off_x, q.lgt_start_x), (Some(0.1), Some(0.4), Some(0.5), Some(0.3)));
    assert_eq!(q.fill_stops.as_ref().unwrap()[1].mid, Some(0.3));
    assert_eq!(q.extra.get("futureKey"), Some(&serde_json::json!(7)));
    let v = serde_json::to_value(&q).unwrap();
    assert_eq!(v["gradAX"], serde_json::json!(0.1f32));
    assert_eq!(v["colorSamples"][0]["reach"], serde_json::json!(0.25f32));
    assert_eq!(Params::from_json(q.to_json()), q);
}

#[test]
fn every_new_kind_has_a_panel() {
    for k in Kind::ALL {
        let spec10 = matches!(
            k,
            Kind::BrightnessContrast
                | Kind::HueSaturation
                | Kind::BlackWhite
                | Kind::Invert
                | Kind::Threshold
                | Kind::Posterize
                | Kind::Levels
                | Kind::Curves
                | Kind::ColorBalance
                | Kind::GradientMap
                | Kind::Noise
                | Kind::GaussianBlur
                | Kind::MotionBlur
                | Kind::Sharpen
                | Kind::SelectiveColor
        );
        assert_eq!(ui::controls(k).is_empty(), spec10, "{k:?}");
    }
}

#[test]
fn the_channel_mixer_panel_shows_colour_rows_or_the_grey_row() {
    let cs = ui::controls(Kind::ChannelMixer);
    let shown = |p: &Params| cs.iter().filter(|c| ui::visible(Kind::ChannelMixer, c, p)).count();
    let p = default_params(Kind::ChannelMixer);
    assert_eq!(shown(&p), 1 + 12);
    assert_eq!(shown(&Params { mix_mono: Some(true), ..p }), 1 + 4);
}
