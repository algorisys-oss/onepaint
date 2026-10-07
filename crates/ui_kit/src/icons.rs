// Adapted from photocraft (MIT/Apache-2.0): crates/ui-egui/src/icons.rs
//! Lucide icons (ISC licence, `assets/icons/LICENSE-lucide.txt`), embedded,
//! recoloured to white, rasterised by egui_extras at the exact on-screen pixel
//! size (crisp at any DPI) and tinted per use.

use crate::theme::Tokens;
use egui::{Color32, Rect, Response, Sense, Vec2};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        &[$(($name, include_bytes!(concat!("../assets/icons/", $name, ".svg")))),*]
    };
}

pub static ICONS: &[(&str, &[u8])] = icons![
    "brush",
    "eraser",
    "layers",
    "palette",
    "undo-2",
    "redo-2",
    "plus",
    "minus",
    "trash",
    "copy",
    "eye",
    "eye-off",
    "lock",
    "lock-open",
    "folder-open",
    "file-plus",
    "image",
    "chevron-down",
    "chevron-up",
    "square-dashed",
    "circle-dashed",
    "lasso",
    "pentagon",
    "wand",
    "paint-bucket",
    "move",
    "contrast",
    "sparkles",
    "arrow-left-right",
    "arrow-up-down",
    "rotate-ccw",
    "brush-cleaning",
    "chevron-right",
    "x",
    "sun",
    "moon",
    "pipette",
    "hand",
    "zoom-in",
    "rotate-cw",
    "maximize-2",
    "ellipsis",
    "droplet",
    "blend",
    "folder-plus",
    "check",
    "settings",
    "sliders-horizontal",
];

fn white_icons() -> &'static HashMap<&'static str, Arc<[u8]>> {
    static MAP: OnceLock<HashMap<&'static str, Arc<[u8]>>> = OnceLock::new();
    MAP.get_or_init(|| {
        ICONS
            .iter()
            .map(|(name, bytes)| {
                let svg = String::from_utf8_lossy(bytes)
                    .replace("currentColor", "#ffffff")
                    .replace("stroke-width=\"2\"", "stroke-width=\"1.75\"");
                (*name, Arc::from(svg.into_bytes().into_boxed_slice()))
            })
            .collect()
    })
}

pub fn exists(name: &str) -> bool {
    white_icons().contains_key(name)
}

pub fn image(name: &str, size: f32, tint: Color32) -> egui::Image<'static> {
    let bytes = white_icons().get(name).or_else(|| white_icons().get("x")).cloned().unwrap_or_default();
    egui::Image::from_bytes(format!("bytes://icons/{name}.svg"), egui::load::Bytes::Shared(bytes))
        .fit_to_exact_size(Vec2::splat(size))
        .tint(tint)
}

pub fn paint(ui: &egui::Ui, rect: Rect, name: &str, size: f32, tint: Color32) {
    image(name, size, tint).paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(size)));
}

/// Square icon button: transparent until hovered; `selected` gets the accent treatment.
pub fn button(ui: &mut egui::Ui, name: &str, box_size: f32, selected: bool, tooltip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(box_size), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip));
    let hover = ui.ctx().animate_bool(resp.id.with("h"), resp.hovered() || selected);
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, t.radius, t.accent_soft);
    } else if hover > 0.0 {
        p.rect_filled(rect, t.radius, t.hover.gamma_multiply(hover));
    }
    let tint = if selected {
        t.accent
    } else if !ui.is_enabled() {
        t.fg_faint
    } else if resp.hovered() {
        t.fg
    } else {
        t.fg_muted
    };
    paint(ui, rect, name, (box_size * 0.55).round(), tint);
    if resp.has_focus() {
        p.rect_stroke(rect.expand(1.0), t.radius, egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
    }
    if tooltip.is_empty() { resp } else { resp.on_hover_text(tooltip) }
}
