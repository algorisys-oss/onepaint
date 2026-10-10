//! Gallery: recently opened and saved drawings with thumbnails (happypaint's
//! home screen; the desktop keeps files, so it lists paths). Persisted as
//! `recent.json` in the config folder.

use crate::App;
use egui::{ColorImage, Sense, TextureHandle, TextureOptions, pos2, vec2};
use onepaint_ui_kit::{Tokens, theme, widgets};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 24;

#[derive(Default)]
pub struct Gallery {
    pub recent: Vec<PathBuf>,
    store: Option<PathBuf>,
    thumbs: HashMap<PathBuf, Option<TextureHandle>>,
    /// Files here never join the gallery (the autosave folder: a recovered
    /// autosave is deleted once the work is saved).
    skip: Option<PathBuf>,
}

pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint").map(|d| d.config_dir().join("recent.json"))
}

impl Gallery {
    /// The saved list, without files that are gone or that live in `skip`.
    pub fn load(store: Option<PathBuf>, skip: Option<PathBuf>) -> Self {
        let recent = store
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Vec<PathBuf>>(&b).ok())
            .unwrap_or_default();
        let mut g = Self { recent, store, thumbs: HashMap::new(), skip };
        g.prune();
        g
    }

    fn skipped(&self, p: &Path) -> bool {
        self.skip.as_ref().is_some_and(|d| p.starts_with(d) || p.starts_with(d.canonicalize().unwrap_or_else(|_| d.clone())))
    }

    /// Drop entries whose file was deleted or moved (and autosaves).
    pub fn prune(&mut self) {
        let before = self.recent.len();
        let skip = self.skip.clone();
        self.recent.retain(|p| p.exists() && !skip.as_ref().is_some_and(|d| p.starts_with(d)));
        if self.recent.len() != before {
            self.thumbs.retain(|p, _| p.exists());
            self.save();
        }
    }

    fn save(&self) {
        let Some(p) = &self.store else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&self.recent) {
            let _ = std::fs::write(p, json);
        }
    }

    /// Put `p` first (opened or saved just now).
    pub fn touch(&mut self, p: &Path) {
        let p = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
        if self.skipped(&p) {
            return;
        }
        self.recent.retain(|q| q != &p);
        self.recent.insert(0, p.clone());
        self.recent.truncate(MAX_RECENT);
        self.thumbs.remove(&p);
        self.save();
    }

    pub fn remove(&mut self, p: &Path) {
        self.recent.retain(|q| q != p);
        self.save();
    }

    fn thumb(&mut self, ctx: &egui::Context, p: &Path) -> Option<TextureHandle> {
        self.thumbs
            .entry(p.to_path_buf())
            .or_insert_with(|| {
                let bytes = std::fs::read(p).ok()?;
                let is_doc = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("happypaint"));
                let jpeg = if is_doc { onepaint_formats::read_thumbnail(&bytes).ok().flatten() } else { None };
                let img = onepaint_formats::decode_image(jpeg.as_deref().unwrap_or(&bytes)).ok()?;
                let step = (img.width.max(img.height) / 256).max(1);
                let (w, h) = (img.width.div_ceil(step), img.height.div_ceil(step));
                let mut rgba = Vec::with_capacity((w * h * 4) as usize);
                for y in (0..img.height).step_by(step as usize) {
                    for x in (0..img.width).step_by(step as usize) {
                        let i = ((y * img.width + x) * 4) as usize;
                        rgba.extend_from_slice(&img.rgba[i..i + 4]);
                    }
                }
                Some(ctx.load_texture(
                    p.display().to_string(),
                    ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                    TextureOptions::LINEAR,
                ))
            })
            .clone()
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(720.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Gallery").font(theme::semibold(17.0)).color(t.fg));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::primary_button(ui, "New drawing", 120.0).clicked() {
                app.popover = Some(crate::Popover::NewDoc);
            }
            if widgets::secondary_button(ui, "Open…", 90.0).clicked() {
                app.popover = None;
                app.request(crate::Pending::Open, ui.ctx());
            }
        });
    });
    ui.add_space(8.0);
    // A file deleted or moved since it was listed leaves the gallery.
    if ui.ctx().cumulative_frame_nr().is_multiple_of(30) {
        app.gallery.prune();
    }
    let list = app.gallery.recent.clone();
    if list.is_empty() {
        ui.label(egui::RichText::new("Drawings you open or save appear here.").color(t.fg_muted));
        return;
    }
    let mut open = None;
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() - 180.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for p in &list {
                let (rect, resp) = ui.allocate_exact_size(vec2(164.0, 152.0), Sense::click());
                let hover = resp.hovered();
                ui.painter().rect_filled(rect, 8.0, if hover { t.hover } else { t.field });
                let img_rect = egui::Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(148.0, 108.0));
                widgets::checker(ui.painter(), img_rect, 6.0);
                match app.gallery.thumb(ui.ctx(), p) {
                    Some(tex) => {
                        let s = tex.size_vec2();
                        let k = (img_rect.width() / s.x).min(img_rect.height() / s.y);
                        let r = egui::Rect::from_center_size(img_rect.center(), s * k);
                        ui.painter().image(tex.id(), r, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
                    }
                    None => {
                        ui.painter().text(img_rect.center(), egui::Align2::CENTER_CENTER, "missing", theme::medium(11.0), t.fg_faint);
                    }
                }
                let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
                ui.painter().text(
                    pos2(rect.left() + 10.0, rect.bottom() - 18.0),
                    egui::Align2::LEFT_CENTER,
                    name,
                    theme::medium(12.0),
                    t.fg,
                );
                let resp = resp.on_hover_text(p.display().to_string());
                if resp.clicked() {
                    open = Some(p.clone());
                }
                resp.context_menu(|ui| {
                    if ui.button("Remove from gallery").clicked() {
                        remove = Some(p.clone());
                        ui.close();
                    }
                });
            }
        });
    });
    if let Some(p) = remove {
        app.gallery.remove(&p);
    }
    if let Some(p) = open {
        app.popover = None;
        app.open_later = Some(p);
        app.request(crate::Pending::Open, ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_most_recent_first_and_persist() {
        let dir = std::env::temp_dir().join(format!("onepaint-gallery-{}", onepaint_doc::new_id()));
        std::fs::create_dir_all(&dir).unwrap_or_default();
        let store = dir.join("recent.json");
        let (a, b) = (dir.join("a.png"), dir.join("b.png"));
        std::fs::write(&a, b"a").unwrap_or_default();
        std::fs::write(&b, b"b").unwrap_or_default();
        let mut g = Gallery::load(Some(store.clone()), None);
        g.touch(&a);
        g.touch(&b);
        g.touch(&a);
        assert_eq!(g.recent, vec![a.clone(), b.clone()]);
        let back = Gallery::load(Some(store), None);
        assert_eq!(back.recent, vec![a.clone(), b]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn deleted_files_and_autosaves_leave_the_gallery() {
        let dir = std::env::temp_dir().join(format!("onepaint-gallery-{}", onepaint_doc::new_id()));
        let autosaves = dir.join("autosave");
        std::fs::create_dir_all(&autosaves).unwrap_or_default();
        let dir = dir.canonicalize().unwrap_or(dir);
        let autosaves = dir.join("autosave");
        let store = dir.join("recent.json");
        let (kept, gone, auto) = (dir.join("kept.png"), dir.join("gone.png"), autosaves.join("x.happypaint"));
        for f in [&kept, &gone, &auto] {
            std::fs::write(f, b"x").unwrap_or_default();
        }
        let mut g = Gallery::load(Some(store.clone()), Some(autosaves.clone()));
        g.touch(&gone);
        g.touch(&kept);
        g.touch(&auto);
        assert_eq!(g.recent, vec![kept.clone(), gone.clone()], "a recovered autosave is not listed");
        // An old list that still names the autosave (and a file since deleted).
        std::fs::write(&store, serde_json::to_vec(&vec![&auto, &gone, &kept]).unwrap_or_default()).unwrap_or_default();
        let _ = std::fs::remove_file(&gone);
        let g = Gallery::load(Some(store.clone()), Some(autosaves));
        assert_eq!(g.recent, vec![kept.clone()]);
        let saved: Vec<PathBuf> = serde_json::from_slice(&std::fs::read(&store).unwrap_or_default()).unwrap_or_default();
        assert_eq!(saved, vec![kept], "the cleaned list is saved");
        let _ = std::fs::remove_dir_all(dir);
    }
}
