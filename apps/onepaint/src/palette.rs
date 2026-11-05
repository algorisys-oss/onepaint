//! The command palette (Ctrl/⌘+K, happypaint v0.204.11): every menu row and
//! every rebindable action, searched by name, menu path and a few other words
//! artists use ("bucket" for Fill). The menus are the registry, so a new menu
//! row shows up here with nothing else to add.

use crate::keymap::Action;
use crate::menus;
use crate::{App, Popover};
use onepaint_ui_kit::command_palette::{self, Outcome, PaletteItem};
use onepaint_ui_kit::menu_bar::{Menu, MenuEntry};

/// What a palette row runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A menu row's id (run as if picked from the bar).
    Menu(u32),
    Action(Action),
}

/// Other words a command answers to, by a word in its title (lower-case).
const KEYWORDS: &[(&str, &[&str])] = &[
    ("fill", &["bucket", "paint bucket", "flood"]),
    ("eyedropper", &["picker", "sample", "colour picker", "color picker"]),
    ("pan", &["hand", "scroll"]),
    ("smudge", &["blend", "finger"]),
    ("eraser", &["rubber", "erase"]),
    ("lasso", &["freehand select"]),
    ("wand", &["select colour", "select color"]),
    ("print preview", &["cmyk", "soft proof", "proof"]),
    ("hue", &["colour", "color", "saturation"]),
    ("colour", &["color"]),
    ("grayscale", &["greyscale", "value", "black and white"]),
    ("export", &["png", "jpeg", "psd", "save as image"]),
    ("zen", &["hide ui", "full screen", "fullscreen", "distraction"]),
    ("undo", &["back", "revert"]),
    ("record", &["timelapse", "replay"]),
    ("canvas size", &["crop", "resize canvas"]),
    ("image size", &["scale", "resize image"]),
    ("move", &["transform", "scale", "rotate"]),
    ("symmetry", &["mirror", "kaleidoscope"]),
];

fn keywords_for(title: &str) -> Vec<String> {
    let t = title.to_lowercase();
    KEYWORDS.iter().filter(|(k, _)| t.contains(k)).flat_map(|(_, w)| w.iter().map(|s| s.to_string())).collect()
}

/// Every command the palette offers, with what each runs. Disabled menu rows
/// are left out: the palette only lists what can run now.
pub fn items(app: &App) -> (Vec<PaletteItem>, Vec<Target>) {
    let mut items = Vec::new();
    let mut targets = Vec::new();
    for m in menus::build(app) {
        flatten(&m, &m.title, &m.entries, &mut items, &mut targets);
    }
    for a in Action::ALL.iter().copied() {
        // A menu row by the same name already runs it.
        let title = a.label().to_string();
        if items.iter().any(|i: &PaletteItem| i.title.eq_ignore_ascii_case(&title)) || a == Action::ToggleCommandPalette {
            continue;
        }
        items.push(PaletteItem {
            keywords: keywords_for(&title),
            category: a.category().label().to_string(),
            path: String::new(),
            shortcut: menus::key(app, a),
            title,
        });
        targets.push(Target::Action(a));
    }
    (items, targets)
}

fn flatten(menu: &Menu, path: &str, entries: &[MenuEntry], items: &mut Vec<PaletteItem>, targets: &mut Vec<Target>) {
    for e in entries {
        match e {
            MenuEntry::Item(it) if it.enabled && it.id != menus::palette_id() => {
                let title = it.label.trim_end_matches('…').to_string();
                // Several menus share a row (Help and Edit both have the palette).
                if targets.contains(&Target::Menu(it.id)) {
                    continue;
                }
                items.push(PaletteItem {
                    keywords: keywords_for(&title),
                    category: menu.title.clone(),
                    // The badge names the menu; the path only adds a submenu.
                    path: if path == menu.title { String::new() } else { path.to_string() },
                    shortcut: it.shortcut.clone(),
                    title,
                });
                targets.push(Target::Menu(it.id));
            }
            MenuEntry::Sub(name, sub) => flatten(menu, &format!("{path} ▸ {name}"), sub, items, targets),
            _ => {}
        }
    }
}

/// Open or close the palette (Ctrl/⌘+K, Edit ▸ / Help ▸ Command palette…).
pub fn toggle(app: &mut App, ctx: &egui::Context) {
    if app.popover == Some(Popover::CommandPalette) {
        app.popover = None;
    } else {
        command_palette::reset(ctx);
        app.popover = Some(Popover::CommandPalette);
    }
}

/// Draw it under the header, centred; run what is picked.
pub fn show(app: &mut App, ctx: &egui::Context, top: egui::Pos2) {
    let (items, targets) = items(app);
    match command_palette::show(ctx, &items, top) {
        Outcome::Open => {}
        Outcome::Close => app.popover = None,
        Outcome::Run(i) => {
            app.popover = None;
            match targets[i] {
                Target::Menu(id) => {
                    if let Some(p) = menus::decode(id) {
                        menus::run(app, p, ctx);
                    }
                }
                Target::Action(a) => app.queued_actions.push(a),
            }
            ctx.request_repaint();
        }
    }
}

/// What the palette row titled `title` runs (tests).
#[cfg(test)]
pub fn find(app: &App, title: &str) -> Option<Target> {
    let (items, targets) = items(app);
    items.iter().position(|i| i.title == title).map(|i| targets[i])
}
