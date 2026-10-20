//! onepaint's design system: theme tokens, fonts, icons and widgets.
//!
//! App code uses only these components and tokens, never raw egui visuals.
//! See `docs/design-system.md`.

pub mod gradient_editor;
pub mod gradient_stops;
pub mod icons;
pub mod inline;
pub mod menu_bar;
pub mod range_bar;
pub mod theme;
pub mod widgets;

pub use theme::{ThemeKind, Tokens};

/// Install fonts, image loaders and the theme. Call once at startup.
pub fn install(ctx: &egui::Context, kind: ThemeKind) {
    egui_extras::install_image_loaders(ctx);
    theme::install_fonts(ctx);
    theme::apply(ctx, kind);
}
