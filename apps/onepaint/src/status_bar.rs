//! The status bar along the bottom (happypaint `components/status-bar`):
//! save state, tool and brush, colour (drag it onto the canvas for
//! ColorDrop), fill threshold, layer, cursor and measure on the left;
//! document, rotation, zoom, history, symmetry, perspective, fill mode and
//! the value check on the right. Narrow windows fold the details into "…".

use crate::{App, Popover};
use egui::{Align2, Area, Id, Order, RichText, Sense, Ui, pos2, vec2};
use onepaint_color::Rgba;
use onepaint_engine::{Command, SymmetryMode, Tool};
use onepaint_ui_kit::widgets::{self, glass_frame, popover_frame};
use onepaint_ui_kit::{Tokens, icons, theme};

/// Height of the bar, including its frame.
pub const HEIGHT: f32 = 34.0;
/// Below this width the details fold into the "…" popover (`max-width: 900px`).
pub const COMPACT_W: f32 = 900.0;
/// ColorDrop: hold this long (with < `CD_MOVE_EPS` px steps) to lock the seed.
const CD_LOCK_S: f64 = 0.3;
const CD_MOVE_EPS: f32 = 5.0;
/// Horizontal screen px per unit of threshold once locked (full range ≈ 400 px).
const CD_PX_PER_TOL: f32 = 400.0;
const ZOOM_STEP: f64 = 1.2;

/// A ColorDrop drag in progress (happypaint `state/color-drop.ts` and the
/// canvas handler, `paint-canvas.tsx:5239-5337`).
#[derive(Clone, Debug)]
pub struct ColorDropDrag {
    pub color: Rgba,
    /// Last pointer position and when the hold timer was last re-armed.
    last: egui::Pos2,
    armed_at: f64,
    /// Seed locked: (doc seed, screen x at lock, threshold at lock).
    locked: Option<([f64; 2], f32, f32)>,
    /// The engine has a preview session.
    live: bool,
}

pub(crate) fn tool_label(t: Tool) -> &'static str {
    match t {
        Tool::Brush => "Brush",
        Tool::Eraser => "Eraser",
        Tool::Fill => "Fill",
        Tool::Eyedropper => "Eyedropper",
        Tool::SelectRect => "Rectangle select",
        Tool::SelectEllipse => "Ellipse select",
        Tool::SelectLasso => "Lasso select",
        Tool::SelectPoly => "Polygon select",
        Tool::SelectWand => "Magic wand",
        Tool::Move => "Move",
        Tool::Gradient => "Gradient",
        Tool::Shape => "Shape",
        Tool::Text => "Text",
        Tool::Smudge => "Smudge",
        Tool::Mixer => "Mixer",
        Tool::Liquify => "Liquify",
        Tool::Pen => "Pen",
        Tool::Hand => "Pan",
        Tool::Panel => "Panel",
    }
}

fn brushy(t: Tool) -> bool {
    matches!(t, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Mixer)
}

fn depth_label(d: onepaint_doc::Depth) -> &'static str {
    match d {
        onepaint_doc::Depth::Rgba8 => "8-bit",
        onepaint_doc::Depth::Rgba16F => "16f",
        onepaint_doc::Depth::Rgba32F => "32f",
    }
}

/// Toggle one mirror axis; both on is quadrant symmetry, both off is off
/// (`toggleSymmetryAxis`, guides-store.ts:207-214). Radial counts as neither.
pub fn toggle_axis(mode: SymmetryMode, vertical: bool) -> SymmetryMode {
    let mut v = matches!(mode, SymmetryMode::Vertical | SymmetryMode::Both);
    let mut h = matches!(mode, SymmetryMode::Horizontal | SymmetryMode::Both);
    if vertical {
        v = !v;
    } else {
        h = !h;
    }
    match (v, h) {
        (true, true) => SymmetryMode::Both,
        (true, false) => SymmetryMode::Vertical,
        (false, true) => SymmetryMode::Horizontal,
        (false, false) => SymmetryMode::Off,
    }
}

/// Draw the bar across `rect`'s bottom edge; returns the bar's rect.
pub fn show(app: &mut App, ctx: &egui::Context, rect: egui::Rect) -> egui::Rect {
    let compact = ctx.content_rect().width() < COMPACT_W;
    let bar = egui::Rect::from_min_max(pos2(rect.left(), rect.bottom() - HEIGHT), rect.max);
    let r = Area::new(Id::new("status")).order(Order::Middle).fixed_pos(bar.min).show(ctx, |ui| {
        glass_frame(ui).corner_radius(0.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
            ui.set_width(bar.width() - 20.0);
            ui.set_height(HEIGHT - 8.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                left(app, ui, compact);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    right(app, ui, compact);
                });
            });
        });
    });
    if app.popover == Some(Popover::StatusMore) {
        let at = pos2(bar.right() - 8.0, bar.top() - 6.0);
        Area::new(Id::new(("popover", "status-more"))).order(Order::Foreground).pivot(Align2::RIGHT_BOTTOM).fixed_pos(at).show(ctx, |ui| {
            popover_frame(ui).show(ui, |ui| more(app, ui));
        });
    }
    color_drop_tick(app, ctx);
    r.response.rect
}

fn mono(ui: &mut Ui, text: impl Into<String>, color: egui::Color32) -> egui::Response {
    ui.label(RichText::new(text).font(theme::mono(11.5)).color(color))
}

fn left(app: &mut App, ui: &mut Ui, compact: bool) {
    let t = Tokens::get(ui.ctx());
    let dirty = app.dirty();
    let (label, color) = if dirty { ("Unsaved", t.warning) } else { ("Saved", t.success) };
    let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().circle_filled(dot.center(), 3.5, color);
    ui.label(RichText::new(label).font(theme::medium(11.5)).color(t.fg_muted)).on_hover_text(label);
    let tool = app.session.tool();
    ui.label(RichText::new(tool_label(tool)).font(theme::semibold(11.5)).color(t.fg));
    if brushy(tool) && !compact {
        let b = app.session.current_brush();
        mono(ui, format!("{} · {:.0} px", b.name, b.size), t.fg_muted);
    }
    // The colour: drag it onto the canvas to fill (ColorDrop).
    let c = app.session.color();
    const DROP_TIP: &str = "Drag onto the canvas to fill (ColorDrop) · hold + slide to set threshold";
    let r = widgets::swatch(ui, c, 18.0, false).interact(Sense::drag()).on_hover_text(DROP_TIP);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, DROP_TIP));
    if r.drag_started()
        && let Some(p) = r.interact_pointer_pos()
    {
        let now = ui.input(|i| i.time);
        app.color_drop = Some(ColorDropDrag { color: c, last: p, armed_at: now, locked: None, live: false });
    }
    mono(ui, c.to_hex(), t.fg_muted);
    if tool == Tool::Fill && !compact {
        threshold_controls(app, ui);
    }
    if !compact && let Some(n) = app.session.doc().find(app.session.active()) {
        // One label, so its accessible name doesn't clash with the layer's row.
        let mut job = egui::text::LayoutJob::default();
        let fmt = |font, color| egui::TextFormat { font_id: font, color, ..Default::default() };
        job.append("Layer  ", 0.0, fmt(theme::medium(11.0), t.fg_faint));
        job.append(n.name(), 0.0, fmt(theme::medium(11.5), t.fg_muted));
        ui.label(job);
    }
    if !compact {
        let pos = app.canvas.hover_doc.map_or("–".to_string(), |[x, y]| format!("{x:.0}, {y:.0}"));
        mono(ui, pos, t.fg_faint);
    }
    if let Some((a, b)) = app.canvas.measure {
        let d = app.session.doc();
        let (len, _, deg) = onepaint_stroke::guide_lines::measure_between(a, b, &app.session.guides().units, d.dpi);
        mono(ui, format!("↔ {len}  {deg:.1}°"), t.accent).on_hover_text("Measured distance and angle (Escape clears)");
    }
    if app.session.is_recording() {
        mono(ui, "● REC", t.danger);
    }
}

fn threshold_controls(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let mut tol = app.canvas.options.fill_tolerance * 100.0;
    ui.allocate_ui(vec2(90.0, 20.0), |ui| {
        ui.set_width(90.0);
        widgets::slider(ui, &mut tol, 0.0..=100.0, None).on_hover_text("ColorDrop / fill threshold");
    });
    app.canvas.options.fill_tolerance = (tol / 100.0).clamp(0.0, 1.0);
    mono(ui, format!("{:.0}%", tol), t.fg_muted);
    let mut grow = app.canvas.options.fill_grow as f32;
    ui.allocate_ui(vec2(70.0, 20.0), |ui| {
        ui.set_width(70.0);
        widgets::slider(ui, &mut grow, 0.0..=8.0, None).on_hover_text("Grow the fill under anti-aliased edges (closes the gap at a line)");
    });
    app.canvas.options.fill_grow = grow.round().clamp(0.0, 8.0) as u32;
    mono(ui, format!("{} px", app.canvas.options.fill_grow), t.fg_muted);
}

/// Right to left: the layout runs backwards, so the last item comes first.
fn right(app: &mut App, ui: &mut Ui, compact: bool) {
    let t = Tokens::get(ui.ctx());
    mono(ui, concat!("v", env!("CARGO_PKG_VERSION")), t.fg_faint);
    if compact && icons::button(ui, "ellipsis", 24.0, app.popover == Some(Popover::StatusMore), "More details").clicked() {
        app.popover = if app.popover == Some(Popover::StatusMore) { None } else { Some(Popover::StatusMore) };
    }
    widgets::vline(ui, 16.0);
    // Value check and fill mode.
    let mut modes = app.canvas.view_modes();
    if icons::button(ui, "contrast", 24.0, modes.grayscale, "Grayscale value-check view (Ctrl+Y)").clicked() {
        modes.grayscale = !modes.grayscale;
        app.canvas.set_view_modes(modes);
    }
    let fill = app.canvas.options.fill_shape;
    if icons::button(ui, "paint-bucket", 24.0, fill, "Fill mode — brush strokes fill their silhouette (F)").clicked() {
        app.canvas.options.fill_shape = !fill;
    }
    // Perspective: edit the vanishing points.
    if app.session.guides().perspective.enabled
        && icons::button(
            ui,
            "spline",
            24.0,
            app.canvas.persp_editing,
            "Edit vanishing points — drag the VP handles on the canvas (Shift+P)",
        )
        .clicked()
    {
        app.canvas.persp_editing = !app.canvas.persp_editing;
    }
    // Symmetry: the two mirror axes, and moving the axis.
    let sym = app.session.guides().symmetry;
    if sym.is_on()
        && icons::button(ui, "crosshair", 24.0, app.canvas.sym_editing, "Move symmetry axis — drag the canvas to reposition (Shift+Y)")
            .clicked()
    {
        app.canvas.sym_editing = !app.canvas.sym_editing;
    }
    let horiz = matches!(sym.mode, SymmetryMode::Horizontal | SymmetryMode::Both);
    if icons::button(ui, "flip-vertical", 24.0, horiz, "Horizontal symmetry — mirror up/down (Y toggles symmetry)").clicked() {
        set_mode(app, toggle_axis(sym.mode, false));
    }
    let vert = matches!(sym.mode, SymmetryMode::Vertical | SymmetryMode::Both);
    if icons::button(ui, "flip-horizontal", 24.0, vert, "Vertical symmetry — mirror left/right (Y toggles symmetry)").clicked() {
        set_mode(app, toggle_axis(sym.mode, true));
    }
    widgets::vline(ui, 16.0);
    ui.add_enabled_ui(app.session.can_redo(), |ui| {
        if icons::button(ui, "redo-2", 24.0, false, "Redo (Ctrl+Shift+Z · three-finger tap)").clicked() {
            let _ = app.session.execute(Command::Redo);
        }
    });
    ui.add_enabled_ui(app.session.can_undo(), |ui| {
        if icons::button(ui, "undo-2", 24.0, false, "Undo (Ctrl+Z · two-finger tap)").clicked() {
            let _ = app.session.execute(Command::Undo);
        }
    });
    widgets::vline(ui, 16.0);
    zoom(app, ui);
    if !compact {
        rotation(app, ui);
        let n = app.session.doc().layer_ids().len();
        mono(ui, format!("{n} layer{}", if n == 1 { "" } else { "s" }), t.fg_faint);
        doc_button(app, ui);
    }
}

fn set_mode(app: &mut App, mode: SymmetryMode) {
    let mut sym = app.session.guides().symmetry;
    if sym.mode != SymmetryMode::Off && mode == SymmetryMode::Off {
        app.last_symmetry = sym.mode;
    }
    sym.mode = mode;
    let _ = app.session.execute(Command::SetSymmetry { symmetry: sym });
}

fn zoom(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let v = &app.canvas.view;
    let c = [v.view_size[0] / 2.0, v.view_size[1] / 2.0];
    let scale = v.scale;
    ui.add_enabled_ui(scale < onepaint_engine::view::MAX_SCALE, |ui| {
        if icons::button(ui, "plus", 24.0, false, "Zoom in").clicked() {
            app.canvas.view.zoom_at(c, ZOOM_STEP);
        }
    });
    let pct = RichText::new(format!("{:.0}%", scale * 100.0)).font(theme::mono(11.5)).color(t.fg);
    let r = ui.add(egui::Button::new(pct).frame(false).min_size(vec2(44.0, 20.0))).on_hover_text("Reset zoom to 100%");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Reset zoom to 100%"));
    if r.clicked() {
        app.canvas.view.zoom_at(c, 1.0 / scale);
    }
    ui.add_enabled_ui(scale > onepaint_engine::view::MIN_SCALE, |ui| {
        if icons::button(ui, "minus", 24.0, false, "Zoom out").clicked() {
            app.canvas.view.zoom_at(c, 1.0 / ZOOM_STEP);
        }
    });
}

/// Degrees in (−180, 180].
fn signed_degrees(rad: f64) -> f64 {
    let d = rad.to_degrees().rem_euclid(360.0);
    if d > 180.0 { d - 360.0 } else { d }
}

fn rotation(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let deg = signed_degrees(app.canvas.view.rotation);
    let label = RichText::new(format!("{deg:.0}°")).font(theme::mono(11.5)).color(t.fg_muted);
    let r = ui.add(egui::Label::new(label).sense(Sense::click())).on_hover_text("Double-click to reset rotation to 0°");
    let v = &app.canvas.view;
    let c = [v.view_size[0] / 2.0, v.view_size[1] / 2.0];
    let mut a = v.rotation as f32;
    let dial = widgets::rotation_dial(ui, &mut a, 16.0, "Drag to rotate · click to reset (Ctrl+[ / Ctrl+] · 5)");
    if dial.changed() {
        let delta = a as f64 - app.canvas.view.rotation;
        app.canvas.view.rotate_at(c, delta);
    }
    if dial.clicked() || r.double_clicked() {
        let r = app.canvas.view.rotation;
        app.canvas.view.rotate_at(c, -r);
    }
}

fn doc_button(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let d = app.session.doc();
    let mut text = format!("{}×{} · {:.0} dpi · {}", d.width, d.height, d.dpi, depth_label(d.depth));
    if d.color_space.eq_ignore_ascii_case("display-p3") {
        text.push_str(" · P3");
    }
    let b = egui::Button::new(RichText::new(text).font(theme::mono(11.5)).color(t.fg_faint)).frame(false);
    let r = ui.add(b).on_hover_text("Document properties");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Document properties"));
    if r.clicked() {
        app.popover = Some(Popover::Paper);
    }
}

/// The details a narrow window hides.
fn more(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(220.0);
    widgets::section_label(ui, "Details");
    let tool = app.session.tool();
    if brushy(tool) {
        let b = app.session.current_brush();
        mono(ui, format!("Brush  {} · {:.0} px", b.name, b.size), t.fg_muted);
    }
    if let Some(n) = app.session.doc().find(app.session.active()) {
        mono(ui, format!("Layer  {}", n.name()), t.fg_muted);
    }
    let pos = app.canvas.hover_doc.map_or("–".to_string(), |[x, y]| format!("{x:.0}, {y:.0}"));
    mono(ui, format!("Cursor  {pos}"), t.fg_muted);
    let n = app.session.doc().layer_ids().len();
    mono(ui, format!("{n} layer{}", if n == 1 { "" } else { "s" }), t.fg_muted);
    if tool == Tool::Fill {
        ui.horizontal(|ui| threshold_controls(app, ui));
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        rotation(app, ui);
        if widgets::secondary_button(ui, "Document…", 90.0).clicked() {
            app.popover = Some(Popover::Paper);
        }
    });
}

/// Track a ColorDrop drag: preview on the canvas, hold to lock the seed and
/// slide to set the threshold, release to commit, Esc to abandon.
fn color_drop_tick(app: &mut App, ctx: &egui::Context) {
    let Some(mut cd) = app.color_drop.take() else {
        return;
    };
    let (now, pos, down, esc) = ctx.input(|i| (i.time, i.pointer.latest_pos(), i.pointer.primary_down(), i.key_pressed(egui::Key::Escape)));
    if esc {
        app.session.cancel_color_drop();
        return;
    }
    if !down {
        if cd.live {
            app.session.commit_color_drop();
            app.push_recent_color(cd.color);
        }
        return;
    }
    ctx.request_repaint();
    let Some(p) = pos else {
        app.color_drop = Some(cd);
        return;
    };
    let over = app.canvas.stage.contains(p) && ctx.layer_id_at(p).is_none_or(|l| l.order == Order::Background);
    let (tol, grow) = (app.canvas.options.fill_tolerance, app.canvas.options.fill_grow);
    if !over {
        // Leaving the canvas drops the preview; re-entering starts afresh.
        if cd.live {
            app.session.cancel_color_drop();
        }
        cd.live = false;
        cd.locked = None;
    } else if let Some((seed, lock_x, base)) = cd.locked {
        let tol = (base + (p.x - lock_x) / CD_PX_PER_TOL).clamp(0.0, 1.0);
        app.canvas.options.fill_tolerance = tol;
        app.session.update_color_drop(seed[0], seed[1], tol, grow);
    } else {
        let d = app.canvas.screen_doc(p);
        if !cd.live {
            cd.live = app.session.begin_color_drop(d[0], d[1], cd.color, tol, grow);
            cd.armed_at = now;
        } else {
            app.session.update_color_drop(d[0], d[1], tol, grow);
        }
        if p.distance(cd.last) >= CD_MOVE_EPS {
            cd.armed_at = now;
        } else if cd.live && now - cd.armed_at >= CD_LOCK_S {
            cd.locked = Some((d, p.x, tol));
        }
    }
    cd.last = p;
    draw_color_drop(app, ctx, &cd, p);
    app.color_drop = Some(cd);
}

fn draw_color_drop(app: &App, ctx: &egui::Context, cd: &ColorDropDrag, p: egui::Pos2) {
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Tooltip, Id::new("color-drop-chip")));
    painter.circle_filled(p + vec2(0.0, 2.0), 14.0, egui::Color32::from_black_alpha(60));
    painter.circle_filled(p, 13.0, widgets::to_color32(cd.color));
    painter.circle_stroke(p, 12.0, egui::Stroke::new(2.0, egui::Color32::from_white_alpha(230)));
    if cd.locked.is_some() {
        let screen = ctx.content_rect();
        Area::new(Id::new("color-drop-hud"))
            .order(Order::Tooltip)
            .pivot(Align2::CENTER_TOP)
            .fixed_pos(pos2(screen.center().x, screen.top() + 14.0))
            .interactable(false)
            .show(ctx, |ui| {
                popover_frame(ui).show(ui, |ui| {
                    let pct = (app.canvas.options.fill_tolerance * 100.0).round();
                    ui.label(RichText::new(format!("Threshold {pct:.0}%")).font(theme::mono(12.0)));
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_toggles_combine_like_happypaint() {
        use SymmetryMode::*;
        assert_eq!(toggle_axis(Off, true), Vertical);
        assert_eq!(toggle_axis(Vertical, false), Both);
        assert_eq!(toggle_axis(Both, true), Horizontal);
        assert_eq!(toggle_axis(Horizontal, false), Off);
        assert_eq!(toggle_axis(Radial, false), Horizontal, "radial counts as neither axis");
    }

    #[test]
    fn rotation_reads_in_minus_180_to_180() {
        assert_eq!(signed_degrees(0.0), 0.0);
        assert!((signed_degrees(190f64.to_radians()) + 170.0).abs() < 1e-9);
        assert!((signed_degrees(180f64.to_radians()) - 180.0).abs() < 1e-9);
        assert!((signed_degrees(-30f64.to_radians()) + 30.0).abs() < 1e-9);
    }
}
