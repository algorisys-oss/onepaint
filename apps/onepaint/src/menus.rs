//! The menu bar's menus (spec 65; happypaint `menu-bar/menu-model.ts`):
//! File, Edit, View, Image, Layer, Select, Adjust, Help, built from the
//! app's state each frame (checks, enabled rows, live key bindings) and run
//! by [`run`]. The keyboard shortcuts share the helpers below.

use crate::keymap::{self, Action};
use crate::layout::{Orientation, PanelId};
use crate::{App, Popover, dock};
use egui::Context;
use onepaint_engine::Command;
use onepaint_filters::Kind;
use onepaint_ui_kit::menu_bar::{Menu, MenuEntry, MenuItem};

macro_rules! cmds {
    ($($v:ident),+ $(,)?) => {
        /// Everything a menu row can do.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Cmd { $($v),+ }

        impl Cmd {
            pub const ALL: &'static [Cmd] = &[$(Cmd::$v),+];
        }
    };
}

cmds! {
    Gallery, NewCanvas, Open, Save, SaveAs, SaveBook, Export, PlaceImage, PasteImage, Paper,
    Undo, Redo, Cut, Copy, Paste, CopyToLayer, CutToLayer, FillForeground, FillBackground, Clear, Settings,
    ZoomIn, ZoomOut, ZoomFit, Zoom100, RotateLeft, RotateRight, RotateReset,
    Grayscale, PrintPreview, PressProfile, PaperWhite, BlackInk, GamutWarning, FillMode,
    ShowGuides, Rulers, EditGuides, Measure, SymmetryVertical, SymmetryHorizontal, MoveSymmetryAxis, Scrollbars,
    PanelLayers, PanelProperties, PanelBrushSettings, PanelReference, PanelAlign, PanelAnimation, PanelPalettes, PanelToolOptions, PanelGuides, PanelText,
    Record, Replay, Timelapse, ExportMp4, ExportGif, ExportPngZip,
    Zen, SideSliders, SlidersLeft, SlidersRight, SlidersTop, SlidersBottom, SlidersFloat, SlidersTools,
    HorizontalToolbar, ResetLayout, Theme,
    CanvasSize, ImageSize, MergeVisible, Flatten,
    NewLayer, NewGroup, Duplicate, Delete, Group, Ungroup, MergeDown, Clip, AlphaLock, Lock, LayerStyle,
    AddMask, MaskFromSelection, InvertMask, DeleteMask, ApplyMask,
    FillSolid, FillGradient, FillPattern, ConvertToPaint,
    SelectAll, Deselect, InvertSelection, Expand, Contract, Smooth,
    SmartFilters, FocusLines, SpeedLines,
    Shortcuts, Guide, Manual, About, CommandPalette,
}

/// Filters and adjustments by their place in [`Kind::ALL`]: row ids from here up.
const KIND_BASE: u32 = 10_000;

/// What an id picked from the bar means.
pub enum Picked {
    Cmd(Cmd),
    Kind(Kind),
}

pub fn decode(id: u32) -> Option<Picked> {
    if id >= KIND_BASE {
        return Kind::ALL.get((id - KIND_BASE) as usize).copied().map(Picked::Kind);
    }
    Cmd::ALL.get(id as usize).copied().map(Picked::Cmd)
}

fn id(c: Cmd) -> u32 {
    Cmd::ALL.iter().position(|x| *x == c).unwrap_or(0) as u32
}

/// The Command palette row's id (the palette leaves itself out).
pub fn palette_id() -> u32 {
    id(Cmd::CommandPalette)
}

fn item(c: Cmd, label: &str) -> MenuItem {
    MenuItem::new(id(c), label)
}

fn row(c: Cmd, label: &str) -> MenuEntry {
    MenuEntry::Item(item(c, label))
}

/// A rebindable action's first binding, as shown in menus.
pub(crate) fn key(app: &App, a: Action) -> String {
    keymap::combos_for(a, &app.settings.keybindings).first().map(|c| keymap::format_combo(c)).unwrap_or_default()
}

/// The selection refine step for Expand / Contract / Smooth.
pub const REFINE_PX: i32 = 4;

/// The menus as the app stands this frame.
pub fn build(app: &App) -> Vec<Menu> {
    use MenuEntry::{Header, Item, Sep};
    let s = &app.session;
    let doc = s.doc();
    let active = s.active().to_string();
    let node = doc.find(&active);
    let layer = doc.layer(&active);
    let group = node.is_some_and(|n| n.is_group());
    let selection = s.selection().is_some();
    let undo = s.can_undo();
    let redo = s.can_redo();
    let v = app.canvas.view_modes();
    let g = s.guides();
    let anim = s.anim_settings().enabled;
    let sliders = app.settings.side_sliders;
    let mode = app.settings.side_sliders_mode;
    let on = |c: Cmd, label: &str, checked: bool| Item(item(c, label).checked(checked));
    let file = vec![
        row(Cmd::Gallery, "New or open drawing…"),
        Item(item(Cmd::NewCanvas, "New canvas…").shortcut("Ctrl+N")),
        Item(item(Cmd::Open, "Open…").shortcut("Ctrl+O")),
        Sep,
        Item(item(Cmd::Save, "Save").shortcut("Ctrl+S")),
        Item(item(Cmd::SaveAs, "Save as…").shortcut("Ctrl+Shift+S")),
        Item(item(Cmd::SaveBook, "Save drawings as book…").enabled(app.tabs.len() > 1)),
        Item(item(Cmd::Export, "Export… (PNG · JPEG · TIFF · PSD)").shortcut("Ctrl+Shift+E")),
        Sep,
        row(Cmd::PlaceImage, "Insert image…"),
        row(Cmd::PasteImage, "Paste image from clipboard"),
        Sep,
        row(Cmd::Paper, "Paper…"),
    ];
    let edit = vec![
        Item(item(Cmd::Undo, "Undo").shortcut("Ctrl+Z").enabled(undo)),
        Item(item(Cmd::Redo, "Redo").shortcut("Ctrl+Shift+Z").enabled(redo)),
        Sep,
        Item(item(Cmd::Cut, "Cut").shortcut("Ctrl+X")),
        Item(item(Cmd::Copy, "Copy").shortcut("Ctrl+C")),
        Item(item(Cmd::Paste, "Paste").shortcut("Ctrl+V")),
        Item(item(Cmd::CopyToLayer, "Copy to new layer").shortcut("Ctrl+J")),
        Item(item(Cmd::CutToLayer, "Cut to new layer").shortcut("Ctrl+Shift+J")),
        Sep,
        Item(item(Cmd::FillForeground, "Fill with foreground colour").shortcut(key(app, Action::FillForeground))),
        Item(item(Cmd::FillBackground, "Fill with background colour").shortcut(key(app, Action::FillBackground))),
        Item(item(Cmd::Clear, "Clear").shortcut("Delete")),
        Sep,
        Item(item(Cmd::CommandPalette, "Command palette…").shortcut(key(app, Action::ToggleCommandPalette))),
        Item(item(Cmd::Settings, "Settings…").shortcut(key(app, Action::ToggleSettings))),
    ];
    let mut view = vec![
        Item(item(Cmd::ZoomIn, "Zoom in").shortcut(key(app, Action::ZoomIn))),
        Item(item(Cmd::ZoomOut, "Zoom out").shortcut(key(app, Action::ZoomOut))),
        Item(item(Cmd::ZoomFit, "Fit to screen").shortcut(key(app, Action::ZoomFit))),
        Item(item(Cmd::Zoom100, "Actual size (100%)").shortcut(key(app, Action::Zoom100))),
        Item(item(Cmd::RotateLeft, "Rotate canvas left").shortcut(key(app, Action::RotateLeft))),
        Item(item(Cmd::RotateRight, "Rotate canvas right").shortcut(key(app, Action::RotateRight))),
        Item(item(Cmd::RotateReset, "Reset rotation").shortcut(key(app, Action::RotateReset))),
        Sep,
        Item(item(Cmd::Grayscale, "Grayscale (value check)").shortcut("Ctrl+Y").checked(v.grayscale)),
        on(Cmd::PrintPreview, "Print preview (CMYK proof)", v.proof),
    ];
    if v.proof {
        let name = crate::press::active(&app.press).name.clone();
        view.push(on(Cmd::PressProfile, &format!("Press: {name}"), app.press.is_some()));
        view.push(on(Cmd::PaperWhite, "Simulate paper white", v.paper_white));
        view.push(on(Cmd::BlackInk, "Simulate black ink", v.black_ink));
    }
    let warn = if v.gamut_warn > 0.0 { format!("Non-printable colours (ΔE > {})", v.gamut_warn) } else { "Non-printable colours".into() };
    view.extend([
        on(Cmd::GamutWarning, &warn, v.gamut_warn > 0.0),
        Item(
            item(Cmd::FillMode, "Fill mode (silhouette)")
                .shortcut(key(app, Action::ToggleFillShape))
                .checked(app.canvas.options.fill_shape),
        ),
        Sep,
        Header("Guides".into()),
        on(Cmd::ShowGuides, "Show guides", g.visible),
        Item(item(Cmd::Rulers, "Rulers").shortcut(key(app, Action::ToggleRulers)).checked(g.rulers)),
        Item(item(Cmd::EditGuides, "Edit guides").shortcut(key(app, Action::ToggleGuidesEdit)).checked(app.canvas.lines_editing)),
        Item(item(Cmd::Measure, "Measure").shortcut(key(app, Action::ToggleMeasure)).checked(app.canvas.measuring)),
        on(Cmd::SymmetryVertical, "Symmetry — vertical", g.symmetry.mode == onepaint_engine::SymmetryMode::Vertical),
        on(Cmd::SymmetryHorizontal, "Symmetry — horizontal", g.symmetry.mode == onepaint_engine::SymmetryMode::Horizontal),
        Item(
            item(Cmd::MoveSymmetryAxis, "Move symmetry axis")
                .shortcut(key(app, Action::ToggleSymmetryMoveAxis))
                .checked(app.canvas.sym_editing)
                .enabled(g.symmetry.is_on()),
        ),
        on(Cmd::PanelGuides, "Guides panel", dock::panel_open(app, PanelId::Guides)),
        on(Cmd::Scrollbars, "Scrollbars", app.settings.scrollbars),
        Sep,
        Header("Panels".into()),
        Item(
            item(Cmd::PanelLayers, "Layers").shortcut(key(app, Action::ToggleLayersPanel)).checked(dock::panel_open(app, PanelId::Layers)),
        ),
        on(Cmd::PanelProperties, "Properties", dock::panel_open(app, PanelId::Properties)),
        Item(
            item(Cmd::PanelBrushSettings, "Brush settings")
                .shortcut(key(app, Action::ToggleBrushPanel))
                .checked(app.popover == Some(Popover::BrushStudio)),
        ),
        Item(item(Cmd::PanelReference, "Reference image").shortcut(key(app, Action::ToggleReference)).checked(app.reference.open)),
        on(Cmd::PanelAlign, "Transform & align", dock::panel_open(app, PanelId::Align)),
        Item(item(Cmd::PanelAnimation, "Animation").shortcut(key(app, Action::AnimToggle)).checked(anim)),
        on(Cmd::PanelPalettes, "Palettes", dock::panel_open(app, PanelId::Palettes)),
        on(Cmd::PanelToolOptions, "Tool options", dock::panel_open(app, PanelId::ToolOptions)),
        on(Cmd::PanelText, "Text", dock::panel_open(app, PanelId::Text)),
        Sep,
        Header("Capture".into()),
        on(Cmd::Record, if s.is_recording() { "Stop recording…" } else { "Record drawing (time-lapse)" }, s.is_recording()),
        row(Cmd::Replay, "Replay recording…"),
        row(Cmd::Timelapse, "Export timelapse…"),
    ]);
    if anim {
        view.extend([
            row(Cmd::ExportMp4, "Export MP4 video…"),
            row(Cmd::ExportGif, "Export animated GIF…"),
            row(Cmd::ExportPngZip, "Export PNG sequence (.zip)…"),
        ]);
    }
    use crate::side_sliders::Mode;
    view.extend([
        Sep,
        Header("Workspace".into()),
        Item(item(Cmd::Zen, "Zen mode (hide all chrome)").shortcut("Tab").checked(app.hide_ui)),
        on(Cmd::SideSliders, "Size & opacity sliders", sliders),
    ]);
    for (c, m, label) in [
        (Cmd::SlidersLeft, Mode::Left, "Sliders: left edge"),
        (Cmd::SlidersRight, Mode::Right, "Sliders: right edge"),
        (Cmd::SlidersTop, Mode::Top, "Sliders: top edge"),
        (Cmd::SlidersBottom, Mode::Bottom, "Sliders: bottom edge"),
        (Cmd::SlidersFloat, Mode::Float, "Sliders: floating"),
        (Cmd::SlidersTools, Mode::Tools, "Sliders: with the tools"),
    ] {
        view.push(Item(item(c, label).checked(sliders && mode == m).enabled(sliders)));
    }
    view.extend([
        on(Cmd::HorizontalToolbar, "Horizontal toolbar", app.layout.toolbar.orientation == Orientation::Horizontal),
        row(Cmd::ResetLayout, "Reset panel layout"),
        row(Cmd::Theme, if app.theme == onepaint_ui_kit::ThemeKind::Dark { "Light theme" } else { "Dark theme" }),
    ]);
    // View's sections open beside it, so the menu stays on screen.
    let view = fold_sections(view);
    let image = vec![
        row(Cmd::CanvasSize, "Canvas size…"),
        row(Cmd::ImageSize, "Image size…"),
        Sep,
        Item(item(Cmd::MergeVisible, "Merge visible to a new layer").shortcut("Ctrl+Shift+Alt+E")),
        row(Cmd::Flatten, "Flatten image"),
    ];
    let has = node.is_some();
    let mask = layer.is_some_and(|l| l.mask.is_some());
    let mut layer_menu = vec![
        row(Cmd::NewLayer, "New layer"),
        row(Cmd::NewGroup, "New group"),
        Item(item(Cmd::Duplicate, if group { "Duplicate group" } else { "Duplicate" }).enabled(has)),
        Item(item(Cmd::Delete, if group { "Delete group" } else { "Delete" }).enabled(has)),
        Sep,
        if group { Item(item(Cmd::Ungroup, "Ungroup").enabled(has)) } else { Item(item(Cmd::Group, "Group").enabled(has)) },
        Item(item(Cmd::MergeDown, "Merge down").enabled(layer.is_some())),
        Sep,
        Item(item(Cmd::Clip, "Clipping mask").checked(layer.is_some_and(|l| l.clipped)).enabled(layer.is_some())),
        Item(item(Cmd::AlphaLock, "Alpha lock").checked(layer.is_some_and(|l| l.alpha_lock)).enabled(layer.is_some())),
        Item(item(Cmd::Lock, "Lock").checked(layer.is_some_and(|l| l.locked)).enabled(layer.is_some())),
        Item(item(Cmd::LayerStyle, "Layer style…").enabled(has)),
        Sep,
        Header("Mask".into()),
    ];
    if mask {
        layer_menu.extend([row(Cmd::InvertMask, "Invert mask"), row(Cmd::ApplyMask, "Apply mask"), row(Cmd::DeleteMask, "Delete mask")]);
    } else {
        layer_menu.extend([
            Item(item(Cmd::AddMask, "Add mask").enabled(layer.is_some())),
            Item(item(Cmd::MaskFromSelection, "Mask from selection").enabled(layer.is_some() && selection)),
        ]);
    }
    layer_menu.extend([
        Sep,
        Header("New fill layer".into()),
        row(Cmd::FillSolid, "Solid colour"),
        row(Cmd::FillGradient, "Gradient"),
        row(Cmd::FillPattern, "Pattern"),
    ]);
    if layer.is_some_and(|l| l.extra.contains_key("texts") || l.extra.contains_key("paths")) {
        layer_menu.extend([Sep, row(Cmd::ConvertToPaint, "Convert to paint layer")]);
    }
    let select = vec![
        Item(item(Cmd::SelectAll, "All").shortcut("Ctrl+A")),
        Item(item(Cmd::Deselect, "Deselect").shortcut("Ctrl+D").enabled(selection)),
        Item(item(Cmd::InvertSelection, "Invert").shortcut("Ctrl+Shift+I").enabled(selection)),
        Sep,
        Item(item(Cmd::Expand, &format!("Expand by {REFINE_PX} px")).enabled(selection)),
        Item(item(Cmd::Contract, &format!("Contract by {REFINE_PX} px")).enabled(selection)),
        Item(item(Cmd::Smooth, &format!("Smooth by {REFINE_PX} px")).enabled(selection)),
    ];
    let mut adjust = Vec::new();
    let smart = s.smart_filters(&active).len();
    if smart > 0 {
        adjust.push(row(Cmd::SmartFilters, &format!("Smart filters on this layer ({smart})")));
        adjust.push(Sep);
    }
    for section in onepaint_filters::SECTIONS {
        let kinds: Vec<MenuEntry> = Kind::ALL
            .iter()
            .enumerate()
            .filter(|(_, k)| k.section() == section)
            .map(|(i, k)| Item(MenuItem::new(KIND_BASE + i as u32, k.label())))
            .collect();
        adjust.push(MenuEntry::Sub((*section).to_string(), kinds));
    }
    adjust.push(MenuEntry::Sub("Effect lines".into(), vec![row(Cmd::FocusLines, "Focus lines"), row(Cmd::SpeedLines, "Speed lines")]));
    let help = vec![
        Item(
            item(Cmd::Shortcuts, "Shortcuts & gestures").shortcut(key(app, Action::ToggleHelp)).checked(app.popover == Some(Popover::Help)),
        ),
        Item(item(Cmd::CommandPalette, "Command palette…").shortcut(key(app, Action::ToggleCommandPalette))),
        row(Cmd::Guide, "Guide"),
        row(Cmd::Manual, "Manual"),
        row(Cmd::About, "About onepaint"),
    ];
    [
        ("File", file),
        ("Edit", edit),
        ("View", view),
        ("Image", image),
        ("Layer", layer_menu),
        ("Select", select),
        ("Adjust", adjust),
        ("Help", help),
    ]
    .into_iter()
    .map(|(title, entries)| Menu { title: title.into(), entries })
    .collect()
}

/// All the menus as one, for narrow windows (happypaint's ☰ sheet): each
/// menu's rows under its title.
pub fn folded(menus: Vec<Menu>) -> Vec<Menu> {
    let mut entries = Vec::new();
    for (i, m) in menus.into_iter().enumerate() {
        if i > 0 {
            entries.push(MenuEntry::Sep);
        }
        entries.push(MenuEntry::Header(m.title));
        entries.extend(m.entries);
    }
    vec![Menu { title: "Menu".into(), entries }]
}

/// Each header and the rows after it, up to the next separator, as a submenu.
fn fold_sections(entries: Vec<MenuEntry>) -> Vec<MenuEntry> {
    let mut out = Vec::new();
    let mut it = entries.into_iter().peekable();
    while let Some(e) = it.next() {
        match e {
            MenuEntry::Header(h) => {
                let mut rows = Vec::new();
                while let Some(next) = it.next_if(|n| !matches!(n, MenuEntry::Sep | MenuEntry::Header(_))) {
                    rows.push(next);
                }
                out.push(MenuEntry::Sub(h, rows));
            }
            other => out.push(other),
        }
    }
    out
}

// ── Shared with the keyboard shortcuts ──

pub fn zoom(app: &mut App, factor: f64) {
    let c = [app.canvas.view.view_size[0] / 2.0, app.canvas.view.view_size[1] / 2.0];
    app.canvas.view.zoom_at(c, factor);
}

pub fn rotate(app: &mut App, deg: Option<f64>) {
    let c = [app.canvas.view.view_size[0] / 2.0, app.canvas.view.view_size[1] / 2.0];
    let by = deg.map_or(-app.canvas.view.rotation, f64::to_radians);
    app.canvas.view.rotate_at(c, by);
}

pub fn swap_colors(app: &mut App) {
    let (fg, bg) = (app.session.color(), app.canvas.options.background);
    app.set_color(bg);
    app.canvas.options.background = fg;
}

pub fn fill_background(app: &mut App) {
    let (fg, bg) = (app.session.color(), app.canvas.options.background);
    let _ = app.session.execute(Command::SetColor { rgba: bg });
    let _ = app.session.execute(Command::FillSelection);
    let _ = app.session.execute(Command::SetColor { rgba: fg });
}

pub fn toggle_rulers(app: &mut App) {
    let mut g = app.session.guides().clone();
    g.rulers = !g.rulers;
    let _ = app.session.execute(Command::SetGuides { guides: Box::new(g) });
}

pub fn toggle_guides_edit(app: &mut App) {
    app.canvas.lines_editing = !app.canvas.lines_editing;
    app.canvas.measuring = false;
}

pub fn toggle_measure(app: &mut App) {
    app.canvas.measuring = !app.canvas.measuring;
    app.canvas.lines_editing = false;
}

pub fn toggle_fill_mode(app: &mut App, now: f64) {
    app.canvas.options.fill_shape = !app.canvas.options.fill_shape;
    let msg = if app.canvas.options.fill_shape { "Fill mode on: brush strokes fill their outline" } else { "Fill mode off" };
    app.notify(msg, now);
}

fn set_symmetry(app: &mut App, mode: onepaint_engine::SymmetryMode) {
    let mut sym = app.session.guides().symmetry;
    sym.mode = if sym.mode == mode { onepaint_engine::SymmetryMode::Off } else { mode };
    let _ = app.session.execute(Command::SetSymmetry { symmetry: sym });
}

fn toggle_panel(app: &mut App, id: PanelId) {
    let open = dock::panel_open(app, id);
    dock::set_panel_open(app, id, !open);
}

fn layer_props(app: &mut App, f: impl FnOnce(&onepaint_doc::Layer, &mut onepaint_engine::LayerProps)) {
    let id = app.session.active().to_string();
    let Some(l) = app.session.doc().layer(&id) else { return };
    let mut props = onepaint_engine::LayerProps::default();
    f(l, &mut props);
    let _ = app.session.execute(Command::SetLayerProps { id, props });
}

fn fill_layer(app: &mut App, kind: Kind) {
    let params = onepaint_filters::default_params(kind);
    let _ = app.session.execute(Command::AddAdjustmentLayer { kind: kind.as_str().to_string(), params });
}

/// Run what the bar picked.
pub fn run(app: &mut App, picked: Picked, ctx: &Context) {
    let now = ctx.input(|i| i.time);
    let cmd = match picked {
        Picked::Kind(k) => {
            crate::adjust::pick(app, k);
            return;
        }
        Picked::Cmd(c) => c,
    };
    let active = app.session.active().to_string();
    let exec = |app: &mut App, c: Command| {
        if let Err(e) = app.session.execute(c) {
            app.notify(e.to_string(), now);
        }
    };
    match cmd {
        Cmd::Gallery => app.popover = Some(Popover::Gallery),
        Cmd::NewCanvas => app.popover = Some(Popover::NewDoc),
        Cmd::Open => app.request(crate::Pending::Open, ctx),
        Cmd::Save => app.save(false, now),
        Cmd::SaveAs => app.save(true, now),
        Cmd::SaveBook => app.save_book(now),
        Cmd::Export => app.popover = Some(Popover::Export),
        Cmd::PlaceImage => app.place_dialog(ctx),
        Cmd::PasteImage | Cmd::Paste => crate::clip::paste(app),
        Cmd::Paper => app.popover = Some(Popover::Paper),
        Cmd::Undo => exec(app, Command::Undo),
        Cmd::Redo => exec(app, Command::Redo),
        Cmd::Cut => crate::clip::cut(app),
        Cmd::Copy => {
            crate::clip::copy(app);
        }
        Cmd::CopyToLayer => exec(app, Command::LayerViaCopy { cut: false }),
        Cmd::CutToLayer => exec(app, Command::LayerViaCopy { cut: true }),
        Cmd::FillForeground => exec(app, Command::FillSelection),
        Cmd::FillBackground => fill_background(app),
        Cmd::Clear => exec(app, Command::ClearSelected),
        Cmd::Settings => app.popover = Some(Popover::Settings),
        Cmd::ZoomIn => zoom(app, 1.25),
        Cmd::ZoomOut => zoom(app, 0.8),
        Cmd::ZoomFit => app.canvas.view.fit(),
        Cmd::Zoom100 => app.canvas.view.actual_pixels(),
        Cmd::RotateLeft => rotate(app, Some(-15.0)),
        Cmd::RotateRight => rotate(app, Some(15.0)),
        Cmd::RotateReset => rotate(app, None),
        Cmd::Grayscale | Cmd::PrintPreview | Cmd::PaperWhite | Cmd::BlackInk | Cmd::GamutWarning => {
            let mut v = app.canvas.view_modes();
            match cmd {
                Cmd::Grayscale => v.grayscale = !v.grayscale,
                Cmd::PrintPreview => v.proof = !v.proof,
                Cmd::PaperWhite => v.paper_white = !v.paper_white,
                Cmd::BlackInk => v.black_ink = !v.black_ink,
                // Off → ΔE > 5 → ΔE > 2 → off.
                _ => v.gamut_warn = [5.0, 2.0, 0.0][[0.0f32, 5.0, 2.0].iter().position(|w| *w == v.gamut_warn).unwrap_or(2)],
            }
            app.canvas.set_view_modes(v);
        }
        Cmd::PressProfile => app.toggle_press_profile(now),
        Cmd::FillMode => toggle_fill_mode(app, now),
        Cmd::ShowGuides => {
            let mut g = app.session.guides().clone();
            g.visible = !g.visible;
            exec(app, Command::SetGuides { guides: Box::new(g) });
        }
        Cmd::Rulers => toggle_rulers(app),
        Cmd::EditGuides => toggle_guides_edit(app),
        Cmd::Measure => toggle_measure(app),
        Cmd::SymmetryVertical => set_symmetry(app, onepaint_engine::SymmetryMode::Vertical),
        Cmd::SymmetryHorizontal => set_symmetry(app, onepaint_engine::SymmetryMode::Horizontal),
        Cmd::MoveSymmetryAxis => app.canvas.sym_editing = !app.canvas.sym_editing,
        Cmd::Scrollbars => {
            app.settings.scrollbars = !app.settings.scrollbars;
            app.settings.save();
        }
        Cmd::PanelLayers => dock::toggle(app, Popover::Layers, ctx.content_rect()),
        Cmd::PanelProperties | Cmd::LayerStyle => toggle_panel(app, PanelId::Properties),
        Cmd::PanelBrushSettings => {
            app.popover = if app.popover == Some(Popover::BrushStudio) { None } else { Some(Popover::BrushStudio) };
        }
        Cmd::PanelReference => app.reference.open = !app.reference.open,
        Cmd::PanelAlign => toggle_panel(app, PanelId::Align),
        Cmd::PanelAnimation => crate::timeline::toggle(app),
        Cmd::PanelPalettes => toggle_panel(app, PanelId::Palettes),
        Cmd::PanelToolOptions => toggle_panel(app, PanelId::ToolOptions),
        Cmd::PanelText => toggle_panel(app, PanelId::Text),
        Cmd::PanelGuides => dock::toggle(app, Popover::Guides, ctx.content_rect()),
        Cmd::Record => app.toggle_recording(now),
        Cmd::Replay => app.replay_recording(now),
        Cmd::Timelapse => app.export_timelapse(now),
        Cmd::ExportMp4 => app.export_animation(crate::AnimFormat::Mp4, false, now),
        Cmd::ExportGif => app.export_animation(crate::AnimFormat::Gif, false, now),
        Cmd::ExportPngZip => app.export_animation(crate::AnimFormat::PngZip, false, now),
        Cmd::Zen => app.hide_ui = !app.hide_ui,
        Cmd::SideSliders => {
            app.settings.side_sliders = !app.settings.side_sliders;
            app.settings.save();
        }
        Cmd::SlidersLeft | Cmd::SlidersRight | Cmd::SlidersTop | Cmd::SlidersBottom | Cmd::SlidersFloat | Cmd::SlidersTools => {
            use crate::side_sliders::Mode;
            app.settings.side_sliders_mode = match cmd {
                Cmd::SlidersLeft => Mode::Left,
                Cmd::SlidersRight => Mode::Right,
                Cmd::SlidersTop => Mode::Top,
                Cmd::SlidersBottom => Mode::Bottom,
                Cmd::SlidersFloat => Mode::Float,
                _ => Mode::Tools,
            };
            app.settings.save();
        }
        Cmd::HorizontalToolbar => {
            let o = &mut app.layout.toolbar.orientation;
            *o = if *o == Orientation::Horizontal { Orientation::Vertical } else { Orientation::Horizontal };
            app.layout.save();
        }
        Cmd::ResetLayout => {
            app.layout.reset();
            app.layout.save();
            app.notify("Panel layout reset", now);
        }
        Cmd::Theme => app.toggle_theme(ctx),
        Cmd::CanvasSize | Cmd::ImageSize => {
            app.size_dialog = ([app.session.doc().width, app.session.doc().height], (1, 1), true);
            app.popover = Some(if cmd == Cmd::CanvasSize { Popover::CanvasSize } else { Popover::ImageSize });
        }
        Cmd::MergeVisible => exec(app, Command::MergeVisible),
        Cmd::Flatten => exec(app, Command::FlattenImage),
        Cmd::NewLayer => exec(app, Command::AddLayer),
        Cmd::NewGroup => exec(app, Command::AddGroup),
        Cmd::Duplicate => exec(app, Command::DuplicateLayer { id: active }),
        Cmd::Delete => exec(app, Command::DeleteLayer { id: active }),
        Cmd::Group => exec(app, Command::GroupLayer { id: active }),
        Cmd::Ungroup => exec(app, Command::Ungroup { id: active }),
        Cmd::MergeDown => exec(app, Command::MergeDown { id: active }),
        Cmd::Clip => layer_props(app, |l, p| p.clipped = Some(!l.clipped)),
        Cmd::AlphaLock => layer_props(app, |l, p| p.alpha_lock = Some(!l.alpha_lock)),
        Cmd::Lock => layer_props(app, |l, p| p.locked = Some(!l.locked)),
        // A plain mask reveals everything, whatever is selected.
        Cmd::AddMask => exec(app, Command::AddRevealMask),
        Cmd::MaskFromSelection => exec(app, Command::AddMask),
        Cmd::InvertMask => exec(app, Command::InvertMask { id: active }),
        Cmd::ApplyMask => exec(app, Command::ApplyMask { id: active }),
        Cmd::DeleteMask => exec(app, Command::DeleteMask { id: active }),
        Cmd::FillSolid => fill_layer(app, Kind::SolidFill),
        Cmd::FillGradient => fill_layer(app, Kind::GradientFill),
        Cmd::FillPattern => fill_layer(app, Kind::PatternFill),
        Cmd::ConvertToPaint => {
            let text = app.session.doc().layer(&active).is_some_and(|l| l.extra.contains_key("texts"));
            exec(app, if text { Command::RasterizeText { id: active } } else { Command::RasterizePath { id: active } });
        }
        Cmd::SelectAll => exec(app, Command::SelectAll),
        Cmd::Deselect => {
            app.canvas.cancel_poly();
            exec(app, Command::Deselect);
        }
        Cmd::InvertSelection => exec(app, Command::InvertSelection),
        Cmd::Expand => exec(app, Command::GrowSelection { delta: REFINE_PX }),
        Cmd::Contract => exec(app, Command::GrowSelection { delta: -REFINE_PX }),
        Cmd::Smooth => exec(app, Command::SmoothSelection { px: REFINE_PX as f32 }),
        Cmd::SmartFilters => app.popover = Some(Popover::SmartFilters),
        Cmd::FocusLines => crate::adjust::effect_lines(app, true),
        Cmd::SpeedLines => crate::adjust::effect_lines(app, false),
        Cmd::Shortcuts | Cmd::Guide => app.popover = if app.popover == Some(Popover::Help) { None } else { Some(Popover::Help) },
        Cmd::Manual => app.popover = Some(Popover::Manual),
        Cmd::About => app.popover = Some(Popover::Help),
        Cmd::CommandPalette => crate::palette::toggle(app, ctx),
    }
}
