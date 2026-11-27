//! Settings ▸ Brush ▸ Pen pressure (happypaint
//! `settings-dialog/pen-pressure-setting.tsx`, v0.212.0): the app-wide pen
//! curve with a graph, a test pad and Calibrate, which fits the curve from a
//! few strokes at normal pressure.

use egui::{Align2, Pos2, RichText, Sense, Stroke, Ui, pos2, vec2};
use onepaint_input::PenCurve;
use onepaint_input::pen_curve::{MIN_CALIBRATION_SAMPLES, PEN_FULL_MIN};
use onepaint_ui_kit::{Tokens, theme, widgets};

const GRAPH: f32 = 96.0;
const PAD_H: f32 = 96.0;
/// The pad's widest mark, for full pressure.
const MAX_W: f32 = 9.0;

/// The pad's state, kept in egui's temporary memory while Settings is open.
#[derive(Clone, Default)]
struct Pad {
    /// Pad-local segments: from, to, width.
    segs: Vec<(Pos2, Pos2, f32)>,
    last: Option<(Pos2, f32)>,
    samples: Vec<f64>,
    calibrating: bool,
    /// The last sample: raw pressure and what the curve made of it.
    live: Option<(f64, f64)>,
    hint: String,
}

/// The "Firm" slider (0..100) ↔ the curve's exponent (1/3..3, 50 = linear).
pub fn gamma_to_firm(g: f64) -> f32 {
    (50.0 + 50.0 * g.ln() / 3f64.ln()).round() as f32
}

pub fn firm_to_gamma(f: f32) -> f64 {
    3f64.powf((f as f64 - 50.0) / 50.0)
}

fn firm_label(g: f64) -> &'static str {
    if g < 0.92 {
        "Soft"
    } else if g > 1.08 {
        "Firm"
    } else {
        "Linear"
    }
}

/// The whole setting; true when the curve changed.
pub fn setting(ui: &mut Ui, curve: &mut PenCurve) -> bool {
    let t = Tokens::get(ui.ctx());
    let id = ui.id().with("pen-pressure-pad");
    let mut pad: Pad = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let before = *curve;

    ui.horizontal(|ui| {
        graph(ui, curve, pad.live);
        ui.vertical(|ui| {
            test_pad(ui, curve, &mut pad);
            let readout = if pad.calibrating {
                format!(
                    "Draw a few strokes at your normal inking pressure ({}/{MIN_CALIBRATION_SAMPLES})",
                    pad.samples.len().min(MIN_CALIBRATION_SAMPLES)
                )
            } else if let Some((raw, out)) = pad.live {
                format!("Pen {:.0}% → brush {:.0}%", raw * 100.0, out * 100.0)
            } else {
                "Draw on the pad to try it.".to_string()
            };
            ui.label(RichText::new(readout).font(theme::medium(10.5)).color(t.fg_muted));
        });
    });

    let mut firm = gamma_to_firm(curve.gamma);
    if widgets::slider_row(ui, "Curve", &mut firm, 0.0..=100.0, |v| firm_label(firm_to_gamma(v)).to_string()).changed() {
        curve.gamma = firm_to_gamma(firm);
    }
    let mut full = (curve.full * 100.0).round() as f32;
    if widgets::slider_row(ui, "Full pressure at", &mut full, (PEN_FULL_MIN * 100.0) as f32..=100.0, |v| format!("{v:.0}%")).changed() {
        curve.full = full as f64 / 100.0;
    }

    ui.horizontal(|ui| {
        if pad.calibrating {
            let ready = pad.samples.len() >= MIN_CALIBRATION_SAMPLES;
            if ui.add_enabled_ui(ready, |ui| widgets::primary_button(ui, "Apply", 72.0)).inner.clicked()
                && let Some(fitted) = PenCurve::calibrate(&pad.samples)
            {
                *curve = fitted;
                pad.calibrating = false;
                pad.hint = format!("Calibrated: full pressure at {:.0}%.", fitted.full * 100.0);
                pad.segs.clear();
            }
            if widgets::secondary_button(ui, "Cancel", 72.0).clicked() {
                pad.calibrating = false;
                pad.hint.clear();
            }
        } else {
            if widgets::secondary_button(ui, "Calibrate", 88.0).clicked() {
                pad = Pad { calibrating: true, ..Pad::default() };
            }
            if widgets::secondary_button(ui, "Clear pad", 80.0).clicked() {
                pad.segs.clear();
                pad.live = None;
            }
            if *curve != PenCurve::default() && widgets::secondary_button(ui, "Reset", 64.0).clicked() {
                *curve = PenCurve::default();
                pad.hint.clear();
            }
        }
    });
    if !pad.hint.is_empty() {
        ui.label(RichText::new(&pad.hint).font(theme::medium(10.5)).color(t.fg_muted));
    }
    ui.data_mut(|d| d.insert_temp(id, pad));
    *curve != before
}

/// Pen pressure across, brush pressure up, over the identity diagonal.
fn graph(ui: &mut Ui, curve: &PenCurve, live: Option<(f64, f64)>) {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(GRAPH, GRAPH), Sense::hover());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Pen pressure curve"));
    let p = ui.painter();
    let at = |raw: f64, out: f64| pos2(rect.left() + raw as f32 * GRAPH, rect.bottom() - out as f32 * GRAPH);
    p.rect_filled(rect, t.radius_sm, t.field);
    p.rect_stroke(rect, t.radius_sm, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    p.line_segment([at(0.0, 0.0), at(1.0, 1.0)], Stroke::new(1.0, t.border));
    let pts: Vec<Pos2> = (0..=40).map(|i| i as f64 / 40.0).map(|raw| at(raw, curve.apply(raw))).collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0, t.accent)));
    if let Some((raw, out)) = live {
        p.circle_filled(at(raw.min(1.0), out), 3.5, t.fg);
    }
}

fn test_pad(ui: &mut Ui, curve: &PenCurve, pad: &mut Pad) {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width().clamp(160.0, 240.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, PAD_H), Sense::drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Pen pressure test pad"));
    // A pen's raw force this frame: tablet samples arrive as touches with force.
    let force = ui.input(|i| {
        i.events.iter().rev().find_map(|e| match e {
            egui::Event::Touch { force: Some(f), .. } => Some(*f as f64),
            _ => None,
        })
    });
    if resp.drag_started() {
        pad.last = None;
        if pad.calibrating && force.is_none() {
            pad.hint = "Calibration needs a pen; the mouse has no pressure.".into();
        }
    }
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let local = p - rect.min.to_vec2();
        let (raw, out) = match force {
            Some(raw) => (raw, curve.apply(raw)),
            None => (0.5, 0.5),
        };
        if pad.calibrating && force.is_some() && raw > 0.0 {
            pad.samples.push(raw);
        }
        let w = 0.6 + out as f32 * MAX_W;
        let (from, fw) = pad.last.unwrap_or((local, w));
        pad.segs.push((from, local, (fw + w) / 2.0));
        pad.last = Some((local, w));
        pad.live = Some((raw, out));
    }
    if resp.drag_stopped() {
        pad.last = None;
    }
    let p = ui.painter_at(rect);
    let fill = if pad.calibrating { t.accent_soft } else { t.field };
    p.rect_filled(rect, t.radius_sm, fill);
    p.rect_stroke(rect, t.radius_sm, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let o = rect.min.to_vec2();
    for &(a, b, w) in &pad.segs {
        p.line_segment([a + o, b + o], Stroke::new(w, t.fg));
        p.circle_filled(b + o, w / 2.0, t.fg);
    }
    if pad.segs.is_empty() && !pad.calibrating {
        p.text(rect.center(), Align2::CENTER_CENTER, "Test pad", theme::medium(11.0), t.fg_faint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_firm_slider_maps_to_the_exponent_range() {
        assert!((firm_to_gamma(50.0) - 1.0).abs() < 1e-12);
        assert!((firm_to_gamma(0.0) - 1.0 / 3.0).abs() < 1e-12);
        assert!((firm_to_gamma(100.0) - 3.0).abs() < 1e-12);
        assert_eq!(gamma_to_firm(0.7), 34.0);
        assert_eq!(firm_label(0.7), "Soft");
        assert_eq!(firm_label(1.0), "Linear");
    }
}
