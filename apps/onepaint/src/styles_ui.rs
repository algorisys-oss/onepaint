//! The Layer Style editor (spec 56; happypaint `layer-style-dialog.tsx`):
//! in Properties for a pixel, text or path layer or a group. Fill opacity,
//! the built-in styles, the effect list (enable, remove, edit one at a
//! time) and adding effects. Every edit is one `SetLayerProps`.

use crate::App;
use crate::adjust::slider;
use egui::{RichText, Ui};
use onepaint_doc::BlendMode;
use onepaint_doc::styles::{Contour, Effect, EffectKind, LayerStyles, MAX_INSTANCES, builtin_presets, default_effect};
use onepaint_engine::{Command, LayerProps};
use onepaint_ui_kit::{Tokens, widgets};

/// The modes an effect blends in (happypaint's 18).
const EFFECT_MODES: [BlendMode; 18] = [
    BlendMode::Normal,
    BlendMode::Multiply,
    BlendMode::Screen,
    BlendMode::Overlay,
    BlendMode::SoftLight,
    BlendMode::HardLight,
    BlendMode::Darken,
    BlendMode::Lighten,
    BlendMode::ColorDodge,
    BlendMode::ColorBurn,
    BlendMode::LinearBurn,
    BlendMode::Add,
    BlendMode::Difference,
    BlendMode::Exclusion,
    BlendMode::Hue,
    BlendMode::Saturation,
    BlendMode::Color,
    BlendMode::Luminosity,
];

fn kind_label(k: EffectKind) -> &'static str {
    match k {
        EffectKind::Bevel => "Bevel & Emboss",
        EffectKind::Stroke => "Stroke",
        EffectKind::InnerShadow => "Inner Shadow",
        EffectKind::InnerGlow => "Inner Glow",
        EffectKind::Satin => "Satin",
        EffectKind::ColorOverlay => "Color Overlay",
        EffectKind::GradientOverlay => "Gradient Overlay",
        EffectKind::PatternOverlay => "Pattern Overlay",
        EffectKind::OuterGlow => "Outer Glow",
        EffectKind::DropShadow => "Drop Shadow",
    }
}

/// A choice among `options` (id, label); true when it changed.
fn choice(ui: &mut Ui, salt: &str, label: &str, value: &mut String, options: &[(&str, &str)]) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(t.fg_muted));
        let shown = options.iter().find(|o| o.0 == value).map_or(value.as_str(), |o| o.1).to_string();
        egui::ComboBox::from_id_salt(salt).selected_text(shown).width(140.0).show_ui(ui, |ui| {
            for (id, name) in options {
                if ui.selectable_label(value == id, *name).clicked() && value != id {
                    *value = (*id).to_string();
                    changed = true;
                }
            }
        });
    });
    changed
}

fn mode_choice(ui: &mut Ui, salt: &str, label: &str, mode: &mut BlendMode) -> bool {
    let t = Tokens::get(ui.ctx());
    let before = *mode;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(t.fg_muted));
        egui::ComboBox::from_id_salt(salt).selected_text(mode.label()).width(140.0).show_ui(ui, |ui| {
            for m in EFFECT_MODES {
                ui.selectable_value(mode, m, m.label());
            }
        });
    });
    *mode != before
}

/// A colour swatch: click to take the current colour.
fn colour(ui: &mut Ui, label: &str, rgb: &mut [f32; 3], current: onepaint_color::Rgba) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(t.fg_muted));
        let c = onepaint_color::Rgba::new(rgb[0], rgb[1], rgb[2], 1.0);
        if widgets::swatch(ui, c, 22.0, false).on_hover_text("Use the current colour").clicked() {
            *rgb = [current.r, current.g, current.b];
            changed = true;
        }
    });
    changed
}

/// A slider in display units (`stored = shown · scale`).
fn num(ui: &mut Ui, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, scale: f32, unit: &str) -> bool {
    let mut x = Some(*v);
    let ch = slider(ui, label, &mut x, *v, range, scale, unit);
    if ch {
        *v = x.unwrap_or(*v);
    }
    ch
}

const CONTOURS: [(&str, &str); 7] = [
    ("linear", "Linear"),
    ("half-round", "Half round"),
    ("gaussian", "Gaussian"),
    ("cone", "Cone"),
    ("cone-inverted", "Cone, inverted"),
    ("ring", "Ring"),
    ("rolling-slope", "Rolling slope"),
];

const PATTERNS: [(&str, &str); 8] = [
    ("checker", "Checker"),
    ("stripes", "Stripes"),
    ("dots", "Dots"),
    ("grid", "Grid"),
    ("brick", "Brick"),
    ("crosshatch", "Crosshatch"),
    ("halftone", "Halftone"),
    ("weave", "Weave"),
];

fn contour_choice(ui: &mut Ui, salt: &str, label: &str, c: &mut Contour) -> bool {
    let mut s = c.as_str().to_string();
    let ch = choice(ui, salt, label, &mut s, &CONTOURS);
    if ch {
        *c = Contour::parse(&s).unwrap_or(*c);
    }
    ch
}

/// One effect's controls; true when it changed.
fn effect_ui(ui: &mut Ui, i: usize, e: &mut Effect, current: onepaint_color::Rgba) -> bool {
    let salt = |s: &str| format!("fx{i}{s}");
    let mut ch = false;
    if e.kind != EffectKind::Bevel {
        ch |= mode_choice(ui, &salt("mode"), "Blend", &mut e.blend_mode);
    }
    ch |= num(ui, "Opacity", &mut e.opacity, 0.0..=100.0, 0.01, "%");
    let light = |ui: &mut Ui, e: &mut Effect| {
        let mut ch = num(ui, "Angle", &mut e.angle, 0.0..=359.0, 1.0, "°");
        let mut g = e.use_global_light;
        if widgets::toggle(ui, &mut g, "Use Global Light").changed() {
            e.use_global_light = g;
            ch = true;
        }
        ch
    };
    match e.kind {
        EffectKind::DropShadow | EffectKind::InnerShadow => {
            ch |= colour(ui, "Colour", &mut e.color, current);
            ch |= light(ui, e);
            ch |= num(ui, "Distance", &mut e.distance, 0.0..=250.0, 1.0, " px");
            let label = if e.kind == EffectKind::InnerShadow { "Choke" } else { "Spread" };
            ch |= num(ui, label, &mut e.spread, 0.0..=100.0, 0.01, "%");
            ch |= num(ui, "Size", &mut e.size, 0.0..=250.0, 1.0, " px");
        }
        EffectKind::OuterGlow | EffectKind::InnerGlow => {
            ch |= colour(ui, "Colour", &mut e.color, current);
            ch |= num(ui, "Spread", &mut e.spread, 0.0..=100.0, 0.01, "%");
            ch |= num(ui, "Size", &mut e.size, 0.0..=250.0, 1.0, " px");
        }
        EffectKind::Stroke => {
            ch |= colour(ui, "Colour", &mut e.color, current);
            ch |= num(ui, "Size", &mut e.size, 1.0..=250.0, 1.0, " px");
            ch |= choice(
                ui,
                &salt("pos"),
                "Position",
                &mut e.position,
                &[("outside", "Outside"), ("inside", "Inside"), ("center", "Centre")],
            );
        }
        EffectKind::ColorOverlay => ch |= colour(ui, "Colour", &mut e.color, current),
        EffectKind::GradientOverlay => {
            let shapes =
                [("linear", "Linear"), ("radial", "Radial"), ("angle", "Angle"), ("reflected", "Reflected"), ("diamond", "Diamond")];
            ch |= choice(ui, &salt("shape"), "Style", &mut e.shape, &shapes);
            ch |= num(ui, "Angle", &mut e.angle, 0.0..=359.0, 1.0, "°");
            ch |= num(ui, "Scale", &mut e.scale, 10.0..=150.0, 0.01, "%");
            let mut r = e.reverse;
            if widgets::toggle(ui, &mut r, "Reverse").changed() {
                e.reverse = r;
                ch = true;
            }
        }
        EffectKind::PatternOverlay => {
            ch |= choice(ui, &salt("pat"), "Pattern", &mut e.pattern, &PATTERNS);
            ch |= colour(ui, "Colour A", &mut e.color, current);
            ch |= colour(ui, "Colour B", &mut e.color_b, current);
            ch |= num(ui, "Scale", &mut e.scale, 25.0..=1000.0, 0.01, "%");
        }
        EffectKind::Satin => {
            ch |= colour(ui, "Colour", &mut e.color, current);
            ch |= num(ui, "Angle", &mut e.angle, 0.0..=359.0, 1.0, "°");
            ch |= num(ui, "Distance", &mut e.distance, 0.0..=250.0, 1.0, " px");
            ch |= num(ui, "Size", &mut e.size, 0.0..=250.0, 1.0, " px");
            ch |= contour_choice(ui, &salt("contour"), "Contour", &mut e.contour);
            let mut inv = e.invert;
            if widgets::toggle(ui, &mut inv, "Invert").changed() {
                e.invert = inv;
                ch = true;
            }
        }
        EffectKind::Bevel => {
            let b = &mut e.bevel;
            let styles = [("inner", "Inner Bevel"), ("outer", "Outer Bevel"), ("emboss", "Emboss"), ("pillow", "Pillow Emboss")];
            ch |= choice(ui, &salt("style"), "Style", &mut b.style, &styles);
            ch |= num(ui, "Depth", &mut b.depth, 1.0..=1000.0, 0.01, "%");
            let mut dir = if b.up { "up" } else { "down" }.to_string();
            if choice(ui, &salt("dir"), "Direction", &mut dir, &[("up", "Up"), ("down", "Down")]) {
                b.up = dir == "up";
                ch = true;
            }
            ch |= num(ui, "Soften", &mut b.soften, 0.0..=16.0, 1.0, " px");
            ch |= num(ui, "Altitude", &mut b.altitude, 0.0..=90.0, 1.0, "°");
            ch |= colour(ui, "Highlight", &mut b.highlight_color, current);
            ch |= num(ui, "Highlight opacity", &mut b.highlight_opacity, 0.0..=100.0, 0.01, "%");
            ch |= mode_choice(ui, &salt("hm"), "Highlight blend", &mut b.highlight_mode);
            ch |= colour(ui, "Shadow", &mut b.shadow_color, current);
            ch |= num(ui, "Shadow opacity", &mut b.shadow_opacity, 0.0..=100.0, 0.01, "%");
            ch |= mode_choice(ui, &salt("sm"), "Shadow blend", &mut b.shadow_mode);
            let mut c = b.contour_on;
            if widgets::toggle(ui, &mut c, "Contour").changed() {
                b.contour_on = c;
                ch = true;
            }
            if b.contour_on {
                ch |= contour_choice(ui, &salt("bc"), "Shape", &mut b.contour);
            }
            let mut tx = b.texture_on;
            if widgets::toggle(ui, &mut tx, "Texture").changed() {
                b.texture_on = tx;
                ch = true;
            }
            if b.texture_on {
                ch |= choice(ui, &salt("tp"), "Pattern", &mut b.texture_pattern, &PATTERNS);
                ch |= num(ui, "Scale", &mut b.texture_scale, 25.0..=1000.0, 0.01, "%");
                ch |= num(ui, "Texture depth", &mut b.texture_depth, 0.0..=100.0, 0.01, "%");
                let mut inv = b.texture_invert;
                if widgets::toggle(ui, &mut inv, "Invert").changed() {
                    b.texture_invert = inv;
                    ch = true;
                }
            }
            // Size and the light sit with the rest of Structure.
            ch |= num(ui, "Size", &mut e.size, 1.0..=250.0, 1.0, " px");
            ch |= light(ui, e);
        }
    }
    ch
}

/// The active layer's or group's styles, in Properties.
pub fn panel(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let id = app.session.active().to_string();
    let Some(node) = app.session.doc().find(&id) else { return };
    let mut styles = match node {
        onepaint_doc::Node::Layer(l) if l.adjustment.is_none() => l.styles(),
        onepaint_doc::Node::Group(g) => g.styles(),
        _ => return,
    }
    .unwrap_or_default();
    let open_id = egui::Id::new("styles-open").with(&id);
    let mut open: Option<usize> = ui.ctx().data(|d| d.get_temp::<Option<usize>>(open_id)).flatten();
    let current = app.session.color();
    let mut ch = false;
    crate::dock::fit_width(ui, 280.0);
    ui.label(RichText::new("Layer Style").font(onepaint_ui_kit::theme::semibold(15.0)).color(t.fg));
    ch |= num(ui, "Fill opacity", &mut styles.fill_opacity, 0.0..=100.0, 0.01, "%");
    widgets::section_label(ui, "Styles");
    let mut picked = None;
    let mut delete = None;
    ui.horizontal_wrapped(|ui| {
        for (name, preset) in builtin_presets() {
            if ui.selectable_label(false, name).clicked() {
                picked = Some(preset);
            }
        }
    });
    // The user's own, one per row with its ×.
    for p in &app.style_presets.list {
        ui.horizontal(|ui| {
            if ui.selectable_label(false, &p.name).on_hover_text("Your style").clicked() {
                picked = Some(p.styles.clone());
            }
            if ui.small_button("×").on_hover_text(format!("Delete {}", p.name)).clicked() {
                delete = Some(p.id.clone());
            }
        });
    }
    if let Some(p) = picked {
        // The whole style is replaced, under this layer's light (`applyPreset`).
        styles = p.with_global_light(styles.global_light()).unwrap_or(p);
        open = None;
        ch = true;
    }
    if let Some(id) = delete {
        app.style_presets.remove(&id);
    }
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut app.style_name).desired_width(150.0).hint_text("New style name"));
        let any = !styles.effects.is_empty() || styles.fill_opacity < 1.0;
        if ui.add_enabled_ui(any, |ui| widgets::secondary_button(ui, "Save style", 90.0)).inner.clicked() {
            app.style_presets.add(&app.style_name, &styles);
            app.style_name.clear();
        }
    });
    if widgets::secondary_button(ui, "Import Photoshop styles (.asl)…", 220.0).clicked()
        && let Some(path) = rfd::FileDialog::new().add_filter("Photoshop styles", &["asl"]).pick_file()
    {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("styles").to_string();
        let now = ui.input(|i| i.time);
        let msg = match std::fs::read(&path) {
            Ok(b) => app.style_presets.import_asl(&b, &name),
            Err(e) => format!("Couldn't read {name}: {e}"),
        };
        app.notify(msg, now);
    }
    widgets::section_label(ui, "Effects");
    let mut remove = None;
    for (i, e) in styles.effects.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut on = e.enabled;
            if widgets::toggle(ui, &mut on, "").changed() {
                e.enabled = on;
                ch = true;
            }
            if ui.selectable_label(open == Some(i), kind_label(e.kind)).clicked() {
                open = if open == Some(i) { None } else { Some(i) };
            }
            if ui.small_button("×").on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        });
        if open == Some(i) {
            ui.indent(("fx", i), |ui| ch |= effect_ui(ui, i, e, current));
        }
    }
    if let Some(i) = remove {
        styles.effects.remove(i);
        open = None;
        ch = true;
    }
    if styles.effects.is_empty() {
        ui.label(RichText::new("No effects yet.").color(t.fg_muted));
    }
    ui.label(RichText::new("Add").color(t.fg_muted));
    ui.horizontal_wrapped(|ui| {
        for k in EffectKind::ALL {
            let n = styles.effects.iter().filter(|e| e.kind == k).count();
            if n >= if k.repeats() { MAX_INSTANCES } else { 1 } {
                continue;
            }
            if ui.selectable_label(false, kind_label(k)).clicked() {
                // New effects go on top, as happypaint's dialog adds them.
                styles.effects.insert(0, default_effect(k));
                open = Some(0);
                ch = true;
            }
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(open_id, open));
    // Global Light: linked shadows and bevels across the whole document.
    let linked = styles
        .effects
        .iter()
        .any(|e| e.use_global_light && matches!(e.kind, EffectKind::DropShadow | EffectKind::InnerShadow | EffectKind::Bevel));
    if linked {
        widgets::section_label(ui, "Global Light");
        let (mut angle, mut altitude) = styles.global_light();
        let a = num(ui, "Light angle", &mut angle, 0.0..=359.0, 1.0, "°");
        let b = num(ui, "Light altitude", &mut altitude, 0.0..=90.0, 1.0, "°");
        if a || b {
            let _ = app.session.execute(Command::ApplyGlobalLight { angle, altitude });
            ch = false;
        }
    }
    let is_layer = matches!(app.session.doc().find(&id), Some(onepaint_doc::Node::Layer(_)));
    if is_layer && !styles.effects.is_empty() && widgets::secondary_button(ui, "Bake into pixels", 140.0).clicked() {
        let _ = app.session.execute(Command::BakeStyles { id: id.clone() });
        ch = false;
    }
    if ch {
        let json = (styles != LayerStyles::default()).then(|| styles.to_json());
        let _ = app.session.execute(Command::SetLayerProps { id, props: LayerProps { styles: Some(json), ..Default::default() } });
    }
    // A drag's edits are one undo step; a click (or a drag's end) closes it.
    if ui.input(|i| i.pointer.any_released()) {
        let _ = app.session.execute(Command::SealHistory);
    }
}
