//! Compact controls for a one-line bar (the options row, spec 65): a
//! labelled slider with its value, a pressed/unpressed chip, and a
//! dropdown. Each names itself for screen readers and UI tests.

use crate::theme::{self, Tokens};
use egui::{Align2, Area, Id, Order, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// `Label ──○── 42`: a slider `width` wide between its label and value.
pub fn inline_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    width: f32,
    fmt: impl Fn(f32) -> String,
) -> Response {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(label).font(theme::medium(12.0)).color(t.fg_muted));
    let r = ui.scope(|ui| {
        ui.set_width(width);
        crate::widgets::slider(ui, value, range, None)
    });
    let r = r.inner;
    let v = *value as f64;
    r.widget_info(|| egui::WidgetInfo::slider(true, v, label));
    ui.label(egui::RichText::new(fmt(*value)).font(theme::mono(11.5)).color(t.fg));
    r
}

/// `Label [42 px]`: a number typed or dragged, kept in `range`, shown with
/// at most `decimals` places. Its change fires while typing.
pub fn inline_number(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
    decimals: usize,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let l = ui.label(egui::RichText::new(label).font(theme::medium(12.0)).color(t.fg_muted));
    ui.add(egui::DragValue::new(value).range(range).suffix(suffix).max_decimals(decimals).speed(1.0)).labelled_by(l.id)
}

/// A chip that reads pressed when `on`; the caller flips it on click.
pub fn chip(ui: &mut Ui, label: &str, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::medium(12.0), t.fg);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, label));
    let (fill, fg) = if on {
        (t.accent_soft, t.accent)
    } else if resp.hovered() {
        (t.hover, t.fg)
    } else {
        (egui::Color32::TRANSPARENT, t.fg_muted)
    };
    ui.painter().rect(rect, t.radius_sm, fill, Stroke::new(1.0, if on { t.accent } else { t.border }), StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, fg);
    resp
}

/// A chip that opens something (a popover): its label and a chevron.
pub fn menu_chip(ui: &mut Ui, label: &str, open: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::medium(12.0), t.fg);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 30.0, 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if open {
        t.accent_soft
    } else if resp.hovered() {
        t.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect(rect, t.radius_sm, fill, Stroke::new(1.0, if open { t.accent } else { t.border }), StrokeKind::Inside);
    ui.painter().galley(pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0), galley, if open { t.accent } else { t.fg });
    crate::icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)),
        "chevron-down",
        12.0,
        t.fg_muted,
    );
    resp
}

/// A dropdown showing `options[selected]`; returns the option picked.
/// `label` names it (and shows before it when `show_label`).
pub fn dropdown(ui: &mut Ui, label: &str, options: &[&str], selected: usize, show_label: bool) -> Option<usize> {
    let shown = options.get(selected).copied().unwrap_or("");
    dropdown_shown(ui, label, shown, options, Some(selected), show_label)
}

/// [`dropdown`] showing `shown` (a readout, like a zoom menu's percent).
pub fn dropdown_shown(ui: &mut Ui, label: &str, shown: &str, options: &[&str], selected: Option<usize>, show_label: bool) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    if show_label {
        ui.label(egui::RichText::new(label).font(theme::medium(12.0)).color(t.fg_muted));
    }
    let current = shown;
    let widest = options
        .iter()
        .chain(std::iter::once(&shown))
        .map(|o| ui.painter().layout_no_wrap(o.to_string(), theme::medium(12.0), t.fg).size().x)
        .fold(0.0, f32::max);
    let (rect, resp) = ui.allocate_exact_size(vec2(widest + 34.0, 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, label));
    let id = resp.id.with("open");
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    if resp.clicked() {
        open = !open;
    }
    ui.painter().rect(rect, t.radius_sm, if resp.hovered() { t.hover } else { t.field }, Stroke::new(1.0, t.border), StrokeKind::Inside);
    ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, current, theme::medium(12.0), t.fg);
    crate::icons::paint(
        ui,
        Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(12.0, 12.0)),
        "chevron-down",
        12.0,
        t.fg_muted,
    );
    let mut picked = None;
    if open {
        let list = Area::new(Id::new(("dropdown", id))).order(Order::Foreground).fixed_pos(rect.left_bottom() + vec2(0.0, 4.0)).show(
            ui.ctx(),
            |ui| {
                crate::widgets::popover_frame(ui).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, o) in options.iter().enumerate() {
                        let (r, row) = ui.allocate_exact_size(vec2(rect.width().max(widest + 24.0), 26.0), Sense::click());
                        row.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, Some(i) == selected, *o));
                        if row.hovered() || Some(i) == selected {
                            ui.painter().rect_filled(r, t.radius_sm, if row.hovered() { t.hover } else { t.accent_soft });
                        }
                        ui.painter().text(pos2(r.left() + 8.0, r.center().y), Align2::LEFT_CENTER, *o, theme::medium(12.0), t.fg);
                        if row.clicked() {
                            picked = Some(i);
                        }
                    }
                });
            },
        );
        // A click anywhere else closes it.
        let press = ui.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        if picked.is_some() || press.is_some_and(|p| !list.response.rect.contains(p) && !rect.contains(p)) {
            open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, open));
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeKind;

    /// One frame of `inline_number` on `v` in `kind`: the label's colour and the value after.
    fn number(kind: ThemeKind, v: f64) -> (egui::Color32, Tokens, f64) {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        theme::apply(&ctx, kind);
        let mut v = v;
        let mut full = ctx.run_ui(egui::RawInput::default(), |ui| {
            inline_number(ui, "Feather", &mut v, 0.0..=1000.0, " px", 0);
        });
        full.textures_delta.clear();
        let mut label = None;
        for c in full.shapes.iter().filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Feather" => {
                let laid = t.galley.job.sections[0].format.color;
                Some(if laid == egui::Color32::PLACEHOLDER { t.fallback_color } else { laid })
            }
            _ => None,
        }) {
            label = Some(c);
        }
        (label.unwrap_or(egui::Color32::TRANSPARENT), Tokens::get(&ctx), v)
    }

    #[test]
    fn a_number_field_follows_the_theme_and_keeps_its_value() {
        for kind in [ThemeKind::Dark, ThemeKind::Light] {
            let (c, t, v) = number(kind, 12.0);
            assert_eq!(c, t.fg_muted, "{kind:?}: label");
            assert_eq!(v, 12.0);
        }
    }
}
