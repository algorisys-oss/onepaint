// Adapted from photocraft (MIT/Apache-2.0): crates/ui-egui/src/widgets.rs
//! onepaint widgets: glass panels, thin sliders with round knobs, value fields,
//! toggles, buttons, progress bars, swatches and an HSV colour picker.

use crate::theme::{self, Tokens};
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use onepaint_color::Rgba;

/// A floating, translucent, rounded panel with a soft shadow.
pub fn glass_frame(ui: &Ui) -> egui::Frame {
    let t = Tokens::get(ui.ctx());
    egui::Frame::new()
        .fill(t.glass)
        .stroke(Stroke::new(1.0, t.border.gamma_multiply(0.8)))
        .corner_radius(t.radius_lg)
        .inner_margin(egui::Margin::same(6))
        .shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: t.shadow })
}

/// An opaque popover panel.
pub fn popover_frame(ui: &Ui) -> egui::Frame {
    let t = Tokens::get(ui.ctx());
    egui::Frame::new()
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(t.radius_lg)
        .inner_margin(egui::Margin::same(12))
        .shadow(egui::Shadow { offset: [0, 12], blur: 36, spread: 0, color: t.shadow })
}

pub fn section_label(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(text.to_uppercase()).font(theme::medium(10.5)).color(t.fg_faint));
}

pub fn hairline(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, t.border));
}

pub fn vline(ui: &mut Ui, height: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, height), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range().shrink(4.0), Stroke::new(1.0, t.border));
}

/// Transparency checkerboard.
pub fn checker(painter: &egui::Painter, rect: Rect, cell: f32) {
    let t = Tokens::get(painter.ctx());
    painter.rect_filled(rect, 0.0, t.checker_b);
    let (nx, ny) = ((rect.width() / cell).ceil() as i32, (rect.height() / cell).ceil() as i32);
    for y in 0..ny {
        for x in 0..nx {
            if (x + y) % 2 == 0 {
                let r = Rect::from_min_size(rect.min + vec2(x as f32 * cell, y as f32 * cell), Vec2::splat(cell)).intersect(rect);
                painter.rect_filled(r, 0.0, t.checker_a);
            }
        }
    }
}

fn knob(painter: &egui::Painter, center: Pos2, hovered: bool, t: &Tokens) {
    painter.circle_filled(center + vec2(0.0, 1.0), 7.5, Color32::from_black_alpha(90));
    painter.circle_filled(center, 7.0, Color32::WHITE);
    if hovered {
        painter.circle_stroke(center, 10.0, Stroke::new(3.0, t.accent_soft));
    }
}

/// Thin-track horizontal slider with a round knob. `gradient` paints the track.
pub fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, gradient: Option<&[Color32]>) -> Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().max(60.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(width, 20.0), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let track = Rect::from_center_size(rect.center(), vec2(rect.width() - 16.0, if gradient.is_some() { 8.0 } else { 4.0 }));
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let f = ((p.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let nv = lo + f * (hi - lo);
        if (nv - *value).abs() > f32::EPSILON {
            *value = nv;
            resp.mark_changed();
        }
    }
    let f = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let kx = track.left() + f * track.width();
    let painter = ui.painter();
    match gradient {
        Some(colors) if colors.len() >= 2 => gradient_rect(painter, track, colors, false, 4.0),
        _ => {
            painter.rect_filled(track, 2.0, t.border);
            painter.rect_filled(Rect::from_min_max(track.min, pos2(kx, track.max.y)), 2.0, t.accent);
        }
    }
    knob(painter, pos2(kx, rect.center().y), resp.hovered() || resp.dragged(), &t);
    resp
}

/// Vertical slider for the side rail (top = max), Procreate style.
pub fn vslider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, height: f32, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, mut resp) = ui.allocate_exact_size(vec2(28.0, height), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let track = Rect::from_center_size(rect.center(), vec2(6.0, rect.height() - 16.0));
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        let f = ((track.bottom() - p.y) / track.height()).clamp(0.0, 1.0);
        let nv = lo + f * (hi - lo);
        if (nv - *value).abs() > f32::EPSILON {
            *value = nv;
            resp.mark_changed();
        }
    }
    let f = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let ky = track.bottom() - f * track.height();
    let painter = ui.painter();
    painter.rect_filled(track, 3.0, t.border);
    painter.rect_filled(Rect::from_min_max(pos2(track.left(), ky), track.max), 3.0, t.fg_muted);
    knob(painter, pos2(rect.center().x, ky), resp.hovered() || resp.dragged(), &t);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// `Label ........ value` above a full-width slider.
pub fn slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    fmt: impl Fn(f32) -> String,
) -> Response {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(t.fg_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(fmt(*value)).font(theme::mono(12.0)).color(t.fg));
        });
    });
    let r = slider(ui, value, range, None);
    // The row's label names the slider for screen readers (and UI tests).
    let v = *value as f64;
    r.widget_info(|| egui::WidgetInfo::slider(true, v, label));
    r
}

/// A round 2D pad: drag sets a direction × strength in the unit disc,
/// double-click centres it (happypaint's wet tilt pad). `label` names it.
pub fn tilt_pad(ui: &mut Ui, v: &mut [f32; 2], size: f32, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let c = rect.center();
    let r = size / 2.0;
    if resp.double_clicked() {
        *v = [0.0, 0.0];
        resp.mark_changed();
    } else if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.dragged() || resp.clicked()) {
        let (mut dx, mut dy) = ((p.x - c.x) / r, (p.y - c.y) / r);
        let m = dx.hypot(dy);
        if m > 1.0 {
            (dx, dy) = (dx / m, dy / m);
        }
        if [dx, dy] != *v {
            *v = [dx, dy];
            resp.mark_changed();
        }
    }
    let painter = ui.painter();
    let fill = if resp.hovered() { t.hover } else { t.field };
    painter.circle_filled(c, r - 1.0, fill);
    painter.circle_stroke(c, r - 1.0, Stroke::new(1.0, t.border));
    painter.circle_stroke(c, 2.0, Stroke::new(1.0, t.fg_faint));
    let level = v[0] == 0.0 && v[1] == 0.0;
    let dot = c + vec2(v[0], v[1]) * (r * 0.72);
    painter.circle_filled(dot, 4.0, if level { t.fg_muted } else { t.accent });
    let (x, y) = (v[0], v[1]);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, format!("{label} {x:.2}, {y:.2}")));
    resp
}

/// iOS-style toggle switch with a label.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    // One control for the switch and its label: both click, one accessible name.
    let galley = ui.painter().layout_no_wrap(label.to_string(), egui::TextStyle::Body.resolve(ui.style()), Color32::WHITE);
    let gap = ui.spacing().item_spacing.x;
    let size = vec2(32.0 + gap + galley.size().x, galley.size().y.max(18.0));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let checked = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, checked, label));
    let sw = egui::Rect::from_min_size(pos2(rect.left(), rect.center().y - 9.0), vec2(32.0, 18.0));
    let how_on = ui.ctx().animate_bool(resp.id, *on);
    ui.painter().rect_filled(sw, 9.0, if *on { t.accent } else { t.border });
    let x = egui::lerp((sw.left() + 9.0)..=(sw.right() - 9.0), how_on);
    ui.painter().circle_filled(pos2(x, sw.center().y), 7.0, Color32::WHITE);
    let text_pos = pos2(sw.right() + gap, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, if *on { t.fg } else { t.fg_muted });
    resp
}

/// A small rotation dial: the needle shows `angle` (radians, clockwise
/// from up). Dragging sets the angle to the pointer's; `changed()` reports a
/// drag, `clicked()` a press without one (callers reset on it).
pub fn rotation_dial(ui: &mut Ui, angle: &mut f32, size: f32, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, mut resp) = ui.allocate_exact_size(vec2(size, size), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, ui.is_enabled(), tooltip));
    let c = rect.center();
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
        && p.distance(c) > 1.0
    {
        let a = (p.x - c.x).atan2(-(p.y - c.y));
        if (a - *angle).abs() > f32::EPSILON {
            *angle = a;
            resp.mark_changed();
        }
    }
    let r = size / 2.0 - 1.0;
    let hot = resp.hovered() || resp.dragged();
    let p = ui.painter();
    p.circle_stroke(c, r, egui::Stroke::new(1.25, if hot { t.fg_muted } else { t.border }));
    let tip = c + vec2(angle.sin(), -angle.cos()) * (r - 2.0);
    p.line_segment([c, tip], egui::Stroke::new(1.5, if hot { t.accent } else { t.fg_muted }));
    p.circle_filled(c, 1.5, t.fg_muted);
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}

/// A thin progress track: filled to `fraction`, or an accent segment
/// sweeping across while the amount is unknown (`None`).
pub fn progress_bar(ui: &mut Ui, fraction: Option<f32>, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 4.0), Sense::hover());
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::new(egui::WidgetType::ProgressIndicator);
        info.value = fraction.map(|f| f as f64);
        info
    });
    let r = rect.height() / 2.0;
    ui.painter().rect_filled(rect, r, t.border);
    let fill = match fraction {
        Some(f) => Rect::from_min_size(rect.min, vec2(rect.width() * f.clamp(0.0, 1.0), rect.height())),
        None => {
            let phase = (ui.input(|i| i.time) * 0.8).fract() as f32;
            let w = rect.width() * 0.3;
            let x = rect.left() - w + (rect.width() + w) * phase;
            ui.ctx().request_repaint();
            Rect::from_x_y_ranges(x.max(rect.left())..=(x + w).min(rect.right()), rect.y_range())
        }
    };
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, r, t.accent);
    }
    resp
}

/// Accent-filled primary button.
pub fn primary_button(ui: &mut Ui, label: &str, min_width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    button_impl(ui, label, min_width, t.accent, t.on_accent, true)
}

pub fn secondary_button(ui: &mut Ui, label: &str, min_width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    button_impl(ui, label, min_width, t.field, t.fg, false)
}

fn button_impl(ui: &mut Ui, label: &str, min_width: f32, bg: Color32, fg: Color32, primary: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::medium(13.0), fg);
    let size = vec2((galley.size().x + 28.0).max(min_width), 32.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    let fill = if resp.is_pointer_button_down_on() {
        bg.gamma_multiply(0.85)
    } else if resp.hovered() {
        if primary { bg.gamma_multiply(0.92) } else { t.bg_elevated }
    } else {
        bg
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    if !primary {
        ui.painter().rect_stroke(rect, t.radius, Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), t.radius + 2.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
    }
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// A list/menu row: full width, hover highlight, optional leading icon.
pub fn menu_item(ui: &mut Ui, icon: Option<&str>, label: &str, shortcut: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius_sm, t.hover);
    }
    let mut x = rect.left() + 10.0;
    if let Some(i) = icon {
        crate::icons::paint(ui, Rect::from_center_size(pos2(x + 8.0, rect.center().y), Vec2::splat(16.0)), i, 16.0, t.fg_muted);
        x += 26.0;
    }
    ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), t.fg);
    if !shortcut.is_empty() {
        ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, shortcut, theme::mono(11.0), t.fg_faint);
    }
    resp
}

pub fn to_color32(c: Rgba) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(b(c.r), b(c.g), b(c.b), b(c.a))
}

/// A round colour swatch; clickable.
pub fn swatch(ui: &mut Ui, color: Rgba, size: f32, selected: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let c = rect.center();
    let r = size / 2.0 - 2.0;
    ui.painter().circle_filled(c, r, to_color32(color));
    ui.painter().circle_stroke(c, r, Stroke::new(1.0, Color32::from_white_alpha(40)));
    if selected || resp.hovered() {
        ui.painter().circle_stroke(c, r + 2.0, Stroke::new(2.0, if selected { t.fg } else { t.fg_muted }));
    }
    resp
}

fn gradient_rect(painter: &egui::Painter, r: Rect, colors: &[Color32], vertical: bool, radius: f32) {
    let n = colors.len() - 1;
    let mut mesh = egui::Mesh::default();
    for (i, c) in colors.iter().enumerate() {
        let f = i as f32 / n as f32;
        let (a, b) = if vertical {
            (pos2(r.left(), r.top() + f * r.height()), pos2(r.right(), r.top() + f * r.height()))
        } else {
            (pos2(r.left() + f * r.width(), r.top()), pos2(r.left() + f * r.width(), r.bottom()))
        };
        mesh.colored_vertex(a, *c);
        mesh.colored_vertex(b, *c);
        if i > 0 {
            let k = (i * 2) as u32;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k + 1, k);
        }
    }
    painter.add(mesh);
    let _ = radius;
}

pub fn hue_stops() -> Vec<Color32> {
    (0..=12).map(|i| to_color32(Rgba::from_hsv(i as f32 * 30.0, 1.0, 1.0, 1.0))).collect()
}

/// HSV state kept across frames so hue survives greys.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hsv {
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Hsv {
    pub fn from_rgba(c: Rgba) -> Self {
        let (h, s, v) = c.to_hsv();
        Self { h, s, v }
    }

    pub fn to_rgba(self) -> Rgba {
        Rgba::from_hsv(self.h, self.s, self.v, 1.0)
    }
}

/// Saturation/value square plus a hue strip. Returns true when the colour changed.
pub fn color_picker(ui: &mut Ui, hsv: &mut Hsv, size: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    // SV square: white→hue horizontally, multiplied by a black gradient vertically.
    let steps = 24;
    let mut mesh = egui::Mesh::default();
    for yi in 0..=steps {
        for xi in 0..=steps {
            let (s, v) = (xi as f32 / steps as f32, 1.0 - yi as f32 / steps as f32);
            let c = to_color32(Rgba::from_hsv(hsv.h, s, v, 1.0));
            mesh.colored_vertex(pos2(rect.left() + s * rect.width(), rect.top() + (1.0 - v) * rect.height()), c);
        }
    }
    let row = (steps + 1) as u32;
    for yi in 0..steps as u32 {
        for xi in 0..steps as u32 {
            let i = yi * row + xi;
            mesh.add_triangle(i, i + 1, i + row);
            mesh.add_triangle(i + 1, i + row + 1, i + row);
        }
    }
    ui.painter().add(mesh);
    ui.painter().rect_stroke(rect, t.radius_sm, Stroke::new(1.0, t.border), StrokeKind::Outside);
    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked())
    {
        hsv.s = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        hsv.v = 1.0 - ((p.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let m = pos2(rect.left() + hsv.s * rect.width(), rect.top() + (1.0 - hsv.v) * rect.height());
    ui.painter().circle_stroke(m, 7.0, Stroke::new(2.5, Color32::WHITE));
    ui.painter().circle_stroke(m, 8.5, Stroke::new(1.0, Color32::from_black_alpha(120)));
    ui.add_space(8.0);
    let mut h = hsv.h;
    if slider(ui, &mut h, 0.0..=360.0, Some(&hue_stops())).changed() {
        hsv.h = h.min(359.99);
        changed = true;
    }
    changed
}

/// Curve editor: drag points, click empty space to add, double-click a point to
/// remove it (end points stay at x = 0 and 1). `eval` draws the curve between
/// points so the editor shows exactly what the filter applies. Returns true on change.
pub fn curve_editor(ui: &mut Ui, points: &mut Vec<[f64; 2]>, size: f32, tint: Color32, eval: &dyn Fn(&[[f64; 2]], f64) -> f64) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(rect, t.radius_sm, t.field);
    for k in 1..4 {
        let f = k as f32 / 4.0;
        p.vline(rect.left() + f * rect.width(), rect.y_range(), Stroke::new(1.0, t.border));
        p.hline(rect.x_range(), rect.top() + f * rect.height(), Stroke::new(1.0, t.border));
    }
    p.line_segment([rect.left_bottom(), rect.right_top()], Stroke::new(1.0, t.fg_faint.gamma_multiply(0.5)));
    let to_screen = |q: [f64; 2]| pos2(rect.left() + q[0] as f32 * rect.width(), rect.bottom() - q[1] as f32 * rect.height());
    let to_curve = |s: Pos2| {
        [((s.x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64, ((rect.bottom() - s.y) / rect.height()).clamp(0.0, 1.0) as f64]
    };
    let line: Vec<Pos2> = (0..=64)
        .map(|i| {
            let x = i as f64 / 64.0;
            to_screen([x, eval(points, x).clamp(0.0, 1.0)])
        })
        .collect();
    p.add(egui::Shape::line(line, Stroke::new(2.0, tint)));
    let id = resp.id.with("drag-point");
    let mut changed = false;
    let nearest = |pts: &[[f64; 2]], s: Pos2| {
        pts.iter()
            .enumerate()
            .map(|(i, q)| (i, to_screen(*q).distance(s)))
            .filter(|(_, d)| *d < 10.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    if resp.double_clicked()
        && let Some(s) = resp.interact_pointer_pos()
        && let Some(i) = nearest(points, s)
        && i != 0
        && i != points.len() - 1
    {
        points.remove(i);
        changed = true;
    } else if (resp.drag_started() || resp.clicked())
        && let Some(s) = resp.interact_pointer_pos()
    {
        let i = match nearest(points, s) {
            Some(i) => i,
            None => {
                let q = to_curve(s);
                let i = points.iter().position(|p| p[0] > q[0]).unwrap_or(points.len());
                points.insert(i, q);
                changed = true;
                i
            }
        };
        ui.ctx().data_mut(|d| d.insert_temp(id, i));
    }
    if resp.dragged()
        && let (Some(s), Some(i)) = (resp.interact_pointer_pos(), ui.ctx().data(|d| d.get_temp::<usize>(id)))
        && i < points.len()
    {
        let mut q = to_curve(s);
        let last = points.len() - 1;
        if i == 0 {
            q[0] = 0.0;
        } else if i == last {
            q[0] = 1.0;
        } else {
            q[0] = q[0].clamp(points[i - 1][0] + 0.01, points[i + 1][0] - 0.01);
        }
        points[i] = q;
        changed = true;
    }
    for q in points.iter() {
        let c = to_screen(*q);
        ui.painter().circle_filled(c, 4.5, Color32::WHITE);
        ui.painter().circle_stroke(c, 4.5, Stroke::new(1.0, Color32::from_black_alpha(140)));
    }
    changed
}
