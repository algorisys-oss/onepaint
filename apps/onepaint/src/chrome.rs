//! The chrome: the header (spec 65), the tool bar, popovers and toasts.

use crate::layout::PanelId;
use crate::{App, Popover, dock};
use egui::{Align2, Area, Id, Order, RichText, Sense, Ui, pos2, vec2};
use onepaint_doc::{BlendIf, BlendMode, Knockout, Node};
use onepaint_engine::{Command, LayerProps, Tool};
use onepaint_ui_kit::widgets::{self, popover_frame};
use onepaint_ui_kit::{Tokens, icons, theme};

pub fn show(app: &mut App, ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let stage_bottom = screen.bottom() - crate::status_bar::HEIGHT;
    let topbar = top_bar(app, ctx);
    // Everything else lives below the header.
    let full = screen;
    let screen = egui::Rect::from_min_max(pos2(full.left(), full.top() + header_h(app)), full.max);
    crate::dock::normalise(app, screen);
    let res = crate::dock::reserved(app, screen);
    let stage = egui::Rect::from_min_max(pos2(screen.left() + res.left, screen.top()), pos2(screen.right() - res.right, stage_bottom));
    crate::side_sliders::show(app, ctx, stage);
    crate::status_bar::show(app, ctx, full);
    crate::dock::show(app, ctx, screen, stage_bottom);
    let toolbar = crate::toolbar::show(app, ctx, screen, stage_bottom, screen.left());
    crate::timeline::show(app, ctx, screen);
    if let Some(p) = app.popover {
        let (anchor, side) = match app.row_anchor {
            Some((rp, at)) if Some(rp) == app.popover => (at, Align2::LEFT_TOP),
            _ => crate::toolbar::popover_anchor(app, screen),
        };
        let rect = match p {
            Popover::Brushes => popover_at(ctx, "brushes", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::Brush, brushes)),
            Popover::Layers => popover_at(ctx, "layers", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::Layers, layers)),
            Popover::Color => popover_at(ctx, "color", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::Color, color)),
            Popover::Selection => {
                popover_at(ctx, "selection", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, selection))
            }
            Popover::CanvasSize => popover_at(ctx, "canvas-size", screen.center(), Align2::CENTER_CENTER, |ui| canvas_size(app, ui)),
            Popover::ImageSize => popover_at(ctx, "image-size", screen.center(), Align2::CENTER_CENTER, |ui| image_size(app, ui)),
            Popover::Adjustments => popover_at(ctx, "adjustments", anchor, side, |ui| crate::adjust::list(app, ui)),
            Popover::Filter => popover_at(ctx, "filter", anchor, side, |ui| crate::adjust::filter_panel(app, ui)),
            Popover::AdjustLayer => popover_at(ctx, "adjust-layer", anchor, side, |ui| {
                crate::dock::with_grip(app, ui, PanelId::Properties, crate::adjust::layer_panel)
            }),
            Popover::GradientOptions => popover_at(ctx, "gradient", anchor, side, |ui| {
                crate::dock::with_grip(app, ui, PanelId::ToolOptions, crate::gradient_ui::tool_panel)
            }),
            Popover::ShapeOptions => {
                popover_at(ctx, "shape", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, shape_options))
            }
            Popover::SmudgeOptions => {
                popover_at(ctx, "smudge", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, smudge_options))
            }
            Popover::MixerOptions => {
                popover_at(ctx, "mixer", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, mixer_options))
            }
            Popover::Guides => popover_at(ctx, "guides", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::Guides, guides)),
            Popover::LiquifyOptions => {
                popover_at(ctx, "liquify", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, liquify_options))
            }
            Popover::PenOptions => {
                popover_at(ctx, "pen", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, pen_options))
            }
            Popover::ClipboardMenu => popover_at(ctx, "clipboard", screen.center(), Align2::CENTER_CENTER, |ui| clipboard_menu(app, ui)),
            Popover::PanelOptions => {
                popover_at(ctx, "panel", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, crate::panel_ui::options))
            }
            Popover::Gallery => popover_at(ctx, "gallery", screen.center(), Align2::CENTER_CENTER, |ui| crate::gallery::show(app, ui)),
            Popover::Help => popover_at(ctx, "help", screen.center(), Align2::CENTER_CENTER, |ui| crate::help::show(app, ui)),
            Popover::Manual => popover_at(ctx, "manual", screen.center(), Align2::CENTER_CENTER, |ui| crate::manual::show(app, ui)),
            Popover::Settings => popover_at(ctx, "settings", screen.center(), Align2::CENTER_CENTER, |ui| crate::settings::dialog(app, ui)),
            Popover::Paper => popover_at(ctx, "paper", screen.center(), Align2::CENTER_CENTER, |ui| paper_dialog(app, ui)),
            Popover::MoveOptions => {
                popover_at(ctx, "move", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, move_options))
            }
            Popover::SmartFilters => popover_at(ctx, "smart", anchor, side, |ui| crate::adjust::smart_panel(app, ui)),
            Popover::Export => popover_at(ctx, "export", screen.center(), Align2::CENTER_CENTER, |ui| export_dialog(app, ui)),
            Popover::Recover => popover_at(ctx, "recover", screen.center(), Align2::CENTER_CENTER, |ui| recover(app, ui)),
            Popover::Fill => {
                popover_at(ctx, "fill", anchor, side, |ui| crate::dock::with_grip(app, ui, PanelId::ToolOptions, fill_options))
            }
            Popover::NewDoc => popover_at(ctx, "newdoc", screen.center(), Align2::CENTER_CENTER, |ui| new_doc(app, ui)),
            Popover::Confirm => popover_at(ctx, "confirm", screen.center(), Align2::CENTER_CENTER, |ui| confirm(app, ui)),
            Popover::BrushStudio => crate::brush_studio::show(app, ctx, screen),
            Popover::StatusMore => egui::Rect::NOTHING,
            // It closes itself on a press outside.
            Popover::CommandPalette => {
                crate::palette::show(app, ctx, egui::pos2(screen.center().x, screen.top() + header_h(app) + 24.0));
                screen
            }
        };
        // Click outside closes the popover (the top bar handles its own toggles).
        let pressed = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        if let Some(p) = pressed
            && !rect.contains(p)
            && !toolbar.contains(p)
            && !topbar.contains(p)
            && app.dock.drag.is_none()
            && !matches!(app.popover, Some(Popover::Confirm | Popover::Filter | Popover::BrushStudio))
            && !crate::adjust::eyedropper_busy(ctx)
        {
            app.popover = None;
        }
    }
    toast(app, ctx, screen);
    resize_edges(app, ctx);
}

fn popover_at(ctx: &egui::Context, id: &str, pos: egui::Pos2, align: Align2, body: impl FnOnce(&mut Ui)) -> egui::Rect {
    Area::new(Id::new(("popover", id)))
        .order(Order::Foreground)
        .pivot(align)
        .fixed_pos(pos)
        .show(ctx, |ui| popover_frame(ui).show(ui, body).response.rect)
        .inner
}

/// Row 1 of the header (spec 65): the app mark and the menu bar, the
/// drawing's name centred in the window, and the panel buttons.
pub const ROW1_H: f32 = 40.0;
/// Row 2, the options row.
pub const ROW2_H: f32 = 40.0;
pub const HEADER_H: f32 = ROW1_H + ROW2_H;

/// The header's height; none in Zen mode.
pub fn header_h(app: &App) -> f32 {
    if app.hide_ui { 0.0 } else { HEADER_H }
}

fn top_bar(app: &mut App, ctx: &egui::Context) -> egui::Rect {
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    if app.hide_ui {
        onepaint_ui_kit::menu_bar::close(ctx);
        return egui::Rect::from_min_size(screen.min, vec2(0.0, 0.0));
    }
    let bar = egui::Rect::from_min_size(screen.min, vec2(screen.width(), ROW1_H));
    let labelled = screen.width() >= 1180.0;
    let mut picked = None;
    Area::new(Id::new("header")).order(Order::Middle).fixed_pos(bar.min).show(ctx, |ui| {
        ui.painter().rect_filled(bar, 0.0, t.panel);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, egui::Stroke::new(1.0, t.border));
        // As the title bar: empty space drags the window, a double-click
        // maximises it (the widgets drawn after it take their own clicks).
        if app.custom_title {
            let r = ui.interact(bar, Id::new("title-drag"), Sense::click_and_drag());
            if r.double_clicked() {
                let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
            } else if r.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
        }
        let inner = bar.shrink2(vec2(8.0, 6.0));
        let mut left = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
        // The app mark: back to the gallery.
        if left
            .add(egui::Button::new(RichText::new("onepaint").font(theme::semibold(14.0)).color(t.fg)).frame(false))
            .on_hover_text("Gallery")
            .clicked()
        {
            app.popover = Some(Popover::Gallery);
        }
        left.add_space(4.0);
        // Narrow windows fold the menus into one, each a captioned section.
        let menus = crate::menus::build(app);
        let menus = if dock::compact(screen) { crate::menus::folded(menus) } else { menus };
        picked = onepaint_ui_kit::menu_bar::menu_bar(&mut left, &menus);
        let menus_end = left.min_rect().right();
        // The end: recording, wet paint, then the panel buttons.
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let view = screen;
        if app.custom_title {
            window_controls(&mut right, ctx);
            widgets::vline(&mut right, 24.0);
        }
        if widgets::icon_label_button(&mut right, "layers", "Layers", dock::showing(app, Popover::Layers, view), labelled).clicked() {
            dock::toggle(app, Popover::Layers, view);
        }
        if widgets::icon_label_button(&mut right, "grid-3x3", "Guides", dock::showing(app, Popover::Guides, view), labelled).clicked() {
            dock::toggle(app, Popover::Guides, view);
        }
        let studio = app.popover == Some(Popover::BrushStudio);
        if widgets::icon_label_button(&mut right, "sliders-horizontal", "Brush settings", studio, labelled).clicked() {
            app.popover = if studio { None } else { Some(Popover::BrushStudio) };
        }
        if widgets::icon_label_button(&mut right, "brush", "Brushes", dock::showing(app, Popover::Brushes, view), labelled).clicked() {
            dock::toggle(app, Popover::Brushes, view);
        }
        if app.session.wet_active() {
            widgets::vline(&mut right, 24.0);
            if icons::button(&mut right, "droplet", 28.0, false, "Dry the wet paint").clicked() {
                let _ = app.session.execute(Command::DryWet);
            }
            // Canvas tilt: wet paint runs downhill; double-click levels it.
            let mut tilt = app.session.wet_tilt();
            let r = widgets::tilt_pad(&mut right, &mut tilt, 26.0, "Canvas tilt")
                .on_hover_text("Canvas tilt: drag to make wet paint run downhill; double-click to level");
            if r.changed() {
                let _ = app.session.execute(Command::SetWetTilt { x: tilt[0], y: tilt[1] });
            }
        }
        if app.session.is_recording() {
            let now = right.input(|i| i.time);
            if widgets::icon_label_button(&mut right, "circle", "REC", true, true).on_hover_text("Recording: click to stop").clicked() {
                app.toggle_recording(now);
            }
        }
        let buttons_start = right.min_rect().left();
        // The name, centred in the window, never under the menus or the buttons.
        let titles = app.tab_titles();
        let many = titles.len() > 1 && app.settings.document_tabs;
        let chips: Vec<(usize, String, bool)> = titles
            .into_iter()
            .enumerate()
            .filter(|(i, _)| many || *i == app.active_tab)
            .map(|(i, (name, dirty))| (i, format!("{name}{}", if dirty { " •" } else { "" }), i == app.active_tab))
            .collect();
        let est: f32 = chips.iter().map(|c| c.1.chars().count() as f32 * 7.5 + 24.0).sum::<f32>() + if many { 22.0 } else { 0.0 };
        let room = (buttons_start - menus_end - 16.0).max(0.0);
        let w = est.min(room);
        let cx = screen.center().x.clamp(menus_end + 8.0 + w / 2.0, (buttons_start - 8.0 - w / 2.0).max(menus_end + 8.0 + w / 2.0));
        let name_rect = egui::Rect::from_center_size(pos2(cx, inner.center().y), vec2(w, inner.height()));
        let mut mid = ui.new_child(egui::UiBuilder::new().max_rect(name_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        mid.set_clip_rect(name_rect);
        // One chip per open drawing (Ctrl+Alt+← / → to switch).
        for (i, label, active) in chips {
            let text = RichText::new(label).color(if active { t.fg } else { t.fg_muted });
            let r = mid.add(egui::Button::new(text).selected(active && many).frame(many));
            if r.clicked() {
                app.switch_tab(i);
            }
            if active && many && icons::button(&mut mid, "x", 18.0, false, "Close drawing").clicked() {
                app.request(crate::Pending::CloseTab, ctx);
            }
        }
    });
    options_row(app, ctx, egui::Rect::from_min_size(bar.left_bottom(), vec2(screen.width(), ROW2_H)));
    if let Some(id) = picked.and_then(crate::menus::decode) {
        crate::menus::run(app, id, ctx);
    }
    bar.union(egui::Rect::from_min_size(bar.left_bottom(), vec2(screen.width(), ROW2_H)))
}

/// Minimise, maximise or restore, and close, at the header's end (laid
/// out right to left: close first).
fn window_controls(ui: &mut Ui, ctx: &egui::Context) {
    let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    if icons::button(ui, "x", 28.0, false, "Close").clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let (icon, label) = if max { ("copy", "Restore") } else { ("square", "Maximize") };
    if icons::button(ui, icon, 28.0, false, label).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
    }
    if icons::button(ui, "minus", 28.0, false, "Minimize").clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

/// Without the system's frame, the window's edges resize it (5 px strips,
/// corners first), unless it is maximised.
fn resize_edges(app: &App, ctx: &egui::Context) {
    use egui::{CursorIcon as C, ResizeDirection as D};
    if !app.custom_title || ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let w = ctx.content_rect();
    const E: f32 = 5.0;
    let (l, r, tp, b) = (w.left(), w.right(), w.top(), w.bottom());
    let strip = |x0: f32, y0: f32, x1: f32, y1: f32| egui::Rect::from_min_max(pos2(x0, y0), pos2(x1, y1));
    let zones = [
        (strip(l, tp, l + E * 2.0, tp + E * 2.0), D::NorthWest, C::ResizeNorthWest),
        (strip(r - E * 2.0, tp, r, tp + E * 2.0), D::NorthEast, C::ResizeNorthEast),
        (strip(l, b - E * 2.0, l + E * 2.0, b), D::SouthWest, C::ResizeSouthWest),
        (strip(r - E * 2.0, b - E * 2.0, r, b), D::SouthEast, C::ResizeSouthEast),
        (strip(l, tp, r, tp + E), D::North, C::ResizeNorth),
        (strip(l, b - E, r, b), D::South, C::ResizeSouth),
        (strip(l, tp, l + E, b), D::West, C::ResizeWest),
        (strip(r - E, tp, r, b), D::East, C::ResizeEast),
    ];
    Area::new(Id::new("resize-edges")).order(Order::Foreground).fixed_pos(w.min).show(ctx, |ui| {
        for (i, (rect, dir, cursor)) in zones.into_iter().enumerate() {
            let resp = ui.interact(rect, Id::new(("resize-edge", i)), Sense::drag()).on_hover_cursor(cursor);
            if resp.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
            }
        }
    });
}

/// Row 2 (spec 65): the options, scrolling sideways, and the zoom menu.
fn options_row(app: &mut App, ctx: &egui::Context, row: egui::Rect) {
    let t = Tokens::get(ctx);
    let view = ctx.content_rect();
    Area::new(Id::new("options-row")).order(Order::Middle).fixed_pos(row.min).show(ctx, |ui| {
        ui.painter().rect_filled(row, 0.0, t.panel);
        ui.painter().hline(row.x_range(), row.bottom() - 0.5, egui::Stroke::new(1.0, t.border));
        let inner = row.shrink2(vec2(8.0, 4.0));
        // Narrow windows read the zoom in the status bar instead.
        let zoom_w = if dock::compact(view) { 0.0 } else { 96.0 };
        let zoom_rect = egui::Rect::from_min_max(pos2(inner.right() - zoom_w, inner.top()), inner.max);
        if zoom_w > 0.0 {
            let mut z = ui.new_child(egui::UiBuilder::new().max_rect(zoom_rect).layout(egui::Layout::right_to_left(egui::Align::Center)));
            crate::options_row::zoom(app, &mut z);
        }
        let left = egui::Rect::from_min_max(inner.min, pos2(zoom_rect.left() - 8.0, inner.bottom()));
        let mut l = ui.new_child(egui::UiBuilder::new().max_rect(left).layout(egui::Layout::left_to_right(egui::Align::Center)));
        l.set_clip_rect(left);
        // The wheel scrolls the row sideways when it overflows.
        egui::ScrollArea::horizontal()
            .id_salt("options-row")
            .max_width(left.width())
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut l, |ui| {
                ui.set_height(left.height());
                ui.horizontal_centered(|ui| crate::options_row::show(app, ui, view));
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
        .fixed_pos(pos2(screen.center().x, screen.bottom() - crate::status_bar::HEIGHT - 16.0))
        .show(ctx, |ui| {
            popover_frame(ui).show(ui, |ui| ui.label(msg));
        });
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
    ui.add_space(8.0);
    ui.label(RichText::new("Paper").color(t.fg_muted));
    let cur = onepaint_doc::PAPER_PRESETS.get(app.new_paper).map_or("smooth", |p| p.id);
    if let Some(id) = paper_picker(ui, cur) {
        app.new_paper = onepaint_doc::PAPER_PRESETS.iter().position(|p| p.id == id).unwrap_or(0);
    }
    ui.add_space(8.0);
    // happypaint's depths and hints (new-document-dialog.tsx DEPTHS).
    use onepaint_doc::Depth;
    let depths = [
        (Depth::Rgba8, "8-bit", "Standard, smallest memory"),
        (Depth::Rgba16F, "16-bit float", "Smoother gradients — recommended"),
        (Depth::Rgba32F, "32-bit float", "HDR / pro, ~2× the memory"),
    ];
    ui.label(RichText::new("Color depth").color(t.fg_muted));
    let cur = depths.iter().position(|d| d.0 == app.new_depth).unwrap_or(1);
    let labels: Vec<&str> = depths.iter().map(|d| d.1).collect();
    if let Some(i) = segmented(ui, &labels, cur) {
        app.new_depth = depths[i].0;
    }
    let hint = depths.iter().find(|d| d.0 == app.new_depth).map_or("", |d| d.2);
    ui.label(RichText::new(hint).font(theme::medium(10.5)).color(t.fg_faint));
    // What the size costs (happypaint v0.208.1): a warning, never a refusal.
    let (level, advice) = crate::memory::size_advice(app.new_doc[0], app.new_doc[1], app.new_depth, crate::memory::budget());
    let color = match level {
        crate::memory::Level::Ok => t.fg_faint,
        crate::memory::Level::Tight => t.warning,
        crate::memory::Level::Over => t.danger,
    };
    ui.add(egui::Label::new(RichText::new(advice).font(theme::medium(10.5)).color(color)).wrap());
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
    crate::dock::fit_width(ui, 340.0);
    if !crate::dock::is_hosted(ui) {
        widgets::section_label(ui, if app.session.tool() == Tool::Eraser { "Eraser" } else { "Brushes" });
    }
    ui.add_space(4.0);
    let current = app.session.current_brush().id.clone();
    let eraser = app.session.tool() == Tool::Eraser;
    // Search (happypaint's library): name or category, over every brush.
    let r = ui.add(egui::TextEdit::singleline(&mut app.brush_query).desired_width(f32::INFINITY).hint_text("Search brushes…"));
    if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.brush_query.clear();
    }
    let query = app.brush_query.trim().to_lowercase();
    let searching = !query.is_empty();
    // Library actions sit under the search box (happypaint v0.205.1).
    if !eraser {
        brush_files(app, ui);
    }
    // The Eraser's scope: its own brushes, or any brush (it stays an eraser);
    // back to its own whenever another tool shows the library.
    if !eraser {
        app.brush_all = None;
    }
    let all = eraser && app.brush_all == Some(Tool::Eraser);
    if eraser && !searching {
        ui.add_space(4.0);
        if let Some(i) = segmented(ui, &["All brushes", "Erasers"], if all { 0 } else { 1 }) {
            app.brush_all = (i == 0).then_some(Tool::Eraser);
        }
        ui.label(RichText::new("Any brush works as an eraser — it stays on the eraser tool.").font(theme::medium(11.0)).color(t.fg_faint));
    }
    if !eraser && !searching {
        ui.add_space(4.0);
        // Category chips; the list shows one category at a time (124 presets).
        let mut cats: Vec<String> = onepaint_brush::categories().into_iter().filter(|c| c != "Eraser").collect();
        let user = onepaint_brush::user_presets();
        for c in [onepaint_brush::IMPORTED_CATEGORY, onepaint_brush::CUSTOM_CATEGORY] {
            if user.iter().any(|p| p.category == c) {
                cats.push(c.into());
            }
        }
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
    }
    ui.add_space(4.0);
    let category = app.brush_category.clone();
    // Imported and custom brushes (and custom erasers) live in the user library.
    let user_cat = category == onepaint_brush::IMPORTED_CATEGORY || category == onepaint_brush::CUSTOM_CATEGORY;
    let list: Vec<_> = if searching {
        let all: Vec<_> = onepaint_brush::presets().into_iter().chain(onepaint_brush::user_presets()).collect();
        all.into_iter().filter(|p| crate::brush_library::matches(&p.name, &p.category, &query)).collect()
    } else if eraser && all {
        // Erasers first, then every other brush in library order.
        let every: Vec<_> = onepaint_brush::presets().into_iter().chain(onepaint_brush::user_presets()).collect();
        let (mut e, rest): (Vec<_>, Vec<_>) = every.into_iter().partition(|p| p.is_eraser());
        e.extend(rest);
        e
    } else if eraser {
        onepaint_brush::presets().into_iter().chain(onepaint_brush::user_presets()).filter(|p| p.is_eraser()).collect()
    } else if user_cat {
        onepaint_brush::user_presets().into_iter().filter(|p| p.category == category).collect()
    } else {
        onepaint_brush::presets().into_iter().filter(|p| p.category == category).collect()
    };
    if searching && list.is_empty() {
        ui.label(RichText::new(format!("No brushes match “{}”.", app.brush_query.trim())).color(t.fg_muted));
    }
    // Recent picks for this tool (happypaint v0.205.1); missing ids are skipped.
    let recent_key = crate::brush_library::recent_key(app.session.tool());
    let recent: Vec<_> = if searching {
        Vec::new()
    } else {
        let every: Vec<_> = onepaint_brush::presets().into_iter().chain(onepaint_brush::user_presets()).collect();
        let ids = app.settings.brush_recent.get(recent_key).cloned().unwrap_or_default();
        ids.iter().filter_map(|id| every.iter().find(|p| &p.id == id).cloned()).collect()
    };
    let section = if eraser { if all { "All brushes" } else { "Erasers" }.to_string() } else { category.clone() };
    let mut delete = None;
    let mut picked = None;
    let mut fold = None;
    // Fit the list below the top bar so the popover never overlaps it: the
    // footer below takes ~260 px (Size, Opacity, Flow, Angle, studio).
    let room = (ui.ctx().content_rect().height() - ui.cursor().top() - 260.0).clamp(120.0, 420.0);
    let folded = |key: &str| app.settings.brush_folded.iter().any(|k| k == key);
    let (recent_open, list_open) =
        (!folded(&crate::brush_library::section_key("Recent")), !folded(&crate::brush_library::section_key(&section)));
    // Docked or floating, the panel scrolls below its search and actions,
    // which stay pinned (happypaint v0.204.18); as a popover, only the list
    // scrolls and the sliders sit under it.
    let hosted = crate::dock::is_hosted(ui);
    let scroll = if hosted {
        egui::ScrollArea::vertical().id_salt("brush-panel-body").max_height(ui.available_height()).auto_shrink([false, false])
    } else {
        egui::ScrollArea::vertical().max_height(room)
    };
    scroll.show(ui, |ui| {
        // A search shows every match, folded sections included.
        if !recent.is_empty() {
            if widgets::fold_header(ui, "Recent", recent_open).clicked() {
                fold = Some("Recent".to_string());
            }
            if recent_open {
                for p in &recent {
                    brush_row(app, ui, p, &current, &mut picked, &mut delete);
                }
            }
        }
        let open = if searching {
            true
        } else {
            if widgets::fold_header(ui, &section, list_open).clicked() {
                fold = Some(section.clone());
            }
            list_open
        };
        if open {
            for p in &list {
                brush_row(app, ui, p, &current, &mut picked, &mut delete);
            }
        }
        if hosted {
            brush_footer(app, ui);
        }
    });
    if let Some(name) = fold {
        let key = crate::brush_library::section_key(&name);
        let f = &mut app.settings.brush_folded;
        if let Some(i) = f.iter().position(|k| *k == key) {
            f.remove(i);
        } else {
            f.push(key);
        }
        app.settings.save();
    }
    if let Some(id) = picked {
        let _ = app.session.execute(Command::SetBrush { preset_id: id.clone() });
        crate::brush_library::push_recent(app.settings.brush_recent.entry(recent_key.to_string()).or_default(), &id);
        app.settings.save();
    }
    if let Some(p) = delete {
        let now = ui.input(|i| i.time);
        match app.brush_library.remove(&p.id) {
            Ok(_) => app.notify(format!("Deleted {}", p.name), now),
            Err(e) => app.notify(format!("Couldn't save the brush library: {e}"), now),
        }
    }
    if !hosted {
        brush_footer(app, ui);
    }
}

/// Size, Opacity, Flow, Angle and the studio, under the library.
fn brush_footer(app: &mut App, ui: &mut Ui) {
    widgets::hairline(ui);
    let b = app.session.current_brush().clone();
    let mut size = b.size as f32;
    let max = app.settings.max_brush_size as f32;
    if widgets::slider_row(ui, "Size", &mut size, 1.0..=max, |v| format!("{:.0} px", v)).changed() {
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
    brush_angle(app, ui);
    ui.add_space(6.0);
    if widgets::secondary_button(ui, "Brush studio…", ui.available_width()).on_hover_text("Edit every setting and try the brush").clicked()
    {
        app.popover = Some(Popover::BrushStudio);
    }
}

/// One brush in the library: name, category and a stroke preview. A click
/// picks it; a user brush has Delete on its context menu.
fn brush_row(
    app: &mut App,
    ui: &mut Ui,
    p: &onepaint_brush::BrushPreset,
    current: &str,
    picked: &mut Option<String>,
    delete: &mut Option<onepaint_brush::BrushPreset>,
) {
    let t = Tokens::get(ui.ctx());
    let sel = p.id == current;
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, sel, &p.name));
    if sel {
        ui.painter().rect_filled(rect, t.radius, t.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    ui.painter().text(rect.left_top() + vec2(10.0, 8.0), Align2::LEFT_TOP, &p.name, theme::medium(12.5), if sel { t.accent } else { t.fg });
    ui.painter().text(rect.right_top() + vec2(-10.0, 9.0), Align2::RIGHT_TOP, &p.category, theme::medium(10.5), t.fg_faint);
    let tex = app.thumbs.brush(ui.ctx(), p);
    let img = egui::Rect::from_min_size(rect.left_top() + vec2(6.0, 18.0), vec2(rect.width() - 12.0, 44.0));
    egui::Image::new(&tex).tint(t.fg).paint_at(ui, img);
    if resp.clicked() {
        *picked = Some(p.id.clone());
    }
    if onepaint_brush::is_user_preset(&p.id) {
        resp.context_menu(|ui| {
            if ui.button("Delete brush").clicked() {
                *delete = Some(p.clone());
                ui.close();
            }
        });
    }
}

/// Import brush files and export the current brush (spec 23).
fn brush_files(app: &mut App, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    ui.add_space(6.0);
    // Two halves of whatever width the host gives.
    let half = ((ui.available_width() - 8.0) / 2.0).clamp(100.0, 160.0);
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Import brushes…", half).clicked()
            && let Some(path) = rfd::FileDialog::new().add_filter("Brushes", &onepaint_formats::brushes::BRUSH_EXTENSIONS).pick_file()
        {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("brushes").to_string();
            match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| app.brush_library.import(&name, &b)) {
                Ok(imp) => {
                    if imp.brushes > 0 {
                        app.brush_category = onepaint_brush::IMPORTED_CATEGORY.into();
                    }
                    app.notify(crate::brush_library::summary(&imp), now);
                }
                Err(e) => app.notify(format!("Couldn't import: {e}"), now),
            }
        }
        if widgets::secondary_button(ui, "Export brush…", half).clicked() {
            let b = app.session.current_brush().clone();
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("happypaint brush", &["happybrush"])
                .set_file_name(format!("{}.happybrush", b.name))
                .save_file()
            {
                match onepaint_formats::brushes::export_happybrush(&b)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
                {
                    Ok(()) => app.notify(format!("Exported {}", path.display()), now),
                    Err(e) => app.notify(format!("Export failed: {e}"), now),
                }
            }
        }
    });
}

fn color(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    crate::dock::fit_width(ui, 256.0);
    if !crate::dock::is_hosted(ui) {
        widgets::section_label(ui, "Colour");
    }
    ui.add_space(4.0);
    if let Some(i) = segmented(ui, &["Picker", "Mix"], usize::from(app.settings.color_mix_mode)) {
        app.settings.color_mix_mode = i == 1;
        app.settings.save();
    }
    ui.add_space(4.0);
    if app.settings.color_mix_mode {
        mix_section(app, ui);
    } else {
        let mut hsv = app.hsv;
        if widgets::color_picker(ui, &mut hsv, 256.0) {
            app.hsv = hsv;
            let c = hsv.to_rgba();
            let _ = app.session.execute(Command::SetColor { rgba: c });
            app.hex = c.to_hex();
        }
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
    // The palette shows here unless it has a panel of its own open.
    if !crate::dock::panel_open(app, PanelId::Palettes) {
        ui.add_space(8.0);
        palette_section(app, ui);
    }
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
                let title = format!("Delete palette \"{}\"?", active.name);
                app.ask = Some(crate::ask::Ask::confirm(
                    title,
                    "This can't be undone.",
                    "Delete",
                    true,
                    crate::ask::AskFor::DeletePalette(active.id.clone()),
                ));
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
            let c = d.space().color([flat[i], flat[i + 1], flat[i + 2], flat[i + 3]]);
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
    crate::dock::fit_width(ui, 300.0);
    ui.horizontal(|ui| {
        if !crate::dock::is_hosted(ui) {
            widgets::section_label(ui, "Layers");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "plus", 28.0, false, "New layer").clicked() {
                let _ = app.session.execute(Command::AddLayer);
            }
            if icons::button(ui, "folder-plus", 28.0, false, "New group").clicked() {
                let _ = app.session.execute(Command::AddGroup);
            }
        });
    });
    crate::layer_bar::show(app, ui);
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
            let picked = app.layer_picks.contains(&r.id);
            if sel {
                ui.painter().rect_filled(rect, t.radius, t.accent_soft);
            } else if picked {
                ui.painter().rect_filled(rect, t.radius, t.accent_soft.gamma_multiply(0.55));
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
                    // happypaint's badge: shown while the vector mask applies.
                    if l.vector_mask().is_some_and(|v| v.active()) {
                        tags.push("vector mask".into());
                    }
                }
                ui.label(RichText::new(tags.join(" · ")).font(theme::medium(10.5)).color(t.fg_faint));
            });
            if resp.clicked() {
                let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
                let ids: Vec<String> = list.iter().map(|r| r.id.clone()).collect();
                crate::align_ui::pick_row(app, &r.id, &ids, ctrl, shift);
                if !sel {
                    let _ = app.session.execute(Command::SetActiveLayer { id: r.id.clone() });
                }
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
        let modes: &[BlendMode] = if node.as_layer().is_some() { &BlendMode::LAYER_MODES } else { &BlendMode::GROUP_MODES };
        egui::ComboBox::from_id_salt("blend").selected_text(mode.label()).width(160.0).show_ui(ui, |ui| {
            for &m in modes {
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
            ui.label(RichText::new("Knockout").color(t.fg_muted)).on_hover_text(
                "Punch the layer's shape through what is under it: Shallow to below its group, Deep to the Background (Photoshop's Knockout)",
            );
            let label = |k: Option<Knockout>| k.map_or("None", Knockout::label);
            let mut k = l.knockout();
            egui::ComboBox::from_id_salt("knockout").selected_text(label(k)).width(160.0).show_ui(ui, |ui| {
                for v in [None, Some(Knockout::Shallow), Some(Knockout::Deep)] {
                    ui.selectable_value(&mut k, v, label(v));
                }
            });
            if k != l.knockout() {
                let _ = app
                    .session
                    .execute(Command::SetLayerProps { id: active.clone(), props: LayerProps { knockout: Some(k), ..Default::default() } });
            }
        });
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
        if l.mask.is_some() {
            let mut d = l.mask_density() * 100.0;
            if widgets::slider_row(ui, "Density", &mut d, 0.0..=100.0, |v| format!("{v:.0}%"))
                .on_hover_text("How strongly the mask hides (Photoshop's Density)")
                .changed()
            {
                let props = LayerProps { mask_density: Some(d.round() / 100.0), ..Default::default() };
                let _ = app.session.execute(Command::SetLayerProps { id: active.clone(), props });
            }
            let mut f = l.mask_feather();
            if widgets::slider_row(ui, "Feather", &mut f, 0.0..=250.0, |v| format!("{v:.1} px"))
                .on_hover_text("Soften the mask's edges, without changing the mask (Photoshop's Feather)")
                .changed()
            {
                let props = LayerProps { mask_feather: Some((f * 2.0).round() / 2.0), ..Default::default() };
                let _ = app.session.execute(Command::SetLayerProps { id: active.clone(), props });
            }
        }
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
        if let Some(v) = l.vector_mask() {
            vector_mask_controls(app, ui, v, &active);
        }
        blend_if_controls(app, ui, l.blend_if(), &active);
        if l.adjustment.is_none() {
            bevel_controls(app, ui, &active);
        }
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Label").color(t.fg_muted));
        let current = app.session.color_label(&active);
        if let Some(label) = label_swatches(ui, Some(current.as_deref())) {
            let _ = app.session.execute(Command::SetColorLabel { id: active.clone(), label });
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

/// A dockable panel's body, wherever it is hosted.
pub fn panel_body(app: &mut App, ui: &mut Ui, id: PanelId) {
    match id {
        PanelId::Layers => layers(app, ui),
        PanelId::Brush => brushes(app, ui),
        PanelId::Color => color(app, ui),
        PanelId::Palettes => {
            crate::dock::fit_width(ui, 260.0);
            palette_section(app, ui);
        }
        PanelId::Guides => guides(app, ui),
        PanelId::Align => crate::align_ui::panel(app, ui),
        PanelId::Reference => crate::reference::body(app, ui),
        PanelId::Properties => properties(app, ui),
        PanelId::ToolOptions => tool_options(app, ui),
    }
}

/// The active adjustment or fill layer's settings (happypaint's
/// Properties panel).
fn properties(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let active = app.session.active().to_string();
    if app.session.doc().layer(&active).is_some_and(|l| l.adjustment.is_some()) {
        crate::adjust::layer_panel(app, ui);
    } else if app.session.doc().find(&active).is_some() {
        // A pixel, text or path layer, or a group: its layer style (spec 56).
        crate::styles_ui::panel(app, ui);
    } else {
        crate::dock::fit_width(ui, 240.0);
        ui.label(RichText::new("Select an adjustment layer to edit its settings.").color(t.fg_muted));
    }
}

/// The current tool's options.
fn tool_options(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let tool = app.session.tool();
    match crate::dock::tool_options(tool) {
        Some(Popover::MoveOptions) => move_options(app, ui),
        Some(Popover::Selection) => selection(app, ui),
        Some(Popover::Fill) => fill_options(app, ui),
        Some(Popover::GradientOptions) => crate::gradient_ui::tool_panel(app, ui),
        Some(Popover::ShapeOptions) => shape_options(app, ui),
        Some(Popover::PenOptions) => pen_options(app, ui),
        Some(Popover::PanelOptions) => crate::panel_ui::options(app, ui),
        Some(Popover::SmudgeOptions) => smudge_options(app, ui),
        Some(Popover::MixerOptions) => mixer_options(app, ui),
        Some(Popover::LiquifyOptions) => liquify_options(app, ui),
        _ => {
            crate::dock::fit_width(ui, 240.0);
            let msg = match tool {
                Tool::Brush | Tool::Eraser => "Brush settings are in the Brushes panel.",
                _ => "This tool has no options.",
            };
            ui.label(RichText::new(crate::status_bar::tool_label(tool)).font(theme::semibold(13.0)).color(t.fg));
            ui.label(RichText::new(msg).color(t.fg_muted));
        }
    }
}

pub(crate) fn select_icon(t: Tool) -> &'static str {
    match t {
        Tool::SelectEllipse => "circle-dashed",
        Tool::SelectLasso => "lasso",
        Tool::SelectPoly => "pentagon",
        Tool::SelectWand => "wand",
        _ => "square-dashed",
    }
}

/// A row of pill-shaped segmented buttons; returns the clicked index.
pub(crate) fn segmented(ui: &mut Ui, labels: &[&str], selected: usize) -> Option<usize> {
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
    crate::dock::fit_width(ui, 300.0);
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
    if matches!(app.session.tool(), Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly) {
        ui.add_space(4.0);
        widgets::toggle(ui, &mut app.canvas.options.select_antialias, "Anti-alias");
    }
    if app.session.tool() == Tool::SelectPoly {
        ui.label(
            RichText::new("Click to add points · click the first point or Enter to close · Esc cancels")
                .font(theme::medium(10.5))
                .color(t.fg_faint),
        );
    }
    widgets::hairline(ui);
    let mut feather = f64::from(app.session.feather());
    let changed = ui
        .horizontal(|ui| {
            let max = f64::from(onepaint_engine::FEATHER_MAX);
            onepaint_ui_kit::inline::inline_number(ui, "Feather", &mut feather, 0.0..=max, " px", 0).changed()
        })
        .inner;
    if changed {
        let _ = app.session.execute(Command::SetFeather { radius: feather as f32 });
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
            app.ask = Some(crate::ask::Ask::text("Save selection as", name, "Save", crate::ask::AskFor::SaveSelection));
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

/// The brush's tip angle (happypaint's options-row dial): the dial and the
/// degrees; it does nothing while the brush is wet (the mixer shapes its
/// own dab), so it shows dimmed then.
pub(crate) fn brush_angle(app: &mut App, ui: &mut Ui) {
    use onepaint_brush::studio::Param;
    let t = Tokens::get(ui.ctx());
    let mut b = app.session.current_brush().clone();
    let wet = Param::Wetness.get(&b) > 0.0;
    let mut deg = Param::Angle.get(&b);
    let tip = if wet {
        "Brush angle — no effect while Wet is above 0 (the wet mixer shapes its own dab)"
    } else {
        "Brush angle — drag the dial (Shift snaps 15°, double-click resets) or Shift+[ · Shift+]"
    };
    let changed = ui
        .horizontal(|ui| {
            ui.label(RichText::new("Angle").color(if wet { t.fg_faint } else { t.fg_muted }));
            let r = widgets::angle_dial(ui, &mut deg, 22.0, tip).changed();
            ui.label(RichText::new(format!("{deg:.0}°")).font(theme::mono(12.0)).color(if wet { t.fg_faint } else { t.fg }));
            r
        })
        .inner;
    if changed {
        Param::Angle.set(&mut b, deg);
        let _ = app.session.execute(Command::SetCurrentBrush { preset: Box::new(b) });
    }
}

fn fill_options(app: &mut App, ui: &mut Ui) {
    crate::dock::fit_width(ui, 260.0);
    widgets::section_label(ui, "Fill");
    let mut tol = app.canvas.options.fill_tolerance * 100.0;
    if widgets::slider_row(ui, "Tolerance", &mut tol, 0.0..=100.0, |v| format!("{v:.0}")).changed() {
        app.canvas.options.fill_tolerance = tol / 100.0;
    }
    let mut grow = app.canvas.options.fill_grow as f32;
    if widgets::slider_row(ui, "Grow", &mut grow, 0.0..=8.0, |v| format!("{v:.0} px")).changed() {
        app.canvas.options.fill_grow = grow.round() as u32;
    }
    // A watercolour wash instead of a flat fill (spec 58 C).
    let o = &mut app.canvas.options;
    widgets::toggle(ui, &mut o.fill_wash, "Wash");
    if !o.fill_wash {
        return;
    }
    let w = &mut o.wash;
    widgets::toggle(ui, &mut w.water_only, "Water only");
    widgets::toggle(ui, &mut w.sample_all, "All layers");
    let pct = |v: f32| format!("{v:.0}%");
    let row = |ui: &mut Ui, label: &str, v: &mut f32| {
        let mut p = *v * 100.0;
        if widgets::slider_row(ui, label, &mut p, 0.0..=100.0, pct).changed() {
            *v = p / 100.0;
        }
    };
    row(ui, "Water", &mut w.water);
    if !w.water_only {
        row(ui, "Pigment", &mut w.pigment);
        row(ui, "Graded", &mut w.grade);
    }
    row(ui, "Soft", &mut w.soft);
    row(ui, "Speed", &mut w.speed);
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
            app.recover_path(p);
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

fn shape_options(app: &mut App, ui: &mut Ui) {
    use onepaint_stroke::shape::ShapeKind as K;
    let t = Tokens::get(ui.ctx());
    crate::dock::fit_width(ui, 300.0);
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
    crate::dock::fit_width(ui, 280.0);
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
pub(crate) const MIXER_PRESETS: [(&str, f64, f64, f64); 12] = [
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
    crate::dock::fit_width(ui, 300.0);
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
        let armed = app.canvas.mixer_load_armed;
        let load = widgets::secondary_button(ui, if armed { "Tap the canvas…" } else { "Load from canvas" }, 130.0);
        if load.on_hover_text("Load the brush from what's under it (or Alt-click the canvas)").clicked() {
            app.canvas.mixer_load_armed = !armed;
        }
        // What the brush holds: a kept reservoir, else the loaded patch.
        let space = app.session.doc().space();
        let shown = app
            .session
            .mixer_reservoir()
            .map(|r| (r, "What the brush holds"))
            .or(app.session.mixer_patch().map(|r| (r, "The loaded patch")));
        match shown {
            Some((onepaint_brush::smudge::Reservoir::Flat(r), tip)) => {
                widgets::swatch(ui, space.color([r[0], r[1], r[2], 1.0]), 22.0, false).on_hover_text(tip);
            }
            Some((r, tip)) => {
                let cells: Vec<_> = r
                    .swatch(8)
                    .into_iter()
                    .map(|c| {
                        let mut col = space.color([c[0], c[1], c[2], 1.0]);
                        col.a = c[3];
                        col
                    })
                    .collect();
                widgets::patch_swatch(ui, &cells, 8, 26.0, tip).on_hover_text(tip);
            }
            None => {}
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

/// File ▸ Export… (spec 25).
fn export_dialog(app: &mut App, ui: &mut Ui) {
    use onepaint_formats::export::{ExportFormat as F, output_size};
    let t = Tokens::get(ui.ctx());
    ui.set_width(340.0);
    widgets::section_label(ui, "Export");
    let formats = [F::Png, F::Jpeg, F::Tiff, F::Ppm];
    let (opts, psd) = (&mut app.export_opts, &mut app.export_psd);
    let cur = if *psd { 4 } else { formats.iter().position(|f| *f == opts.format).unwrap_or(0) };
    if let Some(i) = segmented(ui, &["PNG", "JPEG", "TIFF", "PPM", "PSD"], cur) {
        *psd = i == 4;
        if i < 4 {
            opts.format = formats[i];
        }
    }
    if *psd {
        ui.label(RichText::new("Layers, groups, masks and blend modes, at document size.").font(theme::medium(10.5)).color(t.fg_faint));
    } else {
        let mut pct = (opts.scale * 100.0) as f32;
        if widgets::slider_row(ui, "Scale", &mut pct, 10.0..=400.0, |v| format!("{v:.0}%")).changed() {
            opts.scale = (pct.round() / 100.0) as f64;
        }
        if opts.format == F::Jpeg {
            let mut q = opts.quality as f32;
            if widgets::slider_row(ui, "Quality", &mut q, 1.0..=100.0, |v| format!("{v:.0}")).changed() {
                opts.quality = q.round() as u8;
            }
        }
        if opts.format == F::Ppm {
            widgets::toggle(ui, &mut opts.ascii, "ASCII (P3)");
        }
        if opts.format == F::Tiff {
            widgets::toggle(ui, &mut opts.cmyk, "CMYK for print (coated press, 300% ink)");
        }
        let d = app.session.doc();
        let (w, h) = output_size(d.width, d.height, opts.scale);
        let note = match opts.format {
            F::Jpeg | F::Ppm => " · flattened on white",
            F::Tiff if opts.cmyk => " · CMYK on white",
            _ => "",
        };
        ui.label(RichText::new(format!("{w} × {h} px at {} dpi{note}", d.dpi)).font(theme::mono(11.0)).color(t.fg_muted));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if widgets::primary_button(ui, "Export…", 110.0).clicked() {
            let now = ui.input(|i| i.time);
            let (o, psd) = (app.export_opts, app.export_psd);
            app.popover = None;
            app.export_with(o, psd, now);
        }
        if widgets::secondary_button(ui, "Cancel", 90.0).clicked() {
            app.popover = None;
        }
    });
}

/// Copy / cut / paste (three-finger swipe down).
fn clipboard_menu(app: &mut App, ui: &mut Ui) {
    ui.set_width(200.0);
    widgets::section_label(ui, "Copy & paste");
    if widgets::menu_item(ui, Some("copy"), "Copy", "Ctrl+C").clicked() {
        crate::clip::copy(app);
        app.popover = None;
    }
    if widgets::menu_item(ui, Some("brush-cleaning"), "Cut", "Ctrl+X").clicked() {
        crate::clip::cut(app);
        app.popover = None;
    }
    if widgets::menu_item(ui, Some("plus"), "Paste", "Ctrl+V").clicked() {
        crate::clip::paste(app);
        app.popover = None;
    }
}

/// Shortcuts & gestures (`?`).
fn move_options(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    crate::dock::fit_width(ui, 260.0);
    widgets::section_label(ui, "Transform");
    let active = app.session.transform_active();
    let mode = match app.session.transform_warp() {
        None => 0,
        Some(onepaint_engine::transform::WarpMode::Quad(_)) => 1,
        Some(onepaint_engine::transform::WarpMode::Mesh(_)) => 2,
    };
    ui.add_enabled_ui(active, |ui| {
        if let Some(i) = segmented(ui, &["Free", "Distort", "Warp"], mode)
            && i != mode
            && i > 0
        {
            let _ = app.session.execute(if i == 1 { Command::BeginDistort } else { Command::BeginWarp });
        }
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Apply", 90.0).clicked() {
                app.commit_transform();
            }
            if widgets::secondary_button(ui, "Cancel", 90.0).clicked() {
                app.cancel_transform();
            }
        });
    });
    let hint = match mode {
        1 => "Drag a corner for perspective; drag inside to move the whole quad.",
        2 => "Drag the lattice points to bend the content.",
        _ => "Drag handles to scale and rotate. Distort and Warp start from the current box.",
    };
    ui.label(RichText::new(hint).font(theme::medium(10.5)).color(t.fg_faint));
}

fn pen_options(app: &mut App, ui: &mut Ui) {
    use onepaint_engine::path_ops::StrokePathTool as T;
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    crate::dock::fit_width(ui, 300.0);
    widgets::section_label(ui, "Pen");
    widgets::toggle(ui, &mut app.canvas.options.pen_fill, "Fill closed paths");
    ui.label(
        RichText::new("Click for corners, drag for curves (Shift: 45°). Click the first point to close; Enter or double-click finishes; Backspace undoes a point. On a path layer: drag anchors and handles, Alt-click toggles smooth, Ctrl-click a segment adds a point, Delete removes the last one touched.")
            .font(theme::medium(10.5))
            .color(t.fg_faint),
    );
    widgets::hairline(ui);
    widgets::section_label(ui, "Path");
    let active = app.session.active().to_string();
    let is_path = app.session.is_path_layer(&active);
    let target = app.session.path_op_target();
    let target_name = target.as_deref().and_then(|id| app.session.doc().layer(id)).map(|l| l.name.clone());
    let run = |app: &mut App, cmd: Command| {
        if let Err(e) = app.session.execute(cmd) {
            app.notify(e.to_string(), now);
        }
    };
    ui.add_enabled_ui(is_path, |ui| {
        if widgets::menu_item(ui, Some("square-dashed"), "Make selection", "").clicked() {
            run(app, Command::SelectFromPath { mode: onepaint_selection::Mode::Replace });
        }
        ui.add_enabled_ui(target.is_some(), |ui| {
            if widgets::menu_item(ui, Some("paint-bucket"), "Fill path", "").clicked() {
                run(app, Command::FillPath);
            }
            ui.horizontal(|ui| {
                let tools = [T::Brush, T::Eraser, T::Smudge, T::Mixer];
                let cur = tools.iter().position(|x| *x == app.canvas.options.stroke_path_tool).unwrap_or(0);
                if let Some(i) = segmented(ui, &["Brush", "Eraser", "Smudge", "Mixer"], cur) {
                    app.canvas.options.stroke_path_tool = tools[i];
                }
            });
            widgets::toggle(ui, &mut app.canvas.options.stroke_path_pressure, "Simulate pressure");
            if widgets::menu_item(ui, Some("brush"), "Stroke path", "").clicked() {
                let (tool, simulate_pressure) = (app.canvas.options.stroke_path_tool, app.canvas.options.stroke_path_pressure);
                run(app, Command::StrokePath { tool, simulate_pressure });
            }
            if widgets::menu_item(ui, Some("contrast"), "Create Vector Mask", "")
                .on_hover_text("Mask the paint layer below with these paths")
                .clicked()
            {
                run(app, Command::MaskFromPath);
            }
        });
        if widgets::menu_item(ui, Some("move"), "Free transform path", "").clicked() {
            // The Move tool on a path layer maps the path itself (stays editable).
            app.popover = None;
            app.set_tool(Tool::Move);
        }
        if widgets::menu_item(ui, Some("image"), "Rasterize path layer", "").clicked() {
            run(app, Command::RasterizePath { id: active.clone() });
        }
    });
    let hint = match (is_path, target_name) {
        (false, _) => "Select a path layer to use the path menu.".to_string(),
        (true, Some(n)) => format!("Pixel operations land on \"{n}\", the layer below."),
        (true, None) => "Fill, stroke and mask need a paint layer directly below the path.".to_string(),
    };
    ui.label(RichText::new(hint).font(theme::medium(10.5)).color(t.fg_faint));
}

fn liquify_options(app: &mut App, ui: &mut Ui) {
    use onepaint_filters::liquify::LIQUIFY_MODES;
    let t = Tokens::get(ui.ctx());
    crate::dock::fit_width(ui, 280.0);
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

/// Grayscale, print preview (CMYK soft-proof) and the non-printable warning.
fn view_modes(app: &mut App, ui: &mut Ui) {
    let mut v = app.canvas.view_modes();
    widgets::section_label(ui, "View");
    widgets::toggle(ui, &mut v.grayscale, "Grayscale value check (Ctrl+Y)");
    widgets::toggle(ui, &mut v.proof, "Print preview (CMYK proof)");
    if v.proof {
        widgets::toggle(ui, &mut v.paper_white, "Simulate paper white");
        widgets::toggle(ui, &mut v.black_ink, "Simulate black ink");
    }
    let warn = [0.0f32, 5.0, 2.0];
    let cur = warn.iter().position(|w| *w == v.gamut_warn).unwrap_or(0);
    ui.label(RichText::new("Non-printable colours").font(theme::medium(11.0)).color(Tokens::get(ui.ctx()).fg_muted));
    if let Some(i) = segmented(ui, &["Off", "ΔE > 5", "ΔE > 2"], cur) {
        v.gamut_warn = warn[i];
    }
    app.canvas.set_view_modes(v);
    // The press the proof, the warning and CMYK TIFF all go through.
    let name = crate::press::active(&app.press).name.clone();
    let hint = if app.press.is_some() { "Clear" } else { "Load .icc…" };
    if widgets::menu_item(ui, Some("palette"), &format!("Press: {name}"), hint).clicked() {
        let now = ui.input(|i| i.time);
        app.toggle_press_profile(now);
    }
}

fn guides(app: &mut App, ui: &mut Ui) {
    use onepaint_engine::Guides;
    use onepaint_engine::SymmetryMode as M;
    use onepaint_engine::guides::{PerspectiveSnap as PS, perspective_preset};
    let t = Tokens::get(ui.ctx());
    crate::dock::fit_width(ui, 310.0);
    let mut g = app.session.guides().clone();
    let before = g.clone();
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() - 140.0).show(ui, |ui| {
        view_modes(app, ui);
        widgets::hairline(ui);
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
            if widgets::toggle(ui, &mut g.perspective.curved, "Fish-eye (5-point curvilinear)").changed()
                && g.perspective.curved
                && g.perspective.points.len() < 5
            {
                g.perspective.points = perspective_preset(5);
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

/// Bevel & Emboss (layer-panel.tsx:1964-1995): relief derived from the
/// layer's paint, re-derived when a slider is released. Bevel 0 clears it.
fn bevel_controls(app: &mut App, ui: &mut Ui, active: &str) {
    let [mut depth, mut soft] = app.bevel;
    let a = widgets::slider_row(ui, "Bevel", &mut depth, 0.0..=1.5, |v| format!("{:.0}%", v * 100.0));
    let b = widgets::slider_row(ui, "Soften", &mut soft, 0.0..=1.0, |v| format!("{:.0}%", v * 100.0));
    app.bevel = [depth, soft];
    let commit = |r: &egui::Response| r.drag_stopped() || (r.changed() && !r.dragged());
    if commit(&a) || commit(&b) {
        let id = active.to_string();
        let cmd = if depth > 0.0 {
            Command::BevelFromAlpha { id, depth: depth as f64, softness: soft as f64 }
        } else {
            Command::ClearBevel { id }
        };
        let _ = app.session.execute(cmd);
    }
}

/// The papers to choose from (happypaint `paper-picker.tsx`): each with its
/// hint, a classic paper only while `current` is on it. The id picked.
fn paper_picker(ui: &mut Ui, current: &str) -> Option<&'static str> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        for p in onepaint_doc::pickable_papers(current) {
            if ui.selectable_label(p.id == current, p.label).on_hover_text(p.hint).clicked() {
                picked = Some(p.id);
            }
        }
    });
    picked
}

/// The document paper (happypaint's document properties): a preset, and the
/// texture scale for textured papers. Applies live; not an undo step.
fn paper_dialog(app: &mut App, ui: &mut Ui) {
    use onepaint_doc::{PAPER_PRESETS, paper::PAPER_SCALE_MAX, paper::PAPER_SCALE_MIN, paper_preset_id};
    let t = Tokens::get(ui.ctx());
    ui.set_width(340.0);
    ui.label(RichText::new("Paper").font(theme::semibold(16.0)).color(t.fg));
    ui.label(RichText::new("Dry brushes catch its tooth; wet paint soaks into it.").font(theme::medium(11.0)).color(t.fg_faint));
    ui.add_space(8.0);
    let paper = app.session.doc().paper();
    let cur = paper_preset_id(paper.as_ref());
    if let Some(id) = paper_picker(ui, cur)
        && let Some(p) = PAPER_PRESETS.iter().find(|p| p.id == id)
    {
        let _ = app.session.execute(Command::SetPaper { paper: p.paper() });
    }
    // The sheet colour onto the Background (happypaint's Apply paper colour).
    if let Some(tint) = PAPER_PRESETS.iter().find(|p| p.id == paper_preset_id(app.session.doc().paper().as_ref())).and_then(|p| p.tint) {
        ui.add_space(6.0);
        if widgets::secondary_button(ui, "Apply paper colour to Background", 260.0).clicked() {
            let now = ui.input(|i| i.time);
            match app.session.background_layer_id() {
                Some(id) => {
                    let rgba = onepaint_color::Rgba::from_hex(tint).unwrap_or(onepaint_color::Rgba::WHITE);
                    let _ = app.session.execute(Command::FillLayerSolid { id, rgba });
                }
                None => app.notify("The bottom layer is not a plain paint layer, so there is no Background to fill.", now),
            }
        }
    }
    if let Some(p) = paper {
        ui.add_space(6.0);
        let mut pct = (p.scale() * 100.0) as f32;
        let range = (PAPER_SCALE_MIN * 100.0) as f32..=(PAPER_SCALE_MAX * 100.0) as f32;
        let r = widgets::slider_row(ui, "Texture scale", &mut pct, range, |v| format!("{v:.0}%"));
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            let scale = ((pct / 5.0).round() * 5.0 / 100.0) as f64;
            let _ = app.session.execute(Command::SetPaper { paper: Some(onepaint_doc::Paper { scale: Some(scale), ..p }) });
        }
    }
    // Memory (happypaint's Document Properties row, v0.208.1).
    ui.add_space(8.0);
    let d = app.session.doc();
    let used = app.session.memory_use();
    let (over, text) = crate::memory::document_memory(d.width, d.height, d.depth, used.total() as u64, crate::memory::budget());
    ui.label(RichText::new("Memory").font(theme::medium(12.5)).color(t.fg));
    ui.add(egui::Label::new(RichText::new(text).font(theme::medium(11.0)).color(if over { t.danger } else { t.fg_faint })).wrap())
        .on_hover_text(format!(
            "Layers, masks and relief: {}. Undo: {}. Blank areas cost nothing.",
            crate::memory::format_bytes(used.pixels as u64),
            crate::memory::format_bytes(used.history as u64)
        ));
    // Storage (happypaint's stored size, v0.208.2): the file's real size.
    ui.add_space(6.0);
    let file = app.path.as_ref().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
    let copy = app.autosave.copy_size(&app.session.doc().id);
    ui.label(RichText::new("Storage").font(theme::medium(12.5)).color(t.fg));
    let text = crate::memory::storage_text(file, app.dirty(), copy);
    ui.add(egui::Label::new(RichText::new(text).font(theme::medium(11.0)).color(t.fg_faint)).wrap());
    ui.add_space(10.0);
    if widgets::primary_button(ui, "Done", 90.0).clicked() {
        app.popover = None;
    }
}

/// Properties ▸ Blend If (happypaint `layer-look-editor.tsx`): a channel,
/// Split, and the This Layer / Underlying Layer bars.
fn blend_if_controls(app: &mut App, ui: &mut Ui, current: Option<BlendIf>, active: &str) {
    use onepaint_ui_kit::range_bar::{range_bar, range_readout};
    let t = Tokens::get(ui.ctx());
    const CHANNELS: [(&str, egui::Color32); 4] = [
        ("Gray", egui::Color32::WHITE),
        ("Red", egui::Color32::from_rgb(255, 0, 0)),
        ("Green", egui::Color32::from_rgb(0, 255, 0)),
        ("Blue", egui::Color32::from_rgb(0, 0, 255)),
    ];
    let (ch_id, split_id, open_id) = (Id::new("blend-if-channel"), Id::new("blend-if-split"), Id::new("blend-if-open"));
    let mut ch: usize = ui.data(|d| d.get_temp(ch_id)).unwrap_or(0);
    let mut split: bool = ui.data(|d| d.get_temp(split_id)).unwrap_or(false);
    // Folded away until asked for (or in use), so the panel stays short.
    let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(current.is_some());
    ui.horizontal(|ui| {
        if widgets::toggle(ui, &mut open, "Blend If").on_hover_text("Show this layer's tones only where they fall in a range").changed() {
            ui.data_mut(|d| d.insert_temp(open_id, open));
        }
        if current.is_some() && widgets::secondary_button(ui, "Reset", 56.0).on_hover_text("Let every tone through again").clicked() {
            let props = LayerProps { blend_if: Some(None), ..Default::default() };
            let _ = app.session.execute(Command::SetLayerProps { id: active.to_string(), props });
        }
    });
    if !open {
        return;
    }
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("blend-if-channel").selected_text(CHANNELS[ch].0).width(90.0).show_ui(ui, |ui| {
            for (i, (name, _)) in CHANNELS.iter().enumerate() {
                ui.selectable_value(&mut ch, i, *name);
            }
        });
        widgets::toggle(ui, &mut split, "Split").on_hover_text("Pull a slider's two halves apart for a soft transition (or Alt-drag)");
    });
    ui.data_mut(|d| {
        d.insert_temp(ch_id, ch);
        d.insert_temp(split_id, split);
    });
    let mut b = current.unwrap_or_default();
    let mut changed = false;
    for (label, this) in [("This Layer", true), ("Underlying Layer", false)] {
        let r = if this { &mut b.0[ch].src } else { &mut b.0[ch].dst };
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).color(t.fg_muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(range_readout(*r)).font(theme::mono(12.0)).color(t.fg));
            });
        });
        changed |= range_bar(ui, r, CHANNELS[ch].1, split, &format!("{label} — {}", CHANNELS[ch].0)).changed();
    }
    if changed {
        let props = LayerProps { blend_if: Some(Some(b)), ..Default::default() };
        let _ = app.session.execute(Command::SetLayerProps { id: active.to_string(), props });
    }
}

/// Properties ▸ Vector mask (happypaint `layer-look-editor.tsx`): Apply,
/// Invert, Delete and the path count. Each change is one undo step.
fn vector_mask_controls(app: &mut App, ui: &mut Ui, v: onepaint_doc::VectorMask, active: &str) {
    let t = Tokens::get(ui.ctx());
    let set = |app: &mut App, v: Option<onepaint_doc::VectorMask>| {
        let props = LayerProps { vector_mask: Some(v), ..Default::default() };
        let _ = app.session.execute(Command::SetLayerProps { id: active.to_string(), props });
    };
    ui.horizontal(|ui| {
        widgets::section_label(ui, "Vector mask");
        let n = v.paths.len();
        ui.label(RichText::new(format!("{n} path{}", if n == 1 { "" } else { "s" })).color(t.fg_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::secondary_button(ui, "Delete", 64.0)
                .on_hover_text("Remove the vector mask (the layer's pixels are untouched)")
                .clicked()
            {
                set(app, None);
            }
        });
    });
    ui.horizontal(|ui| {
        let mut apply = !v.disabled;
        if widgets::toggle(ui, &mut apply, "Apply")
            .on_hover_text("Keep the paths but stop hiding with them (Photoshop's Shift-click)")
            .changed()
        {
            set(app, Some(onepaint_doc::VectorMask { disabled: !apply, ..v.clone() }));
        }
        let mut invert = v.invert;
        if widgets::toggle(ui, &mut invert, "Invert").on_hover_text("Show outside the paths instead of inside").changed() {
            set(app, Some(onepaint_doc::VectorMask { invert, ..v.clone() }));
        }
    });
}

/// Colour ▸ Mix (happypaint `mix-section.tsx`): 2–6 wells on a top and a
/// bottom edge, and the grid of colours between them. Tap a well to set it
/// to the current colour, a cell to paint with it.
fn mix_section(app: &mut App, ui: &mut Ui) {
    use onepaint_color::mix_grid::{MIX_COLS, MIX_COUNT_MAX, MIX_COUNT_MIN, MIX_ROWS, MixSlot, MixSpace, layout};
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    let current = app.session.color().to_hex();
    let rgba = |h: &str| onepaint_color::mix_grid::hex_to_rgb01(h);
    let to_rgba = |h: &str| {
        let [r, g, b] = rgba(h);
        onepaint_color::Rgba::new(r as f32, g as f32, b as f32, 1.0)
    };
    let (top, bottom) = layout(app.settings.mix.count);
    let mut changed = false;
    let grid = app.settings.mix.grid();
    let cols = grid.first().map_or(1, Vec::len);
    let gap = 2.0;
    let cell = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).clamp(12.0, 28.0);
    // The wells line up with the grid's corners and edges.
    let grid_w = cell * cols as f32 + gap * (cols as f32 - 1.0);
    let well_row = |app: &mut App, ui: &mut Ui, slots: &[MixSlot], changed: &mut bool| {
        ui.horizontal(|ui| {
            let w = grid_w;
            let n = slots.len().max(1) as f32;
            let gap = if slots.len() > 1 { (w - 22.0 * n) / (n - 1.0) } else { 0.0 };
            if slots.len() == 1 {
                ui.add_space((w - 22.0) / 2.0);
            }
            ui.spacing_mut().item_spacing.x = gap.max(4.0);
            for &slot in slots {
                let hex = app.settings.mix.wells[slot.index()].clone();
                let tip = format!("{} colour, {hex}. Tap to set it to your colour", slot.name());
                if widgets::swatch(ui, to_rgba(&hex), 22.0, false).on_hover_text(tip).clicked() {
                    app.settings.mix.wells[slot.index()] = current.clone();
                    *changed = true;
                }
            }
        });
    };
    well_row(app, ui, top, &mut changed);
    ui.spacing_mut().item_spacing = vec2(gap, gap);
    for row in &grid {
        ui.horizontal(|ui| {
            for hex in row {
                if widgets::color_cell(ui, to_rgba(hex), vec2(cell, cell), *hex == current, &format!("Use {hex}"))
                    .on_hover_text(hex)
                    .clicked()
                {
                    app.set_color(to_rgba(hex));
                }
            }
        });
    }
    if grid.len() > 1 {
        well_row(app, ui, bottom, &mut changed);
    }
    ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
    let m = &mut app.settings.mix;
    let mut count = m.count as f32;
    if widgets::slider_row(ui, "Colours", &mut count, MIX_COUNT_MIN as f32..=MIX_COUNT_MAX as f32, |v| format!("{v:.0}")).changed() {
        m.count = count.round() as u32;
        changed = true;
    }
    if !layout(m.count).1.is_empty() {
        let mut rows = m.rows as f32;
        if widgets::slider_row(ui, "Rows", &mut rows, MIX_ROWS.0 as f32..=MIX_ROWS.1 as f32, |v| format!("{v:.0}")).changed() {
            m.rows = rows.round() as u32;
            changed = true;
        }
    }
    let (t_len, b_len) = (layout(m.count).0.len(), layout(m.count).1.len());
    let min_cols = MIX_COLS.0.max(t_len as u32).max(b_len as u32);
    let mut c = m.cols.max(min_cols) as f32;
    if widgets::slider_row(ui, "Cols", &mut c, min_cols as f32..=MIX_COLS.1 as f32, |v| format!("{v:.0}")).changed() {
        m.cols = c.round() as u32;
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Mixing").color(t.fg_muted));
        let cur = usize::from(app.settings.mix.space == MixSpace::Rgb);
        if let Some(i) = segmented(ui, &["Perceptual", "RGB"], cur) {
            app.settings.mix.space = if i == 1 { MixSpace::Rgb } else { MixSpace::Oklab };
            changed = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "plus", 24.0, false, "Save the whole grid as a new palette").clicked() {
                let mut seen = std::collections::HashSet::new();
                let colors: Vec<String> = grid.iter().flatten().filter(|h| seen.insert((*h).clone())).cloned().collect();
                let name = format!("Mix {}×{}", grid.len(), cols);
                let n = colors.len();
                app.palettes.create(Some(&name), colors);
                app.notify(format!("Saved {n} colours as “{name}”"), now);
            }
        });
    });
    if changed {
        app.settings.save();
    }
}

/// The colour-label swatches: "no label" and each colour. `current` is the
/// label shown as chosen (`Some(None)`: no label; `None`: mixed, nothing
/// marked). Returns the one clicked.
pub(crate) fn label_swatches(ui: &mut Ui, current: Option<Option<&str>>) -> Option<Option<String>> {
    let t = Tokens::get(ui.ctx());
    let mut picked = None;
    let none = current == Some(None);
    if ui
        .add(egui::Button::new(RichText::new("∅").color(t.fg_muted)).min_size(vec2(20.0, 20.0)).fill(if none { t.hover } else { t.field }))
        .on_hover_text("No label")
        .clicked()
    {
        picked = Some(None);
    }
    for name in onepaint_engine::Session::COLOR_LABELS {
        let Some(c) = label_color(name, t.dark()) else { continue };
        let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, current == Some(Some(name)), name));
        ui.painter().circle_filled(r.center(), 7.0, c);
        if current == Some(Some(name)) {
            ui.painter().circle_stroke(r.center(), 9.0, egui::Stroke::new(2.0, t.fg));
        }
        if resp.on_hover_text(name).clicked() {
            picked = Some(Some(name.to_string()));
        }
    }
    picked
}
