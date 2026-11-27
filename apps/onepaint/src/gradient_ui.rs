//! The Gradient editor (happypaint `gradient-section.tsx`, spec 59): shape,
//! stops (with midpoints and opacity), Reverse and presets, for the
//! Gradient tool or, when the active layer is a Gradient fill, for that
//! layer (its stops are fixed colours; one undo step per edit).

use crate::App;
use egui::{RichText, Ui};
use onepaint_engine::Command;
use onepaint_filters::gradient::Shape;
use onepaint_filters::gradient_model::{
    ColorStop, GradientPreset, GradientState, StopSource, editable_to_fill_stops, fill_stops_to_editable, resolve_stops,
};
use onepaint_ui_kit::gradient_editor::{EdStop, GradEdit, gradient_editor};
use onepaint_ui_kit::{Tokens, theme, widgets};
use std::path::PathBuf;

pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint").map(|d| d.config_dir().join("gradients.json"))
}

pub fn load(path: Option<&PathBuf>) -> GradientState {
    GradientState::parse(path.and_then(|p| std::fs::read_to_string(p).ok()).as_deref())
}

fn save(app: &App) {
    let Some(path) = &app.gradient_path else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, app.canvas.gradient.to_json()).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn rgb(c: onepaint_color::Rgba) -> [f64; 3] {
    [c.r as f64, c.g as f64, c.b as f64]
}

/// The active layer, when it is a Gradient fill: its id and params.
fn fill_target(app: &App) -> Option<(String, onepaint_filters::Params)> {
    let l = app.session.doc().layer(app.session.active())?;
    let a = l.adjustment.as_ref().filter(|a| a.kind == "gradient-fill")?;
    Some((l.id.clone(), a.params.clone()))
}

/// What the editor works on this frame.
struct Target {
    shape: Shape,
    stops: Vec<ColorStop>,
    reverse: bool,
}

/// What the Colour panel's picker edits (happypaint v0.211.11). It edits the
/// foreground, except while a gradient editor shows a selected stop: a Bg
/// stop routes it to the background, a Colour stop (or any stop of a Gradient
/// fill layer) to that stop's own colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PickerTarget {
    Fg,
    Bg,
    Stop([f64; 3]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickerRoute {
    pub target: PickerTarget,
    /// The frame the editor last showed; a stale route edits the foreground.
    pub pass: u64,
}

impl App {
    /// Where the picker writes this frame.
    pub fn picker_target(&self, pass: u64) -> PickerTarget {
        match self.picker_route {
            Some(r) if r.pass + 1 >= pass => r.target,
            _ => PickerTarget::Fg,
        }
    }
}

/// Apply one editor action to the stops; the selected stop after it.
fn apply(stops: &mut Vec<ColorStop>, e: GradEdit, fg: [f64; 3], bg: [f64; 3], sel: usize) -> usize {
    match e {
        GradEdit::Move { stop, pos, .. } => {
            if let Some(s) = stops.get_mut(stop) {
                s.pos = f64::from(pos);
            }
            stop
        }
        GradEdit::Mid { stop, mid, .. } => {
            if let Some(s) = stops.get_mut(stop) {
                s.mid = mid.map(f64::from);
            }
            sel
        }
        GradEdit::Add { pos } => {
            // A fixed stop of the colour and opacity already there.
            let at = onepaint_filters::gradient::sample_stops(&resolve_stops(stops, fg, bg), f64::from(pos));
            stops.push(ColorStop::new(f64::from(pos), StopSource::Fixed, [at[0], at[1], at[2]], at[3]));
            stops.len() - 1
        }
        GradEdit::Delete { stop } => {
            if stops.len() > 2 && stop < stops.len() {
                stops.remove(stop);
            }
            0
        }
    }
}

/// The editor itself. Returns the edited target and whether the edit is
/// finished (a release, a click), so a fill layer can seal its undo step.
fn editor(app: &mut App, ui: &mut Ui, mut tg: Target, layer: bool) -> Option<(Target, bool)> {
    let t = Tokens::get(ui.ctx());
    let (fg, bg) = (rgb(app.session.color()), rgb(app.canvas.options.background));
    let mut changed = false;
    let mut done = false;
    let labels: Vec<&str> = Shape::ALL.iter().map(|s| s.label()).collect();
    let cur = Shape::ALL.iter().position(|s| *s == tg.shape).unwrap_or(0);
    if let Some(i) = crate::chrome::segmented(ui, &labels, cur) {
        tg.shape = Shape::ALL[i];
        (changed, done) = (true, true);
    }
    ui.add_space(6.0);
    let ed: Vec<EdStop> = tg
        .stops
        .iter()
        .map(|s| {
            let c = match s.source {
                StopSource::Fg => fg,
                StopSource::Bg => bg,
                StopSource::Fixed => s.color,
            };
            EdStop {
                pos: s.pos as f32,
                color: c.map(|v| v as f32),
                opacity: s.opacity as f32,
                mid: s.mid.map(|m| m as f32),
                tag: match s.source {
                    StopSource::Fg => Some('F'),
                    StopSource::Bg => Some('B'),
                    StopSource::Fixed => None,
                },
            }
        })
        .collect();
    let mut sel = app.gradient_sel.min(tg.stops.len().saturating_sub(1));
    // A colour the Colour panel picked for the selected stop.
    if let Some(c) = app.stop_pick.take()
        && let Some(s) = tg.stops.get_mut(sel)
        && (layer || s.source == StopSource::Fixed)
    {
        (s.source, s.color) = (StopSource::Fixed, c);
        changed = true;
        // Dragging in the picker is one undo step, sealed on release.
        done = !ui.input(|i| i.pointer.any_down());
    }
    let (_, edit) = gradient_editor(ui, &ed, &mut sel, "Gradient");
    if let Some(e) = edit {
        sel = apply(&mut tg.stops, e, fg, bg, sel);
        changed = true;
        done = !matches!(e, GradEdit::Move { commit: false, .. } | GradEdit::Mid { commit: false, .. });
    }
    // The selected stop: its colour source, opacity and Delete.
    if let Some(s) = tg.stops.get_mut(sel) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Stop").color(t.fg_muted));
            if layer {
                // A fill layer keeps fixed colours: the swatches are copied once.
                for (label, c) in [("Fg", fg), ("Bg", bg)] {
                    if widgets::secondary_button(ui, label, 40.0).on_hover_text("Use this swatch's colour").clicked() {
                        (s.source, s.color) = (StopSource::Fixed, c);
                        (changed, done) = (true, true);
                    }
                }
            } else {
                let srcs = [StopSource::Fg, StopSource::Bg, StopSource::Fixed];
                let cur = srcs.iter().position(|x| *x == s.source).unwrap_or(2);
                if let Some(i) = crate::chrome::segmented(ui, &["Fg", "Bg", "Colour"], cur) {
                    let colour = match s.source {
                        StopSource::Fg => fg,
                        StopSource::Bg => bg,
                        StopSource::Fixed => s.color,
                    };
                    (s.source, s.color) = (srcs[i], colour);
                    (changed, done) = (true, true);
                }
            }
            if layer || s.source == StopSource::Fixed {
                let c = onepaint_color::Rgba::new(s.color[0] as f32, s.color[1] as f32, s.color[2] as f32, 1.0);
                if widgets::swatch(ui, c, 22.0, false).on_hover_text("Use the current colour").clicked() {
                    (s.source, s.color) = (StopSource::Fixed, fg);
                    (changed, done) = (true, true);
                }
            }
        });
        let mut op = (s.opacity * 100.0) as f32;
        let r = widgets::slider_row(ui, "Opacity", &mut op, 0.0..=100.0, |v| format!("{v:.0}%"));
        if r.changed() {
            s.opacity = f64::from(op) / 100.0;
            changed = true;
            done |= !r.dragged();
        }
        if r.drag_stopped() {
            done = true;
        }
    }
    ui.horizontal(|ui| {
        let n = tg.stops.len();
        if ui.add_enabled_ui(n > 2, |ui| widgets::secondary_button(ui, "Delete stop", 100.0)).inner.clicked() {
            sel = apply(&mut tg.stops, GradEdit::Delete { stop: sel }, fg, bg, sel);
            (changed, done) = (true, true);
        }
        if widgets::toggle(ui, &mut tg.reverse, "Reverse").changed() {
            (changed, done) = (true, true);
        }
    });
    app.gradient_sel = sel;
    if let Some(s) = tg.stops.get(sel) {
        let target = match s.source {
            _ if layer => PickerTarget::Stop(s.color),
            StopSource::Fg => PickerTarget::Fg,
            StopSource::Bg => PickerTarget::Bg,
            StopSource::Fixed => PickerTarget::Stop(s.color),
        };
        app.picker_route = Some(PickerRoute { target, pass: ui.ctx().cumulative_pass_nr() });
    }
    changed.then_some((tg, done))
}

/// A preset's swatch: the gradient over a checker.
fn preset_swatch(ui: &mut Ui, p: &GradientPreset, fg: [f64; 3], bg: [f64; 3], selected: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(44.0, 16.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &p.name));
    let painter = ui.painter_at(rect);
    widgets::checker(&painter, rect, 4.0);
    let stops = resolve_stops(&p.stops, fg, bg);
    let mut mesh = egui::Mesh::default();
    for k in 0..=16u32 {
        let f = f64::from(k) / 16.0;
        let c = onepaint_filters::gradient::sample_stops(&stops, f);
        let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let col = egui::Color32::from_rgba_unmultiplied(b(c[0]), b(c[1]), b(c[2]), b(c[3]));
        let x = rect.left() + rect.width() * f as f32;
        mesh.colored_vertex(egui::pos2(x, rect.top()), col);
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), col);
        if k > 0 {
            let v = 2 * k;
            mesh.add_triangle(v - 2, v - 1, v);
            mesh.add_triangle(v - 1, v, v + 1);
        }
    }
    painter.add(mesh);
    let stroke = if selected { egui::Stroke::new(2.0, t.accent) } else { egui::Stroke::new(1.0, t.border) };
    painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
    resp.on_hover_text(&p.name)
}

/// The presets row: click one to use it, × deletes the user's own, + saves.
fn presets(app: &mut App, ui: &mut Ui, layer: Option<&mut Target>) -> Option<bool> {
    let t = Tokens::get(ui.ctx());
    let (fg, bg) = (rgb(app.session.color()), rgb(app.canvas.options.background));
    widgets::section_label(ui, "Presets");
    let mut picked = None;
    let mut delete = None;
    let mut add = false;
    let g = &app.canvas.gradient;
    ui.horizontal_wrapped(|ui| {
        for p in g.all_presets() {
            let sel = layer.is_none() && g.active_preset_id.as_deref() == Some(p.id.as_str());
            ui.horizontal(|ui| {
                if preset_swatch(ui, &p, fg, bg, sel).clicked() {
                    picked = Some(p.clone());
                }
                if g.presets.iter().any(|q| q.id == p.id) && ui.small_button("×").on_hover_text(format!("Delete {}", p.name)).clicked() {
                    delete = Some(p.id.clone());
                }
            });
        }
        if ui.small_button("+").on_hover_text("Save these stops as a preset").clicked() {
            add = true;
        }
    });
    ui.label(RichText::new("Fg / Bg stops follow the swatches.").font(theme::medium(10.5)).color(t.fg_faint));
    let mut out = None;
    if let Some(id) = delete {
        let g = &mut app.canvas.gradient;
        g.presets.retain(|p| p.id != id);
        if g.active_preset_id.as_deref() == Some(id.as_str()) {
            g.active_preset_id = None;
        }
        save(app);
    }
    if let Some(p) = picked {
        match layer {
            // A fill layer takes the preset's colours as they are now.
            Some(tg) => {
                tg.stops = fill_stops_to_editable(&editable_to_fill_stops(&p.stops, fg, bg));
                out = Some(true);
            }
            None => {
                let g = &mut app.canvas.gradient;
                g.stops = p.stops.clone();
                g.active_preset_id = Some(p.id.clone());
                app.gradient_sel = 0;
                save(app);
            }
        }
    } else if add {
        let stops = match &layer {
            Some(tg) => tg.stops.clone(),
            None => app.canvas.gradient.stops.clone(),
        };
        let g = &mut app.canvas.gradient;
        let n = g.presets.len();
        let id = format!("g{}", onepaint_doc::new_id());
        g.presets.push(GradientPreset { id: id.clone(), name: format!("Gradient {}", n + 1), stops });
        // Saved from the tool, it becomes the tool's preset.
        if layer.is_none() {
            g.active_preset_id = Some(id);
        }
        save(app);
    }
    out
}

/// The Gradient tool's options (or the active Gradient fill layer's).
pub fn tool_panel(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    crate::dock::fit_width(ui, 330.0);
    if let Some((id, params)) = fill_target(app) {
        let name = app.session.doc().layer(&id).map(|l| l.name.clone()).unwrap_or_default();
        widgets::section_label(ui, &format!("Gradient fill · {name}"));
        layer_editor(app, ui, &id, &params);
        return;
    }
    widgets::section_label(ui, "Gradient");
    let g = &app.canvas.gradient;
    let tg = Target { shape: g.shape, stops: g.stops.clone(), reverse: app.canvas.options.gradient_reverse };
    if let Some((tg, _)) = editor(app, ui, tg, false) {
        let g = &mut app.canvas.gradient;
        if g.stops != tg.stops {
            g.active_preset_id = None;
        }
        (g.shape, g.stops) = (tg.shape, tg.stops);
        app.canvas.options.gradient_reverse = tg.reverse;
        save(app);
    }
    presets(app, ui, None);
    widgets::hairline(ui);
    ui.horizontal(|ui| {
        widgets::toggle(ui, &mut app.canvas.options.gradient_layer, "As a gradient fill layer (editable)");
        ui.label(RichText::new("BG").color(t.fg_muted));
        let bg = app.canvas.options.background;
        if widgets::swatch(ui, bg, 26.0, false).on_hover_text("Click to use the current colour as the background").clicked() {
            app.canvas.options.background = app.session.color();
        }
    });
    ui.label(RichText::new("Drag on the canvas; uses the brush opacity and the selection.").font(theme::medium(10.5)).color(t.fg_faint));
}

/// A Gradient fill layer's gradient, edited in place (Properties and the
/// tool's panel): one undo step per edit.
pub fn layer_editor(app: &mut App, ui: &mut Ui, id: &str, params: &onepaint_filters::Params) {
    let fill = onepaint_filters::default_params(onepaint_filters::Kind::GradientFill);
    let stops = params.fill_stops.clone().filter(|s| s.len() >= 2).or(fill.fill_stops).unwrap_or_default();
    let tg = Target {
        shape: params.grad_shape.as_deref().and_then(Shape::parse).unwrap_or_default(),
        stops: fill_stops_to_editable(&stops),
        reverse: params.grad_reverse.unwrap_or(false),
    };
    let mut result = editor(app, ui, tg, true);
    let mut tg_now = match &result {
        Some((t, _)) => Target { shape: t.shape, stops: t.stops.clone(), reverse: t.reverse },
        None => Target {
            shape: params.grad_shape.as_deref().and_then(Shape::parse).unwrap_or_default(),
            stops: fill_stops_to_editable(&stops),
            reverse: params.grad_reverse.unwrap_or(false),
        },
    };
    if presets(app, ui, Some(&mut tg_now)) == Some(true) {
        result = Some((tg_now, true));
    }
    if let Some((tg, done)) = result {
        let (fg, bg) = (rgb(app.session.color()), rgb(app.canvas.options.background));
        let mut p = params.clone();
        p.grad_shape = Some(tg.shape.id().into());
        p.fill_stops = Some(editable_to_fill_stops(&tg.stops, fg, bg));
        p.grad_reverse = Some(tg.reverse);
        let _ = app.session.execute(Command::SetAdjustment { id: id.to_string(), params: p });
        if done {
            let _ = app.session.execute(Command::SealHistory);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_actions_change_the_stops() {
        let mut stops = onepaint_filters::gradient_model::builtin_presets().remove(1).stops;
        let (fg, bg) = ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        // A new stop takes the colour already there, as a fixed stop.
        let i = apply(&mut stops, GradEdit::Add { pos: 0.5 }, fg, bg, 0);
        assert_eq!((i, stops[2].source, stops[2].color), (2, StopSource::Fixed, [0.5, 0.0, 0.5]));
        apply(&mut stops, GradEdit::Move { stop: 2, pos: 0.25, commit: true }, fg, bg, 2);
        apply(&mut stops, GradEdit::Mid { stop: 0, mid: Some(0.25), commit: true }, fg, bg, 2);
        assert_eq!((stops[2].pos, stops[0].mid), (0.25, Some(0.25)));
        assert_eq!(apply(&mut stops, GradEdit::Delete { stop: 2 }, fg, bg, 2), 0);
        assert_eq!(stops.len(), 2);
        // Two always stay.
        apply(&mut stops, GradEdit::Delete { stop: 0 }, fg, bg, 0);
        assert_eq!(stops.len(), 2);
    }
}
