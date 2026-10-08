//! Reference window (happypaint `components/reference-window`): a floating
//! image with pan and zoom to eyedrop colours from.

use crate::App;
use egui::{Color32, ColorImage, Rect, Sense, TextureHandle, TextureOptions, Vec2, pos2, vec2};
use onepaint_color::Rgba;
use onepaint_ui_kit::{Tokens, icons, theme};

pub struct Reference {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    texture: Option<TextureHandle>,
    /// Screen px per image px; `None` = fit to the window.
    zoom: Option<f32>,
    pan: Vec2,
    pub pick: bool,
}

impl Reference {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Self {
        Self { width, height, rgba, texture: None, zoom: None, pan: Vec2::ZERO, pick: true }
    }

    /// The straight sRGB colour at an image pixel (`None` outside or transparent).
    pub fn color_at(&self, x: i64, y: i64) -> Option<Rgba> {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return None;
        }
        let i = ((y as u64 * self.width as u64 + x as u64) * 4) as usize;
        let p = &self.rgba[i..i + 4];
        (p[3] > 0).then(|| Rgba::new(p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, 1.0))
    }
}

/// Window open state plus the image.
#[derive(Default)]
pub struct ReferenceState {
    pub open: bool,
    pub image: Option<Reference>,
}

pub fn load_dialog(app: &mut App, now: f64) {
    let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"]).pick_file()
    else {
        return;
    };
    match std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| onepaint_formats::decode_image(&b).map_err(|e| e.to_string())) {
        Ok(img) => app.reference.image = Some(Reference::new(img.width, img.height, img.rgba)),
        Err(e) => app.notify(format!("Couldn't open the reference: {e}"), now),
    }
}

/// Use the current canvas as the reference (a snapshot).
pub fn from_canvas(app: &mut App) {
    let d = app.session.doc();
    let (w, h) = (d.width, d.height);
    let rgba = onepaint_formats::flatten_to_srgb8(&app.session.flatten());
    app.reference.image = Some(Reference::new(w, h, rgba));
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.reference.open {
        return;
    }
    let t = Tokens::get(ctx);
    let now = ctx.input(|i| i.time);
    let mut open = true;
    egui::Window::new("Reference").open(&mut open).default_size([320.0, 280.0]).resizable(true).collapsible(true).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if icons::button(ui, "image", 24.0, false, "Load a reference image").clicked() {
                load_dialog(app, now);
            }
            if icons::button(ui, "layers", 24.0, false, "Use a snapshot of the canvas").clicked() {
                from_canvas(app);
            }
            if let Some(r) = app.reference.image.as_mut() {
                if icons::button(ui, "pipette", 24.0, r.pick, "Pick colours").clicked() {
                    r.pick = true;
                }
                if icons::button(ui, "hand", 24.0, !r.pick, "Move the image").clicked() {
                    r.pick = false;
                }
                if icons::button(ui, "maximize-2", 24.0, false, "Fit").clicked() {
                    (r.zoom, r.pan) = (None, Vec2::ZERO);
                }
                if icons::button(ui, "trash", 24.0, false, "Remove the image").clicked() {
                    app.reference.image = None;
                }
            }
        });
        let Some(r) = app.reference.image.as_mut() else {
            ui.label(
                egui::RichText::new("Load an image or snapshot the canvas, then pick colours from it.")
                    .font(theme::medium(11.0))
                    .color(t.fg_faint),
            );
            return;
        };
        let tex = r
            .texture
            .get_or_insert_with(|| {
                let img = ColorImage::from_rgba_unmultiplied([r.width as usize, r.height as usize], &r.rgba);
                ui.ctx().load_texture("reference", img, TextureOptions::LINEAR)
            })
            .clone();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size().max(vec2(120.0, 90.0)), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 4.0, t.stage);
        let fit = (rect.width() / r.width as f32).min(rect.height() / r.height as f32);
        let zoom = r.zoom.unwrap_or(fit);
        let size = vec2(r.width as f32, r.height as f32) * zoom;
        let img_rect = Rect::from_center_size(rect.center() + r.pan, size);
        ui.painter().with_clip_rect(rect).image(tex.id(), img_rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                let f = (scroll * 0.0025).exp();
                let at = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center()) - rect.center();
                r.pan = at + (r.pan - at) * f;
                r.zoom = Some((zoom * f).clamp(0.02, 32.0));
            }
        }
        let image_px = |p: egui::Pos2| ((p.x - img_rect.left()) / zoom, (p.y - img_rect.top()) / zoom);
        if r.pick {
            if (resp.dragged() || resp.clicked())
                && let Some(p) = resp.interact_pointer_pos()
            {
                let (x, y) = image_px(p);
                if let Some(c) = r.color_at(x.floor() as i64, y.floor() as i64) {
                    app.set_color(c);
                }
            }
            if resp.drag_stopped() || resp.clicked() {
                app.push_recent();
            }
        } else if resp.dragged() {
            r.pan += resp.drag_delta();
        }
    });
    app.reference.open = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_opaque_pixels_only() {
        let r = Reference::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 0]);
        assert_eq!(r.color_at(0, 0).unwrap().to_hex(), "#ff0000");
        assert!(r.color_at(1, 0).is_none(), "transparent");
        assert!(r.color_at(2, 0).is_none() && r.color_at(-1, 0).is_none());
    }
}
