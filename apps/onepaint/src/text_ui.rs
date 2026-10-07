//! The text tool's floating editor (spec 14): live text and style controls.

use crate::App;
use egui::{Align2, Area, Id, Order, RichText, pos2, vec2};
use onepaint_engine::{Command, Tool};
use onepaint_text::{Align, FONT_STACKS, Renderer, TextSpec};
use onepaint_ui_kit::{Tokens, theme, widgets};

/// Push the edited spec to the document (creating the object on first text).
fn apply(app: &mut App) {
    let Some(te) = app.canvas.text_edit.clone() else { return };
    app.canvas.text_style = TextSpec { text: String::new(), x: 0.0, y: 0.0, rotation: 0.0, box_width: None, runs: None, ..te.spec.clone() };
    match te.target {
        Some((id, i)) => {
            let _ = app.session.execute(Command::UpdateText { id, index: i, spec: te.spec });
        }
        None if !te.spec.text.is_empty() => {
            let _ = app.session.execute(Command::AddText { spec: te.spec });
            let id = app.session.active().to_string();
            let n = app.session.texts(&id).len();
            if let Some(t) = app.canvas.text_edit.as_mut() {
                t.target = (n > 0).then(|| (id, n - 1));
            }
        }
        None => {}
    }
}

pub fn finish(app: &mut App) {
    if let Some(te) = app.canvas.text_edit.take()
        && te.spec.text.trim().is_empty()
        && let Some((id, i)) = te.target
    {
        let _ = app.session.execute(Command::DeleteText { id, index: i });
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if app.session.tool() != Tool::Text {
        if app.canvas.text_edit.is_some() {
            finish(app);
        }
        return;
    }
    let Some(te) = app.canvas.text_edit.clone() else { return };
    let t = Tokens::get(ctx);
    // Anchor under the text block, clamped on screen.
    let spec = if te.spec.text.is_empty() { TextSpec { text: " ".into(), ..te.spec.clone() } } else { te.spec.clone() };
    let quad = Renderer::new().quad(&spec).unwrap_or([[te.spec.x, te.spec.y]; 4]);
    let bottom = quad
        .iter()
        .map(|p| app.canvas.view.doc_to_screen(*p))
        .fold([f64::INFINITY, f64::NEG_INFINITY], |a, p| [a[0].min(p[0]), a[1].max(p[1])]);
    let screen = ctx.content_rect();
    let pos = pos2((bottom[0] as f32).clamp(70.0, screen.right() - 360.0), (bottom[1] as f32 + 14.0).clamp(70.0, screen.bottom() - 330.0));
    let mut changed = false;
    let mut done = false;
    let mut action: Option<Command> = None;
    Area::new(Id::new("text-editor")).order(Order::Foreground).pivot(Align2::LEFT_TOP).fixed_pos(pos).show(ctx, |ui| {
        widgets::popover_frame(ui).show(ui, |ui| {
            ui.set_width(330.0);
            let Some(ed) = app.canvas.text_edit.as_mut() else { return };
            let r =
                ui.add(egui::TextEdit::multiline(&mut ed.spec.text).desired_rows(3).desired_width(f32::INFINITY).hint_text("Type here…"));
            if ed.target.is_none() && ed.spec.text.is_empty() {
                r.request_focus();
            }
            changed |= r.changed();
            ui.horizontal(|ui| {
                let current = FONT_STACKS.iter().find(|f| f.1 == ed.spec.font_family).map_or("Custom", |f| f.0);
                egui::ComboBox::from_id_salt("font").selected_text(current).width(110.0).show_ui(ui, |ui| {
                    for (name, stack) in FONT_STACKS {
                        if ui.selectable_label(ed.spec.font_family == stack, name).clicked() {
                            ed.spec.font_family = stack.to_string();
                            changed = true;
                        }
                    }
                });
                changed |= ui.add(egui::DragValue::new(&mut ed.spec.font_size_px).range(4.0..=1000.0).suffix(" px").speed(0.5)).changed();
                changed |= ui.toggle_value(&mut ed.spec.bold, RichText::new("B").strong()).changed();
                changed |= ui.toggle_value(&mut ed.spec.italic, RichText::new("I").italics()).changed();
            });
            ui.horizontal(|ui| {
                for (a, l) in [(Align::Left, "Left"), (Align::Center, "Centre"), (Align::Right, "Right")] {
                    if ui.selectable_label(ed.spec.align == a, l).clicked() {
                        ed.spec.align = a;
                        changed = true;
                    }
                }
                ui.label(RichText::new("Line").color(t.fg_muted));
                changed |= ui.add(egui::DragValue::new(&mut ed.spec.line_height).range(0.5..=3.0).speed(0.01)).changed();
            });
            ui.horizontal(|ui| {
                let mut wrap = ed.spec.box_width.is_some();
                if widgets::toggle(ui, &mut wrap, "Wrap").changed() {
                    ed.spec.box_width = wrap.then_some(300.0);
                    changed = true;
                }
                if let Some(w) = ed.spec.box_width.as_mut() {
                    changed |= ui.add(egui::DragValue::new(w).range(8.0..=10000.0).suffix(" px")).changed();
                }
            });
            ui.horizontal(|ui| {
                let mut ow = ed.spec.outline_width.unwrap_or(0.0);
                ui.label(RichText::new("Outline").color(t.fg_muted));
                if ui.add(egui::DragValue::new(&mut ow).range(0.0..=40.0).suffix(" px")).changed() {
                    ed.spec.outline_width = (ow > 0.0).then_some(ow);
                    changed = true;
                }
                let mut bg = ed.spec.bg_color.is_some();
                if widgets::toggle(ui, &mut bg, "Background").changed() {
                    ed.spec.bg_color = bg.then(|| "#ffffff".to_string());
                    changed = true;
                }
            });
            ui.label(
                RichText::new("Colour, outline and background swatches take the current colour:")
                    .font(theme::medium(10.5))
                    .color(t.fg_faint),
            );
            ui.horizontal(|ui| {
                let c = app.session.color();
                if widgets::secondary_button(ui, "Text colour", 0.0).clicked() {
                    ed.spec.color = c.to_hex();
                    changed = true;
                }
                if widgets::secondary_button(ui, "Outline colour", 0.0).clicked() {
                    ed.spec.outline_color = Some(c.to_hex());
                    changed = true;
                }
                if ed.spec.bg_color.is_some() && widgets::secondary_button(ui, "Background", 0.0).clicked() {
                    ed.spec.bg_color = Some(c.to_hex());
                    changed = true;
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Done", 80.0).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = true;
                }
                if let Some((id, i)) = ed.target.clone()
                    && widgets::secondary_button(ui, "Delete", 70.0).clicked()
                {
                    action = Some(Command::DeleteText { id, index: i });
                }
                if let Some((id, _)) = ed.target.clone()
                    && widgets::secondary_button(ui, "Rasterise", 80.0).on_hover_text("Turn the text layer into pixels").clicked()
                {
                    action = Some(Command::RasterizeText { id });
                }
            });
            let _ = vec2(0.0, 0.0);
        });
    });
    if let Some(cmd) = action {
        let _ = app.session.execute(cmd);
        app.canvas.text_edit = None;
        return;
    }
    if changed {
        apply(app);
    }
    if done {
        finish(app);
    }
}
