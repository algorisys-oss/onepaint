//! The Export dialog's preview thumbnail (happypaint
//! `components/export-preview/`, v0.211.12). The drawing is composited once
//! when the dialog opens and downscaled; switching format only re-runs the
//! cheap per-pixel treatment (`onepaint_formats::export_preview`) on the
//! small copy.

use crate::App;
use egui::{ColorImage, Sense, TextureHandle, TextureOptions, Ui, vec2};
use onepaint_formats::export_preview as ep;
use onepaint_ui_kit::{Tokens, theme, widgets};

const MAX_W: f32 = 340.0;
const MAX_H: f32 = 200.0;

/// What the treated texture was made from.
#[derive(Clone, PartialEq)]
struct Key {
    format: onepaint_formats::export::ExportFormat,
    psd: bool,
    cmyk: bool,
    press: Option<usize>,
}

#[derive(Default)]
pub struct Preview {
    /// The pass the dialog last showed: a gap means it was reopened.
    shown: Option<u64>,
    /// The base: straight-alpha sRGB8, fitted.
    base: Option<(u32, u32, Vec<u8>)>,
    tex: Option<(Key, TextureHandle)>,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let pass = ui.ctx().cumulative_pass_nr();
    let reopened = app.export_preview.shown.is_none_or(|p| p + 1 < pass);
    app.export_preview.shown = Some(pass);
    if reopened {
        // Composite on the next frame, so opening never stalls on a big drawing.
        app.export_preview.base = None;
        app.export_preview.tex = None;
        ui.ctx().request_repaint();
    } else if app.export_preview.base.is_none() {
        let d = app.session.doc();
        let (w, h) = (d.width, d.height);
        let ppp = ui.ctx().pixels_per_point().min(2.0);
        let px = app.session.flatten_linear();
        let (mw, mh) = ((MAX_W * ppp) as u32, (MAX_H * ppp) as u32);
        app.export_preview.base = Some(ep::downscale(&px, w, h, mw, mh, onepaint_color::Space::Linear));
    }
    let Some((w, h, base)) = app.export_preview.base.as_ref() else {
        let (rect, _) = ui.allocate_exact_size(vec2(MAX_W, 80.0), Sense::hover());
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Preparing preview…", theme::medium(11.0), t.fg_faint);
        return;
    };
    let o = app.export_opts;
    let press =
        (o.format == onepaint_formats::export::ExportFormat::Tiff && o.cmyk && !app.export_psd).then(|| crate::press::active(&app.press));
    let key = Key { format: o.format, psd: app.export_psd, cmyk: o.cmyk, press: press.map(|p| p as *const _ as usize) };
    let transparent = ep::keeps_transparency(o.format, app.export_psd, o.cmyk);
    if app.export_preview.tex.as_ref().is_none_or(|(k, _)| *k != key) {
        let mut px = base.clone();
        if !transparent {
            ep::flatten_on_white(&mut px);
        }
        if let Some(p) = press {
            ep::proof(p, &mut px);
        }
        let img = ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], &px);
        let tex = ui.ctx().load_texture("export-preview", img, TextureOptions::LINEAR);
        app.export_preview.tex = Some((key, tex));
    }
    let Some((_, tex)) = &app.export_preview.tex else { return };
    let ppp = ui.ctx().pixels_per_point().min(2.0);
    let size = vec2(*w as f32 / ppp, *h as f32 / ppp);
    ui.vertical_centered(|ui| {
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, "Preview of the exported image"));
        if transparent {
            widgets::checker(ui.painter(), rect, 8.0);
        }
        egui::Image::new(tex).paint_at(ui, rect);
        ui.painter().rect_stroke(rect, 0.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
    });
    ui.add_space(6.0);
}
