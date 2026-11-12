//! Row 2 of the header (spec 65; happypaint `editor.tsx` 864–1037 and
//! `toolbar.tsx` 2156–3064): the colours, the brush strip for brush-like
//! tools, the current tool's options in one line, and the zoom menu at the
//! end. It scrolls sideways when it doesn't fit; nothing folds away. The
//! Tool options panel keeps the same settings stacked, with their hints.

use crate::{App, Popover, dock};
use egui::{Pos2, Rect, Ui};
use onepaint_engine::{Command, Tool};
use onepaint_ui_kit::inline::{chip, dropdown, dropdown_shown, inline_number, inline_slider, menu_chip};
use onepaint_ui_kit::{icons, widgets};

const SLIDER_W: f32 = 84.0;

/// Open `p` under the control at `at` (its popover anchors there).
fn open_below(app: &mut App, p: Popover, at: Rect, view: Rect) {
    let was = dock::showing(app, p, view);
    dock::toggle(app, p, view);
    if !was {
        app.row_anchor = Some((p, Pos2::new(at.left(), at.bottom() + 6.0)));
    }
}

fn pct(v: f64) -> f32 {
    (v * 100.0) as f32
}

fn show_pct(v: f32) -> String {
    format!("{v:.0}")
}

/// The row's contents, left to right.
pub fn show(app: &mut App, ui: &mut Ui, view: Rect) {
    let tool = app.session.tool();
    colours(app, ui, view);
    if matches!(tool, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Mixer | Tool::Shape) {
        widgets::vline(ui, 24.0);
        brush_strip(app, ui, view);
    }
    widgets::vline(ui, 24.0);
    tool_options(app, ui, view, tool);
}

/// The zoom menu, laid out from the right by the caller.
pub fn zoom(app: &mut App, ui: &mut Ui) {
    let shown = format!("{:.0}%", app.canvas.view.scale * 100.0);
    let options = ["Zoom in", "Zoom out", "Fit", "100%"];
    match dropdown_shown(ui, "Zoom", &shown, &options, None, false) {
        Some(0) => crate::menus::zoom(app, 1.25),
        Some(1) => crate::menus::zoom(app, 0.8),
        Some(2) => app.canvas.view.fit(),
        Some(3) => app.canvas.view.actual_pixels(),
        _ => {}
    }
}

/// Foreground and background, swap, and black and white.
fn colours(app: &mut App, ui: &mut Ui, view: Rect) {
    let fg = app.session.color();
    let bg = app.canvas.options.background;
    let r = widgets::swatch(ui, fg, 26.0, dock::showing(app, Popover::Color, view)).on_hover_text("Foreground colour");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Foreground colour"));
    if r.clicked() {
        open_below(app, Popover::Color, r.rect, view);
    }
    let r = widgets::swatch(ui, bg, 20.0, false).on_hover_text("Background colour: click to swap");
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Background colour"));
    if r.clicked() {
        crate::menus::swap_colors(app);
    }
    if icons::button(ui, "arrow-left-right", 24.0, false, "Swap colours (X)").clicked() {
        crate::menus::swap_colors(app);
    }
    if icons::button(ui, "rotate-ccw", 24.0, false, "Black and white (D)").clicked() {
        app.set_color(onepaint_color::Rgba::BLACK);
        app.canvas.options.background = onepaint_color::Rgba::WHITE;
    }
}

/// The brush, its angle, the stabiliser and the pressure overrides.
fn brush_strip(app: &mut App, ui: &mut Ui, view: Rect) {
    use onepaint_brush::studio::{self, Param};
    let name = app.session.current_brush().name.clone();
    let r = menu_chip(ui, &name, dock::showing(app, Popover::Brushes, view)).on_hover_text("Brushes");
    if r.clicked() {
        open_below(app, Popover::Brushes, r.rect, view);
    }
    crate::chrome::brush_angle(app, ui);
    let mut b = app.session.current_brush().clone();
    let on = b.stabilizer.as_ref().is_some_and(|s| s.enabled);
    if chip(ui, "Stabilizer", on).on_hover_text("Pulled-string smoothing for clean inking").clicked() {
        studio::set_stabilizer(&mut b, !on);
        let _ = app.session.execute(Command::SetCurrentBrush { preset: Box::new(b.clone()) });
    }
    // Reads 0 while off: the slider is both switch and strength.
    let mut amount = if on { pct(Param::StabilizerAmount.get(&b)) } else { 0.0 };
    if inline_slider(ui, "Amount", &mut amount, 0.0..=100.0, SLIDER_W, show_pct).changed() {
        studio::set_stabilizer(&mut b, amount > 0.0);
        if amount > 0.0 {
            Param::StabilizerAmount.set(&mut b, amount as f64 / 100.0);
        }
        let _ = app.session.execute(Command::SetCurrentBrush { preset: Box::new(b) });
    }
    ui.label(
        egui::RichText::new("Pressure").font(onepaint_ui_kit::theme::medium(12.0)).color(onepaint_ui_kit::Tokens::get(ui.ctx()).fg_muted),
    );
    let s = &mut app.settings;
    let mut changed = false;
    if chip(ui, "Size", s.force_size_pressure).on_hover_text("Pen pressure → size for every brush").clicked() {
        s.force_size_pressure = !s.force_size_pressure;
        changed = true;
    }
    if chip(ui, "Opacity", s.force_opacity_pressure).on_hover_text("Pen pressure → opacity for every brush").clicked() {
        s.force_opacity_pressure = !s.force_opacity_pressure;
        changed = true;
    }
    if changed {
        app.settings.save();
    }
}

fn tool_options(app: &mut App, ui: &mut Ui, view: Rect, tool: Tool) {
    let now = ui.input(|i| i.time);
    // A transform in progress shows its controls whatever the tool.
    if app.session.transform_active() || tool == Tool::Move {
        transform(app, ui);
        return;
    }
    match tool {
        Tool::Smudge => {
            let mut v = pct(app.session.paint_settings().smudge_strength);
            if inline_slider(ui, "Strength", &mut v, 0.0..=100.0, SLIDER_W, show_pct).changed() {
                let _ = app.session.execute(Command::SetSmudgeStrength { strength: v as f64 / 100.0 });
            }
        }
        Tool::Mixer => mixer(app, ui, now),
        Tool::Liquify => {
            use onepaint_filters::liquify::LIQUIFY_MODES;
            let ps = app.session.paint_settings();
            let (mut mode, mut v) = (ps.liquify_mode, pct(ps.liquify_strength));
            let names: Vec<&str> = LIQUIFY_MODES.iter().map(|m| m.1).collect();
            let cur = LIQUIFY_MODES.iter().position(|m| m.0 == mode).unwrap_or(0);
            let mut changed = false;
            if let Some(i) = dropdown(ui, "Liquify mode", &names, cur, false) {
                mode = LIQUIFY_MODES[i].0;
                changed = true;
            }
            changed |= inline_slider(ui, "Strength", &mut v, 0.0..=100.0, SLIDER_W, show_pct).changed();
            if changed {
                let _ = app.session.execute(Command::SetLiquify { mode, strength: v as f64 / 100.0 });
            }
        }
        Tool::Fill => fill(app, ui),
        Tool::Gradient => {
            use onepaint_filters::gradient::Shape;
            let o = &mut app.canvas.options;
            if let Some(i) = dropdown(ui, "Gradient output", &["Pixels", "Fill layer"], usize::from(o.gradient_layer), false) {
                o.gradient_layer = i == 1;
            }
            let shapes: Vec<&str> = Shape::ALL.iter().map(|s| s.label()).collect();
            let cur = Shape::ALL.iter().position(|s| *s == app.canvas.gradient.shape).unwrap_or(0);
            if let Some(i) = dropdown(ui, "Gradient shape", &shapes, cur, false) {
                app.canvas.gradient.shape = Shape::ALL[i];
            }
            if chip(ui, "Reverse", app.canvas.options.gradient_reverse).clicked() {
                app.canvas.options.gradient_reverse = !app.canvas.options.gradient_reverse;
            }
            let r = menu_chip(ui, "Stops", dock::showing(app, Popover::GradientOptions, view)).on_hover_text("Stops and presets");
            if r.clicked() {
                open_below(app, Popover::GradientOptions, r.rect, view);
            }
        }
        Tool::SelectRect | Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly | Tool::SelectWand => select(app, ui, view, tool),
        Tool::Pen => {
            if chip(ui, "Fill", app.canvas.options.pen_fill).on_hover_text("Fill closed paths").clicked() {
                app.canvas.options.pen_fill = !app.canvas.options.pen_fill;
            }
            let r = menu_chip(ui, "Path", dock::showing(app, Popover::PenOptions, view)).on_hover_text("Selection, fill, stroke, mask");
            if r.clicked() {
                open_below(app, Popover::PenOptions, r.rect, view);
            }
        }
        Tool::Shape => {
            use onepaint_stroke::shape::ShapeKind as K;
            let kinds = [K::Line, K::Ellipse, K::Rect, K::Polygon];
            let o = &mut app.canvas.options;
            let cur = kinds.iter().position(|k| *k == o.shape_kind).unwrap_or(0);
            if let Some(i) = crate::chrome::segmented(ui, &["Line", "Ellipse", "Rectangle", "Polygon"], cur) {
                o.shape_kind = kinds[i];
            }
            if o.shape_kind == K::Polygon {
                let mut n = o.shape_sides as f32;
                if inline_slider(ui, "Sides", &mut n, 3.0..=12.0, 60.0, show_pct).changed() {
                    o.shape_sides = onepaint_stroke::shape::clamp_sides(n as f64);
                }
            }
            if o.shape_kind != K::Line && chip(ui, "Fill", o.shape_fill).clicked() {
                o.shape_fill = !o.shape_fill;
            }
        }
        Tool::Panel => {
            ui.label(egui::RichText::new("Grid").font(onepaint_ui_kit::theme::medium(12.0)));
            for l in onepaint_panel::geometry::LAYOUT_PRESETS {
                if chip(ui, &format!("{}×{}", l.rows, l.cols), false).on_hover_text(l.tip).clicked() {
                    crate::panel_ui::create_grid(app, l.rows, l.cols, now);
                }
            }
        }
        _ => {}
    }
}

fn transform(app: &mut App, ui: &mut Ui) {
    use onepaint_engine::transform::{TransformParams, WarpMode};
    let active = app.session.transform_active();
    ui.add_enabled_ui(active, |ui| {
        let (s, c) = (&mut app.session, &mut app.canvas);
        if icons::button(ui, "arrow-left-right", 26.0, false, "Flip horizontal").clicked() {
            c.xform_about_centre(s, |p| TransformParams { sx: -p.sx, ..p });
        }
        if icons::button(ui, "arrow-up-down", 26.0, false, "Flip vertical").clicked() {
            c.xform_about_centre(s, |p| TransformParams { sy: -p.sy, ..p });
        }
        if icons::button(ui, "rotate-ccw", 26.0, false, "Rotate −15°").clicked() {
            c.xform_about_centre(s, |p| TransformParams { rotation: p.rotation - 15f64.to_radians(), ..p });
        }
        if icons::button(ui, "rotate-cw", 26.0, false, "Rotate +15°").clicked() {
            c.xform_about_centre(s, |p| TransformParams { rotation: p.rotation + 15f64.to_radians(), ..p });
        }
        if icons::button(ui, "zoom-out", 26.0, false, "Scale down ÷1.1").clicked() {
            c.xform_about_centre(s, |p| TransformParams { sx: p.sx / 1.1, sy: p.sy / 1.1, ..p });
        }
        if icons::button(ui, "zoom-in", 26.0, false, "Scale up ×1.1").clicked() {
            c.xform_about_centre(s, |p| TransformParams { sx: p.sx * 1.1, sy: p.sy * 1.1, ..p });
        }
        let mode = match s.transform_warp() {
            None => 0,
            Some(WarpMode::Quad(_)) => 1,
            Some(WarpMode::Mesh(_)) => 2,
        };
        if chip(ui, "Distort", mode == 1).clicked() && mode != 1 {
            let _ = s.execute(Command::BeginDistort);
        }
        if chip(ui, "Warp", mode == 2).clicked() && mode != 2 {
            let _ = s.execute(Command::BeginWarp);
        }
        widgets::vline(ui, 24.0);
        if widgets::primary_button(ui, "Apply", 64.0).clicked() {
            app.commit_transform();
        }
        if widgets::secondary_button(ui, "Cancel", 64.0).clicked() {
            app.cancel_transform();
        }
    });
}

fn mixer(app: &mut App, ui: &mut Ui, now: f64) {
    let mut m = app.session.paint_settings().mixer;
    let p = |v: f64| (v * 100.0).round() as i32;
    let presets = crate::chrome::MIXER_PRESETS;
    let mut names = vec!["Custom"];
    names.extend(presets.iter().map(|x| x.0));
    let cur = presets.iter().position(|x| p(x.1) == p(m.wet) && p(x.2) == p(m.load) && p(x.3) == p(m.mix)).map_or(0, |i| i + 1);
    let mut changed = false;
    if let Some(i) = dropdown(ui, "Mixer preset", &names, cur, false)
        && i > 0
    {
        let x = presets[i - 1];
        (m.wet, m.load, m.mix) = (x.1, x.2, x.3);
        changed = true;
    }
    for (label, v) in [("Wet", &mut m.wet), ("Load", &mut m.load), ("Mix", &mut m.mix)] {
        let mut f = pct(*v);
        if inline_slider(ui, label, &mut f, 0.0..=100.0, 64.0, show_pct).changed() {
            *v = f as f64 / 100.0;
            changed = true;
        }
    }
    if chip(ui, "Keep colour", m.keep).clicked() {
        m.keep = !m.keep;
        changed = true;
    }
    if changed {
        let _ = app.session.execute(Command::SetMixer { mixer: m });
    }
    let armed = app.canvas.mixer_load_armed;
    if chip(ui, "Load from canvas", armed).on_hover_text("Tap the canvas to load the brush (or Alt-click)").clicked() {
        app.canvas.mixer_load_armed = !armed;
    }
    if widgets::secondary_button(ui, "Clean", 56.0).clicked() {
        let _ = app.session.execute(Command::CleanMixer);
        app.notify("Brush cleaned", now);
    }
}

fn fill(app: &mut App, ui: &mut Ui) {
    let o = &mut app.canvas.options;
    let mut tol = o.fill_tolerance * 100.0;
    if inline_slider(ui, "Tolerance", &mut tol, 0.0..=100.0, SLIDER_W, show_pct).changed() {
        o.fill_tolerance = tol / 100.0;
    }
    let mut grow = o.fill_grow as f32;
    if inline_slider(ui, "Grow", &mut grow, 0.0..=8.0, 60.0, |v| format!("{v:.0} px")).changed() {
        o.fill_grow = grow.round() as u32;
    }
    if chip(ui, "Wash", o.fill_wash).on_hover_text("A watercolour wash instead of a flat fill").clicked() {
        o.fill_wash = !o.fill_wash;
    }
    if !o.fill_wash {
        return;
    }
    let w = &mut o.wash;
    if chip(ui, "Water only", w.water_only).clicked() {
        w.water_only = !w.water_only;
    }
    let mut rows: Vec<(&str, &mut f32)> = vec![("Water", &mut w.water)];
    if !w.water_only {
        rows.push(("Pigment", &mut w.pigment));
        rows.push(("Graded", &mut w.grade));
    }
    rows.push(("Soft", &mut w.soft));
    rows.push(("Speed", &mut w.speed));
    for (label, v) in rows {
        let mut f = *v * 100.0;
        if inline_slider(ui, label, &mut f, 0.0..=100.0, 60.0, show_pct).changed() {
            *v = f / 100.0;
        }
    }
    if chip(ui, "All layers", w.sample_all).clicked() {
        w.sample_all = !w.sample_all;
    }
}

fn select(app: &mut App, ui: &mut Ui, view: Rect, tool: Tool) {
    use onepaint_engine::SelectMode as M;
    for (m, icon, tip) in [
        (M::Replace, "square-dashed", "New selection"),
        (M::Add, "plus", "Add to the selection (Shift)"),
        (M::Subtract, "minus", "Subtract from the selection (Alt)"),
        (M::Intersect, "blend", "Intersect (Shift+Alt)"),
    ] {
        if icons::button(ui, icon, 26.0, app.canvas.options.select_mode == m, tip).clicked() {
            app.canvas.options.select_mode = m;
        }
    }
    if tool == Tool::SelectWand {
        let o = &mut app.canvas.options;
        let mut tol = o.wand_tolerance * 100.0;
        if inline_slider(ui, "Tolerance", &mut tol, 0.0..=100.0, SLIDER_W, show_pct).changed() {
            o.wand_tolerance = tol / 100.0;
        }
        if chip(ui, "Contiguous", o.wand_contiguous).clicked() {
            o.wand_contiguous = !o.wand_contiguous;
        }
        if chip(ui, "All layers", o.wand_sample_all).clicked() {
            o.wand_sample_all = !o.wand_sample_all;
        }
    }
    if matches!(tool, Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly) {
        let o = &mut app.canvas.options;
        if chip(ui, "Anti-alias", o.select_antialias).on_hover_text("Smooth, partly selected edges").clicked() {
            o.select_antialias = !o.select_antialias;
        }
    }
    widgets::vline(ui, 24.0);
    let has = app.session.selection().is_some();
    let r = crate::menus::REFINE_PX;
    let acts = [
        ("maximize-2", "Select all".to_string(), Command::SelectAll, true),
        ("blend", "Invert".to_string(), Command::InvertSelection, has),
        ("plus", format!("Expand {r} px"), Command::GrowSelection { delta: r }, has),
        ("minus", format!("Contract {r} px"), Command::GrowSelection { delta: -r }, has),
        ("circle-dashed", "Smooth".to_string(), Command::SmoothSelection { px: r as f32 }, has),
        ("x", "Deselect".to_string(), Command::Deselect, has),
    ];
    for (icon, tip, cmd, on) in acts {
        ui.add_enabled_ui(on, |ui| {
            if icons::button(ui, icon, 26.0, false, &tip).clicked() {
                let _ = app.session.execute(cmd);
            }
        });
    }
    let saved = menu_chip(ui, "Saved", dock::showing(app, Popover::Selection, view)).on_hover_text("Save and load selections");
    if saved.clicked() {
        open_below(app, Popover::Selection, saved.rect, view);
    }
    ui.add_enabled_ui(has, |ui| {
        if icons::button(ui, "copy", 26.0, false, "Copy to new layer (Ctrl+J)").clicked() {
            let _ = app.session.execute(Command::LayerViaCopy { cut: false });
        }
        if icons::button(ui, "layers", 26.0, false, "Cut to new layer (Ctrl+Shift+J)").clicked() {
            let _ = app.session.execute(Command::LayerViaCopy { cut: true });
        }
    });
    // Set before or after drawing: it softens the selection as it applies.
    let mut feather = f64::from(app.session.feather());
    let r = inline_number(ui, "Feather", &mut feather, 0.0..=f64::from(onepaint_engine::FEATHER_MAX), " px", 0);
    if r.on_hover_text("Soft edge, px (a Gaussian radius, as in Photoshop)").changed() {
        let _ = app.session.execute(Command::SetFeather { radius: feather as f32 });
    }
}
