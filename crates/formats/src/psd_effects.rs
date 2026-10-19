//! Layer styles in PSD files (spec 56): the `lfx2` / `lmfx` effects block,
//! as happypaint writes and reads it (`storage/psd-effects.ts` through
//! ag-psd), composed straight to descriptor items.
//!
//! The block is `u32 0` (effects version), `u32 16` (descriptor version)
//! and a descriptor of class `null`. `lfx2` holds one of each effect;
//! `lmfx` holds `*Multi` lists when a repeatable kind appears more than once.

use crate::psd_desc::{DVal, Desc, read, write_into};
use onepaint_doc::BlendMode;
use onepaint_doc::styles::{Contour, Effect, EffectKind, LayerStyles, default_effect};
use serde_json::{Value, json};

/// Global Light when a file has none: angle, altitude.
pub const DEFAULT_LIGHT: (f32, f32) = (120.0, 30.0);

fn blend_id(m: BlendMode) -> &'static str {
    use BlendMode::*;
    match m {
        Multiply => "Mltp",
        Screen => "Scrn",
        Overlay => "Ovrl",
        SoftLight => "SftL",
        HardLight => "HrdL",
        Darken => "Drkn",
        Lighten => "Lghn",
        ColorDodge => "CDdg",
        ColorBurn => "CBrn",
        LinearBurn => "linearBurn",
        Add => "linearDodge",
        Difference => "Dfrn",
        Exclusion => "Xclu",
        Hue => "H   ",
        Saturation => "Strt",
        Color => "Clr ",
        Luminosity => "Lmns",
        _ => "Nrml",
    }
}

/// A mode id read back; modes effects don't have become Normal; `None`
/// for an id ag-psd doesn't know (the whole block is dropped then).
fn blend_of(id: &str) -> Option<BlendMode> {
    use BlendMode::*;
    Some(match id {
        "Nrml" => Normal,
        "Mltp" => Multiply,
        "Scrn" => Screen,
        "Ovrl" => Overlay,
        "SftL" => SoftLight,
        "HrdL" => HardLight,
        "Drkn" => Darken,
        "Lghn" => Lighten,
        "CDdg" => ColorDodge,
        "CBrn" => ColorBurn,
        "linearBurn" => LinearBurn,
        "linearDodge" => Add,
        "Dfrn" => Difference,
        "Xclu" => Exclusion,
        "H   " => Hue,
        "Strt" => Saturation,
        "Clr " => Color,
        "Lmns" => Luminosity,
        "Dslv" | "darkerColor" | "lighterColor" | "vividLight" | "linearLight" | "pinLight" | "hardMix" | "blendSubtraction"
        | "blendDivide" | "linearHeight" | "Hght" | "Sbtr" => Normal,
        _ => return None,
    })
}

const CONTOUR_LABELS: [(Contour, &str); 7] = [
    (Contour::Linear, "Linear"),
    (Contour::HalfRound, "Half Round"),
    (Contour::Gaussian, "Gaussian"),
    (Contour::Cone, "Cone"),
    (Contour::ConeInverted, "Cone \u{2013} Inverted"),
    (Contour::Ring, "Ring"),
    (Contour::RollingSlope, "Rolling Slope"),
];

fn contour_desc(c: Contour) -> DVal {
    let label = CONTOUR_LABELS.iter().find(|(k, _)| *k == c).map_or("Linear", |(_, l)| l);
    let pts = (0..=10)
        .map(|i| {
            let mut p = Desc::new("CrPt");
            p.push("Hrzn", DVal::Doub(i as f64 * 25.5)).push("Vrtc", DVal::Doub(c.at(i as f32 / 10.0) as f64 * 255.0));
            DVal::Obj(p)
        })
        .collect();
    let mut d = Desc::new("ShpC");
    d.push("Nm  ", DVal::Text(label.into())).push("Crv ", DVal::List(pts));
    DVal::Obj(d)
}

fn norm(s: &str) -> String {
    s.to_lowercase().replace('\u{2013}', "-").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A contour read back: by its name (Photoshop's or ours), else the preset
/// closest to its curve; `fallback` with fewer than two points.
fn contour_of(d: Option<&Desc>, fallback: Contour) -> Contour {
    let Some(d) = d else { return fallback };
    if let Some(name) = d.text("Nm  ") {
        let n = norm(name);
        let mut labels: Vec<_> = CONTOUR_LABELS.iter().map(|(c, l)| (*c, norm(l))).collect();
        labels.sort_by_key(|(_, l)| std::cmp::Reverse(l.len()));
        if let Some((c, _)) = labels.iter().find(|(_, l)| n.starts_with(l.as_str())) {
            return *c;
        }
    }
    let pts: Vec<(f32, f32)> = d
        .list("Crv ")
        .unwrap_or(&[])
        .iter()
        .filter_map(|p| match p {
            DVal::Obj(p) => Some((p.f64("Hrzn")? as f32 / 255.0, p.f64("Vrtc")? as f32 / 255.0)),
            _ => None,
        })
        .collect();
    if pts.len() < 2 {
        return fallback;
    }
    let err = |c: Contour| pts.iter().map(|(x, y)| (c.at(*x) - y).powi(2)).sum::<f32>();
    Contour::ALL.into_iter().min_by(|a, b| err(*a).total_cmp(&err(*b))).unwrap_or(fallback)
}

fn colour(c: [f32; 3]) -> DVal {
    let mut d = Desc::new("RGBC");
    let v = |x: f32| DVal::Doub((x.clamp(0.0, 1.0) * 255.0).round() as f64);
    d.push("Rd  ", v(c[0])).push("Grn ", v(c[1])).push("Bl  ", v(c[2]));
    DVal::Obj(d)
}

fn hsv_rgb(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// A colour read back (RGB, float RGB, HSB or grey); `None` otherwise.
fn colour_of(d: Option<&Desc>) -> Option<[f32; 3]> {
    let d = d?;
    if let (Some(r), Some(g), Some(b)) = (d.f64("Rd  "), d.f64("Grn "), d.f64("Bl  ")) {
        return Some([r, g, b].map(|v| (v as f32 / 255.0).clamp(0.0, 1.0)));
    }
    if let (Some(r), Some(g), Some(b)) = (d.f64("redFloat"), d.f64("greenFloat"), d.f64("blueFloat")) {
        return Some([r, g, b].map(|v| (v as f32).clamp(0.0, 1.0)));
    }
    if let Some(k) = d.f64("Gry ") {
        let g = (1.0 - k as f32 / 100.0).clamp(0.0, 1.0);
        return Some([g; 3]);
    }
    if let (Some(DVal::Unit(u, h)), Some(s), Some(v)) = (d.get("H   "), d.f64("Strt"), d.f64("Brgh")) {
        let h = if u == b"#Ang" { *h as f32 / 360.0 } else { *h as f32 / 100.0 };
        let pct = |x: f64| if x > 1.0 { x as f32 / 100.0 } else { x as f32 };
        return Some(hsv_rgb(h, pct(s), pct(v)));
    }
    None
}

fn pct(v: f32) -> DVal {
    DVal::Unit(*b"#Prc", (v * 100.0).round() as f64)
}

fn ang(v: f32) -> DVal {
    DVal::Unit(*b"#Ang", v as f64)
}

fn px(v: f32) -> DVal {
    DVal::Unit(*b"#Pxl", v as f64)
}

fn mode(m: BlendMode) -> DVal {
    DVal::Enum("BlnM".into(), blend_id(m).into())
}

/// The opening keys every effect has.
fn head(class: &str, e: &Effect) -> Desc {
    let mut d = Desc::new(class);
    d.push("enab", DVal::Bool(e.enabled));
    if e.kind == EffectKind::DropShadow {
        // ag-psd's default for a shadow without a contour.
        let mut c = Desc::new("ShpC");
        c.push("Nm  ", DVal::Text(String::new())).push("Crv ", DVal::List(Vec::new()));
        d.push("TrnS", DVal::Obj(c));
    }
    d.push("present", DVal::Bool(true)).push("showInDialog", DVal::Bool(true));
    d
}

fn gradient(e: &Effect) -> DVal {
    let mut stops: Vec<(f64, [f32; 3], f32, Option<f64>)> = e
        .stops
        .iter()
        .filter_map(|s| {
            let c = s.get("color")?.as_array()?;
            let rgb = [c.first()?.as_f64()? as f32, c.get(1)?.as_f64()? as f32, c.get(2)?.as_f64()? as f32];
            let op = s.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) as f32;
            Some((s.get("pos")?.as_f64()?, rgb, op, s.get("mid").and_then(Value::as_f64)))
        })
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Photoshop keeps a segment's midpoint on its right-hand stop.
    let mid = |i: usize| if i > 0 { stops[i - 1].3.unwrap_or(0.5) } else { 0.5 };
    let loc = |p: f64| DVal::Long((p.clamp(0.0, 1.0) * 4096.0).round() as i32);
    let clrs = stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut d = Desc::new("Clrt");
            d.push("Clr ", colour(s.1))
                .push("Type", DVal::Enum("Clry".into(), "UsrS".into()))
                .push("Lctn", loc(s.0))
                .push("Mdpn", DVal::Long((mid(i) * 100.0).round() as i32));
            DVal::Obj(d)
        })
        .collect();
    let trns = stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let mut d = Desc::new("TrnS");
            d.push("Opct", pct(s.2)).push("Lctn", loc(s.0)).push("Mdpn", DVal::Long((mid(i) * 100.0).round() as i32));
            DVal::Obj(d)
        })
        .collect();
    let mut g = Desc::new("Grdn");
    g.name = "Gradient".into();
    g.push("Nm  ", DVal::Text("Custom".into()))
        .push("GrdF", DVal::Enum("GrdF".into(), "CstS".into()))
        .push("Intr", DVal::Doub(4096.0))
        .push("Clrs", DVal::List(clrs))
        .push("Trns", DVal::List(trns));
    DVal::Obj(g)
}

fn effect_desc(e: &Effect) -> Desc {
    match e.kind {
        EffectKind::DropShadow | EffectKind::InnerShadow => {
            let mut d = head(if e.kind == EffectKind::DropShadow { "DrSh" } else { "IrSh" }, e);
            d.push("Md  ", mode(e.blend_mode))
                .push("Clr ", colour(e.color))
                .push("Opct", pct(e.opacity))
                .push("lagl", ang(e.angle))
                .push("uglg", DVal::Bool(e.use_global_light))
                .push("Dstn", px(e.distance))
                .push("Ckmt", px(e.spread * 100.0))
                .push("blur", px(e.size));
            d
        }
        EffectKind::OuterGlow | EffectKind::InnerGlow => {
            let mut d = head(if e.kind == EffectKind::OuterGlow { "OrGl" } else { "IrGl" }, e);
            d.push("Md  ", mode(e.blend_mode))
                .push("Clr ", colour(e.color))
                .push("Opct", pct(e.opacity))
                .push("Ckmt", px(e.spread * 100.0))
                .push("blur", px(e.size));
            d
        }
        EffectKind::Bevel => {
            let b = &e.bevel;
            let style = match b.style.as_str() {
                "outer" => "OtrB",
                "emboss" => "Embs",
                "pillow" => "PlEb",
                _ => "InrB",
            };
            let mut d = head("ebbl", e);
            d.push("bvlS", DVal::Enum("BESl".into(), style.into()))
                .push("bvlT", DVal::Enum("bvlT".into(), "SfBL".into()))
                .push("srgR", pct(b.depth))
                .push("bvlD", DVal::Enum("BESs".into(), if b.up { "In  " } else { "Out " }.into()))
                .push("blur", px(e.size))
                .push("Sftn", px(b.soften))
                .push("lagl", ang(e.angle))
                .push("Lald", ang(b.altitude))
                .push("uglg", DVal::Bool(e.use_global_light))
                .push("hglM", mode(b.highlight_mode))
                .push("hglC", colour(b.highlight_color))
                .push("hglO", pct(b.highlight_opacity * e.opacity))
                .push("sdwM", mode(b.shadow_mode))
                .push("sdwC", colour(b.shadow_color))
                .push("sdwO", pct(b.shadow_opacity * e.opacity))
                .push("useShape", DVal::Bool(b.contour_on))
                .push("TrnS", contour_desc(if b.contour_on { b.contour } else { Contour::Linear }));
            d
        }
        EffectKind::Stroke => {
            let style = match e.position.as_str() {
                "center" => "CtrF",
                "inside" => "InsF",
                _ => "OutF",
            };
            let mut d = head("FrFX", e);
            d.push("Styl", DVal::Enum("FStl".into(), style.into()))
                .push("PntT", DVal::Enum("FrFl".into(), "SClr".into()))
                .push("Md  ", mode(e.blend_mode))
                .push("Opct", pct(e.opacity))
                .push("Sz  ", px(e.size))
                .push("Clr ", colour(e.color));
            d
        }
        EffectKind::Satin => {
            let mut d = head("ChFX", e);
            d.push("Md  ", mode(e.blend_mode))
                .push("Clr ", colour(e.color))
                .push("Opct", pct(e.opacity))
                .push("lagl", ang(e.angle))
                .push("Dstn", px(e.distance))
                .push("blur", px(e.size))
                .push("Invr", DVal::Bool(e.invert))
                .push("MpgS", contour_desc(e.contour));
            d
        }
        EffectKind::ColorOverlay => {
            let mut d = head("SoFi", e);
            d.push("Md  ", mode(e.blend_mode)).push("Clr ", colour(e.color)).push("Opct", pct(e.opacity));
            d
        }
        EffectKind::GradientOverlay => {
            let shape = match e.shape.as_str() {
                "radial" => "Rdl ",
                "angle" => "Angl",
                "reflected" => "Rflc",
                "diamond" => "Dmnd",
                _ => "Lnr ",
            };
            let mut d = head("GrFl", e);
            d.push("Md  ", mode(e.blend_mode))
                .push("Opct", pct(e.opacity))
                .push("Rvrs", DVal::Bool(e.reverse))
                .push("Type", DVal::Enum("GrdT".into(), shape.into()))
                .push("Angl", ang(e.angle))
                .push("Scl ", pct(e.scale))
                .push("Algn", DVal::Bool(false))
                .push("Grad", gradient(e));
            d
        }
        // Not written (as happypaint): a placeholder never reaches here.
        EffectKind::PatternOverlay => Desc::new("patternFill"),
    }
}

/// The effects block for `styles`: `lfx2`, or `lmfx` when a repeatable kind
/// appears more than once; `None` when there are no effects to write
/// (Fill opacity goes in `iOpa`, beside it).
pub fn write(styles: &LayerStyles) -> Option<([u8; 4], Vec<u8>)> {
    let written: Vec<&Effect> = styles.effects.iter().filter(|e| e.kind != EffectKind::PatternOverlay).collect();
    if written.is_empty() {
        return None;
    }
    let of = |k: EffectKind| written.iter().copied().filter(|e| e.kind == k).take(4).collect::<Vec<_>>();
    let multi = EffectKind::ALL.into_iter().any(|k| k.repeats() && of(k).len() > 1);
    let mut top = Desc::new("null");
    top.push("Scl ", DVal::Unit(*b"#Prc", 100.0)).push("masterFXSwitch", DVal::Bool(true));
    let one_or_many = |top: &mut Desc, k: EffectKind, single: &str, list: &str| {
        let es = of(k);
        match es.len() {
            0 => {}
            1 => {
                top.push(single, DVal::Obj(effect_desc(es[0])));
            }
            _ => {
                top.push(list, DVal::List(es.iter().map(|e| DVal::Obj(effect_desc(e))).collect()));
            }
        }
    };
    let single = |top: &mut Desc, k: EffectKind, key: &str| {
        if let Some(e) = of(k).first() {
            top.push(key, DVal::Obj(effect_desc(e)));
        }
    };
    let multiples = |k: EffectKind| of(k).len() > 1;
    one_or_many(&mut top, EffectKind::DropShadow, "DrSh", "dropShadowMulti");
    one_or_many(&mut top, EffectKind::InnerShadow, "IrSh", "innerShadowMulti");
    single(&mut top, EffectKind::OuterGlow, "OrGl");
    // The Multi lists of these come before the singles' places (ag-psd's order).
    for (k, list) in [
        (EffectKind::ColorOverlay, "solidFillMulti"),
        (EffectKind::GradientOverlay, "gradientFillMulti"),
        (EffectKind::Stroke, "frameFXMulti"),
    ] {
        if multiples(k) {
            top.push(list, DVal::List(of(k).iter().map(|e| DVal::Obj(effect_desc(e))).collect()));
        }
    }
    single(&mut top, EffectKind::InnerGlow, "IrGl");
    single(&mut top, EffectKind::Bevel, "ebbl");
    if !multiples(EffectKind::ColorOverlay) {
        single(&mut top, EffectKind::ColorOverlay, "SoFi");
    }
    if !multiples(EffectKind::GradientOverlay) {
        single(&mut top, EffectKind::GradientOverlay, "GrFl");
    }
    single(&mut top, EffectKind::Satin, "ChFX");
    if !multiples(EffectKind::Stroke) {
        single(&mut top, EffectKind::Stroke, "FrFX");
    }
    let enabled = written.iter().filter(|e| e.enabled).count();
    top.push("numModifyingFX", DVal::Long(enabled as i32));
    let mut o = Vec::new();
    o.extend(0u32.to_be_bytes());
    o.extend(16u32.to_be_bytes());
    write_into(&mut o, &top);
    Some((if multi { *b"lmfx" } else { *b"lfx2" }, o))
}

fn base(kind: EffectKind, d: &Desc, on: bool) -> Option<Effect> {
    let mut e = default_effect(kind);
    e.enabled = on && d.bool("enab").unwrap_or(false);
    e.blend_mode = match d.enum_val("Md  ") {
        Some(id) => blend_of(id)?,
        None => BlendMode::Normal,
    };
    if let Some(c) = colour_of(d.obj("Clr ")) {
        e.color = c;
    }
    if let Some(o) = d.f64("Opct") {
        e.opacity = (o as f32 / 100.0).clamp(0.0, 1.0);
    }
    Some(e)
}

fn effect_of(kind: EffectKind, d: &Desc, on: bool, light: (f32, f32)) -> Option<Effect> {
    let mut e = base(kind, d, on)?;
    let f = |k: &str| d.f64(k).map(|v| v as f32);
    let linked = |e: &mut Effect| {
        e.use_global_light = d.bool("uglg").unwrap_or(false);
        if let Some(a) = f("lagl") {
            e.angle = a;
        }
        if e.use_global_light {
            e.angle = light.0;
        }
    };
    match kind {
        EffectKind::DropShadow | EffectKind::InnerShadow => {
            linked(&mut e);
            e.distance = f("Dstn").unwrap_or(e.distance);
            e.spread = f("Ckmt").map_or(e.spread, |v| v / 100.0);
            e.size = f("blur").unwrap_or(e.size);
        }
        EffectKind::OuterGlow | EffectKind::InnerGlow => {
            e.spread = f("Ckmt").map_or(e.spread, |v| v / 100.0);
            e.size = f("blur").unwrap_or(e.size);
        }
        EffectKind::Stroke => {
            e.size = f("Sz  ").unwrap_or(e.size);
            e.position = match d.enum_val("Styl") {
                Some("CtrF") => "center",
                Some("InsF") => "inside",
                _ => "outside",
            }
            .into();
            if colour_of(d.obj("Clr ")).is_none() {
                e.color = [0.0; 3];
            }
        }
        EffectKind::Satin => {
            e.angle = f("lagl").unwrap_or(e.angle);
            e.distance = f("Dstn").unwrap_or(e.distance);
            e.size = f("blur").unwrap_or(e.size);
            e.invert = d.bool("Invr").unwrap_or(e.invert);
            e.contour = contour_of(d.obj("MpgS"), Contour::Gaussian);
        }
        EffectKind::Bevel => {
            linked(&mut e);
            let b = &mut e.bevel;
            b.style = match d.enum_val("bvlS") {
                Some("OtrB") => "outer",
                Some("Embs") => "emboss",
                Some("PlEb") => "pillow",
                _ => "inner",
            }
            .into();
            b.depth = f("srgR").map_or(b.depth, |v| v / 100.0);
            b.up = d.enum_val("bvlD") != Some("Out ");
            b.soften = f("Sftn").unwrap_or(b.soften);
            b.altitude = f("Lald").unwrap_or(b.altitude);
            if e.use_global_light {
                b.altitude = light.1;
            }
            if let Some(m) = d.enum_val("hglM") {
                b.highlight_mode = blend_of(m)?;
            }
            if let Some(m) = d.enum_val("sdwM") {
                b.shadow_mode = blend_of(m)?;
            }
            b.highlight_color = colour_of(d.obj("hglC")).unwrap_or(b.highlight_color);
            b.shadow_color = colour_of(d.obj("sdwC")).unwrap_or(b.shadow_color);
            b.highlight_opacity = f("hglO").map_or(b.highlight_opacity, |v| v / 100.0);
            b.shadow_opacity = f("sdwO").map_or(b.shadow_opacity, |v| v / 100.0);
            b.contour_on = d.bool("useShape").unwrap_or(false);
            b.contour = contour_of(d.obj("TrnS"), Contour::Linear);
            e.size = f("blur").unwrap_or(e.size);
            // The master opacity was folded into the highlight and shadow.
            e.opacity = 1.0;
            e.blend_mode = BlendMode::Normal;
        }
        EffectKind::GradientOverlay => {
            e.shape = match d.enum_val("Type") {
                Some("Rdl ") => "radial",
                Some("Angl") => "angle",
                Some("Rflc") => "reflected",
                Some("Dmnd") => "diamond",
                _ => "linear",
            }
            .into();
            e.angle = f("Angl").unwrap_or(e.angle);
            e.scale = f("Scl ").map_or(e.scale, |v| v / 100.0);
            e.reverse = d.bool("Rvrs").unwrap_or(false);
            if let Some(s) = d.obj("Grad").and_then(stops_of) {
                e.stops = s;
            }
        }
        _ => {}
    }
    Some(e)
}

/// A gradient's stops read back: colour stops with the opacity at each
/// (linear between opacity stops), midpoints from the stop on their right.
fn stops_of(g: &Desc) -> Option<Vec<Value>> {
    let intr = g.f64("Intr").filter(|v| *v > 0.0).unwrap_or(4096.0);
    let objs = |k: &str| -> Vec<Desc> {
        g.list(k)
            .unwrap_or(&[])
            .iter()
            .filter_map(|v| match v {
                DVal::Obj(d) => Some(d.clone()),
                _ => None,
            })
            .collect()
    };
    let (clrs, trns) = (objs("Clrs"), objs("Trns"));
    if clrs.len() < 2 {
        return None;
    }
    let ops: Vec<(f64, f64)> = trns.iter().map(|t| (t.f64("Lctn").unwrap_or(0.0) / intr, t.f64("Opct").unwrap_or(100.0) / 100.0)).collect();
    let opacity_at = |p: f64| -> f64 {
        match ops.len() {
            0 => 1.0,
            _ if p <= ops[0].0 => ops[0].1,
            _ => {
                for w in ops.windows(2) {
                    if p <= w[1].0 {
                        let t = if w[1].0 > w[0].0 { (p - w[0].0) / (w[1].0 - w[0].0) } else { 0.0 };
                        return w[0].1 + (w[1].1 - w[0].1) * t;
                    }
                }
                ops[ops.len() - 1].1
            }
        }
    };
    let n = clrs.len();
    Some(
        clrs.iter()
            .enumerate()
            .map(|(i, c)| {
                let pos = (c.f64("Lctn").unwrap_or(0.0) / intr).clamp(0.0, 1.0);
                let rgb = colour_of(c.obj("Clr ")).unwrap_or([0.0; 3]);
                let mut s = json!({"pos": pos, "color": rgb, "opacity": opacity_at(pos)});
                if i + 1 < n {
                    let mid = clrs[i + 1].f64("Mdpn").unwrap_or(50.0) / 100.0;
                    if (mid - 0.5).abs() > 1e-9 {
                        s["mid"] = json!(mid);
                    }
                }
                s
            })
            .collect(),
    )
}

/// The styles in an `lfx2` / `lmfx` block, with the layer's Fill opacity
/// (from `iOpa`) and the document's Global Light; `None` when the block
/// can't be read (happypaint then drops it) or holds nothing.
pub fn read_block(data: &[u8], light: (f32, f32)) -> Option<Vec<Effect>> {
    if data.len() < 8 || data[..4] != [0, 0, 0, 0] || data[4..8] != [0, 0, 0, 16] {
        return None;
    }
    let (top, _) = read(data, 8)?;
    let on = top.bool("masterFXSwitch").unwrap_or(false);
    let many = |single: &str, list: &str| -> Vec<Desc> {
        match top.list(list) {
            Some(l) => l
                .iter()
                .filter_map(|v| match v {
                    DVal::Obj(d) => Some(d.clone()),
                    _ => None,
                })
                .collect(),
            None => top.obj(single).cloned().into_iter().collect(),
        }
    };
    let mut effects = Vec::new();
    for (kind, single, list) in [
        (EffectKind::DropShadow, "DrSh", "dropShadowMulti"),
        (EffectKind::InnerShadow, "IrSh", "innerShadowMulti"),
        (EffectKind::ColorOverlay, "SoFi", "solidFillMulti"),
        (EffectKind::GradientOverlay, "GrFl", "gradientFillMulti"),
        (EffectKind::Stroke, "FrFX", "frameFXMulti"),
    ] {
        for d in many(single, list).iter().take(4) {
            effects.push(effect_of(kind, d, on, light)?);
        }
    }
    for (kind, key) in
        [(EffectKind::OuterGlow, "OrGl"), (EffectKind::InnerGlow, "IrGl"), (EffectKind::Bevel, "ebbl"), (EffectKind::Satin, "ChFX")]
    {
        if let Some(d) = top.obj(key) {
            effects.push(effect_of(kind, d, on, light)?);
        }
    }
    // The dialog's order, stable within a kind.
    effects.sort_by_key(|e| EffectKind::ALL.iter().position(|k| *k == e.kind));
    Some(effects)
}

/// A node's styles from its effects (if any were read) and Fill opacity:
/// `None` when neither changes anything.
pub fn styles_of(effects: Option<Vec<Effect>>, fill: Option<f32>) -> Option<LayerStyles> {
    let fill = fill.unwrap_or(1.0).clamp(0.0, 1.0);
    let effects = effects.unwrap_or_default();
    if effects.is_empty() && fill >= 1.0 {
        return None;
    }
    // Through the sanitizer, as any file-borne style.
    LayerStyles::from_json(&LayerStyles { fill_opacity: fill, effects }.to_json())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fx(kind: EffectKind, f: impl FnOnce(&mut Effect)) -> Effect {
        let mut e = default_effect(kind);
        f(&mut e);
        e
    }

    fn hex(s: &str) -> [f32; 3] {
        let v = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).unwrap() as f32 / 255.0;
        [v(1), v(3), v(5)]
    }

    fn round_trip(s: &LayerStyles, light: (f32, f32)) -> LayerStyles {
        let (_, data) = write(s).unwrap();
        styles_of(read_block(&data, light), Some(s.fill_opacity)).unwrap()
    }

    /// happypaint psd-effects.test.ts `all()`.
    fn all() -> LayerStyles {
        LayerStyles {
            fill_opacity: 0.6,
            effects: vec![
                fx(EffectKind::Bevel, |e| {
                    e.bevel.style = "emboss".into();
                    (e.size, e.bevel.depth, e.angle, e.bevel.altitude, e.use_global_light) = (9.0, 2.5, 45.0, 40.0, false);
                    (e.bevel.contour_on, e.bevel.contour) = (true, Contour::Ring);
                }),
                fx(EffectKind::Stroke, |e| (e.size, e.color, e.position) = (4.0, hex("#112233"), "inside".into())),
                fx(EffectKind::InnerShadow, |e| e.distance = 6.0),
                default_effect(EffectKind::InnerGlow),
                default_effect(EffectKind::Satin),
                fx(EffectKind::ColorOverlay, |e| (e.color, e.opacity, e.blend_mode) = (hex("#00ff00"), 0.5, BlendMode::Overlay)),
                default_effect(EffectKind::GradientOverlay),
                fx(EffectKind::OuterGlow, |e| (e.spread, e.size) = (0.25, 20.0)),
                fx(EffectKind::DropShadow, |e| {
                    (e.angle, e.distance, e.spread, e.size, e.color, e.use_global_light) = (135.0, 12.0, 0.1, 7.0, hex("#ff0000"), false)
                }),
            ],
        }
    }

    #[test]
    fn every_effect_we_have_survives_a_round_trip() {
        let src = all();
        let back = round_trip(&src, DEFAULT_LIGHT);
        assert!((back.fill_opacity - 0.6).abs() < 1e-6);
        let mut kinds: Vec<_> = back.effects.iter().map(|e| e.kind.as_str()).collect();
        let mut want: Vec<_> = src.effects.iter().map(|e| e.kind.as_str()).collect();
        kinds.sort();
        want.sort();
        assert_eq!(kinds, want);
        for e in &src.effects {
            let b = back.effects.iter().find(|x| x.kind == e.kind).unwrap();
            assert!(b.enabled);
            assert!((b.opacity - e.opacity).abs() < 0.01, "{:?}", e.kind);
            assert_eq!(b.blend_mode, e.blend_mode, "{:?}", e.kind);
            if !matches!(e.kind, EffectKind::GradientOverlay | EffectKind::Bevel) {
                assert_eq!(b.color.map(|v| (v * 255.0).round()), e.color.map(|v| (v * 255.0).round()), "{:?}", e.kind);
            }
        }
        let ds = back.effects.iter().find(|e| e.kind == EffectKind::DropShadow).unwrap();
        assert_eq!((ds.angle, ds.distance, ds.size), (135.0, 12.0, 7.0));
        assert!((ds.spread - 0.1).abs() < 1e-6);
        let bv = back.effects.iter().find(|e| e.kind == EffectKind::Bevel).unwrap();
        assert_eq!((bv.bevel.style.as_str(), bv.size, bv.angle, bv.bevel.altitude), ("emboss", 9.0, 45.0, 40.0));
        assert!((bv.bevel.depth - 2.5).abs() < 1e-6);
        assert_eq!((bv.bevel.contour_on, bv.bevel.contour), (true, Contour::Ring));
        assert_eq!((bv.bevel.highlight_mode, bv.bevel.shadow_mode), (BlendMode::Screen, BlendMode::Multiply));
    }

    #[test]
    fn nothing_to_write_means_no_block() {
        assert!(write(&LayerStyles::default()).is_none());
        let (key, _) = write(&LayerStyles { fill_opacity: 1.0, effects: vec![default_effect(EffectKind::Stroke)] }).unwrap();
        assert_eq!(&key, b"lfx2");
        assert_eq!(styles_of(None, Some(0.3)), Some(LayerStyles { fill_opacity: 0.3, effects: vec![] }));
        assert_eq!(styles_of(None, Some(1.0)), None);
    }

    #[test]
    fn global_light_takes_the_documents_angle_on_import() {
        let src =
            LayerStyles { fill_opacity: 1.0, effects: vec![fx(EffectKind::DropShadow, |e| (e.angle, e.use_global_light) = (10.0, true))] };
        let back = round_trip(&src, (77.0, 33.0));
        assert_eq!((back.effects[0].angle, back.effects[0].use_global_light), (77.0, true));
        // A linked effect stays linked.
        let src =
            LayerStyles { fill_opacity: 1.0, effects: vec![fx(EffectKind::DropShadow, |e| (e.angle, e.use_global_light) = (60.0, true))] };
        let back = round_trip(&src, (60.0, 30.0));
        assert_eq!((back.effects[0].use_global_light, back.effects[0].angle), (true, 60.0));
    }

    #[test]
    fn photoshops_names_map_onto_ours() {
        // A shadow in Linear Dodge, a glow in Vivid Light (off), a Stroke Emboss with a named contour.
        let mut shadow = Desc::new("DrSh");
        shadow.push("enab", DVal::Bool(true)).push("Md  ", DVal::Enum("BlnM".into(), "linearDodge".into()));
        let mut glow = Desc::new("OrGl");
        glow.push("enab", DVal::Bool(false)).push("Md  ", DVal::Enum("BlnM".into(), "vividLight".into()));
        let mut c = Desc::new("ShpC");
        c.push("Nm  ", DVal::Text("Cone - Inverted".into())).push("Crv ", DVal::List(vec![]));
        let mut bevel = Desc::new("ebbl");
        bevel
            .push("enab", DVal::Bool(true))
            .push("bvlS", DVal::Enum("BESl".into(), "strokeEmboss".into()))
            .push("useShape", DVal::Bool(true))
            .push("TrnS", DVal::Obj(c));
        let mut top = Desc::new("null");
        top.push("masterFXSwitch", DVal::Bool(true))
            .push("DrSh", DVal::Obj(shadow))
            .push("OrGl", DVal::Obj(glow))
            .push("ebbl", DVal::Obj(bevel));
        let mut data = vec![0, 0, 0, 0, 0, 0, 0, 16];
        write_into(&mut data, &top);
        let fx = read_block(&data, DEFAULT_LIGHT).unwrap();
        let by = |k: EffectKind| fx.iter().find(|e| e.kind == k).unwrap();
        assert_eq!(by(EffectKind::DropShadow).blend_mode, BlendMode::Add);
        assert_eq!((by(EffectKind::OuterGlow).enabled, by(EffectKind::OuterGlow).blend_mode), (false, BlendMode::Normal));
        assert_eq!(by(EffectKind::Bevel).bevel.style, "inner", "Stroke Emboss: nearest is Inner");
        assert_eq!(by(EffectKind::Bevel).bevel.contour, Contour::ConeInverted);
    }

    #[test]
    fn a_contour_named_otherwise_is_matched_by_its_curve() {
        let pts = (0..=10)
            .map(|i| {
                let mut p = Desc::new("CrPt");
                p.push("Hrzn", DVal::Doub(i as f64 * 25.5))
                    .push("Vrtc", DVal::Doub((1.0 - (1.0 - i as f64 / 10.0).powi(2)).sqrt() * 255.0));
                DVal::Obj(p)
            })
            .collect();
        let mut c = Desc::new("ShpC");
        c.push("Nm  ", DVal::Text("My custom dome".into())).push("Crv ", DVal::List(pts));
        assert_eq!(contour_of(Some(&c), Contour::Gaussian), Contour::HalfRound);
    }

    #[test]
    fn several_of_one_effect_cross_in_order() {
        let src = LayerStyles {
            fill_opacity: 1.0,
            effects: vec![
                fx(EffectKind::Stroke, |e| (e.size, e.color) = (3.0, [1.0; 3])),
                fx(EffectKind::Stroke, |e| (e.size, e.color) = (9.0, [0.0; 3])),
                fx(EffectKind::DropShadow, |e| e.distance = 4.0),
                fx(EffectKind::DropShadow, |e| (e.distance, e.color) = (30.0, [1.0, 0.0, 0.0])),
            ],
        };
        let (key, _) = write(&src).unwrap();
        assert_eq!(&key, b"lmfx");
        let back = round_trip(&src, DEFAULT_LIGHT);
        let strokes: Vec<_> = back.effects.iter().filter(|e| e.kind == EffectKind::Stroke).map(|e| (e.size, e.color)).collect();
        assert_eq!(strokes, vec![(3.0, [1.0; 3]), (9.0, [0.0; 3])]);
        let d: Vec<_> = back.effects.iter().filter(|e| e.kind == EffectKind::DropShadow).map(|e| e.distance).collect();
        assert_eq!(d, vec![4.0, 30.0]);
    }

    #[test]
    fn gradient_stops_keep_their_midpoints() {
        let src = LayerStyles {
            fill_opacity: 1.0,
            effects: vec![fx(EffectKind::GradientOverlay, |e| {
                e.stops = vec![
                    json!({"pos": 0.0, "color": [1.0, 0.0, 0.0], "opacity": 1.0, "mid": 0.3}),
                    json!({"pos": 1.0, "color": [0.0, 0.0, 1.0], "opacity": 0.5}),
                ]
            })],
        };
        let back = round_trip(&src, DEFAULT_LIGHT);
        let s = &back.effects[0].stops;
        assert_eq!(s[0]["mid"], json!(0.3));
        assert_eq!(s[1]["opacity"], json!(0.5));
        assert_eq!(s[1]["color"], json!([0.0, 0.0, 1.0]));
    }
}
