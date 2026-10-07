//! Floating chrome over the canvas: top bar, side rail, popovers, toasts.
//! Canvas-first, Procreate-like layout (docs/design-system.md).

use crate::{App, Popover};
use egui::{Align2, Area, Id, Order, RichText, Sense, Ui, pos2, vec2};
use onepaint_doc::{BlendMode, Node};
use onepaint_engine::{Command, LayerProps, Tool};
use onepaint_ui_kit::widgets::{self, glass_frame, popover_frame};
use onepaint_ui_kit::{Tokens, icons, theme};

const M: f32 = 12.0;
const BAR_H: f32 = 44.0;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let screen = ctx.content_rect();
    top_bar(app, ctx);
    side_rail(app, ctx, screen);
    status(app, ctx, screen);
    if let Some(p) = app.popover {
        let anchor = pos2(screen.right() - M, M + BAR_H + 8.0);
        let rect = match p {
            Popover::Menu => popover_at(ctx, "menu", pos2(M, M + BAR_H + 8.0), Align2::LEFT_TOP, |ui| menu(app, ui)),
            Popover::Brushes => popover_at(ctx, "brushes", anchor, Align2::RIGHT_TOP, |ui| brushes(app, ui)),
            Popover::Layers => popover_at(ctx, "layers", anchor, Align2::RIGHT_TOP, |ui| layers(app, ui)),
            Popover::Color => popover_at(ctx, "color", anchor, Align2::RIGHT_TOP, |ui| color(app, ui)),
            Popover::Selection => popover_at(ctx, "selection", anchor, Align2::RIGHT_TOP, |ui| selection(app, ui)),
            Popover::CanvasSize => popover_at(ctx, "canvas-size", screen.center(), Align2::CENTER_CENTER, |ui| canvas_size(app, ui)),
            Popover::ImageSize => popover_at(ctx, "image-size", screen.center(), Align2::CENTER_CENTER, |ui| image_size(app, ui)),
            Popover::Adjustments => popover_at(ctx, "adjustments", anchor, Align2::RIGHT_TOP, |ui| crate::adjust::list(app, ui)),
            Popover::Filter => popover_at(ctx, "filter", anchor, Align2::RIGHT_TOP, |ui| crate::adjust::filter_panel(app, ui)),
            Popover::AdjustLayer => popover_at(ctx, "adjust-layer", anchor, Align2::RIGHT_TOP, |ui| crate::adjust::layer_panel(app, ui)),
            Popover::GradientOptions => popover_at(ctx, "gradient", anchor, Align2::RIGHT_TOP, |ui| gradient_options(app, ui)),
            Popover::ShapeOptions => popover_at(ctx, "shape", anchor, Align2::RIGHT_TOP, |ui| shape_options(app, ui)),
            Popover::SmudgeOptions => popover_at(ctx, "smudge", anchor, Align2::RIGHT_TOP, |ui| smudge_options(app, ui)),
            Popover::MixerOptions => popover_at(ctx, "mixer", anchor, Align2::RIGHT_TOP, |ui| mixer_options(app, ui)),
            Popover::Guides => popover_at(ctx, "guides", anchor, Align2::RIGHT_TOP, |ui| guides(app, ui)),
            Popover::LiquifyOptions => popover_at(ctx, "liquify", anchor, Align2::RIGHT_TOP, |ui| liquify_options(app, ui)),
            Popover::Recover => popover_at(ctx, "recover", screen.center(), Align2::CENTER_CENTER, |ui| recover(app, ui)),
            Popover::Fill => popover_at(ctx, "fill", anchor, Align2::RIGHT_TOP, |ui| fill_options(app, ui)),
            Popover::NewDoc => popover_at(ctx, "newdoc", screen.center(), Align2::CENTER_CENTER, |ui| new_doc(app, ui)),
            Popover::Confirm => popover_at(ctx, "confirm", screen.center(), Align2::CENTER_CENTER, |ui| confirm(app, ui)),
        };
        // Click outside closes the popover (the top bar handles its own toggles).
        let pressed = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        if let Some(p) = pressed
            && !rect.contains(p)
            && p.y > M + BAR_H + 4.0
            && !matches!(app.popover, Some(Popover::Confirm | Popover::Filter))
        {
            app.popover = None;
        }
    }
    if app.session.transform_active() {
        transform_bar(app, ctx, screen);
    }
    toast(app, ctx, screen);
}

fn transform_bar(app: &mut App, ctx: &egui::Context, screen: egui::Rect) {
    Area::new(Id::new("transform-bar")).order(Order::Middle).pivot(Align2::CENTER_TOP).fixed_pos(pos2(screen.center().x, M)).show(
        ctx,
        |ui| {
            glass_frame(ui).show(ui, |ui| {
                ui.set_height(BAR_H - 12.0);
                ui.horizontal_centered(|ui| {
                    let s = &mut app.session;
                    let c = &mut app.canvas;
                    if icons::button(ui, "arrow-left-right", 32.0, false, "Flip horizontal").clicked() {
                        c.xform_about_centre(s, |p| onepaint_engine::transform::TransformParams { sx: -p.sx, ..p });
                    }
                    if icons::button(ui, "arrow-up-down", 32.0, false, "Flip vertical").clicked() {
                        c.xform_about_centre(s, |p| onepaint_engine::transform::TransformParams { sy: -p.sy, ..p });
                    }
                    if icons::button(ui, "rotate-ccw", 32.0, false, "Rotate −15°").clicked() {
                        c.xform_about_centre(s, |p| onepaint_engine::transform::TransformParams {
                            rotation: p.rotation - 15f64.to_radians(),
                            ..p
                        });
                    }
                    if icons::button(ui, "rotate-cw", 32.0, false, "Rotate +15°").clicked() {
                        c.xform_about_centre(s, |p| onepaint_engine::transform::TransformParams {
                            rotation: p.rotation + 15f64.to_radians(),
                            ..p
                        });
                    }
                    widgets::vline(ui, 24.0);
                    if widgets::secondary_button(ui, "Cancel", 80.0).clicked() {
                        app.cancel_transform();
                    }
                    if widgets::primary_button(ui, "Apply", 80.0).clicked() {
                        app.commit_transform();
                    }
                });
            });
        },
    );
}

fn popover_at(ctx: &egui::Context, id: &str, pos: egui::Pos2, align: Align2, body: impl FnOnce(&mut Ui)) -> egui::Rect {
    Area::new(Id::new(("popover", id)))
        .order(Order::Foreground)
        .pivot(align)
        .fixed_pos(pos)
        .show(ctx, |ui| popover_frame(ui).show(ui, body).response.rect)
        .inner
}

fn toggle_popover(app: &mut App, p: Popover) {
    app.popover = if app.popover == Some(p) { None } else { Some(p) };
}

fn top_bar(app: &mut App, ctx: &egui::Context) {
    let t = Tokens::get(ctx);
    Area::new(Id::new("topbar-left")).order(Order::Middle).fixed_pos(pos2(M, M)).show(ctx, |ui| {
        glass_frame(ui).show(ui, |ui| {
            ui.set_height(BAR_H - 12.0);
            ui.horizontal_centered(|ui| {
                if icons::button(ui, "ellipsis", 32.0, app.popover == Some(Popover::Menu), "Menu").clicked() {
                    toggle_popover(app, Popover::Menu);
                }
                ui.label(RichText::new("onepaint").font(theme::semibold(14.0)).color(t.fg));
                widgets::vline(ui, 24.0);
                let name = format!("{}{}", app.session.doc().name, if app.dirty() { " •" } else { "" });
                ui.label(RichText::new(name).color(t.fg_muted));
                ui.add_space(4.0);
            });
        });
    });
    Area::new(Id::new("topbar-right"))
        .order(Order::Middle)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(pos2(ctx.content_rect().right() - M, M))
        .show(ctx, |ui| {
            glass_frame(ui).show(ui, |ui| {
                ui.set_height(BAR_H - 12.0);
                ui.horizontal_centered(|ui| {
                    let tool = app.session.tool();
                    if icons::button(ui, "move", 32.0, tool == Tool::Move, "Move / transform (V)").clicked() {
                        app.set_tool(Tool::Move);
                    }
                    let selecting = is_select_tool(tool);
                    let sel_icon = select_icon(app.last_select_tool);
                    if icons::button(ui, sel_icon, 32.0, selecting, "Select (M, L, W)").clicked() {
                        if selecting {
                            toggle_popover(app, Popover::Selection);
                        } else {
                            let t = app.last_select_tool;
                            app.set_tool(t);
                        }
                    }
                    if icons::button(ui, "paint-bucket", 32.0, tool == Tool::Fill, "Fill (G)").clicked() {
                        if tool == Tool::Fill {
                            toggle_popover(app, Popover::Fill);
                        }
                        app.set_tool(Tool::Fill);
                    }
                    if icons::button(ui, "droplet", 32.0, tool == Tool::Gradient, "Gradient").clicked() {
                        if tool == Tool::Gradient {
                            toggle_popover(app, Popover::GradientOptions);
                        }
                        app.set_tool(Tool::Gradient);
                    }
                    if icons::button(ui, "square", 32.0, tool == Tool::Shape, "Shape (U)").clicked() {
                        if tool == Tool::Shape {
                            toggle_popover(app, Popover::ShapeOptions);
                        }
                        app.set_tool(Tool::Shape);
                    }
                    if icons::button(ui, "type", 32.0, tool == Tool::Text, "Text (T)").clicked() {
                        app.set_tool(Tool::Text);
                    }
                    if icons::button(ui, "pipette", 32.0, tool == Tool::Eyedropper, "Eyedropper (I)").clicked() {
                        app.set_tool(Tool::Eyedropper);
                    }
                    let sym_on = app.session.guides().symmetry.is_on();
                    if icons::button(ui, "grid-3x3", 32.0, sym_on || app.popover == Some(Popover::Guides), "Guides: symmetry (Y)").clicked()
                    {
                        toggle_popover(app, Popover::Guides);
                    }
                    if icons::button(
                        ui,
                        "sparkles",
                        32.0,
                        matches!(app.popover, Some(Popover::Adjustments | Popover::Filter)),
                        "Adjustments",
                    )
                    .clicked()
                    {
                        if app.session.filter_active() {
                            app.popover = Some(Popover::Filter);
                        } else {
                            toggle_popover(app, Popover::Adjustments);
                        }
                    }
                    widgets::vline(ui, 24.0);
                    if icons::button(ui, "brush", 32.0, tool == Tool::Brush, "Brush (B)").clicked() {
                        if tool == Tool::Brush {
                            toggle_popover(app, Popover::Brushes);
                        }
                        app.set_tool(Tool::Brush);
                    }
                    if icons::button(ui, "eraser", 32.0, tool == Tool::Eraser, "Eraser (E)").clicked() {
                        if tool == Tool::Eraser {
                            toggle_popover(app, Popover::Brushes);
                        }
                        app.set_tool(Tool::Eraser);
                    }
                    if icons::button(ui, "fingerprint", 32.0, tool == Tool::Smudge, "Smudge (S)").clicked() {
                        if tool == Tool::Smudge {
                            toggle_popover(app, Popover::SmudgeOptions);
                        }
                        app.set_tool(Tool::Smudge);
                    }
                    if icons::button(ui, "palette", 32.0, tool == Tool::Mixer, "Mixer brush (Shift+B)").clicked() {
                        if tool == Tool::Mixer {
                            toggle_popover(app, Popover::MixerOptions);
                        }
                        app.set_tool(Tool::Mixer);
                    }
                    if icons::button(ui, "waves", 32.0, tool == Tool::Liquify, "Liquify").clicked() {
                        if tool == Tool::Liquify {
                            toggle_popover(app, Popover::LiquifyOptions);
                        }
                        app.set_tool(Tool::Liquify);
                    }
                    if icons::button(ui, "sliders-horizontal", 32.0, app.popover == Some(Popover::Brushes), "Brush library").clicked() {
                        toggle_popover(app, Popover::Brushes);
                    }
                    widgets::vline(ui, 24.0);
                    if icons::button(ui, "layers", 32.0, app.popover == Some(Popover::Layers), "Layers").clicked() {
                        toggle_popover(app, Popover::Layers);
                    }
                    let c = app.session.color();
                    if widgets::swatch(ui, c, 32.0, app.popover == Some(Popover::Color)).on_hover_text("Colour").clicked() {
                        toggle_popover(app, Popover::Color);
                    }
                });
            });
        });
}

fn side_rail(app: &mut App, ctx: &egui::Context, screen: egui::Rect) {
    let t = Tokens::get(ctx);
    Area::new(Id::new("rail")).order(Order::Middle).pivot(Align2::LEFT_CENTER).fixed_pos(pos2(M, screen.center().y)).show(ctx, |ui| {
        glass_frame(ui).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let size = app.session.current_brush().size;
                // Size on a perceptual (sqrt) scale.
                let mut s = (size as f32).sqrt();
                if widgets::vslider(ui, &mut s, 1.0..=31.6, 150.0, "Brush size  [ ]").changed() {
                    let _ = app.session.execute(Command::SetBrushSize { size: (s * s).round() as f64 });
                    app.canvas.size_flash = 1.0;
                }
                ui.label(RichText::new(format!("{}", app.session.current_brush().size.round())).font(theme::mono(10.5)).color(t.fg_faint));
                ui.add_space(6.0);
                let mut o = app.session.current_brush().opacity as f32;
                if widgets::vslider(ui, &mut o, 0.01..=1.0, 120.0, "Brush opacity").changed() {
                    let _ = app.session.execute(Command::SetBrushOpacity { opacity: o as f64 });
                }
                ui.label(RichText::new(format!("{}%", (o * 100.0).round())).font(theme::mono(10.5)).color(t.fg_faint));
                ui.add_space(6.0);
                ui.add_enabled_ui(app.session.can_undo(), |ui| {
                    if icons::button(ui, "undo-2", 32.0, false, "Undo (Ctrl+Z)").clicked() {
                        let _ = app.session.execute(Command::Undo);
                    }
                });
                ui.add_enabled_ui(app.session.can_redo(), |ui| {
                    if icons::button(ui, "redo-2", 32.0, false, "Redo (Ctrl+Shift+Z)").clicked() {
                        let _ = app.session.execute(Command::Redo);
                    }
                });
            });
        });
    });
}

fn status(app: &mut App, ctx: &egui::Context, screen: egui::Rect) {
    let t = Tokens::get(ctx);
    Area::new(Id::new("status")).order(Order::Middle).pivot(Align2::LEFT_BOTTOM).fixed_pos(pos2(M, screen.bottom() - M)).show(ctx, |ui| {
        glass_frame(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                let v = &app.canvas.view;
                let zoom = format!("{:.0}%", v.scale * 100.0);
                if ui
                    .add(egui::Label::new(RichText::new(zoom).font(theme::mono(11.5)).color(t.fg)).sense(Sense::click()))
                    .on_hover_text("Fit (Ctrl+0)")
                    .clicked()
                {
                    app.canvas.view.fit();
                }
                let deg = app.canvas.view.rotation.to_degrees().rem_euclid(360.0);
                if deg.abs() > 0.05
                    && ui
                        .add(
                            egui::Label::new(RichText::new(format!("{deg:.0}°")).font(theme::mono(11.5)).color(t.fg_muted))
                                .sense(Sense::click()),
                        )
                        .on_hover_text("Reset rotation")
                        .clicked()
                {
                    let c = [app.canvas.view.view_size[0] / 2.0, app.canvas.view.view_size[1] / 2.0];
                    let r = app.canvas.view.rotation;
                    app.canvas.view.rotate_at(c, -r);
                }
                let d = app.session.doc();
                ui.label(RichText::new(format!("{}×{}", d.width, d.height)).font(theme::mono(11.5)).color(t.fg_faint));
                if let Some([x, y]) = app.canvas.hover_doc {
                    ui.label(RichText::new(format!("{:.0}, {:.0}", x, y)).font(theme::mono(11.5)).color(t.fg_faint));
                }
                if let Some((a, b)) = app.canvas.measure {
                    let (len, _, deg) = onepaint_stroke::guide_lines::measure_between(a, b, &app.session.guides().units, d.dpi);
                    ui.label(RichText::new(format!("↔ {len}  {deg:.1}°")).font(theme::mono(11.5)).color(t.accent));
                }
            });
        });
    });
}

fn toast(app: &mut App, ctx: &egui::Context, screen: egui::Rect) {
    let now = ctx.input(|i| i.time);
    let Some((msg, until)) = app.toast.clone() else {
        return;
    };
    // Toasts created before the clock started carry a placeholder deadline.
    if until < 3.0 && now > 3.0 {
        app.toast = Some((msg.clone(), now + 2.5));
    }
    if now > until && until >= 3.0 {
        app.toast = None;
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(200));
    Area::new(Id::new("toast"))
        .order(Order::Tooltip)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(screen.center().x, screen.bottom() - 24.0))
        .show(ctx, |ui| {
            popover_frame(ui).show(ui, |ui| ui.label(msg));
        });
}

fn menu(app: &mut App, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    ui.set_width(240.0);
    if widgets::menu_item(ui, Some("file-plus"), "New canvas…", "Ctrl+N").clicked() {
        app.popover = Some(Popover::NewDoc);
        return;
    }
    if widgets::menu_item(ui, Some("folder-open"), "Open…", "Ctrl+O").clicked() {
        app.request(crate::Pending::Open, ui.ctx());
    }
    if widgets::menu_item(ui, Some("image"), "Place image…", "").clicked() {
        app.popover = None;
        app.place_dialog(ui.ctx());
    }
    if widgets::menu_item(ui, Some("check"), "Save", "Ctrl+S").clicked() {
        app.popover = None;
        app.save(false, now);
    }
    if widgets::menu_item(ui, Some("copy"), "Save as…", "Ctrl+Shift+S").clicked() {
        app.popover = None;
        app.save(true, now);
    }
    if widgets::menu_item(ui, Some("image"), "Export PNG…", "Ctrl+Shift+E").clicked() {
        app.popover = None;
        app.export_png(now);
    }
    if widgets::menu_item(ui, Some("layers"), "Export PSD…", "").clicked() {
        app.popover = None;
        app.export_psd(now);
    }
    widgets::hairline(ui);
    if widgets::menu_item(ui, Some("maximize-2"), "Canvas size…", "").clicked() {
        app.size_dialog = ([app.session.doc().width, app.session.doc().height], (1, 1), true);
        app.popover = Some(Popover::CanvasSize);
        return;
    }
    if widgets::menu_item(ui, Some("image"), "Image size…", "").clicked() {
        app.size_dialog = ([app.session.doc().width, app.session.doc().height], (1, 1), true);
        app.popover = Some(Popover::ImageSize);
        return;
    }
    widgets::hairline(ui);
    if widgets::menu_item(ui, Some("maximize-2"), "Fit to screen", "Ctrl+0").clicked() {
        app.canvas.view.fit();
    }
    if widgets::menu_item(ui, Some("zoom-in"), "Actual pixels", "Ctrl+1").clicked() {
        app.canvas.view.actual_pixels();
    }
    let (icon, label) = if app.theme == onepaint_ui_kit::ThemeKind::Dark { ("sun", "Light theme") } else { ("moon", "Dark theme") };
    if widgets::menu_item(ui, Some(icon), label, "").clicked() {
        app.toggle_theme(ui.ctx());
    }
    widgets::hairline(ui);
    let t = Tokens::get(ui.ctx());
    ui.label(
        RichText::new("Space-drag pan · Ctrl+wheel zoom · R-drag rotate · Alt-click pick · Tab hides UI")
            .font(theme::medium(10.5))
            .color(t.fg_faint),
    );
}

fn new_doc(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(320.0);
    ui.label(RichText::new("New canvas").font(theme::semibold(16.0)).color(t.fg));
    ui.add_space(8.0);
    for (name, w, h) in
        [("Screen", 1920, 1080), ("Square", 2048, 2048), ("Sketch", 2048, 1536), ("A4 · 300 dpi", 2480, 3508), ("4K", 3840, 2160)]
    {
        let sel = app.new_doc == [w, h];
        if widgets::menu_item(ui, if sel { Some("check") } else { None }, name, &format!("{w}×{h}")).clicked() {
            app.new_doc = [w, h];
        }
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Size").color(t.fg_muted));
        ui.add(egui::DragValue::new(&mut app.new_doc[0]).range(16..=16384).suffix(" px"));
        ui.label("×");
        ui.add(egui::DragValue::new(&mut app.new_doc[1]).range(16..=16384).suffix(" px"));
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Create", 120.0).clicked() {
            let [w, h] = app.new_doc;
            app.request(crate::Pending::New(w, h), ui.ctx());
        }
        if widgets::secondary_button(ui, "Cancel", 90.0).clicked() {
            app.popover = None;
        }
    });
}

fn brushes(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(340.0);
    widgets::section_label(ui, if app.session.tool() == Tool::Eraser { "Eraser" } else { "Brushes" });
    ui.add_space(4.0);
    let current = app.session.current_brush().id.clone();
    let eraser = app.session.tool() == Tool::Eraser;
    if !eraser {
        // Category chips; the list shows one category at a time (124 presets).
        let cats: Vec<String> = onepaint_brush::categories().into_iter().filter(|c| c != "Eraser").collect();
        if app.brush_category.is_empty() || !cats.contains(&app.brush_category) {
            app.brush_category = app.session.current_brush().category.clone();
            if !cats.contains(&app.brush_category) {
                app.brush_category = cats.first().cloned().unwrap_or_default();
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for c in &cats {
                let sel = *c == app.brush_category;
                let text = RichText::new(c).font(theme::medium(11.0)).color(if sel { t.on_accent } else { t.fg_muted });
                if ui.add(egui::Button::new(text).fill(if sel { t.accent } else { t.field }).corner_radius(10.0)).clicked() {
                    app.brush_category = c.clone();
                }
            }
        });
        ui.add_space(4.0);
    }
    let category = app.brush_category.clone();
    // Fit the list below the top bar so the popover never overlaps it.
    let room = (ui.ctx().content_rect().height() - ui.cursor().top() - 230.0).clamp(120.0, 360.0);
    egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
        for p in onepaint_brush::presets().into_iter().filter(|p| if eraser { p.is_eraser() } else { p.category == category }) {
            let sel = p.id == current;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
            if sel {
                ui.painter().rect_filled(rect, t.radius, t.accent_soft);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, t.radius, t.hover);
            }
            ui.painter().text(
                rect.left_top() + vec2(10.0, 8.0),
                Align2::LEFT_TOP,
                &p.name,
                theme::medium(12.5),
                if sel { t.accent } else { t.fg },
            );
            ui.painter().text(rect.right_top() + vec2(-10.0, 9.0), Align2::RIGHT_TOP, &p.category, theme::medium(10.5), t.fg_faint);
            let tex = app.thumbs.brush(ui.ctx(), &p);
            let img = egui::Rect::from_min_size(rect.left_top() + vec2(6.0, 18.0), vec2(rect.width() - 12.0, 44.0));
            egui::Image::new(&tex).tint(t.fg).paint_at(ui, img);
            if resp.clicked() {
                let _ = app.session.execute(Command::SetBrush { preset_id: p.id.clone() });
            }
        }
    });
    widgets::hairline(ui);
    let b = app.session.current_brush().clone();
    let mut size = b.size as f32;
    if widgets::slider_row(ui, "Size", &mut size, 1.0..=500.0, |v| format!("{:.0} px", v)).changed() {
        let _ = app.session.execute(Command::SetBrushSize { size: size.round() as f64 });
    }
    let mut op = b.opacity as f32;
    if widgets::slider_row(ui, "Opacity", &mut op, 0.01..=1.0, |v| format!("{:.0}%", v * 100.0)).changed() {
        let _ = app.session.execute(Command::SetBrushOpacity { opacity: op as f64 });
    }
    let mut flow = b.flow as f32;
    if widgets::slider_row(ui, "Flow", &mut flow, 0.01..=1.0, |v| format!("{:.0}%", v * 100.0)).changed() {
        let _ = app.session.execute(Command::SetBrushFlow { flow: flow as f64 });
    }
}

fn color(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(256.0);
    widgets::section_label(ui, "Colour");
    ui.add_space(4.0);
    let mut hsv = app.hsv;
    if widgets::color_picker(ui, &mut hsv, 256.0) {
        app.hsv = hsv;
        let c = hsv.to_rgba();
        let _ = app.session.execute(Command::SetColor { rgba: c });
        app.hex = c.to_hex();
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        widgets::swatch(ui, app.session.color(), 28.0, false);
        let r = ui.add(egui::TextEdit::singleline(&mut app.hex).font(theme::mono(12.5)).desired_width(90.0));
        if r.changed()
            && let Some(c) = onepaint_color::Rgba::from_hex(&app.hex)
        {
            let _ = app.session.execute(Command::SetColor { rgba: c });
            app.hsv = onepaint_ui_kit::widgets::Hsv::from_rgba(c);
        }
        ui.label(RichText::new("Alt-click canvas to pick").font(theme::medium(10.5)).color(t.fg_faint));
    });
    if !app.recent.is_empty() {
        ui.add_space(8.0);
        widgets::section_label(ui, "Recent");
        ui.horizontal_wrapped(|ui| {
            for c in app.recent.clone() {
                if widgets::swatch(ui, c, 24.0, c == app.session.color()).clicked() {
                    app.set_color(c);
                }
            }
        });
    }
    ui.add_space(8.0);
    palette_section(app, ui);
}

/// The active swatch palette: pick, manage, import/export (spec 21).
fn palette_section(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    ui.horizontal(|ui| {
        widgets::section_label(ui, "Palette");
        let active = app.palettes.active().clone();
        if let Some(name) = app.palette_rename.as_mut() {
            let r = ui.add(egui::TextEdit::singleline(name).desired_width(150.0));
            if r.lost_focus() {
                let name = app.palette_rename.take().unwrap_or_default();
                app.palettes.rename(&active.id, &name);
            } else {
                r.request_focus();
            }
            return;
        }
        egui::ComboBox::from_id_salt("palette").selected_text(&active.name).width(150.0).show_ui(ui, |ui| {
            for p in app.palettes.palettes.clone() {
                if ui.selectable_label(p.id == active.id, &p.name).clicked() {
                    app.palettes.set_active(&p.id);
                }
            }
        });
        ui.menu_button(RichText::new("…").font(theme::semibold(14.0)), |ui| {
            if ui.button("New palette").clicked() {
                app.palettes.create(None, Vec::new());
                ui.close();
            }
            if ui.button("Rename").clicked() {
                app.palette_rename = Some(active.name.clone());
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                app.palettes.duplicate(&active.id);
                ui.close();
            }
            if ui.button("Delete").clicked() {
                app.palettes.delete(&active.id);
                ui.close();
            }
            ui.separator();
            if ui.button("Import…  (.swatches .aco .ase .gpl)").clicked() {
                ui.close();
                if let Some(path) = rfd::FileDialog::new().add_filter("Palettes", &["swatches", "aco", "ase", "gpl"]).pick_file() {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("palette").to_string();
                    match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|b| onepaint_formats::palette::parse_palette(&name, &b).map_err(|e| e.to_string()))
                    {
                        Ok(list) => {
                            let n = app.palettes.import(list);
                            app.notify(format!("Imported {n} palette{}", if n == 1 { "" } else { "s" }), now);
                        }
                        Err(e) => app.notify(format!("Couldn't import: {e}"), now),
                    }
                }
            }
            if ui.button("Export as .swatches…").clicked() {
                ui.close();
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Procreate swatches", &["swatches"])
                    .set_file_name(format!("{}.swatches", active.name))
                    .save_file()
                {
                    let bytes = onepaint_formats::palette::export_swatches(&active.name, &active.colors);
                    match std::fs::write(&path, bytes) {
                        Ok(()) => app.notify(format!("Exported {}", path.display()), now),
                        Err(e) => app.notify(format!("Export failed: {e}"), now),
                    }
                }
            }
            ui.separator();
            if ui.button("New palette from canvas").clicked() {
                ui.close();
                let colors = canvas_palette(app);
                if colors.is_empty() {
                    app.notify("The canvas has no opaque colour to sample", now);
                } else {
                    app.palettes.create(Some("From canvas"), colors);
                }
            }
            if ui.button("New palette from image…").clicked() {
                ui.close();
                if let Some(path) =
                    rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"]).pick_file()
                {
                    match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|b| onepaint_formats::decode_image(&b).map_err(|e| e.to_string()))
                    {
                        Ok(img) => {
                            let small = image_thumb_rgba8(&img, 100);
                            let name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("Image").to_string();
                            app.palettes.create(Some(&name), onepaint_color::median_cut::extract_palette(&small, 12));
                        }
                        Err(e) => app.notify(format!("Couldn't read the image: {e}"), now),
                    }
                }
            }
        });
    });
    let colors = app.palettes.active().colors.clone();
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for (i, hex) in colors.iter().enumerate() {
            let Some(c) = onepaint_color::Rgba::from_hex(hex) else { continue };
            let r = widgets::swatch(ui, c, 24.0, c.to_hex() == app.session.color().to_hex()).on_hover_text(hex);
            if r.clicked() {
                app.set_color(c);
            }
            r.context_menu(|ui| {
                if ui.button("Remove swatch").clicked() {
                    remove = Some(i);
                    ui.close();
                }
            });
        }
        if icons::button(ui, "plus", 24.0, false, "Add the current colour").clicked() {
            let hex = app.session.color().to_hex();
            app.palettes.add_swatch(&hex);
        }
    });
    if let Some(i) = remove {
        app.palettes.remove_swatch(i);
    }
    if colors.is_empty() {
        ui.label(RichText::new("Empty palette: + adds the current colour.").font(theme::medium(10.5)).color(t.fg_faint));
    }
}

/// Median-cut colours from a small downscale of the flattened canvas.
fn canvas_palette(app: &App) -> Vec<String> {
    let d = app.session.doc();
    let (w, h) = (d.width as usize, d.height as usize);
    let flat = app.session.flatten();
    let step = (w.max(h) / 100).max(1);
    let mut px = Vec::new();
    for y in (0..h).step_by(step) {
        for x in (0..w).step_by(step) {
            let i = (y * w + x) * 4;
            let c = onepaint_color::Rgba::from_linear_premul([flat[i], flat[i + 1], flat[i + 2], flat[i + 3]]);
            let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            px.extend([b(c.r), b(c.g), b(c.b), b(c.a)]);
        }
    }
    onepaint_color::median_cut::extract_palette(&px, 12)
}

/// A nearest-neighbour RGBA8 thumbnail of a decoded image, at most `max` px.
fn image_thumb_rgba8(img: &onepaint_formats::DecodedImage, max: u32) -> Vec<u8> {
    let (w, h) = (img.width, img.height);
    let step = (w.max(h) / max).max(1);
    let mut out = Vec::new();
    for y in (0..h).step_by(step as usize) {
        for x in (0..w).step_by(step as usize) {
            let i = ((y * w + x) * 4) as usize;
            out.extend_from_slice(&img.rgba[i..i + 4]);
        }
    }
    out
}

struct Row {
    id: String,
    depth: usize,
    group: bool,
}

fn rows(nodes: &[Node], depth: usize, out: &mut Vec<Row>) {
    for n in nodes.iter().rev() {
        out.push(Row { id: n.id().to_string(), depth, group: n.is_group() });
        if let Node::Group(g) = n
            && !g.collapsed
        {
            rows(&g.children, depth + 1, out);
        }
    }
}

fn layers(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(300.0);
    ui.horizontal(|ui| {
        widgets::section_label(ui, "Layers");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "plus", 28.0, false, "New layer").clicked() {
                let _ = app.session.execute(Command::AddLayer);
            }
            if icons::button(ui, "folder-plus", 28.0, false, "New group").clicked() {
                let _ = app.session.execute(Command::AddGroup);
            }
        });
    });
    let mut list = Vec::new();
    rows(&app.session.doc().root, 0, &mut list);
    let doc_size = [app.session.doc().width, app.session.doc().height];
    let active = app.session.active().to_string();
    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
        let mut row_rects: Vec<(String, egui::Rect)> = Vec::new();
        let mut drag_stopped = false;
        for r in &list {
            let Some(node) = app.session.doc().find(&r.id).cloned() else {
                continue;
            };
            let sel = r.id == active;
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click_and_drag());
            row_rects.push((r.id.clone(), rect));
            if resp.drag_started() {
                app.layer_drag = Some(r.id.clone());
            }
            if resp.drag_stopped() {
                drag_stopped = true;
            }
            if app.layer_drag.as_deref() == Some(r.id.as_str()) {
                ui.painter().rect_filled(rect, t.radius, t.hover);
            }
            if let Some(c) = app.session.color_label(&r.id).and_then(|l| label_color(&l, t.dark())) {
                let bar = egui::Rect::from_min_size(rect.left_top() + vec2(1.0, 6.0), vec2(3.0, rect.height() - 12.0));
                ui.painter().rect_filled(bar, 1.5, c);
            }
            if sel {
                ui.painter().rect_filled(rect, t.radius, t.accent_soft);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, t.radius, t.hover);
            }
            let x0 = rect.left() + 6.0 + r.depth as f32 * 14.0;
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_max(pos2(x0, rect.top()), rect.max))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let vis = node.visible();
            if icons::button(&mut child, if vis { "eye" } else { "eye-off" }, 26.0, false, "Visibility").clicked() {
                let _ = app
                    .session
                    .execute(Command::SetLayerProps { id: r.id.clone(), props: LayerProps { visible: Some(!vis), ..Default::default() } });
            }
            if r.group {
                let collapsed = node.as_group().is_some_and(|g| g.collapsed);
                if icons::button(&mut child, if collapsed { "chevron-right" } else { "chevron-down" }, 22.0, false, "Expand").clicked() {
                    let _ = app.session.execute(Command::SetLayerProps {
                        id: r.id.clone(),
                        props: LayerProps { collapsed: Some(!collapsed), ..Default::default() },
                    });
                }
                let (fr, _) = child.allocate_exact_size(vec2(36.0, 34.0), Sense::hover());
                icons::paint(&child, fr, "folder-open", 20.0, t.fg_muted);
            } else if let Some(l) = node.as_layer() {
                let mask_target = sel && app.session.mask_target();
                let tex = app.thumbs.layer(child.ctx(), l, doc_size);
                let (tr, tresp) = child.allocate_exact_size(vec2(44.0, 34.0), Sense::click());
                let sz = tex.size_vec2();
                let ir = egui::Rect::from_center_size(tr.center(), sz);
                widgets::checker(child.painter(), ir, 4.0);
                child.painter().image(tex.id(), ir, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
                let edge = |on: bool| if on { egui::Stroke::new(2.0, t.accent) } else { egui::Stroke::new(1.0, t.border) };
                child.painter().rect_stroke(ir, 2.0, edge(sel && l.mask.is_some() && !mask_target), egui::StrokeKind::Outside);
                if tresp.clicked() {
                    let _ = app.session.execute(Command::SetActiveLayer { id: r.id.clone() });
                    let _ = app.session.execute(Command::SetMaskTarget { on: false });
                }
                if let Some(mtex) = app.thumbs.mask(child.ctx(), l, doc_size) {
                    let (mr, mresp) = child.allocate_exact_size(vec2(44.0, 34.0), Sense::click());
                    let mir = egui::Rect::from_center_size(mr.center(), mtex.size_vec2());
                    child.painter().image(mtex.id(), mir, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
                    child.painter().rect_stroke(mir, 2.0, edge(mask_target), egui::StrokeKind::Outside);
                    if !l.mask_enabled {
                        child.painter().line_segment([mir.left_top(), mir.right_bottom()], egui::Stroke::new(2.0, t.danger));
                    }
                    let mresp = mresp.on_hover_text("Paint on the mask (black hides, white reveals)");
                    if mresp.clicked() {
                        let _ = app.session.execute(Command::SetActiveLayer { id: r.id.clone() });
                        let _ = app.session.execute(Command::SetMaskTarget { on: true });
                    }
                }
            }
            child.vertical(|ui| {
                ui.add_space(6.0);
                let name_color = if vis { t.fg } else { t.fg_faint };
                ui.label(RichText::new(node.name()).font(theme::medium(12.5)).color(name_color));
                let mut tags = vec![node.blend_mode().label().to_string()];
                if node.opacity() < 1.0 {
                    tags.push(format!("{:.0}%", node.opacity() * 100.0));
                }
                if let Some(l) = node.as_layer() {
                    if l.clipped {
                        tags.push("clipped".into());
                    }
                    if l.alpha_lock {
                        tags.push("α-lock".into());
                    }
                }
                ui.label(RichText::new(tags.join(" · ")).font(theme::medium(10.5)).color(t.fg_faint));
            });
            if resp.clicked() && !sel {
                let _ = app.session.execute(Command::SetActiveLayer { id: r.id.clone() });
            }
        }
        // Drag-and-drop: show where the layer would land; drop on release.
        if let Some(dragged) = app.layer_drag.clone() {
            let pointer = ui.input(|i| i.pointer.interact_pos());
            let target = pointer.and_then(|p| {
                let clamped = row_rects.first().map_or(p.y, |f| p.y.clamp(f.1.top(), row_rects.last().map_or(p.y, |l| l.1.bottom() - 1.0)));
                row_rects
                    .iter()
                    .find(|(_, r)| clamped >= r.top() && clamped < r.bottom())
                    .map(|(id, r)| (id.clone(), *r, ((clamped - r.top()) / r.height()).clamp(0.0, 1.0)))
            });
            let resolved = target.as_ref().and_then(|(id, _, f)| app.session.resolve_drop(&dragged, id, *f).map(|d| (d, id.clone(), *f)));
            if let Some(((parent, _), id, frac)) = &resolved
                && let Some((_, r, _)) = &target
            {
                let into = parent.as_deref() == Some(id.as_str());
                if into {
                    ui.painter().rect_stroke(*r, t.radius, egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
                } else {
                    let y = if *frac < 0.5 || (app.session.doc().find(id).is_some_and(|n| n.is_group())) { r.top() } else { r.bottom() };
                    ui.painter().hline(r.x_range(), y, egui::Stroke::new(2.0, t.accent));
                }
            }
            if drag_stopped || !ui.input(|i| i.pointer.any_down()) {
                if let Some(((parent, index), _, _)) = resolved {
                    let _ = app.session.execute(Command::MoveLayer { id: dragged, parent, index });
                }
                app.layer_drag = None;
            }
        }
    });
    widgets::hairline(ui);
    // Active layer properties.
    let Some(node) = app.session.doc().find(&active).cloned() else {
        return;
    };
    let mut name = node.name().to_string();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Name").color(t.fg_muted));
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
        if r.lost_focus() && name != node.name() {
            let _ = app.session.execute(Command::SetLayerProps {
                id: active.clone(),
                props: LayerProps { name: Some(name.clone()), ..Default::default() },
            });
        }
    });
    let mut op = node.opacity();
    if widgets::slider_row(ui, "Opacity", &mut op, 0.0..=1.0, |v| format!("{:.0}%", v * 100.0)).changed() {
        let _ = app
            .session
            .execute(Command::SetLayerProps { id: active.clone(), props: LayerProps { opacity: Some(op), ..Default::default() } });
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Blend").color(t.fg_muted));
        let mut mode = node.blend_mode();
        egui::ComboBox::from_id_salt("blend").selected_text(mode.label()).width(160.0).show_ui(ui, |ui| {
            for m in BlendMode::LAYER_MODES {
                ui.selectable_value(&mut mode, m, m.label());
            }
        });
        if mode != node.blend_mode() {
            let _ = app
                .session
                .execute(Command::SetLayerProps { id: active.clone(), props: LayerProps { blend_mode: Some(mode), ..Default::default() } });
        }
    });
    if let Some(l) = node.as_layer() {
        ui.horizontal(|ui| {
            if l.mask.is_none() {
                if widgets::secondary_button(ui, "Add mask", 90.0).on_hover_text("From the selection, if there is one").clicked() {
                    let _ = app.session.execute(Command::AddMask);
                }
            } else {
                let mut on = l.mask_enabled;
                if widgets::toggle(ui, &mut on, "Mask").changed() {
                    let _ = app.session.execute(Command::SetMaskEnabled { id: active.clone(), enabled: on });
                }
                if widgets::secondary_button(ui, "Apply", 64.0).on_hover_text("Bake the mask into the layer").clicked() {
                    let _ = app.session.execute(Command::ApplyMask { id: active.clone() });
                }
                if widgets::secondary_button(ui, "Delete", 64.0).on_hover_text("Remove the mask").clicked() {
                    let _ = app.session.execute(Command::DeleteMask { id: active.clone() });
                }
            }
        });
        ui.horizontal(|ui| {
            let mut clip = l.clipped;
            if widgets::toggle(ui, &mut clip, "Clip").changed() {
                let _ = app.session.execute(Command::SetLayerProps {
                    id: active.clone(),
                    props: LayerProps { clipped: Some(clip), ..Default::default() },
                });
            }
            let mut al = l.alpha_lock;
            if widgets::toggle(ui, &mut al, "Alpha lock").changed() {
                let _ = app.session.execute(Command::SetLayerProps {
                    id: active.clone(),
                    props: LayerProps { alpha_lock: Some(al), ..Default::default() },
                });
            }
        });
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Label").color(t.fg_muted));
        let current = app.session.color_label(&active);
        if ui
            .add(egui::Button::new(RichText::new("∅").color(t.fg_muted)).min_size(vec2(20.0, 20.0)).fill(if current.is_none() {
                t.hover
            } else {
                t.field
            }))
            .on_hover_text("No label")
            .clicked()
        {
            let _ = app.session.execute(Command::SetColorLabel { id: active.clone(), label: None });
        }
        for name in onepaint_engine::Session::COLOR_LABELS {
            let Some(c) = label_color(name, t.dark()) else { continue };
            let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
            ui.painter().circle_filled(r.center(), 7.0, c);
            if current.as_deref() == Some(name) {
                ui.painter().circle_stroke(r.center(), 9.0, egui::Stroke::new(2.0, t.fg));
            }
            if resp.on_hover_text(name).clicked() {
                let _ = app.session.execute(Command::SetColorLabel { id: active.clone(), label: Some(name.to_string()) });
            }
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let path = app.session.doc().path_of(&active).unwrap_or_default();
        let idx = path.last().copied().unwrap_or(0);
        let parent = app.session.doc().parent_of(&active).flatten();
        let siblings = app.session.doc().children(parent.as_deref()).map_or(0, |c| c.len());
        if icons::button(ui, "chevron-up", 28.0, false, "Move up").clicked() && idx + 1 < siblings {
            let _ = app.session.execute(Command::MoveLayer { id: active.clone(), parent: parent.clone(), index: idx + 1 });
        }
        if icons::button(ui, "chevron-down", 28.0, false, "Move down").clicked() && idx > 0 {
            let _ = app.session.execute(Command::MoveLayer { id: active.clone(), parent: parent.clone(), index: idx - 1 });
        }
        if node.as_layer().is_some_and(|l| l.adjustment.is_some())
            && icons::button(ui, "sparkles", 28.0, false, "Edit adjustment").clicked()
        {
            app.curve_tab = 0;
            app.popover = Some(Popover::AdjustLayer);
            return;
        }
        if icons::button(ui, "copy", 28.0, false, "Duplicate").clicked() {
            let _ = app.session.execute(Command::DuplicateLayer { id: active.clone() });
        }
        if !node.is_group() && icons::button(ui, "blend", 28.0, false, "Merge down").clicked() {
            let _ = app.session.execute(Command::MergeDown { id: active.clone() });
        }
        if !node.is_group() && icons::button(ui, "brush-cleaning", 28.0, false, "Clear layer").clicked() {
            let _ = app.session.execute(Command::ClearLayer { id: active.clone() });
        }
        if icons::button(ui, "trash", 28.0, false, "Delete").clicked() {
            let _ = app.session.execute(Command::DeleteLayer { id: active.clone() });
        }
    });
}

fn confirm(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    let Some(action) = app.pending else {
        app.popover = None;
        return;
    };
    ui.set_width(340.0);
    ui.label(RichText::new("Unsaved changes").font(theme::semibold(16.0)).color(t.fg));
    ui.add_space(4.0);
    ui.label(RichText::new(format!("Save “{}” before continuing?", app.session.doc().name)).color(t.fg_muted));
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Save", 90.0).clicked() {
            app.save(false, now);
            if !app.dirty() {
                app.perform(action, ui.ctx());
            }
        }
        if widgets::secondary_button(ui, "Discard", 90.0).clicked() {
            app.perform(action, ui.ctx());
        }
        if widgets::secondary_button(ui, "Cancel", 90.0).clicked() {
            app.pending = None;
            app.popover = None;
        }
    });
}

pub fn is_select_tool(t: Tool) -> bool {
    matches!(t, Tool::SelectRect | Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly | Tool::SelectWand)
}

fn select_icon(t: Tool) -> &'static str {
    match t {
        Tool::SelectEllipse => "circle-dashed",
        Tool::SelectLasso => "lasso",
        Tool::SelectPoly => "pentagon",
        Tool::SelectWand => "wand",
        _ => "square-dashed",
    }
}

/// A row of pill-shaped segmented buttons; returns the clicked index.
fn segmented(ui: &mut Ui, labels: &[&str], selected: usize) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, l) in labels.iter().enumerate() {
            let sel = i == selected;
            let text = RichText::new(*l).font(theme::medium(12.0)).color(if sel { t.on_accent } else { t.fg_muted });
            let b =
                egui::Button::new(text).fill(if sel { t.accent } else { t.field }).corner_radius(t.radius_sm).min_size(vec2(62.0, 26.0));
            if ui.add(b).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

fn selection(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(300.0);
    widgets::section_label(ui, "Select");
    ui.horizontal(|ui| {
        for (tool, name) in [
            (Tool::SelectRect, "Rectangle (M)"),
            (Tool::SelectEllipse, "Ellipse"),
            (Tool::SelectLasso, "Lasso (L)"),
            (Tool::SelectPoly, "Polygon"),
            (Tool::SelectWand, "Magic wand (W)"),
        ] {
            if icons::button(ui, select_icon(tool), 36.0, app.session.tool() == tool, name).clicked() {
                app.set_tool(tool);
            }
        }
    });
    ui.add_space(4.0);
    let modes = [
        onepaint_engine::SelectMode::Replace,
        onepaint_engine::SelectMode::Add,
        onepaint_engine::SelectMode::Subtract,
        onepaint_engine::SelectMode::Intersect,
    ];
    let cur = modes.iter().position(|m| *m == app.canvas.options.select_mode).unwrap_or(0);
    if let Some(i) = segmented(ui, &["New", "Add", "Subtract", "Intersect"], cur) {
        app.canvas.options.select_mode = modes[i];
    }
    ui.label(RichText::new("Shift adds · Alt subtracts · Shift+Alt intersects").font(theme::medium(10.5)).color(t.fg_faint));
    if app.session.tool() == Tool::SelectWand {
        ui.add_space(4.0);
        let mut tol = app.canvas.options.wand_tolerance * 100.0;
        if widgets::slider_row(ui, "Tolerance", &mut tol, 0.0..=100.0, |v| format!("{v:.0}")).changed() {
            app.canvas.options.wand_tolerance = tol / 100.0;
        }
        ui.horizontal(|ui| {
            widgets::toggle(ui, &mut app.canvas.options.wand_contiguous, "Contiguous");
            widgets::toggle(ui, &mut app.canvas.options.wand_sample_all, "All layers");
        });
    }
    if app.session.tool() == Tool::SelectPoly {
        ui.label(
            RichText::new("Click to add points · click the first point or Enter to close · Esc cancels")
                .font(theme::medium(10.5))
                .color(t.fg_faint),
        );
    }
    widgets::hairline(ui);
    let mut feather = app.session.feather();
    if widgets::slider_row(ui, "Feather", &mut feather, 0.0..=50.0, |v| format!("{v:.0} px")).changed() {
        let _ = app.session.execute(Command::SetFeather { radius: feather.round() });
    }
    let has = app.session.selection().is_some();
    let mut act = |ui: &mut Ui, icon: &str, label: &str, key: &str, cmd: Command, enabled: bool| {
        ui.add_enabled_ui(enabled, |ui| {
            if widgets::menu_item(ui, Some(icon), label, key).clicked() {
                let _ = app.session.execute(cmd);
            }
        });
    };
    act(ui, "maximize-2", "Select all", "Ctrl+A", Command::SelectAll, true);
    act(ui, "x", "Deselect", "Ctrl+D", Command::Deselect, has);
    act(ui, "blend", "Invert", "Ctrl+Shift+I", Command::InvertSelection, has);
    act(ui, "plus", "Grow 2 px", "", Command::GrowSelection { delta: 2 }, has);
    act(ui, "minus", "Shrink 2 px", "", Command::GrowSelection { delta: -2 }, has);
    act(ui, "circle-dashed", "Smooth", "", Command::SmoothSelection { px: 4.0 }, has);
    act(ui, "paint-bucket", "Fill with colour", "Alt+Backspace", Command::FillSelection, has);
    act(ui, "brush-cleaning", "Clear", "Delete", Command::ClearSelected, true);
    saved_selections(app, ui, has);
}

/// Select › Save / Load Selection (spec 20).
fn saved_selections(app: &mut App, ui: &mut Ui, has: bool) {
    use onepaint_selection::Mode;
    let t = Tokens::get(ui.ctx());
    widgets::hairline(ui);
    widgets::section_label(ui, "Saved selections");
    ui.add_enabled_ui(has, |ui| {
        if widgets::menu_item(ui, Some("check"), "Save selection", "").clicked() {
            let name = format!("Selection {}", app.session.saved_selections().len() + 1);
            if let Err(e) = app.session.execute(Command::SaveSelection { name }) {
                let now = ui.input(|i| i.time);
                app.notify(e.to_string(), now);
            }
        }
    });
    let list: Vec<(String, String)> = app.session.saved_selections().iter().map(|s| (s.id.clone(), s.name.clone())).collect();
    if list.is_empty() {
        ui.label(RichText::new("Saved selections are kept with the document.").font(theme::medium(10.5)).color(t.fg_faint));
    }
    for (id, name) in list {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&name).color(t.fg));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icons::button(ui, "trash", 24.0, false, "Delete").clicked() {
                    let _ = app.session.execute(Command::DeleteSavedSelection { id: id.clone() });
                }
                for (icon, mode, tip) in
                    [("minus", Mode::Subtract, "Subtract from the selection"), ("plus", Mode::Add, "Add to the selection")]
                {
                    if icons::button(ui, icon, 24.0, false, tip).clicked() {
                        let _ = app.session.execute(Command::LoadSelection { id: id.clone(), mode });
                    }
                }
                if icons::button(ui, "square-dashed", 24.0, false, "Load (replace)").clicked() {
                    let _ = app.session.execute(Command::LoadSelection { id: id.clone(), mode: Mode::Replace });
                }
            });
        });
    }
}

fn fill_options(app: &mut App, ui: &mut Ui) {
    ui.set_width(260.0);
    widgets::section_label(ui, "Fill");
    let mut tol = app.canvas.options.fill_tolerance * 100.0;
    if widgets::slider_row(ui, "Tolerance", &mut tol, 0.0..=100.0, |v| format!("{v:.0}")).changed() {
        app.canvas.options.fill_tolerance = tol / 100.0;
    }
    let mut grow = app.canvas.options.fill_grow as f32;
    if widgets::slider_row(ui, "Grow", &mut grow, 0.0..=8.0, |v| format!("{v:.0} px")).changed() {
        app.canvas.options.fill_grow = grow.round() as u32;
    }
}

fn size_fields(app: &mut App, ui: &mut Ui, keep_aspect: bool) {
    let t = Tokens::get(ui.ctx());
    let (old_w, old_h) = (app.session.doc().width as f64, app.session.doc().height as f64);
    let [mut w, mut h] = app.size_dialog.0;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Size").color(t.fg_muted));
        let rw = ui.add(egui::DragValue::new(&mut w).range(1..=16384).suffix(" px"));
        ui.label("×");
        let rh = ui.add(egui::DragValue::new(&mut h).range(1..=16384).suffix(" px"));
        if keep_aspect && rw.changed() {
            h = ((w as f64 * old_h / old_w).round() as u32).max(1);
        } else if keep_aspect && rh.changed() {
            w = ((h as f64 * old_w / old_h).round() as u32).max(1);
        }
    });
    app.size_dialog.0 = [w, h];
}

fn dialog_buttons(app: &mut App, ui: &mut Ui, apply: Command) {
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Apply", 110.0).clicked() {
            let _ = app.session.execute(apply);
            app.popover = None;
        }
        if widgets::secondary_button(ui, "Cancel", 90.0).clicked() {
            app.popover = None;
        }
    });
}

fn canvas_size(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(320.0);
    ui.label(RichText::new("Canvas size").font(theme::semibold(16.0)).color(t.fg));
    ui.label(RichText::new("Crops or extends the canvas without resampling.").font(theme::medium(11.0)).color(t.fg_faint));
    ui.add_space(8.0);
    size_fields(app, ui, false);
    ui.add_space(6.0);
    ui.label(RichText::new("Anchor").color(t.fg_muted));
    for row in 0..3u8 {
        ui.horizontal(|ui| {
            for col in 0..3u8 {
                let sel = app.size_dialog.1 == (col, row);
                let b =
                    egui::Button::new("").fill(if sel { t.accent } else { t.field }).corner_radius(t.radius_sm).min_size(vec2(26.0, 22.0));
                if ui.add(b).clicked() {
                    app.size_dialog.1 = (col, row);
                }
            }
        });
    }
    let [w, h] = app.size_dialog.0;
    let (ow, oh) = (app.session.doc().width as i64, app.session.doc().height as i64);
    let (col, row) = app.size_dialog.1;
    // Centre offset = round((new - old) / 2), as in happypaint's anchor dialog.
    let off = |new: i64, old: i64, k: u8| match k {
        0 => 0,
        1 => ((new - old) as f64 / 2.0).round() as i32,
        _ => (new - old) as i32,
    };
    let (dx, dy) = (off(w as i64, ow, col), off(h as i64, oh, row));
    dialog_buttons(app, ui, Command::ResizeCanvas { width: w, height: h, dx, dy });
}

fn image_size(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(320.0);
    ui.label(RichText::new("Image size").font(theme::semibold(16.0)).color(t.fg));
    ui.label(RichText::new("Resamples every layer.").font(theme::medium(11.0)).color(t.fg_faint));
    ui.add_space(8.0);
    let keep = app.size_dialog.2;
    size_fields(app, ui, keep);
    let mut k = keep;
    widgets::toggle(ui, &mut k, "Keep aspect ratio");
    app.size_dialog.2 = k;
    let [w, h] = app.size_dialog.0;
    dialog_buttons(app, ui, Command::ScaleImage { width: w, height: h });
}

fn recover(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(340.0);
    ui.label(RichText::new("Recover unsaved work?").font(theme::semibold(16.0)).color(t.fg));
    ui.label(RichText::new("onepaint found an autosave from a session that didn't close cleanly.").color(t.fg_muted));
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Recover", 110.0).clicked()
            && let Some(p) = app.recover.take()
        {
            app.open_path(p);
            // Recovered work is unsaved: Save asks for a location.
            app.path = None;
            app.saved_history_len = None;
            app.popover = None;
        }
        if widgets::secondary_button(ui, "Discard", 90.0).clicked() {
            if let Some(p) = app.recover.take() {
                let _ = std::fs::remove_file(p);
            }
            app.popover = None;
        }
    });
}

fn gradient_options(app: &mut App, ui: &mut Ui) {
    use onepaint_filters::gradient::Shape as G;
    let t = Tokens::get(ui.ctx());
    ui.set_width(330.0);
    widgets::section_label(ui, "Gradient");
    let shapes = G::ALL;
    let cur = shapes.iter().position(|s| *s == app.canvas.options.gradient_shape).unwrap_or(0);
    let labels: Vec<&str> = shapes.iter().map(|s| s.label()).collect();
    if let Some(i) = segmented(ui, &labels, cur) {
        app.canvas.options.gradient_shape = shapes[i];
    }
    ui.add_space(4.0);
    if let Some(i) = segmented(ui, &["Colour → clear", "Colour → BG"], app.canvas.options.gradient_to_bg as usize) {
        app.canvas.options.gradient_to_bg = i == 1;
    }
    ui.horizontal(|ui| {
        widgets::toggle(ui, &mut app.canvas.options.gradient_reverse, "Reverse");
        ui.label(RichText::new("BG").color(t.fg_muted));
        let bg = app.canvas.options.background;
        if widgets::swatch(ui, bg, 26.0, false).on_hover_text("Click to use the current colour as the background").clicked() {
            app.canvas.options.background = app.session.color();
        }
    });
    ui.label(RichText::new("Drag on the canvas; uses the brush opacity and the selection.").font(theme::medium(10.5)).color(t.fg_faint));
}

fn shape_options(app: &mut App, ui: &mut Ui) {
    use onepaint_stroke::shape::ShapeKind as K;
    let t = Tokens::get(ui.ctx());
    ui.set_width(300.0);
    widgets::section_label(ui, "Shape");
    let kinds = [K::Line, K::Rect, K::Ellipse, K::Polygon];
    let cur = kinds.iter().position(|k| *k == app.canvas.options.shape_kind).unwrap_or(0);
    if let Some(i) = segmented(ui, &["Line", "Rect", "Ellipse", "Polygon"], cur) {
        app.canvas.options.shape_kind = kinds[i];
    }
    if app.canvas.options.shape_kind == K::Polygon {
        let mut n = app.canvas.options.shape_sides as f32;
        if widgets::slider_row(ui, "Sides", &mut n, 3.0..=12.0, |v| format!("{v:.0}")).changed() {
            app.canvas.options.shape_sides = onepaint_stroke::shape::clamp_sides(n as f64);
        }
    }
    if app.canvas.options.shape_kind != K::Line {
        widgets::toggle(ui, &mut app.canvas.options.shape_fill, "Fill");
    }
    ui.label(
        RichText::new("Shift: square / circle / 45° · Alt: from centre. Strokes with the current brush.")
            .font(theme::medium(10.5))
            .color(t.fg_faint),
    );
    ui.label(
        RichText::new("Tip: hold still at the end of any brush stroke to straighten it into a shape.")
            .font(theme::medium(10.5))
            .color(t.fg_faint),
    );
}

fn smudge_options(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(280.0);
    widgets::section_label(ui, "Smudge");
    let mut v = (app.session.paint_settings().smudge_strength * 100.0) as f32;
    if widgets::slider_row(ui, "Strength", &mut v, 0.0..=100.0, |v| format!("{v:.0}%")).changed() {
        let _ = app.session.execute(Command::SetSmudgeStrength { strength: v as f64 / 100.0 });
    }
    ui.label(
        RichText::new("Smears with the current brush's size, hardness, tip and pressure.").font(theme::medium(10.5)).color(t.fg_faint),
    );
}

/// Photoshop's Mixer Brush combinations (`utils/mixer-presets.ts`): (name, wet, load, mix).
const MIXER_PRESETS: [(&str, f64, f64, f64); 12] = [
    ("Dry", 0.0, 0.5, 0.0),
    ("Dry, Light Load", 0.0, 0.15, 0.0),
    ("Dry, Heavy Load", 0.0, 1.0, 0.0),
    ("Moist", 0.25, 0.4, 0.3),
    ("Moist, Light Mix", 0.25, 0.4, 0.1),
    ("Moist, Heavy Mix", 0.25, 0.4, 0.6),
    ("Wet", 0.6, 0.3, 0.5),
    ("Wet, Light Mix", 0.6, 0.3, 0.25),
    ("Wet, Heavy Mix", 0.6, 0.3, 0.75),
    ("Very Wet", 1.0, 0.15, 0.6),
    ("Very Wet, Light Mix", 1.0, 0.15, 0.35),
    ("Very Wet, Heavy Mix", 1.0, 0.15, 0.85),
];

fn mixer_options(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(300.0);
    widgets::section_label(ui, "Mixer brush");
    let mut m = app.session.paint_settings().mixer;
    let pct = |v: f64| (v * 100.0).round() as i32;
    let current = MIXER_PRESETS
        .iter()
        .find(|p| pct(p.1) == pct(m.wet) && pct(p.2) == pct(m.load) && pct(p.3) == pct(m.mix))
        .map_or("Custom", |p| p.0);
    let mut changed = false;
    egui::ComboBox::from_id_salt("mixer-preset").selected_text(current).width(280.0).show_ui(ui, |ui| {
        for p in MIXER_PRESETS {
            if ui.selectable_label(current == p.0, p.0).clicked() {
                (m.wet, m.load, m.mix) = (p.1, p.2, p.3);
                changed = true;
            }
        }
    });
    let mut row = |ui: &mut Ui, label: &str, v: &mut f64| {
        let mut f = (*v * 100.0) as f32;
        if widgets::slider_row(ui, label, &mut f, 0.0..=100.0, |v| format!("{v:.0}%")).changed() {
            *v = f as f64 / 100.0;
            changed = true;
        }
    };
    row(ui, "Wet", &mut m.wet);
    row(ui, "Load", &mut m.load);
    row(ui, "Mix", &mut m.mix);
    if widgets::toggle(ui, &mut m.keep, "Keep colour").changed() {
        changed = true;
    }
    if changed {
        let _ = app.session.execute(Command::SetMixer { mixer: m });
    }
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Clean brush", 120.0).clicked() {
            let _ = app.session.execute(Command::CleanMixer);
        }
        if let Some(r) = app.session.mixer_reservoir() {
            let c = onepaint_color::Rgba::from_linear_premul([r[0], r[1], r[2], 1.0]);
            widgets::swatch(ui, c, 22.0, false).on_hover_text("The colour the brush holds");
        }
    });
    ui.label(
        RichText::new(
            "Wet: how much canvas paint it drags. Load: paint on the brush. Mix: canvas share of the colour. Pigment mixing is on.",
        )
        .font(theme::medium(10.5))
        .color(t.fg_faint),
    );
}

fn liquify_options(app: &mut App, ui: &mut Ui) {
    use onepaint_filters::liquify::LIQUIFY_MODES;
    let t = Tokens::get(ui.ctx());
    ui.set_width(280.0);
    widgets::section_label(ui, "Liquify");
    let ps = app.session.paint_settings();
    let (mut mode, strength) = (ps.liquify_mode, ps.liquify_strength);
    let label = LIQUIFY_MODES.iter().find(|m| m.0 == mode).map_or("Push", |m| m.1);
    let mut changed = false;
    egui::ComboBox::from_id_salt("liquify-mode").selected_text(label).width(260.0).show_ui(ui, |ui| {
        for (m, name) in LIQUIFY_MODES {
            if ui.selectable_label(mode == m, name).clicked() {
                mode = m;
                changed = true;
            }
        }
    });
    let mut v = (strength * 100.0) as f32;
    if widgets::slider_row(ui, "Strength", &mut v, 0.0..=100.0, |v| format!("{v:.0}%")).changed() {
        changed = true;
    }
    if changed {
        let _ = app.session.execute(Command::SetLiquify { mode, strength: v as f64 / 100.0 });
    }
    ui.label(RichText::new("Size follows the brush size ([ and ]).").font(theme::medium(10.5)).color(t.fg_faint));
}

fn guides(app: &mut App, ui: &mut Ui) {
    use onepaint_engine::Guides;
    use onepaint_engine::SymmetryMode as M;
    use onepaint_engine::guides::{PerspectiveSnap as PS, perspective_preset};
    let t = Tokens::get(ui.ctx());
    ui.set_width(310.0);
    let mut g = app.session.guides().clone();
    let before = g.clone();
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() - 140.0).show(ui, |ui| {
        widgets::section_label(ui, "Grid");
        widgets::toggle(ui, &mut g.grid.enabled, "Show grid");
        if g.grid.enabled {
            let mut size = g.grid.size as f32;
            if widgets::slider_row(ui, "Cell size", &mut size, 4.0..=512.0, |v| format!("{v:.0} px")).changed() {
                g.grid.size = size.round() as f64;
            }
            let mut sub = g.grid.subdivisions as f32;
            if widgets::slider_row(ui, "Subdivisions", &mut sub, 1.0..=8.0, |v| format!("{v:.0}")).changed() {
                g.grid.subdivisions = sub.round() as u32;
            }
            widgets::toggle(ui, &mut g.grid.snap, "Snap to grid");
        }
        widgets::toggle(ui, &mut g.snap_hv, "Snap horizontal / vertical");
        widgets::hairline(ui);

        widgets::section_label(ui, "Rulers and guide lines");
        widgets::toggle(ui, &mut g.rulers, "Rulers (Shift+R): drag a guide out of a bar");
        let units = ["px", "cm", "in"];
        let cur = units.iter().position(|u| *u == g.units).unwrap_or(0);
        if let Some(i) = segmented(ui, &["px", "cm", "in"], cur) {
            g.units = units[i].into();
        }
        widgets::toggle(ui, &mut g.snap_guides, "Snap to guide lines");
        if widgets::toggle(ui, &mut app.canvas.lines_editing, "Edit guides (Shift+G)").changed() && app.canvas.lines_editing {
            app.canvas.measuring = false;
        }
        if widgets::toggle(ui, &mut app.canvas.measuring, "Measure").changed() && app.canvas.measuring {
            app.canvas.lines_editing = false;
        }
        ui.horizontal(|ui| {
            let (w, h) = (app.session.doc().width as f64, app.session.doc().height as f64);
            let mut add = |ui: &mut Ui, label: &str, angle: f64| {
                if widgets::secondary_button(ui, label, 70.0).clicked() {
                    let mut lines = app.session.guides().lines.clone();
                    lines.push(onepaint_engine::guides::GuideLine::new(onepaint_doc::new_id(), w / 2.0, h / 2.0, angle));
                    let _ = app.session.execute(Command::EditGuideLines { lines });
                    let _ = app.session.execute(Command::CommitGuideLines);
                }
            };
            add(ui, "+ Horiz", 0.0);
            add(ui, "+ Vert", std::f64::consts::FRAC_PI_2);
            if !app.session.guides().lines.is_empty() && widgets::secondary_button(ui, "Clear", 60.0).clicked() {
                let _ = app.session.execute(Command::EditGuideLines { lines: Vec::new() });
                let _ = app.session.execute(Command::CommitGuideLines);
            }
        });
        widgets::hairline(ui);

        widgets::section_label(ui, "Symmetry");
        let modes = [M::Off, M::Vertical, M::Horizontal, M::Both, M::Radial];
        let cur = modes.iter().position(|m| *m == g.symmetry.mode).unwrap_or(0);
        if let Some(i) = segmented(ui, &["Off", "Vert", "Horiz", "Quad", "Radial"], cur) {
            g.symmetry.mode = modes[i];
            if g.symmetry.mode != M::Off {
                app.last_symmetry = g.symmetry.mode;
            }
        }
        if g.symmetry.mode == M::Radial {
            let mut n = g.symmetry.radial_count as f32;
            if widgets::slider_row(ui, "Spokes", &mut n, 2.0..=24.0, |v| format!("{v:.0}")).changed() {
                g.symmetry.radial_count = n.round() as u32;
            }
        }
        if g.symmetry.is_on() {
            let mut deg = g.symmetry.angle.to_degrees() as f32;
            if widgets::slider_row(ui, "Angle", &mut deg, -90.0..=90.0, |v| format!("{v:.0}°")).changed() {
                g.symmetry.angle = (deg.round() as f64).to_radians();
            }
            ui.horizontal(|ui| {
                widgets::toggle(ui, &mut app.canvas.sym_editing, "Move axis");
                if widgets::secondary_button(ui, "Centre", 80.0).clicked() {
                    (g.symmetry.cx, g.symmetry.cy) = (0.5, 0.5);
                }
            });
        } else {
            app.canvas.sym_editing = false;
        }
        widgets::hairline(ui);

        widgets::section_label(ui, "Perspective");
        widgets::toggle(ui, &mut g.perspective.enabled, "Perspective guides");
        if g.perspective.enabled {
            let n = g.perspective.points.len().clamp(1, 5);
            if let Some(i) = segmented(ui, &["1", "2", "3", "4", "5"], n - 1) {
                g.perspective.points = perspective_preset(i + 1);
            }
            let snaps = [PS::Off, PS::Soft, PS::Hard];
            let cur = snaps.iter().position(|m| *m == g.perspective.snap).unwrap_or(2);
            if let Some(i) = segmented(ui, &["No snap", "Soft", "Hard line"], cur) {
                g.perspective.snap = snaps[i];
            }
            let mut d = g.perspective.density as f32;
            if widgets::slider_row(ui, "Density", &mut d, 2.0..=32.0, |v| format!("{v:.0}")).changed() {
                g.perspective.density = d.round() as u32;
            }
            widgets::toggle(ui, &mut app.canvas.persp_editing, "Edit points (Shift+P)");
        } else {
            app.canvas.persp_editing = false;
        }
        widgets::hairline(ui);
        widgets::toggle(ui, &mut g.visible, "Show guides (snapping stays on)");
        ui.label(
            RichText::new(
                "Guides are saved with the document. Y toggles symmetry; hard perspective draws straight lines to the vanishing points.",
            )
            .font(theme::medium(10.5))
            .color(t.fg_faint),
        );
    });
    // Buttons above may have changed the lines through their own undo steps.
    g.lines = app.session.guides().lines.clone();
    let before = Guides { lines: g.lines.clone(), ..before };
    if g != before {
        let _ = app.session.execute(Command::SetGuides { guides: Box::new(g) });
    }
}

/// happypaint's layer label colours (global.css `--layer-label-*`).
fn label_color(name: &str, dark: bool) -> Option<egui::Color32> {
    let hex: u32 = match (name, dark) {
        ("red", true) => 0xe5484d,
        ("orange", true) => 0xe8862f,
        ("yellow", true) => 0xddb42c,
        ("green", true) => 0x3ec46d,
        ("blue", true) => 0x4c8dff,
        ("violet", true) => 0xa06cf0,
        ("gray", true) => 0x8a8a90,
        ("red", false) => 0xcc3237,
        ("orange", false) => 0xc46c15,
        ("yellow", false) => 0xb08e10,
        ("green", false) => 0x2a9c55,
        ("blue", false) => 0x2f74e8,
        ("violet", false) => 0x8348d6,
        ("gray", false) => 0x75757c,
        _ => return None,
    };
    Some(egui::Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8))
}
