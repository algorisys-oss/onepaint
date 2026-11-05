//! App settings (happypaint `state/settings-store.ts`, `settings-dialog.tsx`):
//! documents, brush and cursor, history. Persisted as JSON in the config
//! dir (`None` in tests and scripted runs, so nothing touches real folders).

use crate::App;
use egui::{RichText, Ui};
use onepaint_engine::{BRUSH_SIZE_HARD_MAX, Preferences};
use onepaint_ui_kit::{Tokens, theme, widgets};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_MAX_BRUSH_SIZE: f64 = 1000.0;
pub const DEFAULT_MAX_OPEN_TABS: usize = 6;
pub const MIN_OPEN_TABS: usize = 2;
pub const MAX_OPEN_TABS: usize = 12;
pub const DEFAULT_HISTORY_STEPS: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SizeScaling {
    /// Brush size in document px: zooming makes the brush look bigger.
    #[default]
    Canvas,
    /// Brush size in screen px: the brush looks the same at any zoom.
    Screen,
}

/// How the timeline counts (`timelineMetric`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TimelineMetric {
    Seconds,
    /// Also anything unrecognised (`settings-store.ts:183`).
    #[default]
    #[serde(other)]
    Frames,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    #[default]
    Ring,
    Crosshair,
    Both,
    Dot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub max_brush_size: f64,
    pub document_tabs: bool,
    pub max_open_tabs: usize,
    pub brush_size_scaling: SizeScaling,
    pub force_size_pressure: bool,
    pub force_opacity_pressure: bool,
    pub brush_cursor: CursorStyle,
    pub cursor_use_color: bool,
    pub cursor_thickness: u8,
    /// Trace the brush tip's shape (image tips; round and square are exact already).
    pub cursor_tip_shape: bool,
    pub max_history_steps: usize,
    pub timeline_metric: TimelineMetric,
    /// Shortcut overrides: action id → combo (`keymap.rs`).
    pub keybindings: crate::keymap::Overrides,
    /// The size and opacity sliders beside the canvas (happypaint `sideSliders*`).
    pub side_sliders: bool,
    /// Scrollbars on the canvas when the document runs past the view.
    pub scrollbars: bool,
    /// The system's title bar instead of onepaint's header (spec 65; for
    /// window managers that misbehave without one). Read at start-up.
    pub system_title_bar: bool,
    pub side_sliders_mode: crate::side_sliders::Mode,
    /// Px from the centre along the edge.
    pub side_sliders_offset: f32,
    pub side_sliders_float: crate::side_sliders::Float,
    /// The colour panel shows the Mix grid instead of the picker.
    pub color_mix_mode: bool,
    /// The Mix grid's wells, size and space (happypaint `mix-grid-store.ts`).
    pub mix: onepaint_color::mix_grid::MixSettings,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_brush_size: DEFAULT_MAX_BRUSH_SIZE,
            document_tabs: true,
            max_open_tabs: DEFAULT_MAX_OPEN_TABS,
            brush_size_scaling: SizeScaling::Canvas,
            force_size_pressure: false,
            force_opacity_pressure: false,
            brush_cursor: CursorStyle::Ring,
            cursor_use_color: false,
            cursor_thickness: 1,
            cursor_tip_shape: true,
            max_history_steps: DEFAULT_HISTORY_STEPS,
            keybindings: Default::default(),
            timeline_metric: TimelineMetric::Frames,
            side_sliders: true,
            scrollbars: true,
            system_title_bar: false,
            side_sliders_mode: Default::default(),
            side_sliders_offset: 0.0,
            side_sliders_float: Default::default(),
            color_mix_mode: false,
            mix: Default::default(),
            path: None,
        }
    }
}

pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint").map(|d| d.config_dir().join("settings.json"))
}

impl Settings {
    pub fn load(path: Option<PathBuf>) -> Self {
        let mut s =
            path.as_ref().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default();
        s.clamp();
        s.path = path;
        s
    }

    /// Keep every value in range (`clamp*` / `coerce*` in the store).
    fn clamp(&mut self) {
        self.max_brush_size = if self.max_brush_size.is_finite() {
            self.max_brush_size.round().clamp(10.0, BRUSH_SIZE_HARD_MAX)
        } else {
            DEFAULT_MAX_BRUSH_SIZE
        };
        self.max_open_tabs = self.max_open_tabs.clamp(MIN_OPEN_TABS, MAX_OPEN_TABS);
        self.cursor_thickness = self.cursor_thickness.clamp(1, 4);
        self.max_history_steps = self.max_history_steps.clamp(5, 1000);
        self.mix = std::mem::take(&mut self.mix).sanitized();
        self.side_sliders_float = self.side_sliders_float.coerced();
        if !self.side_sliders_offset.is_finite() {
            self.side_sliders_offset = 0.0;
        }
        // Unknown actions and unreadable combos are dropped (happypaint keeps them).
        self.keybindings.retain(|id, combo| crate::keymap::Action::from_id(id).is_some() && crate::keymap::is_valid(combo));
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    /// What the engine honours, at the current zoom.
    pub fn preferences(&self, zoom: f64) -> Preferences {
        Preferences {
            max_brush_size: self.max_brush_size,
            force_size_pressure: self.force_size_pressure,
            force_flow_pressure: self.force_opacity_pressure,
            screen_zoom: (self.brush_size_scaling == SizeScaling::Screen).then_some(zoom),
            view_zoom: zoom,
            max_history_steps: self.max_history_steps,
        }
    }
}

fn row(ui: &mut Ui, title: &str, sub: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(title).font(theme::medium(12.5)).color(t.fg));
    ui.label(RichText::new(sub).font(theme::medium(10.5)).color(t.fg_faint));
}

/// The Settings dialog.
pub fn dialog(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.set_width(700.0);
    ui.label(RichText::new("Settings").font(theme::semibold(15.0)).color(t.fg));
    let before = app.settings.clone();
    let s = &mut app.settings;
    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() - 160.0).show(ui, |ui| {
        ui.columns(2, |cols| {
            let ui = &mut cols[0];
            widgets::section_label(ui, "Documents");
            row(ui, "Document tabs", "One tab per open drawing in the top bar.");
            widgets::toggle(ui, &mut s.document_tabs, "Show document tabs");
            row(ui, "Maximum open tabs", "Opening past this closes the least recently used saved tab.");
            let mut tabs = s.max_open_tabs as f32;
            if widgets::slider_row(ui, "Tabs", &mut tabs, MIN_OPEN_TABS as f32..=MAX_OPEN_TABS as f32, |v| format!("{v:.0}")).changed() {
                s.max_open_tabs = tabs.round() as usize;
            }
            widgets::hairline(ui);

            widgets::section_label(ui, "Brush");
            row(ui, "Maximum brush size", "The size slider's ceiling, in document px (10–10000).");
            let mut max = s.max_brush_size as f32;
            if widgets::slider_row(ui, "Max size", &mut max, 10.0..=BRUSH_SIZE_HARD_MAX as f32, |v| format!("{v:.0} px")).changed() {
                s.max_brush_size = max.round() as f64;
            }
            row(ui, "Brush size scaling", "Relative to the canvas (zoom changes it) or to the screen.");
            let cur = usize::from(s.brush_size_scaling == SizeScaling::Screen);
            if let Some(i) = crate::chrome::segmented(ui, &["Relative to canvas", "Relative to screen"], cur) {
                s.brush_size_scaling = if i == 1 { SizeScaling::Screen } else { SizeScaling::Canvas };
            }
            row(ui, "Pen pressure", "Pressure drives size or opacity for every brush, whatever its own settings.");
            widgets::toggle(ui, &mut s.force_size_pressure, "Always use pressure for size");
            widgets::toggle(ui, &mut s.force_opacity_pressure, "Always use pressure for opacity");
            row(
                ui,
                "Size & opacity sliders",
                "Procreate's sidebar beside the canvas: brush size, opacity, undo and redo. Drag its grip to move it.",
            );
            let on_label = if s.side_sliders { "On" } else { "Off" };
            widgets::toggle(ui, &mut s.side_sliders, on_label);
            row(
                ui,
                "Placement",
                "An edge (right suits left-handed painting), floating anywhere, or inside the tool panel. Pulling the grip away from an edge also floats it.",
            );
            ui.add_enabled_ui(s.side_sliders, |ui| {
                let mut mode = s.side_sliders_mode;
                egui::ComboBox::from_id_salt("side-sliders-mode").selected_text(mode.label()).width(180.0).show_ui(ui, |ui| {
                    for m in crate::side_sliders::Mode::ALL {
                        ui.selectable_value(&mut mode, m, m.label());
                    }
                });
                if mode != s.side_sliders_mode {
                    s.side_sliders_mode = mode;
                    s.side_sliders_offset = 0.0;
                }
            });

            let ui = &mut cols[1];
            widgets::section_label(ui, "Cursor");
            let styles = [CursorStyle::Ring, CursorStyle::Crosshair, CursorStyle::Both, CursorStyle::Dot];
            let cur = styles.iter().position(|c| *c == s.brush_cursor).unwrap_or(0);
            if let Some(i) = crate::chrome::segmented(ui, &["Ring", "Cross", "Both", "Dot"], cur) {
                s.brush_cursor = styles[i];
            }
            widgets::toggle(ui, &mut s.cursor_use_color, "Draw the cursor in the paint colour");
            widgets::toggle(ui, &mut s.scrollbars, "Scrollbars on the canvas")
                .on_hover_text("Shown when the drawing runs past the view: drag to pan, click the track to page.");
            if !cfg!(target_os = "macos") {
                widgets::toggle(ui, &mut s.system_title_bar, "Use the system title bar")
                    .on_hover_text("Takes effect when onepaint starts again.");
            }
            widgets::toggle(ui, &mut s.cursor_tip_shape, "Trace the tip")
                .on_hover_text("A leaf brush shows a leaf, not a circle. Round and square tips are unaffected.");
            let mut th = s.cursor_thickness as f32;
            if widgets::slider_row(ui, "Thickness", &mut th, 1.0..=4.0, |v| format!("{v:.0} px")).changed() {
                s.cursor_thickness = th.round() as u8;
            }
            widgets::hairline(ui);

            widgets::section_label(ui, "History");
            row(ui, "Undo steps", "Fewer free memory on big canvases; the memory budget still applies.");
            let mut steps = s.max_history_steps as f32;
            if widgets::slider_row(ui, "Steps", &mut steps, 5.0..=1000.0, |v| format!("{v:.0}")).changed() {
                s.max_history_steps = steps.round() as usize;
            }
        });
        ui.add_space(8.0);
        widgets::hairline(ui);
        shortcuts(ui, &mut s.keybindings, &mut app.capture);
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if widgets::secondary_button(ui, "Reset all shortcuts", 150.0).clicked() {
            app.settings.keybindings.clear();
            app.capture = None;
        }
        if widgets::secondary_button(ui, "Reset to defaults", 140.0).clicked() {
            let path = app.settings.path.take();
            app.settings = Settings { path, ..Settings::default() };
        }
        if widgets::primary_button(ui, "Done", 90.0).clicked() {
            app.popover = None;
        }
    });
    if app.settings != before {
        app.settings.clamp();
        app.settings.save();
    }
}

/// Settings ▸ Keyboard shortcuts (`settings-dialog.tsx:104-143`).
fn shortcuts(ui: &mut Ui, ov: &mut crate::keymap::Overrides, capture: &mut Option<crate::keymap::Action>) {
    use crate::keymap::{Action, Category, combos_for, duplicated, format_combo};
    let t = Tokens::get(ui.ctx());
    widgets::section_label(ui, "Keyboard shortcuts");
    ui.label(
        RichText::new("Click a shortcut, then press the new key combination. Esc cancels.").font(theme::medium(10.5)).color(t.fg_faint),
    );
    let dups = duplicated(ov);
    ui.columns(2, |cols| {
        for (i, cat) in Category::ALL.iter().enumerate() {
            let ui = &mut cols[usize::from(i >= 2)];
            ui.label(RichText::new(cat.label()).font(theme::semibold(12.5)).color(t.fg));
            egui::Grid::new(("keys", cat.label())).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                for a in Action::ALL.iter().copied().filter(|a| a.category() == *cat) {
                    ui.label(RichText::new(a.label()).color(t.fg_muted));
                    ui.horizontal(|ui| {
                        if dups.contains(&a) {
                            ui.label(RichText::new("duplicate").font(theme::medium(10.0)).color(t.danger))
                                .on_hover_text("This shortcut is used by another action");
                        }
                        let capturing = *capture == Some(a);
                        let text = if capturing {
                            "Press keys…".to_string()
                        } else {
                            combos_for(a, ov).first().map_or_else(String::new, |c| format_combo(c))
                        };
                        let b = egui::Button::new(RichText::new(text).font(theme::mono(11.0)).color(if capturing {
                            t.on_accent
                        } else {
                            t.fg
                        }))
                        .fill(if capturing { t.accent } else { t.field })
                        .min_size(egui::vec2(96.0, 20.0));
                        let r = ui.add(b);
                        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{} shortcut", a.label())));
                        if r.clicked() {
                            *capture = if capturing { None } else { Some(a) };
                        }
                        let custom = ov.contains_key(a.id());
                        let reset = ui.add_enabled(custom, egui::Button::new("↺").min_size(egui::vec2(20.0, 20.0)));
                        reset.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, custom, format!("Reset {} shortcut", a.label()))
                        });
                        if reset.on_hover_text("Reset to default").clicked() {
                            ov.remove(a.id());
                        }
                    });
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_clamps_and_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("onepaint-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.json");
        std::fs::write(&p, r#"{"maxBrushSize": 99999, "maxOpenTabs": 1, "cursorThickness": 9, "brushCursor": "dot", "timelineMetric": "minutes", "bogus": 1}"#).unwrap();
        let s = Settings::load(Some(p.clone()));
        assert_eq!((s.max_brush_size, s.max_open_tabs, s.cursor_thickness), (10000.0, 2, 4));
        assert_eq!((s.brush_cursor, s.timeline_metric), (CursorStyle::Dot, TimelineMetric::Frames));
        assert!(s.document_tabs, "absent keys keep their defaults");
        let mut t = s.clone();
        t.force_size_pressure = true;
        t.save();
        assert!(Settings::load(Some(p)).force_size_pressure);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(Settings::load(None), Settings::default());
        let pr = Settings { brush_size_scaling: SizeScaling::Screen, ..Default::default() }.preferences(2.0);
        assert_eq!(pr.screen_zoom, Some(2.0));
    }
}
