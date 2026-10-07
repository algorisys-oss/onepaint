// Adapted from photocraft (MIT/Apache-2.0): crates/ui-egui/src/theme.rs
//! Theme tokens and their mapping onto egui's style.
//!
//! The palette starts from happypaint's (`frontend/src/styles/global.css`)
//! so the two apps feel like siblings.

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeKind {
    #[default]
    Dark,
    Light,
}

impl ThemeKind {
    pub fn toggled(self) -> Self {
        match self {
            ThemeKind::Dark => ThemeKind::Light,
            ThemeKind::Light => ThemeKind::Dark,
        }
    }
}

/// Colour, shape and spacing tokens. Widgets read [`Tokens::get`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub kind: ThemeKind,
    /// Behind the canvas (the pasteboard).
    pub stage: Color32,
    pub bg: Color32,
    pub bg_elevated: Color32,
    /// Floating panels and popovers.
    pub panel: Color32,
    /// Translucent chrome floating over the canvas.
    pub glass: Color32,
    pub border: Color32,
    pub field: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub fg: Color32,
    pub fg_muted: Color32,
    pub fg_faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub shadow: Color32,
    pub checker_a: Color32,
    pub checker_b: Color32,
    pub radius_sm: f32,
    pub radius: f32,
    pub radius_lg: f32,
}

impl Tokens {
    pub fn for_kind(kind: ThemeKind) -> Self {
        match kind {
            ThemeKind::Dark => Tokens {
                kind,
                stage: Color32::from_rgb(20, 20, 22),
                bg: Color32::from_rgb(0x1a, 0x1a, 0x1a),
                bg_elevated: Color32::from_rgb(0x24, 0x24, 0x24),
                panel: Color32::from_rgba_unmultiplied(30, 30, 34, 240),
                glass: Color32::from_rgba_unmultiplied(28, 28, 32, 214),
                border: Color32::from_rgb(0x33, 0x33, 0x38),
                field: Color32::from_rgb(38, 38, 42),
                hover: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
                pressed: Color32::from_rgba_unmultiplied(255, 255, 255, 34),
                fg: Color32::from_rgb(0xf2, 0xf2, 0xf2),
                fg_muted: Color32::from_rgb(0xa0, 0xa0, 0xa0),
                fg_faint: Color32::from_rgb(110, 110, 118),
                accent: Color32::from_rgb(0x4c, 0x8d, 0xff),
                accent_soft: Color32::from_rgba_unmultiplied(0x4c, 0x8d, 0xff, 48),
                on_accent: Color32::WHITE,
                danger: Color32::from_rgb(0xe5, 0x48, 0x4d),
                success: Color32::from_rgb(0x3e, 0xc4, 0x6d),
                warning: Color32::from_rgb(0xe0, 0xa2, 0x3c),
                shadow: Color32::from_black_alpha(115),
                checker_a: Color32::from_rgb(0x3a, 0x3a, 0x3a),
                checker_b: Color32::from_rgb(0x44, 0x44, 0x44),
                radius_sm: 6.0,
                radius: 8.0,
                radius_lg: 12.0,
            },
            ThemeKind::Light => Tokens {
                kind,
                stage: Color32::from_rgb(226, 226, 230),
                bg: Color32::from_rgb(0xf2, 0xf2, 0xf4),
                bg_elevated: Color32::WHITE,
                panel: Color32::from_rgba_unmultiplied(255, 255, 255, 245),
                glass: Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                border: Color32::from_rgb(0xd9, 0xd9, 0xde),
                field: Color32::from_rgb(240, 240, 243),
                hover: Color32::from_rgba_unmultiplied(0, 0, 0, 15),
                pressed: Color32::from_rgba_unmultiplied(0, 0, 0, 28),
                fg: Color32::from_rgb(0x1d, 0x1d, 0x1f),
                fg_muted: Color32::from_rgb(0x6e, 0x6e, 0x73),
                fg_faint: Color32::from_rgb(150, 150, 158),
                accent: Color32::from_rgb(0x2f, 0x74, 0xe8),
                accent_soft: Color32::from_rgba_unmultiplied(0x2f, 0x74, 0xe8, 40),
                on_accent: Color32::WHITE,
                danger: Color32::from_rgb(0xd2, 0x3b, 0x3b),
                success: Color32::from_rgb(0x2e, 0xa8, 0x5a),
                warning: Color32::from_rgb(0xc0, 0x7d, 0x18),
                shadow: Color32::from_black_alpha(46),
                checker_a: Color32::from_rgb(0xc8, 0xc8, 0xcc),
                checker_b: Color32::from_rgb(0xe2, 0xe2, 0xe6),
                radius_sm: 6.0,
                radius: 8.0,
                radius_lg: 12.0,
            },
        }
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::new("onepaint-theme")))
            .unwrap_or_else(|| Tokens::for_kind(ThemeKind::Dark))
    }

    pub fn dark(&self) -> bool {
        self.kind == ThemeKind::Dark
    }
}

pub const INTER_REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
pub const INTER_MEDIUM: &[u8] = include_bytes!("../assets/fonts/Inter-Medium.ttf");
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");
pub const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        ("Inter", INTER_REGULAR),
        ("Inter-Medium", INTER_MEDIUM),
        ("Inter-SemiBold", INTER_SEMIBOLD),
        ("JetBrainsMono", JETBRAINS_MONO),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "JetBrainsMono".to_owned());
    let fallback: Vec<String> = fonts.families[&FontFamily::Proportional].clone();
    for (fam, primary) in [("medium", "Inter-Medium"), ("semibold", "Inter-SemiBold")] {
        let mut stack = vec![primary.to_owned()];
        stack.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(fam.into()), stack);
    }
    ctx.set_fonts(fonts);
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Apply a theme to egui's global style and publish its tokens.
pub fn apply(ctx: &egui::Context, kind: ThemeKind) {
    let t = Tokens::for_kind(kind);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("onepaint-theme"), t));
    let mut v = if t.dark() {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = t.bg;
    v.window_fill = t.panel;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.bg_elevated;
    v.code_bg_color = t.field;
    v.override_text_color = Some(t.fg);
    v.hyperlink_color = t.accent;
    v.warn_fg_color = t.warning;
    v.error_fg_color = t.danger;
    v.window_corner_radius = CornerRadius::same(t.radius_lg as u8);
    v.menu_corner_radius = CornerRadius::same(t.radius as u8);
    v.window_shadow = egui::Shadow {
        offset: [0, 12],
        blur: 36,
        spread: 0,
        color: t.shadow,
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: t.shadow,
    };
    v.selection.bg_fill = t.accent_soft;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.striped = false;
    let r = CornerRadius::same(t.radius_sm as u8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.panel;
    w.noninteractive.weak_bg_fill = t.panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.fg_muted);
    w.noninteractive.corner_radius = r;
    for (wv, bg, stroke) in [
        (&mut w.inactive, t.field, t.border),
        (&mut w.hovered, t.field, t.fg_faint),
        (&mut w.active, t.field, t.accent),
        (&mut w.open, t.field, t.border),
    ] {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::new(1.0, stroke);
        wv.fg_stroke = Stroke::new(1.0, t.fg);
        wv.corner_radius = r;
        wv.expansion = 0.0;
    }
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, semibold(15.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size = egui::vec2(24.0, 24.0);
        s.spacing.menu_margin = egui::Margin::same(8);
        s.spacing.window_margin = egui::Margin::same(12);
        s.spacing.scroll = egui::style::ScrollStyle::thin();
        s.interaction.tooltip_delay = 0.35;
        s.animation_time = 0.14;
    });
}
