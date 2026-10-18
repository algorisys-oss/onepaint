//! The brush studio (spec 27): every brush parameter, a live preview stroke
//! and a try-it pad, in one modal dialog. Edits go straight to the engine
//! (`Command::SetBrushPreset`); Save / Save as new write the user library.

use crate::App;
use egui::{
    Align, Align2, Area, ColorImage, Id, Layout, Order, Rect, RichText, Sense, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2,
};
use onepaint_brush::studio::{self, CurveChannel, Param, SECTIONS};
use onepaint_brush::{BrushPreset, Dab, StrokeEngine, Tip};
use onepaint_engine::Command;
use onepaint_raster::{Rect as PxRect, Surface};
use onepaint_stroke::{CurvePoint, Interp, StrokePoint};
use onepaint_ui_kit::widgets::{self, popover_frame};
use onepaint_ui_kit::{Tokens, icons, theme};
use std::collections::HashMap;

const ASIDE_W: f32 = 340.0;
const PREVIEW_H: usize = 84;
const TIP_THUMB: usize = 34;

/// A stroke in progress on the try-it pad.
struct TryStroke {
    eng: StrokeEngine,
    buf: Surface,
    opacity: f32,
}

#[derive(Default)]
pub struct Studio {
    pad: Surface,
    pad_size: [usize; 2],
    pad_tex: Option<TextureHandle>,
    pad_dirty: bool,
    pad_empty: bool,
    stroke: Option<TryStroke>,
    seed: u32,
    preview_key: Option<(BrushPreset, [f32; 3])>,
    preview_tex: Option<TextureHandle>,
    tips: HashMap<String, TextureHandle>,
    pub search: String,
    /// The name field and the brush id it belongs to.
    name: (String, String),
}

impl Studio {
    /// Total ink on the pad (tests).
    #[cfg(test)]
    pub fn pad_ink(&self) -> f32 {
        let [w, h] = self.pad_size;
        self.pad.read_rect(PxRect::new(0, 0, w as i32, h as i32)).chunks_exact(4).map(|p| p[3]).sum()
    }

    /// Paint a sample scribble on the pad (scripted visual checks).
    pub fn scribble(&mut self, brush: &BrushPreset, color: [f32; 3]) {
        let [w, h] = self.pad_size;
        if w < 64 || h < 64 {
            return;
        }
        let pts: Vec<StrokePoint> =
            studio::preview_path(w as f64, h as f64 * 0.6).into_iter().map(|p| StrokePoint { y: p.y + h as f64 * 0.2, ..p }).collect();
        let mut s = studio::render_stroke(brush, color, &pts, 11);
        studio::scale_alpha(&mut s, brush.opacity as f32);
        studio::composite_over(&mut self.pad, &s);
        self.pad_dirty = true;
        self.pad_empty = false;
    }

    pub fn clear_pad(&mut self) {
        self.pad = Surface::new();
        self.stroke = None;
        self.pad_dirty = true;
        self.pad_empty = true;
    }
}

fn linear(app: &App) -> [f32; 3] {
    let p = app.session.color().to_linear_premul();
    [p[0], p[1], p[2]]
}

/// Premultiplied linear pixels (plus an optional live stroke at `k`) over white paper.
fn over_white(base: &Surface, top: Option<(&Surface, f32)>, w: usize, h: usize) -> ColorImage {
    let r = PxRect::new(0, 0, w as i32, h as i32);
    let a = base.read_rect(r);
    let b = top.map(|(s, k)| (s.read_rect(r), k));
    let mut px = Vec::with_capacity(w * h);
    for i in 0..w * h {
        let mut p = [a[i * 4], a[i * 4 + 1], a[i * 4 + 2], a[i * 4 + 3]];
        if let Some((b, k)) = &b {
            let s = [b[i * 4] * k, b[i * 4 + 1] * k, b[i * 4 + 2] * k, b[i * 4 + 3] * k];
            let q = 1.0 - s[3];
            p = [s[0] + p[0] * q, s[1] + p[1] * q, s[2] + p[2] * q, s[3] + p[3] * q];
        }
        let q = |c: f32| (onepaint_color::linear_to_srgb((c + 1.0 - p[3]).clamp(0.0, 1.0)) * 255.0).round() as u8;
        px.push(egui::Color32::from_rgb(q(p[0]), q(p[1]), q(p[2])));
    }
    ColorImage::new([w, h], px)
}

fn tip_thumb(ctx: &egui::Context, cache: &mut HashMap<String, TextureHandle>, id: &str) -> TextureHandle {
    if let Some(t) = cache.get(id) {
        return t.clone();
    }
    let n = TIP_THUMB;
    let mut s = Surface::new();
    let c = n as f64 / 2.0;
    let d =
        Dab { x: c, y: c, radius: c - 3.0, alpha: 1.0, rotation: 0.0, aspect: 1.0, hardness: 0.8, color: [1.0; 3], grain_off: [0.0; 2] };
    let tip: Tip = onepaint_brush::tip_for(id);
    onepaint_brush::rasterize_dab_tip(&mut s, &d, &tip, None, false, None);
    let a = s.read_rect(PxRect::new(0, 0, n as i32, n as i32));
    let px = (0..n * n).map(|i| egui::Color32::from_white_alpha((a[i * 4 + 3].clamp(0.0, 1.0) * 255.0) as u8)).collect();
    let tex = ctx.load_texture(format!("studio-tip-{id}"), ColorImage::new([n, n], px), TextureOptions::LINEAR);
    cache.insert(id.to_string(), tex.clone());
    tex
}

/// A pill chip (category-chip look); accessible by its label.
fn chip(ui: &mut Ui, label: &str, sel: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let text = RichText::new(label).font(theme::medium(11.0)).color(if sel { t.on_accent } else { t.fg_muted });
    ui.add(egui::Button::new(text).fill(if sel { t.accent } else { t.field }).corner_radius(10.0))
}

/// `slider_row` with an accessible slider (role Slider, named by `label`).
fn slider(ui: &mut Ui, p: Param, b: &mut BrushPreset) -> bool {
    let i = p.info();
    let label = p.label_for(b);
    let mut v = p.get(b) as f32;
    let resp = widgets::slider_row(ui, label, &mut v, i.min as f32..=i.max as f32, |v| p.format(v as f64));
    resp.widget_info(|| egui::WidgetInfo::slider(true, v as f64, label));
    if resp.changed() {
        p.set(b, v as f64);
        return true;
    }
    false
}

/// Opens as a modal over a dimmed backdrop; returns the dialog's rect.
pub fn show(app: &mut App, ctx: &egui::Context, screen: Rect) -> Rect {
    let t = Tokens::get(ctx);
    Area::new(Id::new("brush-studio-backdrop")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        ui.painter().rect_filled(r, 0.0, t.stage.gamma_multiply(0.7));
    });
    let size = vec2((screen.width() * 0.94).min(1100.0), (screen.height() * 0.94).min(820.0)) - Vec2::splat(24.0);
    let area = Area::new(Id::new("brush-studio")).order(Order::Foreground).pivot(Align2::CENTER_CENTER).fixed_pos(screen.center()).show(
        ctx,
        |ui| {
            popover_frame(ui)
                .show(ui, |ui| {
                    ui.set_width(size.x);
                    ui.set_height(size.y);
                    header(app, ui);
                    ui.add_space(8.0);
                    let body = vec2(size.x, ui.available_height());
                    if size.x < 720.0 {
                        // Narrow: the pad on top, the controls below (global.css @720px).
                        ui.vertical(|ui| {
                            try_pad(app, ui, vec2(body.x, 200.0));
                            ui.add_space(8.0);
                            aside(app, ui);
                        });
                    } else {
                        ui.horizontal_top(|ui| {
                            try_pad(app, ui, vec2(body.x - ASIDE_W - 12.0, body.y));
                            ui.add_space(12.0);
                            ui.allocate_ui_with_layout(vec2(ASIDE_W, body.y), Layout::top_down(Align::Min), |ui| aside(app, ui));
                        });
                    }
                })
                .response
                .rect
        },
    );
    ctx.move_to_top(area.response.layer_id);
    area.inner
}

fn header(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let b = app.session.current_brush().clone();
    let stored = onepaint_brush::preset(&b.id);
    let user = onepaint_brush::is_user_preset(&b.id);
    let dirty = stored.as_ref() != Some(&b);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Brush studio").font(theme::semibold(16.0)).color(t.fg));
        widgets::vline(ui, 20.0);
        if user {
            if app.studio.name.1 != b.id {
                app.studio.name = (b.name.clone(), b.id.clone());
            }
            let r = ui.add(egui::TextEdit::singleline(&mut app.studio.name.0).desired_width(200.0).hint_text("Brush name"));
            if r.lost_focus() && app.studio.name.0.trim() != b.name {
                rename(app, &b, ui.input(|i| i.time));
            }
        } else {
            ui.label(RichText::new(&b.name).font(theme::medium(13.0)).color(t.fg));
        }
        ui.label(RichText::new(&b.category).font(theme::medium(11.0)).color(t.fg_faint));
        if dirty {
            ui.label(RichText::new(" Edited ").font(theme::medium(10.5)).color(t.accent).background_color(t.accent_soft));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icons::button(ui, "x", 28.0, false, "Close").clicked() {
                app.popover = None;
            }
        });
    });
}

fn rename(app: &mut App, b: &BrushPreset, now: f64) {
    let name = app.studio.name.0.clone();
    match app.brush_library.rename(&b.id, &name) {
        Ok(true) => {
            // Keep any unsaved edits; only the name changes.
            if let Some(stored) = onepaint_brush::preset(&b.id) {
                let mut cur = b.clone();
                cur.name = stored.name.clone();
                app.studio.name.0 = stored.name;
                let _ = app.session.execute(Command::SetBrushPreset { preset: Box::new(cur) });
            }
        }
        Ok(false) => {}
        Err(e) => app.notify(format!("Couldn't save the brush library: {e}"), now),
    }
}

fn try_pad(app: &mut App, ui: &mut Ui, size: Vec2) {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Try-it pad"));
    let (w, h) = ((rect.width().max(1.0)) as usize, (rect.height().max(1.0)) as usize);
    let color = linear(app);
    let now = ui.input(|i| i.time * 1000.0);
    let st = &mut app.studio;
    if st.pad_size != [w, h] {
        st.pad_size = [w, h];
        st.clear_pad();
    }
    let local = |p: egui::Pos2| StrokePoint::at((p.x - rect.left()) as f64, (p.y - rect.top()) as f64, 0.5, now);
    let primary = ui.input(|i| i.pointer.primary_down() || i.pointer.primary_released());
    if resp.drag_started()
        && primary
        && let Some(p) = resp.interact_pointer_pos()
    {
        let brush = app.session.current_brush().clone();
        st.seed = st.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let opacity = brush.opacity as f32;
        let mut eng = StrokeEngine::new(brush, color, st.seed);
        let mut buf = Surface::new();
        for d in eng.begin(local(p)) {
            eng.rasterize(&mut buf, &d, None);
        }
        st.stroke = Some(TryStroke { eng, buf, opacity });
        st.pad_dirty = true;
        st.pad_empty = false;
    }
    if let Some(s) = st.stroke.as_mut() {
        let mut dabs = Vec::new();
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
        {
            dabs.extend(s.eng.add(local(p)));
        }
        dabs.extend(s.eng.airbrush_tick(now));
        if resp.drag_stopped() || !resp.dragged() {
            dabs.extend(s.eng.end());
        }
        for d in &dabs {
            s.eng.rasterize(&mut s.buf, d, None);
        }
        st.pad_dirty |= !dabs.is_empty();
        if resp.drag_stopped() || !resp.dragged() {
            if let Some(mut s) = st.stroke.take() {
                studio::scale_alpha(&mut s.buf, s.opacity);
                studio::composite_over(&mut st.pad, &s.buf);
            }
            st.pad_dirty = true;
        } else {
            ui.ctx().request_repaint();
        }
    }
    if st.pad_dirty || st.pad_tex.is_none() {
        let img = over_white(&st.pad, st.stroke.as_ref().map(|s| (&s.buf, s.opacity)), w, h);
        match &mut st.pad_tex {
            Some(tex) => tex.set(img, TextureOptions::LINEAR),
            None => st.pad_tex = Some(ui.ctx().load_texture("studio-pad", img, TextureOptions::LINEAR)),
        }
        st.pad_dirty = false;
    }
    if let Some(tex) = &st.pad_tex {
        egui::Image::new(tex).corner_radius(t.radius).paint_at(ui, rect);
    }
    ui.painter().rect_stroke(rect, t.radius, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    if st.pad_empty {
        let hint = Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 28.0), vec2(200.0, 26.0));
        ui.painter().rect_filled(hint, 13.0, t.panel.gamma_multiply(0.85));
        ui.painter().text(hint.center(), Align2::CENTER_CENTER, "Draw here to try the brush", theme::medium(12.0), t.fg_muted);
    }
    let clear = Rect::from_min_size(pos2(rect.right() - 82.0, rect.top() + 10.0), vec2(72.0, 32.0));
    if ui.put(clear, |ui: &mut Ui| widgets::secondary_button(ui, "Clear", 72.0)).clicked() {
        app.studio.clear_pad();
    }
}

fn preview(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let b = app.session.current_brush().clone();
    let key = (b, linear(app));
    let st = &mut app.studio;
    let (w, h) = (ui.available_width().floor().max(64.0) as usize, PREVIEW_H);
    let stale = st.preview_tex.as_ref().is_none_or(|t| t.size() != [w, h]);
    if st.preview_key.as_ref() != Some(&key) || stale {
        let s = studio::render_preview(&key.0, key.1, w, h);
        let img = over_white(&s, None, w, h);
        match &mut st.preview_tex {
            Some(tex) => tex.set(img, TextureOptions::LINEAR),
            None => st.preview_tex = Some(ui.ctx().load_texture("studio-preview", img, TextureOptions::LINEAR)),
        }
        st.preview_key = Some(key);
    }
    let (rect, _) = ui.allocate_exact_size(vec2(w as f32, h as f32), Sense::hover());
    if let Some(tex) = &st.preview_tex {
        egui::Image::new(tex).corner_radius(t.radius_sm).paint_at(ui, rect);
    }
    ui.painter().rect_stroke(rect, t.radius_sm, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
}

fn actions(app: &mut App, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let b = app.session.current_brush().clone();
    let stored = onepaint_brush::preset(&b.id);
    let dirty = stored.as_ref() != Some(&b);
    let user = onepaint_brush::is_user_preset(&b.id);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if ui.add_enabled_ui(dirty, |ui| widgets::secondary_button(ui, "Reset", 70.0)).inner.clicked() {
            let _ = app.session.execute(Command::SetBrush { preset_id: b.id.clone() });
        }
        if user && ui.add_enabled_ui(dirty, |ui| widgets::primary_button(ui, "Save", 70.0)).inner.clicked() {
            match app.brush_library.update(&b) {
                Ok(_) => {
                    app.thumbs.forget_brush(&b.id);
                    app.notify(format!("Saved {}", b.name), now);
                }
                Err(e) => app.notify(format!("Couldn't save the brush library: {e}"), now),
            }
        }
        if widgets::secondary_button(ui, "Save as new", 100.0).clicked() {
            match app.brush_library.save_custom(&b, &format!("{} copy", b.name)) {
                Ok(p) => {
                    app.notify(format!("Saved {}", p.name), now);
                    app.brush_category = p.category.clone();
                    let _ = app.session.execute(Command::SetBrush { preset_id: p.id });
                }
                Err(e) => app.notify(format!("Couldn't save the brush library: {e}"), now),
            }
        }
    });
}

/// Labels a section answers to in the search box, besides its sliders.
fn extra_labels(section: &str) -> &'static [&'static str] {
    match section {
        "General" => &["Blend", "Tip", "Wash"],
        "Stroke" => &["Stabilizer"],
        "Pressure" => &["Size ← Pressure", "Flow ← Pressure", "Hardness ← Pressure", "curve"],
        "Grain" => &["Random", "texture", "paper"],
        "Wet" => &["Blender", "Oil", "Pigment", "mixer"],
        "Fluid" => &["live", "watercolor", "watercolour"],
        "Dual brush" => &["mask", "second tip", "texture"],
        "Colour tip" => &["Tip colour", "rgba", "daub"],
        "Jitter" => &["Tip col", "two-colour", "fade"],
        _ => &[],
    }
}

fn matches(section: &str, params: &[Param], q: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    let has = |s: &str| s.to_lowercase().contains(q);
    has(section) || params.iter().any(|p| has(p.info().label)) || extra_labels(section).iter().any(|s| has(s))
}

fn aside(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    preview(app, ui);
    ui.add_space(8.0);
    actions(app, ui);
    ui.add_space(8.0);
    ui.add(egui::TextEdit::singleline(&mut app.studio.search).desired_width(f32::INFINITY).hint_text("Search settings…"));
    ui.add_space(6.0);
    let q = app.studio.search.trim().to_lowercase();
    let mut b = app.session.current_brush().clone();
    let before = b.clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(ui.available_width() - 8.0);
        let mut any = false;
        for (name, params) in SECTIONS {
            if !matches(name, params, &q) {
                continue;
            }
            any = true;
            egui::Frame::new().fill(t.bg_elevated).corner_radius(t.radius).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                widgets::section_label(ui, name);
                ui.add_space(2.0);
                section(app, ui, name, params, &mut b);
            });
            ui.add_space(8.0);
        }
        if !any {
            ui.label(RichText::new(format!("No settings match \"{}\".", app.studio.search.trim())).color(t.fg_muted));
        }
    });
    if b != before {
        let _ = app.session.execute(Command::SetBrushPreset { preset: Box::new(b) });
    }
}

fn section(app: &mut App, ui: &mut Ui, name: &str, params: &[Param], b: &mut BrushPreset) {
    let t = Tokens::get(ui.ctx());
    match name {
        "General" => {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Blend").color(t.fg_muted));
                let mut mode = b.blend_mode();
                egui::ComboBox::from_id_salt("studio-blend").selected_text(mode.label()).width(160.0).show_ui(ui, |ui| {
                    for m in onepaint_doc::BlendMode::BRUSH_MODES {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
                if mode != b.blend_mode() {
                    studio::set_blend_mode(b, mode);
                }
            });
            ui.label(RichText::new("Tip").color(t.fg_muted));
            let mut ids: Vec<String> = onepaint_brush::tip_ids().into_iter().map(String::from).collect();
            ids.extend(onepaint_brush::user_library().tips.keys().cloned());
            // About three rows; the rest scrolls.
            egui::ScrollArea::vertical().id_salt("studio-tips").max_height(3.0 * (TIP_THUMB as f32 + 8.0)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                    for id in ids {
                        let sel = b.tip_id == id;
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(TIP_THUMB as f32 + 4.0), Sense::click());
                        let label = id.rsplit(':').next().unwrap_or(&id).to_string();
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, sel, &label));
                        let fill = if sel {
                            t.accent_soft
                        } else if resp.hovered() {
                            t.hover
                        } else {
                            t.field
                        };
                        ui.painter().rect_filled(r, t.radius_sm, fill);
                        if sel {
                            ui.painter().rect_stroke(r, t.radius_sm, egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                        }
                        let tex = tip_thumb(ui.ctx(), &mut app.studio.tips, &id);
                        egui::Image::new(&tex).tint(if sel { t.accent } else { t.fg }).paint_at(ui, r.shrink(2.0));
                        if resp.on_hover_text(&label).clicked() {
                            b.tip_id = id.clone();
                        }
                    }
                });
            });
            ui.add_space(4.0);
        }
        "Stroke" => {}
        "Wet" => {
            let mut on = studio::is_blender(b);
            if widgets::toggle(ui, &mut on, "Blender").on_hover_text("Blend instead of painting: the brush keeps its tip, takes its colour from the canvas and softens the impasto it crosses").changed() {
                studio::set_blender(b, on);
            }
        }
        "Dual brush" => {
            // A second image tip carves every dab (happypaint's mask tip).
            let cur = b.extra.get("maskTip").and_then(|m| m.get("id")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut ids: Vec<String> =
                onepaint_brush::tip_ids().into_iter().filter(|id| !["round", "soft", "square"].contains(id)).map(String::from).collect();
            ids.extend(onepaint_brush::user_library().tips.keys().cloned());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                if chip(ui, "None", cur.is_empty()).clicked() {
                    studio::set_mask_tip(b, None);
                }
                for id in ids {
                    let label = id.rsplit(':').next().unwrap_or(&id).to_string();
                    if chip(ui, &label, cur == id).clicked() {
                        studio::set_mask_tip(b, Some(&id));
                    }
                }
            });
        }
        "Colour tip" if !Param::TipHue.visible(b) => {
            ui.label(RichText::new("Colour tips only (Oil daub, Impasto daub).").color(t.fg_faint));
        }
        "Colour tip" => {
            let mut on = b.use_tip_color();
            if widgets::toggle(ui, &mut on, "Tip colour")
                .on_hover_text("Paint the tip's own colours instead of tinting your colour")
                .changed()
            {
                studio::set_use_tip_color(b, on);
            }
        }
        "Fluid" if !Param::FluidWater.visible(b) => {
            ui.label(RichText::new("Live watercolour and oil brushes only.").color(t.fg_faint));
        }
        "Grain" => {
            let mut grains: Vec<(String, String)> = vec![("".into(), "None".into())];
            grains.extend(onepaint_brush::grain_names().into_iter().map(|(id, n)| (id.to_string(), n.to_string())));
            grains.extend(onepaint_brush::user_library().grains.into_iter().map(|(id, g)| (id, g.name)));
            let cur = b.grain.as_ref().map_or(String::new(), |g| g.id.clone());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (id, n) in grains {
                    if chip(ui, &n, cur == id).clicked() {
                        studio::set_grain(b, (!id.is_empty()).then_some(id.as_str()));
                    }
                }
            });
            ui.add_space(4.0);
        }
        _ => {}
    }
    for &p in params {
        if p == Param::StabilizerAmount {
            let mut on = b.stabilizer.as_ref().is_some_and(|s| s.enabled);
            if widgets::toggle(ui, &mut on, "Stabilizer").changed() {
                studio::set_stabilizer(b, on);
            }
        }
        if p.visible(b) {
            slider(ui, p, b);
        }
    }
    match name {
        "General" => {
            let mut wash = b.wash;
            if widgets::toggle(ui, &mut wash, "Wash").changed() {
                b.wash = wash;
            }
        }
        "Pressure" => {
            for ch in CurveChannel::ALL {
                curve(ui, ch, b);
            }
        }
        "Jitter" => {
            // Two-colour load: the paint fades from your colour to this one.
            let to = b.dynamics.as_ref().and_then(|d| d.color.as_ref()).and_then(|c| c.to_color);
            ui.horizontal(|ui| {
                let mut on = to.is_some();
                if widgets::toggle(ui, &mut on, "Tip col")
                    .on_hover_text("The paint fades toward a second colour along the stroke")
                    .changed()
                {
                    studio::set_to_color(b, on.then_some([1.0, 42.0 / 255.0, 0.0]));
                }
                if let Some(c) = to {
                    let rgba = onepaint_color::Rgba::new(c[0] as f32, c[1] as f32, c[2] as f32, 1.0);
                    if widgets::swatch(ui, rgba, 22.0, false).on_hover_text("Use the current paint colour").clicked() {
                        let p = app.session.color();
                        studio::set_to_color(b, Some([p.r as f64, p.g as f64, p.b as f64]));
                    }
                }
            });
        }
        "Grain" if b.grain.is_some() => {
            let mut rnd = b.grain.as_ref().and_then(|g| g.random).unwrap_or(0.0) > 0.0;
            if widgets::toggle(ui, &mut rnd, "Random per stroke").changed()
                && let Some(g) = &mut b.grain
            {
                g.random = rnd.then_some(1.0);
            }
        }
        _ => {}
    }
}

fn curve(ui: &mut Ui, ch: CurveChannel, b: &mut BrushPreset) {
    let t = Tokens::get(ui.ctx());
    let (pts, mut interp) = ch.curve(b);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(ch.label()).color(t.fg_muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (m, l) in [(Interp::Linear, "Linear"), (Interp::Smooth, "Smooth")] {
                if chip(ui, l, interp == m).clicked() && interp != m {
                    interp = m;
                    ch.set_curve(b, pts.clone(), m);
                }
            }
        });
    });
    let mut raw: Vec<[f64; 2]> = pts.iter().map(|p| [p.x, p.y]).collect();
    let eval = move |p: &[[f64; 2]], x: f64| {
        let c: Vec<CurvePoint> = p.iter().map(|q| CurvePoint::new(q[0], q[1])).collect();
        onepaint_stroke::eval_curve(&c, x, interp)
    };
    if widgets::curve_editor(ui, &mut raw, 150.0, t.accent, &eval) {
        ch.set_curve(b, raw.into_iter().map(|q| CurvePoint::new(q[0], q[1])).collect(), interp);
    }
}

#[cfg(test)]
mod tests {
    use crate::{App, Popover};
    use egui::accesskit::Role;
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn harness() -> Harness<'static, App> {
        let mut h = Harness::builder().with_size(vec2(1280.0, 860.0)).with_max_steps(400).build_eframe(|cc| App::new(cc, None));
        h.run();
        h
    }

    #[test]
    fn studio_opens_from_the_popover_and_a_slider_edits_the_brush() {
        let mut h = harness();
        h.state_mut().popover = Some(Popover::Brushes);
        h.run();
        h.get_by_label("Brush studio…").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::BrushStudio));
        assert!(h.query_by_label("Try-it pad").is_some());
        let before = h.state().session.current_brush().clone();
        assert!(before.spacing < 0.4, "hard round starts tight: {}", before.spacing);
        // Search brings the Stroke section to the top; clicking the middle of
        // the Spacing slider sets it to about half its range.
        h.state_mut().studio.search = "spacing".into();
        h.run();
        h.get_by_role_and_label(Role::Slider, "Spacing").click();
        h.run();
        let after = h.state().session.current_brush().clone();
        assert_eq!(after.id, before.id);
        assert!((after.spacing - 0.51).abs() < 0.05, "spacing {}", after.spacing);
        assert!(h.query_by_label("Reset").is_some());
        // Reset brings back the stored preset.
        h.get_by_label("Reset").click();
        h.run();
        assert_eq!(h.state().session.current_brush(), &before);
        // Escape closes the studio.
        h.state_mut().popover = None;
        h.run();
    }

    #[test]
    fn save_as_new_makes_a_custom_brush_and_tip_picks_change_the_tip() {
        let mut h = harness();
        h.state_mut().popover = Some(Popover::BrushStudio);
        h.run();
        h.state_mut().studio.search = "hardness".into();
        h.run();
        h.get_by_role_and_label(Role::Slider, "Hardness").click();
        h.run();
        assert!((h.state().session.current_brush().hardness - 0.5).abs() < 0.05);
        // Only the General section's tip grid (the dual-brush chips share tip names).
        h.state_mut().studio.search = "blend".into();
        h.run();
        h.get_by_label("chalk").click();
        h.run();
        assert_eq!(h.state().session.current_brush().tip_id, "chalk");
        let edited = h.state().session.current_brush().clone();
        h.get_by_label("Save as new").click();
        h.run();
        let cur = h.state().session.current_brush().clone();
        assert!(cur.id.starts_with(onepaint_brush::CUSTOM_ID_PREFIX), "{}", cur.id);
        assert_eq!(cur.name, format!("{} copy", edited.name));
        assert_eq!(cur.category, onepaint_brush::CUSTOM_CATEGORY);
        assert_eq!((cur.tip_id.as_str(), cur.hardness), ("chalk", edited.hardness));
        assert_eq!(onepaint_brush::preset(&cur.id).as_ref(), Some(&cur), "stored in the user library");
        // A custom brush can be saved over.
        h.get_by_role_and_label(Role::Slider, "Size").click();
        h.run();
        let resized = h.state().session.current_brush().clone();
        assert_ne!(resized.size, cur.size);
        h.get_by_label("Save").click();
        h.run();
        assert_eq!(onepaint_brush::preset(&cur.id).map(|p| p.size), Some(resized.size));
        assert!(h.state().brush_library.remove(&cur.id).unwrap());
    }

    #[test]
    fn the_try_pad_takes_strokes_without_painting_the_canvas() {
        use egui::{Event, PointerButton};
        let mut h = harness();
        h.state_mut().popover = Some(Popover::BrushStudio);
        h.run();
        let r = h.get_by_label("Try-it pad").rect();
        let (from, to) = (r.center() - vec2(120.0, 40.0), r.center() + vec2(120.0, 40.0));
        let press = |h: &mut Harness<'_, App>, p, pressed| {
            h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        };
        h.event(Event::PointerMoved(from));
        h.step();
        press(&mut h, from, true);
        for i in 1..=12 {
            h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 12.0)));
            h.step();
        }
        press(&mut h, to, false);
        h.run();
        assert!(h.state().studio.pad_ink() > 50.0, "ink {}", h.state().studio.pad_ink());
        assert_eq!(h.state().session.history_len(), 0, "the canvas under the studio is untouched");
        h.get_by_label("Clear").click();
        h.run();
        assert_eq!(h.state().studio.pad_ink(), 0.0);
    }

    #[test]
    fn search_hides_sections() {
        let mut h = harness();
        h.state_mut().popover = Some(Popover::BrushStudio);
        h.state_mut().studio.search = "scatter".into();
        h.run();
        assert!(h.query_by_role_and_label(Role::Slider, "Scatter").is_some());
        assert!(h.query_by_role_and_label(Role::Slider, "Spacing").is_none());
        h.state_mut().studio.search = "zzz".into();
        h.run();
        assert!(h.query_by_label("No settings match \"zzz\".").is_some());
    }
}
