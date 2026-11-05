//! Rebindable keyboard shortcuts (happypaint `input/keybindings.ts` and
//! `settings-store.ts` keybindings).
//!
//! A combo is happypaint's string: lower-case, modifiers in the order
//! `ctrl+alt+shift+key`, `ctrl` meaning Ctrl or Cmd. Symbol keys drop
//! `shift` (`?`, `+`), and Shift+, / Shift+. read `<` / `>`. Matching is
//! exact on modifiers, so `b` never fires for Shift+B. Overrides replace an
//! action's defaults and live in `settings.json`.

use egui::{Event, Key, Modifiers};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Tools,
    Documents,
    Brush,
    View,
    Animation,
}

impl Category {
    pub const ALL: [Category; 5] = [Category::Tools, Category::Documents, Category::Brush, Category::View, Category::Animation];

    pub fn label(self) -> &'static str {
        match self {
            Category::Tools => "Tools",
            Category::Documents => "Documents",
            Category::Brush => "Brush",
            Category::View => "View",
            Category::Animation => "Animation",
        }
    }
}

macro_rules! actions {
    ($($v:ident = $id:literal, $label:literal, $cat:ident, [$($d:literal),+];)+) => {
        /// Every rebindable action, in happypaint's registry order (the first
        /// action bound to a combo wins).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Action { $($v),+ }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$v),+];

            pub fn id(self) -> &'static str {
                match self { $(Action::$v => $id),+ }
            }
            pub fn label(self) -> &'static str {
                match self { $(Action::$v => $label),+ }
            }
            pub fn category(self) -> Category {
                match self { $(Action::$v => Category::$cat),+ }
            }
            pub fn defaults(self) -> &'static [&'static str] {
                match self { $(Action::$v => &[$($d),+]),+ }
            }
        }
    };
}

actions! {
    ToolBrush = "tool-brush", "Brush", Tools, ["b"];
    ToolEraser = "tool-eraser", "Eraser", Tools, ["e"];
    ToolFill = "tool-fill", "Fill (bucket)", Tools, ["g"];
    ToolEyedropper = "tool-eyedropper", "Eyedropper (colour picker)", Tools, ["i"];
    ToolMove = "tool-move", "Move / transform", Tools, ["v", "ctrl+t"];
    ToolPan = "tool-pan", "Pan (hand) — hold Space for a temporary pan", Tools, ["h"];
    ToolSmudge = "tool-smudge", "Smudge", Tools, ["s"];
    ToolMixer = "tool-mixer", "Mixer brush", Tools, ["shift+b"];
    ToolText = "tool-text", "Text", Tools, ["t"];
    ToolPen = "tool-pen", "Pen (vector path)", Tools, ["p"];
    ToolShape = "tool-shape", "Shape (line / ellipse / rect / polygon)", Tools, ["u"];
    ToolSelectRect = "tool-select-rect", "Rectangle select", Tools, ["m"];
    ToolSelectLasso = "tool-select-lasso", "Lasso select", Tools, ["l"];
    ToolSelectWand = "tool-select-wand", "Magic wand select", Tools, ["w"];
    DocNextTab = "doc-next-tab", "Next open drawing", Documents, ["ctrl+alt+arrowright"];
    DocPrevTab = "doc-prev-tab", "Previous open drawing", Documents, ["ctrl+alt+arrowleft"];
    BrushSizeDown = "brush-size-down", "Brush size down", Brush, ["["];
    BrushSizeUp = "brush-size-up", "Brush size up", Brush, ["]"];
    BrushAngleDown = "brush-angle-down", "Brush angle −15°", Brush, ["{"];
    BrushAngleUp = "brush-angle-up", "Brush angle +15°", Brush, ["}"];
    ToggleFillShape = "toggle-fill-shape", "Fill mode (silhouette)", Brush, ["f"];
    ColorSwap = "color-swap", "Swap foreground / background colour", Brush, ["x"];
    ColorReset = "color-reset", "Reset colours to black & white", Brush, ["d"];
    FillForeground = "fill-foreground", "Fill with the foreground colour", Brush, ["alt+backspace"];
    FillBackground = "fill-background", "Fill with the background colour", Brush, ["ctrl+backspace"];
    ZoomIn = "zoom-in", "Zoom in", View, ["=", "+"];
    ZoomOut = "zoom-out", "Zoom out", View, ["-"];
    ZoomFit = "zoom-fit", "Fit to screen", View, ["0"];
    Zoom100 = "zoom-100", "Zoom 100%", View, ["1"];
    RotateLeft = "rotate-left", "Rotate canvas left 15°", View, ["ctrl+["];
    RotateRight = "rotate-right", "Rotate canvas right 15°", View, ["ctrl+]"];
    ToggleSymmetry = "toggle-symmetry", "Symmetry on/off", View, ["y"];
    ToggleSymmetryMoveAxis = "toggle-symmetry-move-axis", "Move symmetry axis", View, ["shift+y"];
    TogglePerspectiveEdit = "toggle-perspective-edit", "Edit perspective points", View, ["shift+p"];
    ToggleRulers = "toggle-rulers", "Rulers on/off", View, ["shift+r"];
    ToggleGuidesEdit = "toggle-guides-edit", "Edit guides (draw / move / angle)", View, ["shift+g"];
    ToggleMeasure = "toggle-measure", "Measure (distance & angle)", View, ["shift+d"];
    RotateReset = "rotate-reset", "Reset canvas rotation", View, ["5"];
    ToggleLayersPanel = "toggle-layers-panel", "Layers panel", View, ["alt+l"];
    ToggleBrushPanel = "toggle-brush-panel", "Brush settings panel", View, ["alt+b"];
    ToggleReference = "toggle-reference", "Reference window", View, ["alt+r"];
    ToggleHelp = "toggle-help", "Shortcuts & gestures", View, ["?"];
    ToggleSettings = "toggle-settings", "Settings", View, ["ctrl+,"];
    ToggleCommandPalette = "toggle-command-palette", "Command palette", View, ["ctrl+k"];
    AnimToggle = "anim-toggle", "Animation mode on/off", Animation, ["shift+a"];
    AnimPlayPause = "anim-play-pause", "Play / pause", Animation, ["k"];
    AnimPrevFrame = "anim-prev-frame", "Previous frame (flip back)", Animation, [","];
    AnimNextFrame = "anim-next-frame", "Next frame (flip forward)", Animation, ["."];
    AnimFirstFrame = "anim-first-frame", "First frame", Animation, ["<"];
    AnimLastFrame = "anim-last-frame", "Last frame", Animation, [">"];
    AnimAddFrame = "anim-add-frame", "Add frame after the current one", Animation, ["n"];
    AnimToggleOnion = "anim-toggle-onion", "Onion skins on/off", Animation, ["o"];
    AnimAddMarker = "anim-add-marker", "Add a marker here", Animation, ["shift+m"];
    AnimPrevMarker = "anim-prev-marker", "Previous marker", Animation, ["alt+,"];
    AnimNextMarker = "anim-next-marker", "Next marker", Animation, ["alt+."];
    AnimMarkIn = "anim-mark-in", "Play range: mark in", Animation, ["shift+i"];
    AnimMarkOut = "anim-mark-out", "Play range: mark out", Animation, ["shift+o"];
}

impl Action {
    pub fn from_id(id: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|a| a.id() == id)
    }
}

/// Keys whose token is a printed symbol: shift is implied, not part of the combo.
const SYMBOLS: &[(Key, &str)] = &[
    (Key::Comma, ","),
    (Key::Period, "."),
    (Key::OpenBracket, "["),
    (Key::CloseBracket, "]"),
    (Key::OpenCurlyBracket, "{"),
    (Key::CloseCurlyBracket, "}"),
    (Key::Equals, "="),
    (Key::Minus, "-"),
    (Key::Plus, "+"),
    (Key::Questionmark, "?"),
    (Key::Slash, "/"),
    (Key::Backslash, "\\"),
    (Key::Semicolon, ";"),
    (Key::Colon, ":"),
    (Key::Quote, "'"),
    (Key::Backtick, "`"),
    (Key::Exclamationmark, "!"),
    (Key::Pipe, "|"),
];

/// Named keys whose DOM name (happypaint's token) differs from egui's.
const NAMED: &[(Key, &str)] =
    &[(Key::ArrowLeft, "arrowleft"), (Key::ArrowRight, "arrowright"), (Key::ArrowUp, "arrowup"), (Key::ArrowDown, "arrowdown")];

fn token_of(key: Key) -> String {
    NAMED.iter().find(|(k, _)| *k == key).map_or_else(|| key.name().to_lowercase(), |(_, n)| n.to_string())
}

/// Combos the app hard-wires (happypaint checks them before the registry):
/// binding them would never fire, so capture refuses them.
pub const RESERVED: &[&str] = &[
    "ctrl+z",
    "ctrl+shift+z",
    "ctrl+y",
    "ctrl+s",
    "ctrl+shift+s",
    "ctrl+o",
    "ctrl+n",
    "ctrl+shift+e",
    "ctrl+d",
    "ctrl+a",
    "ctrl+shift+i",
    "ctrl+c",
    "ctrl+x",
    "ctrl+v",
    "ctrl+j",
    "ctrl+shift+j",
    "ctrl+0",
    "ctrl+1",
    "ctrl+=",
    "ctrl++",
    "ctrl+-",
    "enter",
    "tab",
    "escape",
    "space",
    "backspace",
    "delete",
];

/// A parsed combo: modifiers and the key token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Parsed<'a> {
    ctrl: bool,
    alt: bool,
    shift: bool,
    key: &'a str,
}

/// Strips `ctrl+`, `alt+`, `shift+` prefixes; the rest is the key, so a `+`
/// key parses too (happypaint's `formatCombo` splits on `+` and mangles it).
fn parse(combo: &str) -> Option<Parsed<'_>> {
    let mut p = Parsed { ctrl: false, alt: false, shift: false, key: combo };
    loop {
        if let Some(r) = p.key.strip_prefix("ctrl+").filter(|r| !r.is_empty()) {
            (p.ctrl, p.key) = (true, r);
        } else if let Some(r) = p.key.strip_prefix("alt+").filter(|r| !r.is_empty()) {
            (p.alt, p.key) = (true, r);
        } else if let Some(r) = p.key.strip_prefix("shift+").filter(|r| !r.is_empty()) {
            (p.shift, p.key) = (true, r);
        } else {
            break;
        }
    }
    (!p.key.is_empty()).then_some(p)
}

/// The egui key for a token, and whether shift is implied (`<`, `>`, symbols).
fn key_for(token: &str) -> Option<(Key, Option<bool>)> {
    match token {
        "<" => return Some((Key::Comma, Some(true))),
        ">" => return Some((Key::Period, Some(true))),
        _ => {}
    }
    if let Some((k, _)) = SYMBOLS.iter().find(|(_, s)| *s == token) {
        // `,` and `.` stay unshifted (their shifted forms are `<` / `>`).
        let shift = if matches!(k, Key::Comma | Key::Period) { Some(false) } else { None };
        return Some((*k, shift));
    }
    Key::ALL.iter().copied().find(|k| token_of(*k) == token).map(|k| (k, Some(false)))
}

/// Whether a valid combo string (also used to validate stored overrides).
pub fn is_valid(combo: &str) -> bool {
    parse(combo).and_then(|p| key_for(p.key)).is_some()
}

fn matches(combo: &str, key: Key, m: Modifiers) -> bool {
    let Some(p) = parse(combo) else { return false };
    let Some((k, implied)) = key_for(p.key) else { return false };
    if k != key || (m.ctrl || m.command || m.mac_cmd) != p.ctrl || m.alt != p.alt {
        return false;
    }
    match implied {
        Some(s) => m.shift == (s || p.shift),
        None => true, // symbol keys: shift is however the layout types them
    }
}

/// The combo a key press makes (capture).
pub fn combo_of(key: Key, m: Modifiers) -> String {
    let token = match (key, m.shift) {
        (Key::Comma, true) => "<".to_string(),
        (Key::Period, true) => ">".to_string(),
        _ => SYMBOLS.iter().find(|(k, _)| *k == key).map_or_else(|| token_of(key), |(_, s)| s.to_string()),
    };
    let symbol = SYMBOLS.iter().any(|(k, _)| *k == key) || token == "<" || token == ">";
    let mut s = String::new();
    if m.ctrl || m.command || m.mac_cmd {
        s.push_str("ctrl+");
    }
    if m.alt {
        s.push_str("alt+");
    }
    if m.shift && !symbol {
        s.push_str("shift+");
    }
    s + &token
}

/// A combo for display: `Ctrl + Shift + B` (Cmd on macOS).
pub fn format_combo(combo: &str) -> String {
    let Some(p) = parse(combo) else { return combo.to_string() };
    let mut parts: Vec<String> = Vec::new();
    if p.ctrl {
        parts.push(if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" }.into());
    }
    if p.alt {
        parts.push(if cfg!(target_os = "macos") { "Option" } else { "Alt" }.into());
    }
    if p.shift {
        parts.push("Shift".into());
    }
    let key = match key_for(p.key) {
        Some((k, _)) if p.key.chars().count() > 1 => k.name().to_string(),
        _ => p.key.to_uppercase(),
    };
    parts.push(key);
    parts.join(" + ")
}

/// The user's overrides: action id → combo (replaces that action's defaults).
pub type Overrides = BTreeMap<String, String>;

/// The combos an action answers to.
pub fn combos_for(a: Action, ov: &Overrides) -> Vec<String> {
    match ov.get(a.id()) {
        Some(c) => vec![c.clone()],
        None => a.defaults().iter().map(|s| s.to_string()).collect(),
    }
}

/// The action a combo triggers: the first in registry order.
pub fn action_for(combo_key: Key, m: Modifiers, ov: &Overrides) -> Option<Action> {
    Action::ALL.iter().copied().find(|a| combos_for(*a, ov).iter().any(|c| matches(c, combo_key, m)))
}

/// Actions whose combo another action also uses (the "duplicate" badge).
pub fn duplicated(ov: &Overrides) -> Vec<Action> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for a in Action::ALL {
        for c in combos_for(*a, ov) {
            *seen.entry(c).or_default() += 1;
        }
    }
    Action::ALL.iter().copied().filter(|a| combos_for(*a, ov).iter().any(|c| seen[c] > 1)).collect()
}

/// Take this frame's key presses that trigger an action, in order. Keys the
/// app hard-wires are left alone; while a text field has focus only combos
/// with Ctrl or Alt fire (plain keys are typing).
pub fn take_actions(ctx: &egui::Context, ov: &Overrides, typing: bool) -> Vec<Action> {
    let mut fired = Vec::new();
    ctx.input_mut(|i| {
        i.events.retain(|e| {
            let Event::Key { key, pressed: true, modifiers, .. } = e else { return true };
            let combo = combo_of(*key, *modifiers);
            if RESERVED.contains(&combo.as_str()) || (typing && !(modifiers.command || modifiers.ctrl || modifiers.alt)) {
                return true;
            }
            match action_for(*key, *modifiers, ov) {
                Some(a) => {
                    fired.push(a);
                    false
                }
                None => true,
            }
        });
    });
    fired
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: Modifiers = Modifiers::COMMAND;

    #[test]
    fn defaults_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for a in Action::ALL {
            assert!(!a.defaults().is_empty());
            for d in a.defaults() {
                assert!(is_valid(d), "{d}");
                assert!(seen.insert(*d), "{d} is bound twice");
                assert!(!RESERVED.contains(d), "{d} is reserved");
            }
        }
        let ids: std::collections::HashSet<_> = Action::ALL.iter().map(|a| a.id()).collect();
        assert_eq!(ids.len(), Action::ALL.len());
        assert_eq!((Action::ToggleSettings.category(), Action::ToggleSettings.defaults()[0]), (Category::View, "ctrl+,"));
    }

    #[test]
    fn combos_follow_happypaint() {
        assert_eq!(combo_of(Key::Comma, CTRL), "ctrl+,");
        assert_eq!(combo_of(Key::Comma, Modifiers::NONE), ",");
        assert_eq!(combo_of(Key::Comma, Modifiers::SHIFT), "<");
        assert_eq!(combo_of(Key::B, Modifiers::SHIFT), "shift+b");
        assert_eq!(combo_of(Key::Questionmark, Modifiers::SHIFT), "?");
        assert_eq!(combo_of(Key::ArrowRight, CTRL | Modifiers::ALT), "ctrl+alt+arrowright");
        assert_eq!(format_combo("ctrl+,").replace("Cmd", "Ctrl"), "Ctrl + ,");
        assert_eq!(format_combo("+"), "+");
        assert_eq!(format_combo("ctrl++").replace("Cmd", "Ctrl"), "Ctrl + +");
        assert_eq!(format_combo("shift+arrowleft"), "Shift + Left");
    }

    #[test]
    fn matching_is_exact_and_first_wins() {
        let ov = Overrides::new();
        assert_eq!(action_for(Key::B, Modifiers::NONE, &ov), Some(Action::ToolBrush));
        assert_eq!(action_for(Key::B, Modifiers::SHIFT, &ov), Some(Action::ToolMixer));
        assert_eq!(action_for(Key::Comma, Modifiers::NONE, &ov), Some(Action::AnimPrevFrame));
        assert_eq!(action_for(Key::Comma, Modifiers::SHIFT, &ov), Some(Action::AnimFirstFrame));
        assert_eq!(action_for(Key::Plus, Modifiers::SHIFT, &ov), Some(Action::ZoomIn));
        assert_eq!(action_for(Key::T, CTRL, &ov), Some(Action::ToolMove));
        // An override replaces the defaults; clashes go to the earlier action.
        let mut ov = Overrides::new();
        ov.insert("tool-move".into(), "shift+v".into());
        assert_eq!(action_for(Key::T, CTRL, &ov), None);
        assert_eq!(action_for(Key::V, Modifiers::SHIFT, &ov), Some(Action::ToolMove));
        ov.insert("tool-smudge".into(), "b".into());
        assert_eq!(action_for(Key::B, Modifiers::NONE, &ov), Some(Action::ToolBrush));
        assert_eq!(duplicated(&ov), vec![Action::ToolBrush, Action::ToolSmudge]);
    }
}
