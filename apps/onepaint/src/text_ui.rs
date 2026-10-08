//! The text tool's floating editor (specs 14 and 28): live text, per-run
//! styling of the selection, whole-object style controls and the balloon.

use crate::App;
use egui::text::LayoutJob;
use egui::{Align2, Area, Color32, Id, Order, RichText, Stroke, TextFormat, pos2};
use onepaint_color::Rgba;
use onepaint_engine::{Command, Tool};
use onepaint_text::balloon::{BalloonShape, default_balloon, tail_angle, tail_from_angle};
use onepaint_text::runs::{StyleEdit, apply_style, apply_to_object, normalize_color, replace_text, spec_runs, style_at};
use onepaint_text::{Align, FONT_STACKS, Renderer, TextSpec};
use onepaint_ui_kit::{Tokens, theme, widgets};
use std::ops::Range;

/// Balloon shapes as the menu names them (TP:300-304).
const SHAPES: [(Option<BalloonShape>, &str); 5] = [
    (None, "None"),
    (Some(BalloonShape::Oval), "Speech"),
    (Some(BalloonShape::Rounded), "Rounded"),
    (Some(BalloonShape::Burst), "Shout"),
    (Some(BalloonShape::Cloud), "Thought"),
];

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

/// A toolbar command on the selection, or on the whole object when nothing is selected.
fn style(spec: &TextSpec, sel: Option<Range<usize>>, edit: StyleEdit) -> TextSpec {
    match sel.filter(|r| r.start < r.end) {
        Some(r) => apply_style(spec, r, &edit),
        None => apply_to_object(spec, &edit),
    }
}

/// The field's rich preview of the runs, kept in step with the typed `text`.
fn rich_job(spec: &TextSpec, text: &str, fg: Color32, wrap: f32) -> LayoutJob {
    let s = replace_text(spec, text);
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let base = s.font_size_px.max(1.0);
    for r in spec_runs(&s) {
        let size = (15.0 * r.font_size_px.unwrap_or(base) / base).clamp(9.0, 30.0) as f32;
        let font_id = if r.bold.unwrap_or(s.bold) { theme::semibold(size) } else { egui::FontId::proportional(size) };
        let color = r.color.as_deref().and_then(|c| Rgba::from_hex(&normalize_color(c))).map_or(fg, widgets::to_color32);
        let line = |on: Option<bool>| if on == Some(true) { Stroke::new(1.0, color) } else { Stroke::NONE };
        let format = TextFormat {
            font_id,
            color,
            italics: r.italic.unwrap_or(s.italic),
            underline: line(r.underline),
            strikethrough: line(r.strikethrough),
            ..Default::default()
        };
        job.append(&r.text, 0.0, format);
    }
    job
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
    let pos = pos2((bottom[0] as f32).clamp(70.0, screen.right() - 360.0), (bottom[1] as f32 + 14.0).clamp(70.0, screen.bottom() - 470.0));
    let sel_id = Id::new(("text-editor-selection", te.target.clone()));
    let mut changed = false;
    let mut done = false;
    let mut action: Option<Command> = None;
    let current = app.session.color().to_hex();
    Area::new(Id::new("text-editor")).order(Order::Foreground).pivot(Align2::LEFT_TOP).fixed_pos(pos).show(ctx, |ui| {
        widgets::popover_frame(ui).show(ui, |ui| {
            ui.set_width(330.0);
            let Some(ed) = app.canvas.text_edit.as_mut() else { return };
            let before = ed.spec.clone();
            let mut text = ed.spec.text.clone();
            let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                let job = rich_job(&before, buf.as_str(), t.fg, wrap);
                ui.ctx().fonts_mut(|f| f.layout_job(job))
            };
            let out = egui::TextEdit::multiline(&mut text)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Type here…")
                .layouter(&mut layouter)
                .show(ui);
            let field = out.response.clone();
            if ed.target.is_none() && text.is_empty() {
                field.request_focus();
            }
            if field.changed() {
                ed.spec = replace_text(&before, &text);
                changed = true;
            }
            if field.has_focus()
                && let Some(cr) = out.cursor_range
            {
                let r = cr.as_sorted_char_range();
                ui.data_mut(|d| d.insert_temp(sel_id, r.start.0..r.end.0));
            }
            let sel: Option<Range<usize>> = ui.data(|d| d.get_temp(sel_id)).filter(|r: &Range<usize>| r.start < r.end);
            let state = style_at(&ed.spec, sel.clone().unwrap_or(0..usize::MAX));
            // B / I / U / S, A− / A+ and the font act on the selection, else on the object.
            let mut edit: Option<StyleEdit> = None;
            ui.horizontal(|ui| {
                let toggles = [
                    (state.bold, RichText::new("B").strong(), StyleEdit::Bold, "Bold"),
                    (state.italic, RichText::new("I").italics(), StyleEdit::Italic, "Italic"),
                    (state.underline, RichText::new("U").underline(), StyleEdit::Underline, "Underline"),
                    (state.strikethrough, RichText::new("S").strikethrough(), StyleEdit::Strikethrough, "Strikethrough"),
                ];
                for (on, label, e, tip) in toggles {
                    if ui.selectable_label(on, label).on_hover_text(tip).clicked() {
                        edit = Some(e);
                    }
                }
                if ui.button("A−").on_hover_text("Smaller").clicked() {
                    edit = Some(StyleEdit::ScaleSize(1.0 / 1.1));
                }
                if ui.button("A+").on_hover_text("Larger").clicked() {
                    edit = Some(StyleEdit::ScaleSize(1.1));
                }
                let family = state.family.as_deref().unwrap_or("");
                let name = match FONT_STACKS.iter().find(|f| f.1 == family) {
                    Some(f) => f.0,
                    None if state.family.is_none() => "Mixed",
                    None => "Custom",
                };
                egui::ComboBox::from_id_salt("font").selected_text(name).width(96.0).show_ui(ui, |ui| {
                    for (name, stack) in FONT_STACKS {
                        if ui.selectable_label(family == stack, name).clicked() {
                            edit = Some(StyleEdit::Font(stack.to_string()));
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Size").color(t.fg_muted));
                changed |= ui.add(egui::DragValue::new(&mut ed.spec.font_size_px).range(4.0..=1000.0).suffix(" px").speed(0.5)).changed();
                for (a, l) in [(Align::Left, "Left"), (Align::Center, "Centre"), (Align::Right, "Right")] {
                    if ui.selectable_label(ed.spec.align == a, l).clicked() {
                        ed.spec.align = a;
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Line").color(t.fg_muted));
                changed |= ui.add(egui::DragValue::new(&mut ed.spec.line_height).range(0.5..=3.0).speed(0.01)).changed();
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
                    if bg {
                        ed.spec.balloon = None; // a balloon is a background with a shape
                    }
                    changed = true;
                }
            });
            // Speech balloon (TP:285-344).
            ui.horizontal(|ui| {
                let shape = ed.spec.balloon.as_ref().map(|b| b.shape);
                let name = SHAPES.iter().find(|s| s.0 == shape).map_or("None", |s| s.1);
                egui::ComboBox::from_label("Balloon").selected_text(name).width(90.0).show_ui(ui, |ui| {
                    for (s, label) in SHAPES {
                        if ui.selectable_label(shape == s, label).clicked() && shape != s {
                            match s {
                                None => ed.spec.balloon = None,
                                Some(s) => {
                                    ed.spec.bg_color = None;
                                    let b = ed.spec.balloon.get_or_insert_with(|| default_balloon(s));
                                    b.shape = s;
                                }
                            }
                            changed = true;
                        }
                    }
                });
                if let Some(b) = ed.spec.balloon.as_mut() {
                    let mut sw = b.stroke_width.unwrap_or(2.0);
                    if ui
                        .add(egui::DragValue::new(&mut sw).range(0.0..=40.0).suffix(" px"))
                        .on_hover_text("Balloon outline width")
                        .changed()
                    {
                        b.stroke_width = Some(sw);
                        changed = true;
                    }
                }
            });
            if let Some(b) = ed.spec.balloon.as_mut() {
                let size = ed.spec.font_size_px;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tail").color(t.fg_muted));
                    let mut deg = tail_angle(b.tail.as_ref()) as f32;
                    ui.scope(|ui| {
                        ui.set_width(150.0);
                        if widgets::slider(ui, &mut deg, 0.0..=360.0, None)
                            .on_hover_text("Point the balloon at whoever is speaking")
                            .changed()
                        {
                            b.tail = Some(tail_from_angle(size, deg.round() as f64));
                            changed = true;
                        }
                    });
                    if ui.selectable_label(b.tail.is_none(), "No tail").clicked() {
                        b.tail = None;
                        changed = true;
                    }
                });
            }
            ui.label(RichText::new("Colour buttons take the current colour:").font(theme::medium(10.5)).color(t.fg_faint));
            ui.horizontal_wrapped(|ui| {
                if widgets::secondary_button(ui, "Text", 0.0).on_hover_text("Colour the selection, or all the text").clicked() {
                    edit = Some(StyleEdit::Color(current.clone()));
                }
                if widgets::secondary_button(ui, "Outline", 0.0).on_hover_text("Glyph outline colour").clicked() {
                    ed.spec.outline_color = Some(current.clone());
                    changed = true;
                }
                if ed.spec.bg_color.is_some() && widgets::secondary_button(ui, "Background", 0.0).clicked() {
                    ed.spec.bg_color = Some(current.clone());
                    changed = true;
                }
                if let Some(b) = ed.spec.balloon.as_mut() {
                    if widgets::secondary_button(ui, "Fill", 0.0).on_hover_text("Balloon fill").clicked() {
                        b.fill = current.clone();
                        changed = true;
                    }
                    if widgets::secondary_button(ui, "Rim", 0.0).on_hover_text("Balloon outline colour").clicked() {
                        b.stroke = Some(current.clone());
                        changed = true;
                    }
                }
            });
            if let Some(e) = edit {
                ed.spec = style(&ed.spec, sel, e);
                changed = true;
                field.request_focus(); // keep editing the same selection
            }
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
