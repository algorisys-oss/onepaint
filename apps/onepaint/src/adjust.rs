//! Adjustments UI: the kind list, live destructive filters, and editing
//! adjustment layers (spec 10). Ranges follow happypaint's panels.

use crate::{App, Popover};
use egui::{RichText, Ui};
use onepaint_engine::Command;
use onepaint_filters::ui::Control;
use onepaint_filters::{BlurPin, ColorSample, Kind, MAX_BLUR_PINS, Params, SECTIONS, Stop, default_params};
use onepaint_stroke::{CurvePoint, Interp, eval_curve};
use onepaint_ui_kit::{Tokens, theme, widgets};
use serde_json::Value;

/// The adjustments menu, sectioned like happypaint's (`ADJUSTMENT_SECTIONS`):
/// pick a kind to start a live filter on the active layer, or (fills and
/// screentone) to add it as a layer.
pub fn list(app: &mut App, ui: &mut Ui) {
    ui.set_width(240.0);
    let id = app.session.active().to_string();
    let n = app.session.smart_filters(&id).len();
    if n > 0 && widgets::menu_item(ui, Some("sliders-horizontal"), &format!("Smart filters on this layer ({n})"), "").clicked() {
        app.popover = Some(Popover::SmartFilters);
        return;
    }
    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
        for section in SECTIONS {
            widgets::section_label(ui, section);
            for k in Kind::ALL.into_iter().filter(|k| k.section() == section) {
                if widgets::menu_item(ui, None, k.label(), "").clicked() {
                    pick(app, k);
                }
            }
            ui.add_space(4.0);
        }
        widgets::section_label(ui, "Effect lines");
        for (label, focus) in [("Focus lines", true), ("Speed lines", false)] {
            if widgets::menu_item(ui, None, label, "").clicked() {
                effect_lines(app, focus);
                app.popover = None;
            }
        }
    });
}

/// Manga focus or speed lines as a new path layer stroked with the current
/// brush (adjustments.tsx `makeEffectLines`): editable with the pen afterwards.
pub fn effect_lines(app: &mut App, focus: bool) {
    use onepaint_stroke::speed_lines::{FocusLines, SpeedLines, focus_lines, speed_lines};
    let (w, h) = (app.session.doc().width as f64, app.session.doc().height as f64);
    let seed = (app.session.history_len() as u32).wrapping_mul(2_654_435_761) ^ 0x9e37_79b9;
    let subpaths = if focus {
        focus_lines(&FocusLines {
            center: [w / 2.0, h / 2.0],
            count: 90,
            inner_radius: w.min(h) * 0.18,
            outer_radius: w.hypot(h) / 2.0,
            jitter: 0.35,
            angle_jitter: 0.7,
            start_angle: 0.0,
            sweep: std::f64::consts::TAU,
            seed,
        })
    } else {
        speed_lines(&SpeedLines { angle: 0.0, count: 60, width: w, height: h, jitter: 0.35, spacing_jitter: 0.6, seed })
    };
    let c = app.session.color();
    let b = app.session.current_brush();
    let spec = onepaint_stroke::bezier::PathSpec {
        subpaths,
        stroke: Some(onepaint_stroke::bezier::PathStroke {
            brush_id: b.id.clone(),
            size: b.size,
            color: [c.r, c.g, c.b],
            opacity: 1.0,
            seed: Some(seed),
        }),
        fill: None,
    };
    let _ = app.session.execute(Command::CreatePathLayer { spec: Box::new(spec) });
}

fn pick(app: &mut App, k: Kind) {
    app.curve_tab = 0;
    if k.is_layer_only() {
        let params = seeded(k, app.session.color());
        let _ = app.session.execute(Command::AddAdjustmentLayer { kind: k.as_str().into(), params });
        app.popover = Some(Popover::AdjustLayer);
    } else {
        let _ = app.session.execute(Command::BeginFilter { kind: k.as_str().into() });
        app.popover = app.session.filter_active().then_some(Popover::Filter);
    }
}

/// Defaults for a new fill layer, seeded from the current colour as happypaint's UI does.
fn seeded(k: Kind, c: onepaint_color::Rgba) -> Params {
    let mut p = default_params(k);
    let fg = [c.r, c.g, c.b];
    match k {
        Kind::SolidFill => p.fill_color = Some(fg),
        Kind::PatternFill => p.pattern_color_a = Some(fg),
        Kind::GradientFill => {
            if let Some(s) = p.fill_stops.as_mut().and_then(|s| s.first_mut()) {
                s.color = fg;
            }
        }
        _ => {}
    }
    p
}

/// Slider in display units mapped onto a stored value (`stored = shown * scale`).
fn slider(
    ui: &mut Ui,
    label: &str,
    v: &mut Option<f32>,
    default: f32,
    range: std::ops::RangeInclusive<f32>,
    scale: f32,
    unit: &str,
) -> bool {
    let mut shown = v.unwrap_or(default) / scale;
    let unit = unit.to_string();
    let r = widgets::slider_row(ui, label, &mut shown, range, |x| format!("{x:.0}{unit}"));
    if r.changed() {
        *v = Some((shown.round()) * scale);
        return true;
    }
    false
}

fn curve_eval(points: &[[f64; 2]], x: f64, mode: Interp) -> f64 {
    let pts: Vec<CurvePoint> = points.iter().map(|p| CurvePoint::new(p[0], p[1])).collect();
    eval_curve(&pts, x, mode)
}

/// Controls for one kind; returns true when `p` changed.
pub fn params_ui(ui: &mut Ui, kind: Kind, p: &mut Params, curve_tab: &mut usize, colour: onepaint_color::Rgba) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut ch = false;
    match kind {
        Kind::BrightnessContrast => {
            ch |= slider(ui, "Brightness", &mut p.brightness, 0.0, -100.0..=100.0, 0.01, "");
            ch |= slider(ui, "Contrast", &mut p.contrast, 0.0, -100.0..=100.0, 0.01, "");
        }
        Kind::HueSaturation => {
            ch |= slider(ui, "Hue", &mut p.hue, 0.0, -180.0..=180.0, 1.0, "°");
            ch |= slider(ui, "Saturation", &mut p.saturation, 0.0, -100.0..=100.0, 0.01, "");
            ch |= slider(ui, "Value", &mut p.brightness, 0.0, -100.0..=100.0, 0.01, "");
        }
        Kind::BlackWhite => ch |= slider(ui, "Amount", &mut p.amount, 1.0, 0.0..=100.0, 0.01, "%"),
        Kind::Invert => {
            ui.label(RichText::new("No settings.").color(t.fg_muted));
        }
        Kind::Threshold => ch |= slider(ui, "Level", &mut p.threshold, 0.5, 0.0..=100.0, 0.01, ""),
        Kind::Posterize => ch |= slider(ui, "Levels", &mut p.levels, 4.0, 2.0..=32.0, 1.0, ""),
        Kind::Levels => {
            ch |= slider(ui, "In black", &mut p.in_black, 0.0, 0.0..=100.0, 0.01, "");
            ch |= slider(ui, "In white", &mut p.in_white, 1.0, 0.0..=100.0, 0.01, "");
            ch |= slider(ui, "Gamma ×100", &mut p.gamma, 1.0, 10.0..=300.0, 0.01, "");
            ch |= slider(ui, "Out black", &mut p.out_black, 0.0, 0.0..=100.0, 0.01, "");
            ch |= slider(ui, "Out white", &mut p.out_white, 1.0, 0.0..=100.0, 0.01, "");
        }
        Kind::ColorBalance => {
            ch |= slider(ui, "Cyan – Red", &mut p.cyan_red, 0.0, -100.0..=100.0, 0.01, "");
            ch |= slider(ui, "Magenta – Green", &mut p.magenta_green, 0.0, -100.0..=100.0, 0.01, "");
            ch |= slider(ui, "Yellow – Blue", &mut p.yellow_blue, 0.0, -100.0..=100.0, 0.01, "");
        }
        Kind::Curves => {
            let names = ["RGB", "Red", "Green", "Blue"];
            ui.horizontal(|ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_label(*curve_tab == i, *n).clicked() {
                        *curve_tab = i;
                    }
                }
            });
            let mode = if p.curve_mode.as_deref() == Some("linear") { Interp::Linear } else { Interp::Smooth };
            let slot = match *curve_tab {
                1 => &mut p.curve_r,
                2 => &mut p.curve_g,
                3 => &mut p.curve_b,
                _ => &mut p.curve,
            };
            let mut pts: Vec<[f64; 2]> = slot
                .clone()
                .filter(|c| c.len() >= 2)
                .unwrap_or_else(|| vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)])
                .iter()
                .map(|c| [c.x, c.y])
                .collect();
            let tint =
                [t.fg, egui::Color32::from_rgb(235, 80, 80), egui::Color32::from_rgb(80, 200, 110), egui::Color32::from_rgb(90, 140, 255)]
                    [*curve_tab];
            if widgets::curve_editor(ui, &mut pts, 240.0, tint, &|pts, x| curve_eval(pts, x, mode)) {
                *slot = Some(pts.iter().map(|q| CurvePoint::new(q[0], q[1])).collect());
                ch = true;
            }
            let mut smooth = mode == Interp::Smooth;
            if widgets::toggle(ui, &mut smooth, "Smooth").changed() {
                p.curve_mode = Some(if smooth { "smooth" } else { "linear" }.into());
                ch = true;
            }
        }
        Kind::GradientMap => {
            let stops = p
                .gradient_stops
                .clone()
                .filter(|s| s.len() >= 2)
                .unwrap_or_else(|| vec![Stop { pos: 0.0, color: [0.0; 3] }, Stop { pos: 1.0, color: [1.0; 3] }]);
            ui.horizontal(|ui| {
                for (i, s) in stops.iter().enumerate() {
                    let c = onepaint_color::Rgba::new(s.color[0], s.color[1], s.color[2], 1.0);
                    let label = if i == 0 { "Shadows" } else { "Highlights" };
                    ui.vertical(|ui| {
                        ui.label(RichText::new(label).color(t.fg_muted));
                        if widgets::swatch(ui, c, 30.0, false).on_hover_text("Click to use the current colour").clicked() {
                            let mut st = stops.clone();
                            let idx = if i == 0 { 0 } else { st.len() - 1 };
                            st[idx].color = [colour.r, colour.g, colour.b];
                            p.gradient_stops = Some(st);
                            ch = true;
                        }
                    });
                }
            });
        }
        Kind::Noise => {
            ch |= slider(ui, "Amount", &mut p.amount, 0.0, 0.0..=100.0, 0.01, "%");
            let mut mono = p.mono.unwrap_or(true);
            if widgets::toggle(ui, &mut mono, "Monochromatic").changed() {
                p.mono = Some(mono);
                ch = true;
            }
        }
        Kind::GaussianBlur => ch |= slider(ui, "Radius", &mut p.radius, 0.0, 0.0..=50.0, 1.0, " px"),
        Kind::MotionBlur => {
            ch |= slider(ui, "Angle", &mut p.angle, 0.0, 0.0..=360.0, 1.0, "°");
            ch |= slider(ui, "Distance", &mut p.distance, 0.0, 0.0..=50.0, 1.0, " px");
        }
        Kind::Sharpen => ch |= slider(ui, "Amount", &mut p.amount, 0.0, 0.0..=100.0, 0.01, "%"),
        Kind::SelectiveColor => ch |= selective_ui(ui, p, colour),
        Kind::FieldBlur => {
            ch |= pins_ui(ui, p);
            ch |= generic(ui, kind, p, colour);
        }
        Kind::PatternFill => ch |= pattern_ui(ui, p, colour),
        _ => ch |= generic(ui, kind, p, colour),
    }
    ch
}

/// Controls built from `onepaint_filters::ui::controls`, edited through the params' JSON form.
fn generic(ui: &mut Ui, kind: Kind, p: &mut Params, colour: onepaint_color::Rgba) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut m = p.to_json();
    let mut ch = false;
    for c in onepaint_filters::ui::controls(kind) {
        match *c {
            Control::Slider { key, label, min, max, scale, unit, default } => {
                let mut v = m.get(key).and_then(Value::as_f64).map(|v| v as f32);
                if slider(ui, label, &mut v, default, min..=max, scale, unit) {
                    m.insert(key.into(), serde_json::json!(v));
                    ch = true;
                }
            }
            Control::Toggle { key, label, default } => {
                let mut on = m.get(key).and_then(Value::as_bool).unwrap_or(default);
                if widgets::toggle(ui, &mut on, label).changed() {
                    m.insert(key.into(), Value::Bool(on));
                    ch = true;
                }
            }
            Control::Choice { key, label, options, default } => {
                let cur = m.get(key).and_then(Value::as_str).unwrap_or(default).to_string();
                ui.label(RichText::new(label).color(t.fg_muted));
                ui.horizontal_wrapped(|ui| {
                    for (id, name) in options {
                        if ui.selectable_label(cur == *id, *name).clicked() && cur != *id {
                            m.insert(key.into(), Value::String((*id).into()));
                            ch = true;
                        }
                    }
                });
            }
            Control::Color { key, label, default } => {
                let rgb = m.get(key).and_then(|v| serde_json::from_value::<[f32; 3]>(v.clone()).ok()).unwrap_or(default);
                if let Some(c) = colour_row(ui, label, rgb, SampleTarget { key, slot: None }, colour) {
                    m.insert(key.into(), serde_json::json!(c));
                    ch = true;
                }
            }
            Control::Seed { key, label } => {
                if widgets::secondary_button(ui, label, 90.0).clicked() {
                    let s = m.get(key).and_then(Value::as_f64).unwrap_or(1.0);
                    m.insert(key.into(), serde_json::json!(s.floor() + 1.0));
                    ch = true;
                }
            }
        }
    }
    if ch {
        *p = Params::from_json(m);
    }
    ch
}

/// A labelled swatch (click: use the current colour) with a pipette that arms
/// the canvas eyedropper for this target (happypaint `adjustmentsState.sampling`).
/// Returns the new colour, from either.
fn colour_row(ui: &mut Ui, label: &str, rgb: [f32; 3], target: SampleTarget, colour: onepaint_color::Rgba) -> Option<[f32; 3]> {
    let t = Tokens::get(ui.ctx());
    if let Some(c) = take_sample(ui.ctx(), &target) {
        return Some(c);
    }
    ui.horizontal(|ui| {
        let c = onepaint_color::Rgba::new(rgb[0], rgb[1], rgb[2], 1.0);
        let hit = widgets::swatch(ui, c, 22.0, false).on_hover_text("Click to use the current colour").clicked();
        let armed = sampling(ui.ctx()).as_ref() == Some(&target);
        let (r, resp) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Sample {label}")));
        if armed || resp.hovered() {
            ui.painter().rect_filled(r, 5.0, if armed { t.accent } else { t.hover });
        }
        onepaint_ui_kit::icons::paint(ui, r.shrink(3.0), "pipette", 16.0, if armed { t.on_accent } else { t.fg_muted });
        if resp.on_hover_text("Pick from the canvas").clicked() {
            set_sampling(ui.ctx(), (!armed).then_some(target));
        }
        ui.label(RichText::new(label).color(t.fg_muted));
        hit.then_some([colour.r, colour.g, colour.b])
    })
    .inner
}

/// Which colour param the canvas eyedropper feeds: a key, and the row of a
/// Selective Colour stack.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleTarget {
    pub key: &'static str,
    pub slot: Option<usize>,
}

fn sampling_id() -> egui::Id {
    egui::Id::new("adjust-sampling")
}

fn sampled_id() -> egui::Id {
    egui::Id::new("adjust-sampled")
}

/// The armed eyedropper target, if any (the canvas checks this on press).
pub fn sampling(ctx: &egui::Context) -> Option<SampleTarget> {
    ctx.data(|d| d.get_temp::<Option<SampleTarget>>(sampling_id())).flatten()
}

pub fn set_sampling(ctx: &egui::Context, t: Option<SampleTarget>) {
    ctx.data_mut(|d| d.insert_temp(sampling_id(), t));
}

/// The canvas sampled `rgb` for the armed target: disarm and hand it to the panel.
pub fn deliver_sample(ctx: &egui::Context, rgb: [f32; 3]) {
    if let Some(t) = sampling(ctx) {
        set_sampling(ctx, None);
        ctx.data_mut(|d| d.insert_temp(sampled_id(), Some((t, rgb))));
    }
}

/// The eyedropper is armed or has a sample on its way to the panel: a canvas
/// press then must not dismiss the panel.
pub fn eyedropper_busy(ctx: &egui::Context) -> bool {
    sampling(ctx).is_some() || ctx.data(|d| d.get_temp::<Option<(SampleTarget, [f32; 3])>>(sampled_id())).flatten().is_some()
}

fn take_sample(ctx: &egui::Context, target: &SampleTarget) -> Option<[f32; 3]> {
    let got = ctx.data(|d| d.get_temp::<Option<(SampleTarget, [f32; 3])>>(sampled_id())).flatten();
    match got {
        Some((t, rgb)) if &t == target => {
            ctx.data_mut(|d| d.insert_temp::<Option<(SampleTarget, [f32; 3])>>(sampled_id(), None));
            Some(rgb)
        }
        _ => None,
    }
}

/// One slider over a plain `f32` field (display = stored / scale).
fn field_slider(ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, scale: f32, unit: &str) -> bool {
    let mut o = Some(*v);
    let changed = slider(ui, label, &mut o, *v, range, scale, unit);
    if changed {
        *v = o.unwrap_or(*v);
    }
    changed
}

/// Selective Colour+: every sampled range (target, reach, softness and the
/// H/S/V shift), plus add and remove.
fn selective_ui(ui: &mut Ui, p: &mut Params, colour: onepaint_color::Rgba) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut samples = p.color_samples.clone().unwrap_or_default();
    if samples.is_empty() {
        samples = default_params(Kind::SelectiveColor).color_samples.unwrap_or_default();
    }
    let mut ch = false;
    let mut drop = None;
    let n = samples.len();
    for (i, s) in samples.iter_mut().enumerate() {
        let s: &mut ColorSample = s;
        if n > 1 {
            ui.label(RichText::new(format!("Range {}", i + 1)).font(onepaint_ui_kit::theme::semibold(11.5)).color(t.fg));
        }
        if let Some(c) = colour_row(ui, "Sampled colour", s.target, SampleTarget { key: "colorSamples", slot: Some(i) }, colour) {
            s.target = c;
            ch = true;
        }
        ch |= field_slider(ui, "Range", &mut s.reach, 0.0..=100.0, 0.01, "%");
        ch |= field_slider(ui, "Softness", &mut s.falloff, 0.0..=100.0, 0.01, "%");
        ch |= field_slider(ui, "Hue", &mut s.dh, -180.0..=180.0, 1.0, "°");
        ch |= field_slider(ui, "Saturation", &mut s.ds, -100.0..=100.0, 0.01, "");
        ch |= field_slider(ui, "Value", &mut s.dv, -100.0..=100.0, 0.01, "");
        if n > 1 && widgets::secondary_button(ui, "Remove range", 120.0).clicked() {
            drop = Some(i);
        }
        widgets::hairline(ui);
    }
    if let Some(i) = drop {
        samples.remove(i);
        ch = true;
    }
    if widgets::secondary_button(ui, "Add range", 120.0).clicked() {
        let mut fresh =
            default_params(Kind::SelectiveColor).color_samples.unwrap_or_default().into_iter().next().unwrap_or_else(|| samples[0].clone());
        fresh.target = [colour.r, colour.g, colour.b];
        samples.push(fresh);
        ch = true;
    }
    let mut show = p.show_range.unwrap_or(false);
    if widgets::toggle(ui, &mut show, "Show range").changed() {
        p.show_range = Some(show);
        ch = true;
    }
    if ch {
        p.color_samples = Some(samples);
    }
    ch
}

/// Pattern Fill: the built-ins, or an imported image tile kept in the params
/// as a PNG data URL (fit within 512 px, as happypaint stores it).
fn pattern_ui(ui: &mut Ui, p: &mut Params, colour: onepaint_color::Rgba) -> bool {
    let t = Tokens::get(ui.ctx());
    let id_before = p.pattern_id.clone();
    let mut ch = generic(ui, Kind::PatternFill, p, colour);
    if ch && p.pattern_id != id_before {
        // Picking a built-in replaces an imported tile.
        p.pattern_image = None;
    }
    let imported = p.pattern_image.as_deref().is_some_and(|u| !u.is_empty());
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Import pattern…", 130.0).clicked()
            && let Some(path) =
                rfd::FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"]).pick_file()
            && let Some(url) = import_pattern(&path)
        {
            p.pattern_image = Some(url);
            ch = true;
        }
        if imported && widgets::secondary_button(ui, "Use built-in", 110.0).clicked() {
            p.pattern_image = None;
            ch = true;
        }
    });
    if imported {
        ui.label(RichText::new("Imported image tile").font(theme::medium(10.5)).color(t.fg_faint));
    }
    ch
}

/// Decode an image file into a pattern tile data URL.
pub fn import_pattern(path: &std::path::Path) -> Option<String> {
    use onepaint_filters::pattern::{PATTERN_IMPORT_MAX, png_data_url};
    let bytes = std::fs::read(path).ok()?;
    let img = onepaint_formats::fit_image(onepaint_formats::decode_image(&bytes).ok()?, PATTERN_IMPORT_MAX);
    png_data_url(&img.rgba, img.width, img.height)
}

/// Field Blur pins: position and blur per pin, plus add and delete.
fn pins_ui(ui: &mut Ui, p: &mut Params) -> bool {
    let mut pins = p.pins.clone().unwrap_or_default();
    let mut ch = false;
    let mut drop = None;
    for (i, pin) in pins.iter_mut().enumerate() {
        widgets::section_label(ui, &format!("Pin {}", i + 1));
        ch |= field_slider(ui, "Blur", &mut pin.blur, 0.0..=100.0, 1.0, " px");
        ch |= field_slider(ui, "X", &mut pin.x, 0.0..=100.0, 0.01, "%");
        ch |= field_slider(ui, "Y", &mut pin.y, 0.0..=100.0, 0.01, "%");
        if widgets::secondary_button(ui, &format!("Delete pin {}", i + 1), 90.0).clicked() {
            drop = Some(i);
        }
    }
    if let Some(i) = drop {
        pins.remove(i);
        ch = true;
    }
    if pins.len() < MAX_BLUR_PINS && widgets::secondary_button(ui, "Add pin", 90.0).clicked() {
        pins.push(BlurPin { x: 0.5, y: 0.5, blur: 20.0 });
        ch = true;
    }
    if ch {
        p.pins = Some(pins);
    }
    ch
}

/// The live filter panel for the session in progress.
pub fn filter_panel(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((kind, params)) = app.session.filter_state().map(|(k, p)| (k, p.clone())) else {
        app.popover = None;
        return;
    };
    ui.set_width(280.0);
    ui.label(RichText::new(kind.label()).font(theme::semibold(15.0)).color(t.fg));
    ui.add_space(6.0);
    let mut p = params;
    let colour = app.session.color();
    if params_ui(ui, kind, &mut p, &mut app.curve_tab, colour) {
        let _ = app.session.execute(Command::UpdateFilter { params: p.clone() });
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Apply", 90.0).clicked() {
            let _ = app.session.execute(Command::CommitFilter);
            app.popover = None;
        }
        if widgets::secondary_button(ui, "Cancel", 80.0).clicked() {
            let _ = app.session.execute(Command::CancelFilter);
            app.popover = None;
        }
    });
    if onepaint_engine::smart_filter_ops::is_smart_kind(kind)
        && widgets::menu_item(ui, Some("sliders-horizontal"), "As smart filter", "").clicked()
    {
        let _ = app.session.execute(Command::CancelFilter);
        let id = app.session.active().to_string();
        let _ = app.session.execute(Command::AddSmartFilter { id, kind: kind.as_str().into(), params: p.clone() });
        app.smart_edit = Some(app.session.smart_filters(app.session.active()).len().saturating_sub(1));
        app.popover = Some(Popover::SmartFilters);
        return;
    }
    if kind.is_color() && widgets::menu_item(ui, Some("layers"), "As adjustment layer", "").clicked() {
        let _ = app.session.execute(Command::CancelFilter);
        let _ = app.session.execute(Command::AddAdjustmentLayer { kind: kind.as_str().into(), params: p });
        app.popover = None;
    }
}

/// Edit the active adjustment layer's params (live; coalesced into one undo step per session).
pub fn layer_panel(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let id = app.session.active().to_string();
    let Some(adj) = app.session.doc().layer(&id).and_then(|l| l.adjustment.clone()) else {
        app.popover = None;
        return;
    };
    ui.set_width(280.0);
    let Some(kind) = Kind::parse(&adj.kind) else {
        ui.label(RichText::new(format!("“{}” layers can't be edited yet.", adj.kind)).color(t.fg_muted));
        return;
    };
    ui.label(RichText::new(kind.label()).font(theme::semibold(15.0)).color(t.fg));
    ui.label(RichText::new("Adjustment layer").font(theme::medium(11.0)).color(t.fg_faint));
    ui.add_space(6.0);
    let mut p = adj.params.clone();
    let colour = app.session.color();
    if params_ui(ui, kind, &mut p, &mut app.curve_tab, colour) {
        let _ = app.session.execute(Command::SetAdjustment { id, params: p });
    }
}

/// The active layer's smart-filter stack: toggle, edit, delete, bake.
pub fn smart_panel(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let id = app.session.active().to_string();
    let mut list = app.session.smart_filters(&id);
    ui.set_width(300.0);
    ui.label(RichText::new("Smart filters").font(theme::semibold(15.0)).color(t.fg));
    ui.label(RichText::new("Non-destructive: the layer's pixels stay as painted.").font(theme::medium(10.5)).color(t.fg_faint));
    ui.add_space(6.0);
    if list.is_empty() {
        ui.label(RichText::new("None on this layer. Run a filter and choose “As smart filter”.").color(t.fg_muted));
        return;
    }
    let mut changed = false;
    let mut remove = None;
    for (i, f) in list.iter_mut().enumerate() {
        let label = Kind::parse(&f.kind).map_or(f.kind.clone(), |k| k.label().to_string());
        ui.horizontal(|ui| {
            let mut on = f.is_on();
            if ui.checkbox(&mut on, "").changed() {
                f.enabled = (!on).then_some(false);
                changed = true;
                app.session_seal = true;
            }
            let sel = app.smart_edit == Some(i);
            if ui.selectable_label(sel, &label).clicked() {
                app.smart_edit = if sel { None } else { Some(i) };
                app.session_seal = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if onepaint_ui_kit::icons::button(ui, "trash", 22.0, false, "Remove filter").clicked() {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
        app.smart_edit = None;
        app.session_seal = true;
    }
    if let Some(i) = app.smart_edit
        && let Some(f) = list.get_mut(i)
        && let Some(kind) = Kind::parse(&f.kind)
    {
        widgets::hairline(ui);
        let colour = app.session.color();
        if params_ui(ui, kind, &mut f.params, &mut app.curve_tab, colour) {
            changed = true;
        }
    }
    if app.session_seal {
        let _ = app.session.execute(Command::SealHistory);
        app.session_seal = false;
    }
    if changed {
        let _ = app.session.execute(Command::SetSmartFilters { id: id.clone(), filters: list });
    }
    ui.add_space(8.0);
    if widgets::secondary_button(ui, "Bake into pixels", 140.0).clicked() {
        let _ = app.session.execute(Command::BakeSmartFilters { id });
        app.smart_edit = None;
        app.popover = None;
    }
}

#[cfg(test)]
mod pattern_tests {
    #[test]
    fn importing_a_large_image_fits_the_tile_within_512() {
        let dir = std::env::temp_dir().join(format!("onepaint-pattern-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.png");
        let px: Vec<u8> = [10u8, 200, 30, 255].repeat(1024 * 512);
        let big = onepaint_filters::pattern::png_data_url(&px, 1024, 512).unwrap();
        std::fs::write(&path, onepaint_filters::pattern::base64_decode(big.split_once(',').unwrap().1).unwrap()).unwrap();
        let url = super::import_pattern(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let mut p = onepaint_filters::default_params(onepaint_filters::Kind::PatternFill);
        p.pattern_image = Some(url);
        let pr = onepaint_filters::prepare_in(onepaint_filters::Kind::PatternFill, &p, onepaint_filters::Frame::region(0, 0, 8, 8, 72.0));
        let c = pr.color_px([0.0; 4], 3, 3);
        assert!(c[1] > 0.5 && c[3] > 0.99, "{c:?}");
        let (_, body) = p.pattern_image.as_deref().unwrap().split_once(',').unwrap();
        let png = onepaint_filters::pattern::base64_decode(body).unwrap();
        let img = onepaint_formats::decode_image(&png).unwrap();
        assert_eq!((img.width, img.height), (512, 256));
    }
}
