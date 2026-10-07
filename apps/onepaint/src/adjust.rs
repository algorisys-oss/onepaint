//! Adjustments UI: the kind list, live destructive filters, and editing
//! adjustment layers (spec 10). Ranges follow happypaint's panels.

use crate::{App, Popover};
use egui::{RichText, Ui};
use onepaint_engine::Command;
use onepaint_filters::{Kind, Params, Stop};
use onepaint_stroke::{CurvePoint, Interp, eval_curve};
use onepaint_ui_kit::{Tokens, theme, widgets};

const TONE: [Kind; 10] = [
    Kind::BrightnessContrast,
    Kind::Levels,
    Kind::Curves,
    Kind::HueSaturation,
    Kind::ColorBalance,
    Kind::BlackWhite,
    Kind::GradientMap,
    Kind::Invert,
    Kind::Threshold,
    Kind::Posterize,
];
const EFFECTS: [Kind; 4] = [Kind::GaussianBlur, Kind::MotionBlur, Kind::Sharpen, Kind::Noise];

/// The adjustments menu: pick a kind to start a live filter on the active layer.
pub fn list(app: &mut App, ui: &mut Ui) {
    ui.set_width(240.0);
    for (title, kinds) in [("Adjust", &TONE[..]), ("Effects", &EFFECTS[..])] {
        widgets::section_label(ui, title);
        for k in kinds {
            if widgets::menu_item(ui, None, k.label(), "").clicked() {
                let _ = app.session.execute(Command::BeginFilter { kind: k.as_str().into() });
                app.popover = app.session.filter_active().then_some(Popover::Filter);
                app.curve_tab = 0;
            }
        }
        ui.add_space(4.0);
    }
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
    if !kind.is_color() {
        ui.label(
            RichText::new("Blur and sharpen layers are kept in the file but not shown yet.").font(theme::medium(11.0)).color(t.warning),
        );
    }
}
