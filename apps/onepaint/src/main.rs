//! onepaint desktop app: a thin egui shell over `onepaint_engine::Session`.

mod adjust;
mod align_ui;

/// The animation export formats (happypaint `AnimFormat`, less WebM).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimFormat {
    Gif,
    Mp4,
    PngZip,
}
mod autosave;
mod brush_library;
mod brush_studio;
mod canvas;
mod chrome;
mod clip;
mod debug;
mod dock;
mod gallery;
mod gestures;
mod help;
mod keymap;
mod layout;
mod palettes;
mod panel_ui;
mod pen;
mod press;
mod progressive;
mod reference;
mod settings;
mod status_bar;
mod tablet;
mod text_ui;
mod thumbs;
mod timeline;
mod timing;
mod toolbar;
mod track_grid;
#[cfg(test)]
mod ui_tests;

use canvas::Canvas;
use egui::{Key, KeyboardShortcut, Modifiers};
use keymap::Action;
use onepaint_color::Rgba;
use onepaint_engine::{Command, Event, Session, Tool};
use onepaint_ui_kit::{ThemeKind, widgets::Hsv};
use std::path::PathBuf;

fn main() -> eframe::Result {
    env_logger::init();
    let open = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("onepaint")
            .with_inner_size([1440.0, 920.0])
            .with_min_inner_size([720.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native("onepaint", options, Box::new(move |cc| Ok(Box::new(App::new(cc, open)))))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popover {
    Menu,
    Brushes,
    Layers,
    Color,
    NewDoc,
    Confirm,
    Selection,
    Fill,
    CanvasSize,
    ImageSize,
    Recover,
    Adjustments,
    Filter,
    AdjustLayer,
    GradientOptions,
    ShapeOptions,
    SmudgeOptions,
    MixerOptions,
    Guides,
    LiquifyOptions,
    PenOptions,
    PanelOptions,
    Export,
    SmartFilters,
    MoveOptions,
    Help,
    Gallery,
    Settings,
    Paper,
    /// Three-finger swipe down: copy, cut, paste.
    ClipboardMenu,
    BrushStudio,
    /// The status bar's "…" details on narrow windows.
    StatusMore,
}

/// What a file opens as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Native,
    Psd,
    Book,
    Image,
}

/// How a file opens, by extension: `.happypaint`, PSD/PSB, or any other image.
pub fn kind_of(p: &std::path::Path) -> FileKind {
    match p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("happypaint") => FileKind::Native,
        Some("psd" | "psb") => FileKind::Psd,
        Some("happypaint-book") => FileKind::Book,
        _ => FileKind::Image,
    }
}

/// An action that would discard unsaved work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    New(u32, u32),
    Open,
    Close,
    /// Close the active drawing's tab.
    CloseTab,
}

/// A drawing open in a background tab.
pub struct DocSlot {
    session: Session,
    canvas: Canvas,
    path: Option<PathBuf>,
    saved_history_len: Option<usize>,
    playback: timeline::Playback,
    replaying: Option<(Vec<onepaint_engine::recording::RecEvent>, usize)>,
    thumbs: thumbs::Thumbs,
    xform_layer: Option<String>,
    /// When this tab was last in front (for the open-tab cap).
    last_active: std::time::Instant,
}

impl DocSlot {
    fn blank(w: u32, h: u32) -> Self {
        Self {
            session: Session::new(w, h, Some(Rgba::WHITE)),
            canvas: Canvas::new([w, h]),
            path: None,
            saved_history_len: Some(0),
            playback: Default::default(),
            replaying: None,
            thumbs: Default::default(),
            xform_layer: None,
            last_active: std::time::Instant::now(),
        }
    }

    fn name(&self) -> &str {
        &self.session.doc().name
    }

    fn dirty(&self) -> bool {
        self.saved_history_len != Some(self.session.history_len())
    }
}

pub struct App {
    /// Open drawings; the active one's slot is `None` (its state lives in the fields below).
    pub tabs: Vec<Option<DocSlot>>,
    pub active_tab: usize,
    pub session: Session,
    pub canvas: Canvas,
    pub theme: ThemeKind,
    pub popover: Option<Popover>,
    pub hide_ui: bool,
    pub path: Option<PathBuf>,
    pub saved_history_len: Option<usize>,
    pub hsv: Hsv,
    pub hex: String,
    pub recent: Vec<Rgba>,
    pub palettes: palettes::Palettes,
    pub settings: settings::Settings,
    /// Layer panel Bevel / Soften sliders (not saved, as in happypaint).
    pub bevel: [f32; 2],
    /// The New canvas dialog's paper preset (index into `PAPER_PRESETS`).
    pub new_paper: usize,
    /// Frame selection and out-of-pegs mode (the timeline's view state).
    pub timing: timing::Timing,
    /// A loaded CMYK press profile; the built-in press when none.
    pub press: Option<std::sync::Arc<onepaint_color::cmyk::Press>>,
    /// Settings ▸ Keyboard shortcuts: the action waiting for a key press.
    pub capture: Option<Action>,
    pub playback: timeline::Playback,
    pub reference: reference::ReferenceState,
    pub gallery: gallery::Gallery,
    pub export_opts: onepaint_formats::export::ExportOptions,
    pub export_psd: bool,
    /// The smart filter whose params are open.
    pub smart_edit: Option<usize>,
    /// A recording playing back on the canvas: events and position.
    pub replaying: Option<(Vec<onepaint_engine::recording::RecEvent>, usize)>,
    /// End live-edit coalescing before the next smart-filter change.
    pub session_seal: bool,
    /// Inline rename of the active palette in progress.
    pub palette_rename: Option<String>,
    pub thumbs: thumbs::Thumbs,
    pub toast: Option<(String, f64)>,
    pub new_doc: [u32; 2],
    pub gpu_ok: bool,
    /// The canvas composites on the GPU (else on the CPU).
    pub gpu_compose: bool,
    /// Live wet media steps on the GPU when there is one (spec 35 B).
    wet_gpu: Option<onepaint_gpu::wet::SharedWetGpu>,
    /// The pen tablet (Wayland tablet protocol), when the platform has one.
    tablet: Option<onepaint_input::tablet::Tablet>,
    /// The tool to go back to when the pen's eraser end leaves.
    eraser_from: Option<Tool>,
    pub debug: debug::Debug,
    title: String,
    pub pending: Option<Pending>,
    pub last_select_tool: Tool,
    pub last_symmetry: onepaint_engine::SymmetryMode,
    xform_layer: Option<String>,
    pub clipboard: clip::Clipboard,
    /// Layer being dragged in the layers panel.
    pub layer_drag: Option<String>,
    /// A colour being dragged onto the canvas (ColorDrop).
    pub color_drop: Option<status_bar::ColorDropDrag>,
    /// Where the tool bar and the panels live (saved as `layout.json`).
    pub layout: layout::Layout,
    pub dock: dock::DockUi,
    pub toolbar: toolbar::ToolbarUi,
    /// Layers picked for align & distribute (Ctrl/⌘- and Shift-click).
    pub layer_picks: Vec<String>,
    pub pick_anchor: Option<String>,
    /// Align to the selection's bounds rather than the canvas.
    pub align_to_selection: bool,
    /// Canvas/image size dialog state: size, anchor (col, row), keep aspect.
    pub size_dialog: ([u32; 2], (u8, u8), bool),
    pub autosave: autosave::Autosave,
    /// A leftover autosave offered for recovery at launch.
    pub recover: Option<PathBuf>,
    /// Selected tab in the curves editor.
    pub curve_tab: usize,
    /// Category shown in the brush library.
    pub brush_category: String,
    /// Imported brushes and where they persist.
    pub brush_library: brush_library::BrushLibrary,
    /// Brush studio state: try-it pad, preview, search.
    pub studio: brush_studio::Studio,
    /// A file chosen by drag-and-drop, opened once unsaved work is dealt with.
    pub open_later: Option<PathBuf>,
    pub(crate) allow_close: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext, open: Option<PathBuf>) -> Self {
        let debug = debug::Debug::from_env();
        let scripted = debug.scripted();
        let theme = if debug.light { ThemeKind::Light } else { ThemeKind::Dark };
        onepaint_ui_kit::install(&cc.egui_ctx, theme);
        let (gpu_ok, gpu_compose) = canvas::init_gpu(cc);
        let wet_gpu = canvas::init_wet_gpu(cc);
        let tablet = tablet::connect(cc);
        let session = Session::new(2048, 1536, Some(Rgba::WHITE));
        let size = [session.doc().width, session.doc().height];
        // Tests and scripted runs never touch the user's saved palettes.
        let palettes = palettes::Palettes::load(if cfg!(test) || debug.scripted() { None } else { palettes::default_path() });
        // Nor the user's imported brushes.
        let brush_library =
            brush_library::BrushLibrary::load(if cfg!(test) || debug.scripted() { None } else { brush_library::default_dir() });
        let mut app = Self {
            tabs: vec![None],
            active_tab: 0,
            session,
            canvas: Canvas::new(size),
            theme,
            popover: None,
            hide_ui: false,
            path: None,
            saved_history_len: Some(0),
            hsv: Hsv { h: 0.0, s: 0.0, v: 0.0 },
            hex: "#000000".into(),
            recent: Vec::new(),
            palettes,
            bevel: [0.0, 0.25],
            new_paper: 0,
            press: None,
            timing: Default::default(),
            capture: None,
            settings: settings::Settings::load(if cfg!(test) || debug.scripted() { None } else { settings::default_path() }),
            playback: Default::default(),
            reference: Default::default(),
            gallery: gallery::Gallery::load(if cfg!(test) || debug.scripted() { None } else { gallery::default_path() }),
            export_opts: Default::default(),
            export_psd: false,
            smart_edit: None,
            replaying: None,
            session_seal: false,
            palette_rename: None,
            thumbs: thumbs::Thumbs::default(),
            toast: None,
            new_doc: [2048, 1536],
            gpu_ok,
            gpu_compose,
            wet_gpu,
            tablet,
            eraser_from: None,
            debug,
            title: String::new(),
            pending: None,
            last_select_tool: Tool::SelectRect,
            last_symmetry: onepaint_engine::SymmetryMode::Vertical,
            xform_layer: None,
            clipboard: clip::Clipboard::default(),
            layer_drag: None,
            color_drop: None,
            layout: layout::Layout::load(if cfg!(test) || scripted { None } else { layout::default_path() }),
            dock: Default::default(),
            toolbar: Default::default(),
            layer_picks: Vec::new(),
            pick_anchor: None,
            align_to_selection: false,
            size_dialog: ([0, 0], (1, 1), true),
            autosave: autosave::Autosave::new(if cfg!(test) { None } else { autosave::dir() }),
            recover: None,
            curve_tab: 0,
            brush_category: String::new(),
            brush_library,
            studio: brush_studio::Studio::default(),
            open_later: None,
            allow_close: false,
        };
        app.press = press::restore(press::saved_path(app.settings.path.as_deref()).as_deref());
        app.canvas.set_press(app.press.clone());
        if let Some(p) = open {
            app.open_path(p);
        } else if !app.debug.scripted()
            && let Some(p) = app.autosave.leftover()
        {
            app.recover = Some(p);
            app.popover = Some(Popover::Recover);
        } else if !app.debug.scripted() && !cfg!(test) && !app.gallery.recent.is_empty() {
            app.popover = Some(Popover::Gallery);
        }
        app
    }

    pub fn dirty(&self) -> bool {
        self.saved_history_len != Some(self.session.history_len())
    }

    pub fn notify(&mut self, msg: impl Into<String>, now: f64) {
        self.toast = Some((msg.into(), now + 2.5));
    }

    pub fn set_color(&mut self, c: Rgba) {
        let _ = self.session.execute(Command::SetColor { rgba: c });
        self.hsv = Hsv::from_rgba(c);
        self.hex = c.to_hex();
    }

    /// Remember the colour that was used for a stroke.
    pub fn push_recent(&mut self) {
        self.push_recent_color(self.session.color());
    }

    pub fn push_recent_color(&mut self, c: Rgba) {
        if self.recent.first() != Some(&c) {
            self.recent.retain(|r| r.to_hex() != c.to_hex());
            self.recent.insert(0, c);
            self.recent.truncate(12);
        }
    }

    pub fn new_document(&mut self, w: u32, h: u32) {
        let id = self.session.doc().id.clone();
        self.autosave.clear(&id);
        let mut doc = onepaint_doc::Document::new(w, h, Some(Rgba::WHITE));
        doc.set_paper(onepaint_doc::PAPER_PRESETS.get(self.new_paper).and_then(|p| p.paper()));
        self.session.replace_document(doc);
        self.canvas.reset_for([w, h]);
        self.path = None;
        self.saved_history_len = Some(0);
        self.thumbs.clear();
    }

    /// Open every page of a `.happypaint-book` as a tab.
    pub fn open_book(&mut self, p: PathBuf) {
        let now = 0.0;
        let decoded =
            std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| onepaint_formats::book::decode_book(&b).map_err(|e| e.to_string()));
        let (manifest, pages) = match decoded {
            Ok(d) => d,
            Err(e) => return self.notify(format!("Couldn't open the book: {e}"), now),
        };
        for (i, bytes) in pages.iter().enumerate() {
            if i > 0 || !self.pristine() {
                self.new_tab();
            }
            match self.session.open_happypaint(bytes) {
                Ok(()) => {
                    let d = self.session.doc();
                    self.canvas.reset_for([d.width, d.height]);
                    self.saved_history_len = Some(0);
                    self.thumbs.clear();
                }
                Err(e) => self.notify(format!("Page {}: {e}", i + 1), now),
            }
        }
        self.gallery.touch(&p);
        self.notify(format!("Opened “{}” ({} pages)", manifest.name, pages.len()), now);
    }

    /// File ▸ Save drawings as book…: every open tab, in order.
    pub fn save_book(&mut self, now: f64) {
        let Some(path) =
            rfd::FileDialog::new().add_filter("happypaint book", &["happypaint-book"]).set_file_name("Chapter.happypaint-book").save_file()
        else {
            return;
        };
        let mut names = Vec::new();
        let mut pages = Vec::new();
        for i in 0..self.tabs.len() {
            let r = match self.tabs[i].as_mut() {
                Some(s) => s.session.save_happypaint().map(|b| (s.session.doc().name.clone(), b)),
                None => self.session.save_happypaint().map(|b| (self.session.doc().name.clone(), b)),
            };
            match r {
                Ok((n, b)) => {
                    names.push(n);
                    pages.push(b);
                }
                Err(e) => return self.notify(format!("Save failed: {e}"), now),
            }
        }
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Chapter").to_string();
        let bytes = onepaint_formats::book::encode_book(&name, &names, &pages, concat!("onepaint ", env!("CARGO_PKG_VERSION")));
        match bytes.map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.gallery.touch(&path);
                self.notify(format!("Saved {} pages to {}", names.len(), path.display()), now);
            }
            Err(e) => self.notify(format!("Save failed: {e}"), now),
        }
    }

    pub fn open_path(&mut self, p: PathBuf) {
        let now = 0.0;
        if kind_of(&p) == FileKind::Book {
            return self.open_book(p);
        }
        let kind = kind_of(&p);
        let native = kind == FileKind::Native;
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Image").to_string();
        match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| {
            match kind {
                FileKind::Native => self.session.open_happypaint(&b),
                FileKind::Psd => self.session.open_psd(&b, &name),
                FileKind::Image | FileKind::Book => self.session.open_image(&b, &name),
            }
            .map_err(|e| e.to_string())
        }) {
            Ok(()) => {
                let d = self.session.doc();
                self.canvas.reset_for([d.width, d.height]);
                self.gallery.touch(&p);
                // An imported image saves as a new .happypaint, never over the source file.
                self.path = native.then_some(p);
                self.saved_history_len = Some(0);
                self.thumbs.clear();
            }
            Err(e) => self.notify(format!("Couldn't open {}: {e}", p.display()), now),
        }
    }

    fn swap_slot(&mut self, s: &mut DocSlot) {
        std::mem::swap(&mut self.session, &mut s.session);
        std::mem::swap(&mut self.canvas, &mut s.canvas);
        std::mem::swap(&mut self.path, &mut s.path);
        std::mem::swap(&mut self.saved_history_len, &mut s.saved_history_len);
        std::mem::swap(&mut self.playback, &mut s.playback);
        std::mem::swap(&mut self.replaying, &mut s.replaying);
        std::mem::swap(&mut self.thumbs, &mut s.thumbs);
        std::mem::swap(&mut self.xform_layer, &mut s.xform_layer);
        s.last_active = std::time::Instant::now();
    }

    /// Bring tab `i` to the front, keeping the current tools.
    pub fn switch_tab(&mut self, i: usize) {
        if i == self.active_tab || i >= self.tabs.len() {
            return;
        }
        let _ = self.session.execute(Command::CommitTransform);
        let Some(mut s) = self.tabs[i].take() else { return };
        s.session.carry_tools_from(&self.session);
        self.swap_slot(&mut s);
        self.tabs[self.active_tab] = Some(s);
        self.active_tab = i;
        self.popover = None;
    }

    /// Open a blank tab after the current one and switch to it.
    pub fn new_tab(&mut self) {
        let _ = self.session.execute(Command::CommitTransform);
        let mut s = DocSlot::blank(2048, 1536);
        s.session.carry_tools_from(&self.session);
        self.swap_slot(&mut s);
        self.tabs[self.active_tab] = Some(s);
        let at = self.active_tab + 1;
        self.tabs.insert(at, None);
        self.active_tab = at;
        self.enforce_tab_cap();
    }

    /// Opening past the cap closes the least recently used saved tab
    /// (happypaint closes the oldest tab; unsaved drawings are kept here).
    fn enforce_tab_cap(&mut self) {
        while self.tabs.len() > self.settings.max_open_tabs {
            let victim = self
                .tabs
                .iter()
                .enumerate()
                .filter_map(|(i, t)| t.as_ref().filter(|s| !s.dirty()).map(|s| (i, s.last_active)))
                .min_by_key(|(_, t)| *t)
                .map(|(i, _)| i);
            let Some(i) = victim else { break };
            if let Some(s) = self.tabs.remove(i) {
                self.autosave.clear(&s.session.doc().id);
            }
            if i < self.active_tab {
                self.active_tab -= 1;
            }
        }
    }

    /// Close the active tab (the last one becomes a fresh drawing).
    pub fn close_tab(&mut self) {
        let id = self.session.doc().id.clone();
        self.autosave.clear(&id);
        if self.tabs.len() == 1 {
            self.new_document(2048, 1536);
            return;
        }
        self.tabs.remove(self.active_tab);
        let i = self.active_tab.min(self.tabs.len() - 1);
        if let Some(mut s) = self.tabs[i].take() {
            s.session.carry_tools_from(&self.session);
            self.swap_slot(&mut s);
        }
        self.active_tab = i;
    }

    /// Tab titles with their unsaved flag.
    pub fn tab_titles(&self) -> Vec<(String, bool)> {
        self.tabs
            .iter()
            .map(|t| match t {
                Some(s) => (s.name().to_string(), s.dirty()),
                None => (self.session.doc().name.clone(), self.dirty()),
            })
            .collect()
    }

    pub fn any_dirty(&self) -> bool {
        self.dirty() || self.tabs.iter().flatten().any(DocSlot::dirty)
    }

    /// The current drawing is untouched and untitled (a new one can reuse it).
    fn pristine(&self) -> bool {
        self.path.is_none() && self.session.history_len() == 0 && !self.dirty()
    }

    /// Run `action`, or ask first when there are unsaved changes.
    pub fn request(&mut self, action: Pending, ctx: &egui::Context) {
        // New and Open never discard: they go to a new tab unless this one is blank.
        if matches!(action, Pending::New(..) | Pending::Open) {
            if !self.pristine() {
                self.new_tab();
            }
            return self.perform(action, ctx);
        }
        let dirty = if action == Pending::Close { self.any_dirty() } else { self.dirty() };
        if dirty {
            self.pending = Some(action);
            self.popover = Some(Popover::Confirm);
        } else {
            self.perform(action, ctx);
        }
    }

    pub fn perform(&mut self, action: Pending, ctx: &egui::Context) {
        self.pending = None;
        // The user saved or chose to discard: the current autosave is obsolete.
        let id = self.session.doc().id.clone();
        self.autosave.clear(&id);
        self.popover = None;
        match action {
            Pending::New(w, h) => self.new_document(w, h),
            Pending::Open => match self.open_later.take() {
                Some(p) => self.open_path(p),
                None => self.open_dialog(),
            },
            Pending::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::CloseTab => self.close_tab(),
        }
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(
                "Images and documents",
                &[
                    "happypaint",
                    "happypaint-book",
                    "psd",
                    "psb",
                    "pbm",
                    "pgm",
                    "ppm",
                    "pam",
                    "pnm",
                    "png",
                    "jpg",
                    "jpeg",
                    "webp",
                    "bmp",
                    "gif",
                    "tif",
                    "tiff",
                ],
            )
            .pick_file()
        {
            self.open_path(p);
        }
    }

    pub fn place_dialog(&mut self, ctx: &egui::Context) {
        if let Some(p) =
            rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"]).pick_file()
        {
            clip::drop_file(self, ctx, Some(p), None);
        }
    }

    pub fn save(&mut self, as_new: bool, now: f64) {
        let path = match (&self.path, as_new) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("happypaint", &["happypaint"])
                .set_file_name(format!("{}.happypaint", self.session.doc().name))
                .save_file(),
        };
        let Some(path) = path else { return };
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            self.session.doc_mut_meta().name = stem.to_string();
        }
        match self.session.save_happypaint().map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.saved_history_len = Some(self.session.history_len());
                let id = self.session.doc().id.clone();
                self.autosave.clear(&id);
                self.notify(format!("Saved {}", path.display()), now);
                self.gallery.touch(&path);
                self.path = Some(path);
            }
            Err(e) => self.notify(format!("Save failed: {e}"), now),
        }
    }

    /// File ▸ Start / Stop recording (spec 26).
    pub fn toggle_recording(&mut self, now: f64) {
        if !self.session.is_recording() {
            match self.session.start_recording() {
                Ok(()) => self.notify("Recording", now),
                Err(e) => self.notify(format!("Couldn't start recording: {e}"), now),
            }
            return;
        }
        let Some(rec) = self.session.stop_recording() else { return };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("onepaint recording", &["onepaint-rec"])
            .set_file_name(format!("{}.onepaint-rec", self.session.doc().name))
            .save_file()
        else {
            return;
        };
        match rec.to_bytes().map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => self.notify(format!("Saved {}", path.display()), now),
            Err(e) => self.notify(format!("Save failed: {e}"), now),
        }
    }

    fn pick_recording(&mut self, now: f64) -> Option<onepaint_engine::recording::Recording> {
        let path = rfd::FileDialog::new().add_filter("onepaint recording", &["onepaint-rec"]).pick_file()?;
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| onepaint_engine::recording::Recording::from_bytes(&b).map_err(|e| e.to_string()))
        {
            Ok(r) => Some(r),
            Err(e) => {
                self.notify(format!("Couldn't open the recording: {e}"), now);
                None
            }
        }
    }

    /// Play a recording on the canvas (replaces the document, asks first if dirty).
    pub fn replay_recording(&mut self, now: f64) {
        let Some(rec) = self.pick_recording(now) else { return };
        match onepaint_engine::recording::Replay::new(&rec) {
            Ok(r) => {
                let (session, events) = r.into_parts();
                self.session = session;
                let d = self.session.doc();
                self.canvas.reset_for([d.width, d.height]);
                self.path = None;
                self.saved_history_len = None;
                self.thumbs.clear();
                self.replaying = Some((events, 0));
            }
            Err(e) => self.notify(format!("Couldn't replay: {e}"), now),
        }
    }

    pub fn export_timelapse(&mut self, now: f64) {
        let Some(rec) = self.pick_recording(now) else { return };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MP4 video", &["mp4"])
            .add_filter("Animated GIF", &["gif"])
            .set_file_name("timelapse.mp4")
            .save_file()
        else {
            return;
        };
        let gif = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("gif"));
        let bytes = onepaint_engine::recording::Replay::new(&rec).and_then(|r| if gif { r.timelapse_gif() } else { r.timelapse_mp4() });
        match bytes.map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => self.notify(format!("Exported {}", path.display()), now),
            Err(e) => self.notify(format!("Export failed: {e}"), now),
        }
    }

    /// Act on recognised touch gestures (Procreate's vocabulary).
    fn gestures(&mut self, ctx: &egui::Context) {
        use gestures::Gesture;
        for g in std::mem::take(&mut self.canvas.gesture_out) {
            match g {
                Gesture::Tap(2) => {
                    let _ = self.session.execute(Command::Undo);
                }
                Gesture::Tap(3) => {
                    let _ = self.session.execute(Command::Redo);
                }
                Gesture::Tap(4) => self.hide_ui = !self.hide_ui,
                Gesture::SwipeDown3 => self.popover = Some(Popover::ClipboardMenu),
                _ => {}
            }
        }
        let _ = ctx;
    }

    /// Advance an on-canvas replay (~4 s for the whole log at 60 fps).
    fn replay_tick(&mut self, ctx: &egui::Context) {
        let Some((events, pos)) = self.replaying.as_mut() else { return };
        let per = (events.len() / 240).max(1);
        let end = (*pos + per).min(events.len());
        for ev in &events[*pos..end] {
            self.session.apply_event(ev);
        }
        *pos = end;
        if *pos >= events.len() {
            self.replaying = None;
            let now = ctx.input(|i| i.time);
            self.notify("Replay finished", now);
        } else {
            ctx.request_repaint();
        }
    }

    /// An animated GIF, an MP4 or a zip of PNG frames (white behind GIF and
    /// video frames); `range_only` exports just the marked play range.
    pub fn export_animation(&mut self, format: AnimFormat, range_only: bool, now: f64) {
        let name = self.session.doc().name.clone();
        let (filter, ext) = match format {
            AnimFormat::Gif => ("Animated GIF", "gif"),
            AnimFormat::Mp4 => ("MP4 video", "mp4"),
            AnimFormat::PngZip => ("PNG sequence", "zip"),
        };
        let Some(path) = rfd::FileDialog::new().add_filter(filter, &[ext]).set_file_name(format!("{name}.{ext}")).save_file() else {
            return;
        };
        let bytes = match format {
            AnimFormat::Gif => self.session.export_gif(Some(Rgba::WHITE), range_only),
            AnimFormat::Mp4 => self.session.export_mp4(None, range_only),
            AnimFormat::PngZip => self.session.export_png_sequence(None, range_only),
        };
        match bytes.map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => self.notify(format!("Exported {}", path.display()), now),
            Err(e) => self.notify(format!("Export failed: {e}"), now),
        }
    }

    pub fn export_psd(&mut self, now: f64) {
        let Some(path) =
            rfd::FileDialog::new().add_filter("Photoshop", &["psd"]).set_file_name(format!("{}.psd", self.session.doc().name)).save_file()
        else {
            return;
        };
        match self.session.export_psd().map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => self.notify(format!("Exported {}", path.display()), now),
            Err(e) => self.notify(format!("Export failed: {e}"), now),
        }
    }

    /// Export in the dialog's format (PSD keeps layers; the rest are flat).
    pub fn export_with(&mut self, opts: onepaint_formats::export::ExportOptions, psd: bool, now: f64) {
        // Wet paint is shown with a damp veil; export what it dries to.
        let _ = self.session.execute(Command::DryWet);
        if psd {
            return self.export_psd(now);
        }
        let ext = opts.format.extension();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(format!("{}.{ext}", self.session.doc().name))
            .save_file()
        else {
            return;
        };
        match self
            .session
            .export_image_with(&opts, press::active(&self.press))
            .map_err(|e| e.to_string())
            .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()))
        {
            Ok(()) => self.notify(format!("Exported {}", path.display()), now),
            Err(e) => self.notify(format!("Export failed: {e}"), now),
        }
    }

    /// View ▸ Press: load a `.icc` / `.icm` press profile, or go back to the
    /// built-in press when one is loaded.
    pub fn toggle_press_profile(&mut self, now: f64) {
        let path = press::saved_path(self.settings.path.as_deref());
        if self.press.take().is_some() {
            press::forget(path.as_deref());
            self.notify("Press: Generic coated (built-in)", now);
        } else {
            let Some(file) = rfd::FileDialog::new().add_filter("ICC profile", &["icc", "icm"]).pick_file() else { return };
            match std::fs::read(&file).map_err(|e| e.to_string()).and_then(|b| press::load(&b, path.as_deref())) {
                Ok(p) => {
                    self.notify(format!("Press profile loaded: {}", p.name), now);
                    self.press = Some(p);
                }
                Err(e) => self.notify(format!("Couldn't use that profile: {e}"), now),
            }
        }
        self.canvas.set_press(self.press.clone());
    }

    pub fn export_png(&mut self, now: f64) {
        let Some(path) =
            rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(format!("{}.png", self.session.doc().name)).save_file()
        else {
            return;
        };
        match self.session.export_png().map_err(|e| e.to_string()).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => self.notify(format!("Exported {}", path.display()), now),
            Err(e) => self.notify(format!("Export failed: {e}"), now),
        }
    }

    /// Start (or restart) a transform session on the active layer.
    pub fn begin_transform(&mut self) {
        self.xform_layer = Some(self.session.active().to_string());
        self.canvas.xform = onepaint_engine::transform::TransformParams::default();
        let _ = self.session.execute(Command::BeginTransform);
    }

    pub fn commit_transform(&mut self) {
        let _ = self.session.execute(Command::CommitTransform);
        if self.session.tool() == Tool::Move {
            self.begin_transform();
        }
    }

    pub fn cancel_transform(&mut self) {
        let _ = self.session.execute(Command::CancelTransform);
        if self.session.tool() == Tool::Move {
            self.begin_transform();
        }
    }

    /// Y: symmetry on/off, restoring the last mode used (vertical at first).
    pub fn toggle_symmetry(&mut self) {
        let mut sym = self.session.guides().symmetry;
        if sym.is_on() {
            self.last_symmetry = sym.mode;
            sym.mode = onepaint_engine::SymmetryMode::Off;
        } else {
            sym.mode = self.last_symmetry;
        }
        let _ = self.session.execute(Command::SetSymmetry { symmetry: sym });
    }

    pub fn set_tool(&mut self, tool: Tool) {
        let was = self.session.tool();
        if tool != Tool::Pen {
            self.canvas.pen.building = None;
        }
        if was == Tool::Move && tool != Tool::Move {
            let _ = self.session.execute(Command::CommitTransform);
        }
        if matches!(tool, Tool::SelectRect | Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly | Tool::SelectWand) {
            self.last_select_tool = tool;
        }
        let _ = self.session.execute(Command::SetTool { tool });
        if tool == Tool::Move && was != Tool::Move {
            self.begin_transform();
        }
        // The Panel tool shows its page layouts as soon as it is picked.
        if tool == Tool::Panel && was != Tool::Panel {
            self.popover = Some(Popover::PanelOptions);
        }
    }

    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.theme = self.theme.toggled();
        onepaint_ui_kit::theme::apply(ctx, self.theme);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) -> (bool, bool) {
        let now = ctx.input(|i| i.time);
        let cmd = Modifiers::COMMAND;
        let sc = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        let typing = ctx.egui_wants_keyboard_input();
        // Settings ▸ Keyboard shortcuts is waiting for a key: it takes the press.
        if self.popover != Some(Popover::Settings) {
            self.capture = None;
        }
        if self.capture.is_some() {
            self.capture_key(ctx, now);
            return (false, false);
        }
        // Rebindable actions (keymap.rs); hard-wired keys are left for below.
        let fired = keymap::take_actions(ctx, &self.settings.keybindings, typing);
        let on = |a: Action| fired.contains(&a);
        if sc(cmd | Modifiers::SHIFT, Key::Z) {
            let _ = self.session.execute(Command::Redo);
        }
        if sc(cmd, Key::Y) {
            // Value check (happypaint: redo is Ctrl+Shift+Z, so Y is free).
            let mut v = self.canvas.view_modes();
            v.grayscale = !v.grayscale;
            self.canvas.set_view_modes(v);
        }
        if sc(cmd, Key::Z) {
            let _ = self.session.execute(Command::Undo);
        }
        if sc(cmd | Modifiers::SHIFT, Key::S) {
            self.save(true, now);
        }
        if sc(cmd, Key::S) {
            self.save(false, now);
        }
        if sc(cmd, Key::O) {
            self.request(Pending::Open, ctx);
        }
        if sc(cmd, Key::N) {
            self.popover = Some(Popover::NewDoc);
        }
        if on(Action::DocNextTab) && self.tabs.len() > 1 {
            self.switch_tab((self.active_tab + 1) % self.tabs.len());
        }
        if on(Action::DocPrevTab) && self.tabs.len() > 1 {
            self.switch_tab((self.active_tab + self.tabs.len() - 1) % self.tabs.len());
        }
        if sc(cmd | Modifiers::SHIFT, Key::E) {
            self.popover = Some(Popover::Export);
        }
        if sc(cmd, Key::Num0) {
            self.canvas.view.fit();
        }
        if sc(cmd, Key::Num1) {
            self.canvas.view.actual_pixels();
        }
        if sc(cmd, Key::Plus) || sc(cmd, Key::Equals) {
            let c = [self.canvas.view.view_size[0] / 2.0, self.canvas.view.view_size[1] / 2.0];
            self.canvas.view.zoom_at(c, 1.25);
        }
        if sc(cmd, Key::Minus) {
            let c = [self.canvas.view.view_size[0] / 2.0, self.canvas.view.view_size[1] / 2.0];
            self.canvas.view.zoom_at(c, 0.8);
        }
        if !typing {
            if sc(cmd | Modifiers::SHIFT, Key::J) {
                let _ = self.session.execute(Command::LayerViaCopy { cut: true });
            }
            if sc(cmd, Key::J) {
                let _ = self.session.execute(Command::LayerViaCopy { cut: false });
            }
            if sc(cmd, Key::C) || ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy))) {
                clip::copy(self);
            }
            if sc(cmd, Key::X) || ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Cut))) {
                clip::cut(self);
            }
            if sc(cmd, Key::V) || ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_)))) {
                clip::paste(self);
            }
            if sc(cmd | Modifiers::SHIFT, Key::I) {
                let _ = self.session.execute(Command::InvertSelection);
            }
            if sc(cmd, Key::A) {
                let _ = self.session.execute(Command::SelectAll);
            }
            if sc(cmd, Key::D) {
                self.canvas.cancel_poly();
                let _ = self.session.execute(Command::Deselect);
            }
            // The pen's path in progress takes Enter, Escape and Backspace.
            if self.canvas.pen.building.is_some() {
                if sc(Modifiers::NONE, Key::Enter) {
                    let fill = self.canvas.options.pen_fill;
                    pen::finish(&mut self.canvas.pen, &mut self.session, false, fill, ctx.input(|i| (i.time * 1000.0) as u32));
                }
                if sc(Modifiers::NONE, Key::Escape) {
                    self.canvas.pen.building = None;
                }
                if sc(Modifiers::NONE, Key::Backspace)
                    && let Some(b) = self.canvas.pen.building.as_mut()
                {
                    b.pop();
                    if b.is_empty() {
                        self.canvas.pen.building = None;
                    }
                }
            } else if self.session.tool() == Tool::Pen
                && (sc(Modifiers::NONE, Key::Delete) || sc(Modifiers::NONE, Key::Backspace))
                && let Some((obj, sub, anchor)) = self.canvas.pen.last.take()
            {
                pen::edit_object(&mut self.session, obj, |s| {
                    if let Some(sp) = s.subpaths.get_mut(sub) {
                        onepaint_stroke::bezier::delete_anchor(sp, anchor);
                    }
                });
                let _ = self.session.execute(Command::SealHistory);
            }
            // Rebindable actions (keymap.rs).
            if on(Action::AnimToggle) {
                timeline::toggle(self);
            }
            if on(Action::ToggleSymmetryMoveAxis) && self.session.guides().symmetry.is_on() {
                self.canvas.sym_editing = !self.canvas.sym_editing;
            }
            if on(Action::ToggleMeasure) {
                self.canvas.measuring = !self.canvas.measuring;
                self.canvas.lines_editing = false;
            }
            if on(Action::ToggleLayersPanel) {
                dock::toggle(self, Popover::Layers, ctx.content_rect());
            }
            if on(Action::ToggleReference) {
                self.reference.open = !self.reference.open;
            }
            if on(Action::ToggleBrushPanel) {
                dock::toggle(self, Popover::Brushes, ctx.content_rect());
            }
            if on(Action::ToggleHelp) {
                self.popover = if self.popover == Some(Popover::Help) { None } else { Some(Popover::Help) };
            }
            if on(Action::ColorSwap) {
                let fg = self.session.color();
                let bg = self.canvas.options.background;
                self.set_color(bg);
                self.canvas.options.background = fg;
            }
            if on(Action::ColorReset) {
                self.set_color(Rgba::BLACK);
                self.canvas.options.background = Rgba::WHITE;
            }
            if on(Action::FillBackground) {
                let (fg, bg) = (self.session.color(), self.canvas.options.background);
                let _ = self.session.execute(Command::SetColor { rgba: bg });
                let _ = self.session.execute(Command::FillSelection);
                let _ = self.session.execute(Command::SetColor { rgba: fg });
            }
            let centre = [self.canvas.view.view_size[0] / 2.0, self.canvas.view.view_size[1] / 2.0];
            if on(Action::ZoomIn) {
                self.canvas.view.zoom_at(centre, 1.25);
            }
            if on(Action::ZoomOut) {
                self.canvas.view.zoom_at(centre, 0.8);
            }
            if on(Action::ZoomFit) {
                self.canvas.view.fit();
            }
            if on(Action::Zoom100) {
                self.canvas.view.actual_pixels();
            }
            if on(Action::RotateReset) {
                let r = self.canvas.view.rotation;
                self.canvas.view.rotate_at(centre, -r);
            }
            if on(Action::RotateLeft) {
                self.canvas.view.rotate_at(centre, -15f64.to_radians());
            }
            if on(Action::RotateRight) {
                self.canvas.view.rotate_at(centre, 15f64.to_radians());
            }
            if self.session.anim_settings().enabled {
                if on(Action::AnimPlayPause) {
                    let playing = self.playback.playing;
                    timeline::set_playing(self, !playing);
                }
                if on(Action::AnimAddFrame) {
                    let _ = self.session.execute(Command::AddFrame);
                }
                if on(Action::AnimToggleOnion) {
                    let mut a = self.session.anim_settings().clone();
                    a.onion_enabled = !a.onion_enabled;
                    let _ = self.session.execute(Command::SetAnimation { settings: Box::new(a) });
                }
                if on(Action::AnimFirstFrame) {
                    let _ = self.session.execute(Command::SetAnimSlot { slot: 0 });
                }
                if on(Action::AnimLastFrame) {
                    let last = self.session.anim_total_slots().saturating_sub(1);
                    let _ = self.session.execute(Command::SetAnimSlot { slot: last });
                }
                if on(Action::AnimAddMarker) {
                    let slot = self.session.anim_slot();
                    timeline::set_settings(self, |a| a.add_marker(slot));
                }
                for (action, delta) in [(Action::AnimPrevMarker, -1), (Action::AnimNextMarker, 1)] {
                    if on(action)
                        && let Some(slot) = self.session.anim_settings().marker_from(self.session.anim_slot(), delta)
                    {
                        let _ = self.session.execute(Command::SetAnimSlot { slot });
                    }
                }
                if on(Action::AnimMarkIn) {
                    let slot = self.session.anim_slot();
                    timeline::set_settings(self, |a| a.mark_in = Some(slot));
                }
                if on(Action::AnimMarkOut) {
                    let slot = self.session.anim_slot();
                    timeline::set_settings(self, |a| a.mark_out = Some(slot));
                }
            }
            if on(Action::ToolPan) {
                self.set_tool(Tool::Hand);
            }
            if on(Action::ToggleFillShape) {
                self.canvas.options.fill_shape = !self.canvas.options.fill_shape;
                let msg = if self.canvas.options.fill_shape { "Fill mode on: brush strokes fill their outline" } else { "Fill mode off" };
                self.notify(msg, now);
            }
            if on(Action::FillForeground) {
                let _ = self.session.execute(Command::FillSelection);
            }
            if sc(Modifiers::NONE, Key::Delete) || sc(Modifiers::NONE, Key::Backspace) {
                let _ = self.session.execute(Command::ClearSelected);
            }
            if on(Action::TogglePerspectiveEdit) && self.session.guides().perspective.enabled {
                self.canvas.persp_editing = !self.canvas.persp_editing;
            }
            if on(Action::ToggleRulers) {
                let mut g = self.session.guides().clone();
                g.rulers = !g.rulers;
                let _ = self.session.execute(Command::SetGuides { guides: Box::new(g) });
            }
            if on(Action::ToggleGuidesEdit) {
                self.canvas.lines_editing = !self.canvas.lines_editing;
                self.canvas.measuring = false;
            }
            for (action, tool) in [
                (Action::ToolSelectRect, Tool::SelectRect),
                (Action::ToolSelectLasso, Tool::SelectLasso),
                (Action::ToolSelectWand, Tool::SelectWand),
                (Action::ToolFill, Tool::Fill),
                (Action::ToolEyedropper, Tool::Eyedropper),
            ] {
                if on(action) {
                    self.set_tool(tool);
                }
            }
            if on(Action::ToggleSettings) {
                self.popover = if self.popover == Some(Popover::Settings) { None } else { Some(Popover::Settings) };
            }
            if on(Action::ToolMixer) {
                self.set_tool(Tool::Mixer);
            }
            if on(Action::ToolBrush) {
                self.set_tool(Tool::Brush);
            }
            if on(Action::ToolSmudge) {
                self.set_tool(Tool::Smudge);
            }
            if on(Action::ToggleSymmetry) {
                self.toggle_symmetry();
            }
            if on(Action::ToolEraser) {
                self.set_tool(Tool::Eraser);
            }
            if on(Action::ToolText) {
                self.set_tool(Tool::Text);
            }
            if on(Action::ToolShape) {
                self.set_tool(Tool::Shape);
            }
            if on(Action::ToolPen) {
                self.set_tool(Tool::Pen);
            }
            if self.session.anim_settings().enabled {
                if on(Action::AnimPrevFrame) {
                    timeline::step(self, -1);
                }
                if on(Action::AnimNextFrame) {
                    timeline::step(self, 1);
                }
            }
            if on(Action::ToolMove) {
                self.set_tool(Tool::Move);
            }
            if self.session.transform_active() && sc(Modifiers::NONE, Key::Enter) {
                self.commit_transform();
            }
            if on(Action::BrushSizeDown) {
                let s = self.session.current_brush().size;
                let _ = self.session.execute(Command::SetBrushSize { size: (s / 1.15).floor().max(1.0) });
                self.canvas.size_flash = 1.0;
            }
            if on(Action::BrushSizeUp) {
                let s = self.session.current_brush().size;
                let _ = self.session.execute(Command::SetBrushSize { size: (s * 1.15).ceil() });
                self.canvas.size_flash = 1.0;
            }
            if sc(Modifiers::NONE, Key::Tab) {
                self.hide_ui = !self.hide_ui;
            }
            if sc(Modifiers::NONE, Key::Escape) && !self.canvas.cancel_poly() {
                if self.canvas.persp_editing || self.canvas.sym_editing || self.canvas.lines_editing || self.canvas.measuring {
                    self.canvas.persp_editing = false;
                    self.canvas.sym_editing = false;
                    self.canvas.lines_editing = false;
                    self.canvas.measuring = false;
                    self.canvas.measure = None;
                } else if self.session.transform_active() {
                    self.cancel_transform();
                } else {
                    self.popover = None;
                }
            }
        }
        let held = |k| !typing && ctx.input(|i| i.key_down(k));
        (held(Key::Space), held(Key::R))
    }

    /// Settings ▸ Keyboard shortcuts: the next key press becomes the
    /// action's combo. Escape cancels; hard-wired keys are refused.
    fn capture_key(&mut self, ctx: &egui::Context, now: f64) {
        let Some(action) = self.capture else { return };
        let press = ctx.input_mut(|i| {
            let p = i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            });
            i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            p
        });
        let Some((key, m)) = press else { return };
        self.capture = None;
        if key == Key::Escape && !(m.ctrl || m.command || m.alt || m.shift) {
            return;
        }
        let combo = keymap::combo_of(key, m);
        if keymap::RESERVED.contains(&combo.as_str()) {
            self.notify(format!("{} is reserved", keymap::format_combo(&combo)), now);
            return;
        }
        self.settings.keybindings.insert(action.id().to_string(), combo);
        self.settings.save();
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        tablet::feed(self, ctx, raw);
    }

    fn on_exit(&mut self) {
        // The tablet borrows the window's handles: let go before it closes.
        self.tablet = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        debug::tick(self, &ctx);
        timeline::tick(self, &ctx);
        self.gestures(&ctx);
        self.replay_tick(&ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.any_dirty() && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Pending::Close, &ctx);
        }
        // Every tab's session steps wet paint on the shared GPU stepper.
        if let Some(w) = &self.wet_gpu
            && !self.session.has_wet_backend()
        {
            self.session.set_wet_backend(Some(Box::new(w.clone())));
        }
        let (space, r) = self.shortcuts(&ctx);
        let rect = ui.max_rect();
        let prefs = self.settings.preferences(self.canvas.view.scale);
        if self.session.preferences() != prefs {
            self.session.set_preferences(prefs);
        }
        self.canvas.cursor = canvas::CursorPrefs {
            style: self.settings.brush_cursor,
            use_color: self.settings.cursor_use_color,
            thickness: self.settings.cursor_thickness as f32,
            tip_shape: self.settings.cursor_tip_shape,
        };
        let was_stroking = self.session.is_stroking();
        let t = onepaint_ui_kit::Tokens::get(&ctx);
        ui.painter().rect_filled(rect, 0.0, t.stage);
        // The docks, the tool rail and the status bar take their room from the stage.
        let rect = if self.hide_ui {
            rect
        } else {
            let res = dock::reserved(self, rect);
            egui::Rect::from_min_max(
                egui::pos2(rect.left() + res.left, rect.top()),
                egui::pos2(rect.right() - res.right, rect.bottom() - status_bar::HEIGHT),
            )
        };
        let pegging = self.timing.peg_mode && self.session.anim_settings().enabled;
        self.canvas.peg_target = self.timing.peg_target.clone().filter(|_| pegging);
        self.canvas.show(ui, rect, &mut self.session, space, r, self.gpu_ok, self.gpu_compose);
        // Keep a short history of pen samples (the canvas may use a move a frame late).
        let excess = self.canvas.pen_queue.len().saturating_sub(512);
        self.canvas.pen_queue.drain(..excess);
        if !self.gpu_ok {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "onepaint needs a GPU (wgpu) renderer.",
                egui::FontId::proportional(14.0),
                t.fg_muted,
            );
        }
        if was_stroking && !self.session.is_stroking() {
            self.push_recent();
        }
        for e in self.session.drain_events() {
            match e {
                Event::LayersChanged | Event::DocumentReplaced => self.thumbs.clear(),
                Event::HistoryChanged { .. } => self.thumbs.invalidate_active(self.session.active()),
                _ => {}
            }
        }
        let now = ctx.input(|i| i.time);
        let (dirty, busy) = (self.dirty(), self.session.is_stroking() || self.session.transform_active());
        let doc = &self.session;
        if !self.debug.scripted() {
            self.autosave.tick(now, dirty, || (!busy).then(|| doc.doc().clone()));
        }
        if dirty {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(autosave::INTERVAL_S));
        }
        let doc_size = [self.session.doc().width, self.session.doc().height];
        if self.canvas.doc_size != doc_size {
            self.canvas.reset_for(doc_size);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            clip::drop_file(self, &ctx, Some(f.path().to_path_buf()), None);
        }
        // Switching layers while moving restarts the transform on the new layer.
        if self.session.tool() == Tool::Move
            && !self.session.transform_active()
            && self.xform_layer.as_deref() != Some(self.session.active())
        {
            self.begin_transform();
        }
        if !self.hide_ui {
            chrome::show(self, &ctx);
        }
        text_ui::show(self, &ctx);
        let title = format!("{}{} — onepaint", if self.dirty() { "• " } else { "" }, self.session.doc().name);
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        if self.session.is_stroking() {
            ctx.request_repaint();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn harness() -> Harness<'static, App> {
        Harness::builder().with_size(vec2(1200.0, 800.0)).with_max_steps(400).build_eframe(|cc| App::new(cc, None))
    }

    fn press(h: &mut Harness<'_, App>, p: egui::Pos2, pressed: bool) {
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }

    fn drag_stroke(h: &mut Harness<'_, App>, from: egui::Pos2, to: egui::Pos2) {
        h.event(Event::PointerMoved(from));
        h.step();
        press(h, from, true);
        for i in 1..=10 {
            let f = i as f32 / 10.0;
            h.event(Event::PointerMoved(from + (to - from) * f));
            h.step();
        }
        press(h, to, false);
    }

    #[test]
    fn toolbars_keep_the_plain_cursor_and_the_canvas_gets_the_crosshair() {
        let mut h = harness();
        h.run();
        let cursor = |h: &mut Harness<'_, App>, at: egui::Pos2| {
            h.event(Event::PointerMoved(at));
            h.step();
            h.step();
            h.output().platform_output.cursor_icon
        };
        assert_eq!(cursor(&mut h, pos2(640.0, 420.0)), egui::CursorIcon::Crosshair);
        let over_bar = h.get_by_label("Layers").rect().center();
        assert_ne!(cursor(&mut h, over_bar), egui::CursorIcon::Crosshair, "a toolbar over the canvas keeps its own cursor");
    }

    #[test]
    fn image_tips_get_a_traced_cursor() {
        // Longest cursor polyline drawn near the pointer (0 = only rings).
        fn traced(h: &mut Harness<'_, App>, at: egui::Pos2) -> usize {
            h.event(Event::PointerMoved(at));
            h.step();
            h.step();
            h.output()
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Path(p) if p.points.len() > 8 && p.points.iter().all(|q| q.distance(at) < 120.0) => Some(p.points.len()),
                    _ => None,
                })
                .max()
                .unwrap_or(0)
        }
        let mut h = harness();
        h.run();
        let at = pos2(640.0, 420.0);
        assert_eq!(traced(&mut h, at), 0, "a round tip keeps its ring");
        let foliage = onepaint_brush::preset("foliage").unwrap();
        let _ = h.state_mut().session.execute(Command::SetBrushPreset { preset: Box::new(foliage) });
        assert!(traced(&mut h, at + egui::vec2(1.0, 0.0)) > 8, "the leaf's outline");
        h.state_mut().settings.cursor_tip_shape = false;
        assert_eq!(traced(&mut h, at), 0, "tracing off: the ring again");
    }

    #[test]
    fn tablet_samples_carry_pressure_tilt_and_twist_and_the_eraser_end_swaps() {
        use onepaint_engine::recording::RecEvent;
        use onepaint_input::tablet::{PenEvent, PenSample, PenTool};
        let mut h = harness();
        h.run();
        let send = |h: &mut Harness<'_, App>, evs: &[PenEvent]| {
            let out = tablet::translate(h.state_mut(), evs, 1.0, egui::Modifiers::NONE);
            for e in out {
                h.event(e);
            }
            h.step();
        };
        let sample = |x: f32, p: f32| PenSample { pos: [x, 420.0], pressure: Some(p), tilt: [0.5, -0.25], twist: 30.0 };
        h.state_mut().session.start_recording().unwrap();
        send(&mut h, &[PenEvent::In(PenTool::Pen), PenEvent::Move(sample(500.0, 0.0))]);
        send(&mut h, &[PenEvent::Down(sample(500.0, 0.1))]);
        // Several samples in one frame: every one is a stroke point with its own pressure.
        send(&mut h, &(1..=8).map(|i| PenEvent::Move(sample(500.0 + i as f32 * 10.0, 0.1 * i as f32))).collect::<Vec<_>>());
        send(&mut h, &[PenEvent::Up, PenEvent::Out]);
        let rec = h.state_mut().session.stop_recording().unwrap();
        let pts: Vec<_> = rec
            .events
            .iter()
            .flat_map(|e| match e {
                RecEvent::Begin { p, .. } => vec![*p],
                RecEvent::Extend { pts, .. } => pts.clone(),
                _ => vec![],
            })
            .collect();
        assert!(pts.len() >= 9, "every sample is a point: {}", pts.len());
        assert!(pts.last().unwrap().pressure > pts[1].pressure + 0.3, "pressure rises with the pen");
        assert!(pts.iter().all(|p| (p.tilt_x - 0.5).abs() < 1e-6 && (p.tilt_y + 0.25).abs() < 1e-6 && p.twist == 30.0));
        assert!(h.state().session.can_undo(), "the pen painted");
        // The eraser end erases while in range, then the brush comes back.
        send(&mut h, &[PenEvent::In(PenTool::Eraser)]);
        assert_eq!(h.state().session.tool(), Tool::Eraser);
        send(&mut h, &[PenEvent::Out]);
        assert_eq!(h.state().session.tool(), Tool::Brush);
    }

    #[test]
    fn dragging_on_the_canvas_paints_and_undo_removes_it() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 440.0));
        let app = h.state();
        let layer = app.session.doc().layer(app.session.active()).unwrap();
        assert!(layer.pixels.tile_count() > 0, "the stroke reached the active layer");
        let d = app.canvas.view.screen_to_doc([640.0, 430.0]);
        assert!(layer.pixels.get(d[0] as i32, d[1] as i32)[3] > 0.5, "paint under the stroke");
        assert!(app.dirty());
        h.event(Event::Key { key: egui::Key::Z, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.step();
        let app = h.state();
        assert_eq!(app.session.doc().layer(app.session.active()).unwrap().pixels.tile_count(), 0);
    }

    #[test]
    fn strokes_on_the_chrome_do_not_paint() {
        let mut h = harness();
        h.run();
        // The side rail sits at the left edge.
        drag_stroke(&mut h, pos2(30.0, 400.0), pos2(34.0, 330.0));
        let app = h.state();
        assert_eq!(app.session.doc().layer(app.session.active()).unwrap().pixels.tile_count(), 0);
    }

    #[test]
    fn top_bar_buttons_open_popovers_and_add_layers() {
        let mut h = harness();
        h.run();
        h.get_by_label("Layers").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Layers));
        h.get_by_label("New layer").click();
        h.run();
        assert_eq!(h.state().session.doc().root.len(), 3);
        h.get_by_label("Brush library").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Brushes));
        h.get_by_label("Eraser (E)").click();
        h.run();
        assert_eq!(h.state().session.tool(), Tool::Eraser);
    }

    #[test]
    fn paper_is_chosen_from_the_menu_and_for_new_canvases() {
        let mut h = harness();
        h.run();
        h.state_mut().popover = Some(Popover::Menu);
        h.run();
        h.get_by_label("Paper…").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Paper));
        for _ in 0..5 {
            h.step();
        }
        h.get_by_label("Rough").click();
        h.run();
        let p = h.state().session.doc().paper().unwrap();
        assert_eq!(p.grain_id, "rough-tooth");
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Slider, "Texture scale").is_some());
        h.get_by_label("Smooth").click();
        h.run();
        assert!(h.state().session.doc().paper().is_none());
        // New canvases take the dialog's paper.
        h.state_mut().new_paper = 3;
        h.state_mut().new_document(640, 480);
        assert_eq!(h.state().session.doc().paper().unwrap().grain_id, "canvas");
    }

    #[test]
    fn the_studio_turns_a_brush_into_a_blender() {
        let mut h = harness();
        h.run();
        h.state_mut().popover = Some(Popover::BrushStudio);
        for _ in 0..4 {
            h.step();
        }
        h.state_mut().studio.search = "blender".into();
        h.run();
        h.get_by_label("Blender").click();
        h.run();
        let b = h.state().session.current_brush().clone();
        assert!(onepaint_brush::studio::is_blender(&b), "the current brush blends");
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Slider, "Oil").is_some());
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Slider, "Pigment").is_some());
    }

    #[test]
    fn the_studio_adds_a_dual_brush() {
        let mut h = harness();
        h.run();
        h.state_mut().popover = Some(Popover::BrushStudio);
        for _ in 0..4 {
            h.step();
        }
        h.state_mut().studio.search = "dual".into();
        h.run();
        h.get_by_label("bristle").click();
        h.run();
        let b = h.state().session.current_brush().clone();
        assert_eq!(b.mask_tip(), Some(("bristle".into(), 0.7)));
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Slider, "Strength").is_some());
    }

    #[test]
    fn wet_paint_stays_wet_until_dried() {
        let mut h = harness();
        h.run();
        let wc = onepaint_brush::preset("watercolor-live").unwrap();
        let _ = h.state_mut().session.execute(Command::SetBrushPreset { preset: Box::new(wc) });
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 440.0));
        assert!(h.state().session.wet_active(), "the wash keeps moving after the pen lifts");
        // The tilt pad sits beside Dry: press its right edge to tilt right.
        let pad = h.get_by_label_contains("Canvas tilt").rect();
        let at = egui::pos2(pad.right() - 2.0, pad.center().y);
        h.event(Event::PointerMoved(at));
        h.step();
        press(&mut h, at, true);
        press(&mut h, at, false);
        h.step();
        let tilt = h.state().session.wet_tilt();
        assert!(tilt[0] > 0.8 && tilt[1].abs() < 0.1, "tilted right: {tilt:?}");
        h.get_by_label("Dry the wet paint").click();
        h.run();
        let s = &h.state().session;
        assert!(!s.wet_active() && s.can_undo(), "dried into one undo step");
        assert!(h.query_by_label("Dry the wet paint").is_none());
    }

    #[test]
    fn layer_bevel_slider_embosses_the_paint() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 440.0));
        h.get_by_label("Layers").click();
        h.run();
        h.get_by_role_and_label(egui::accesskit::Role::Slider, "Bevel").click();
        h.run();
        let s = &h.state().session;
        assert!(h.state().bevel[0] > 0.0);
        assert!(s.doc().layer(s.active()).unwrap().height.is_some(), "relief derived from the stroke");
    }

    #[test]
    fn new_drawings_open_in_tabs_and_closing_asks() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 440.0));
        let ctx = h.ctx.clone();
        h.state_mut().request(Pending::New(800, 600), &ctx);
        h.run();
        assert_eq!(h.state().tabs.len(), 2, "a second tab; nothing discarded");
        assert_eq!(h.state().session.doc().width, 800);
        assert_eq!(h.state().popover, None);
        let titles = h.state().tab_titles();
        assert!(titles[0].1 && !titles[1].1, "the first drawing is still unsaved: {titles:?}");
        // Switch back: the painted drawing and its pixels return.
        key(&mut h, egui::Key::ArrowLeft, egui::Modifiers::COMMAND | egui::Modifiers::ALT);
        assert_eq!(h.state().active_tab, 0);
        assert_eq!(h.state().session.doc().width, 2048);
        assert!(h.state().dirty());
        // Closing an unsaved tab asks first.
        h.state_mut().request(Pending::CloseTab, &ctx);
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Confirm));
        h.get_by_label("Cancel").click();
        h.run();
        assert_eq!(h.state().tabs.len(), 2);
        h.state_mut().request(Pending::CloseTab, &ctx);
        h.run();
        h.get_by_label("Discard").click();
        h.run();
        assert_eq!(h.state().tabs.len(), 1);
        assert_eq!(h.state().session.doc().width, 800);
    }

    #[test]
    fn clean_document_skips_the_prompt() {
        let mut h = harness();
        h.run();
        let ctx = h.ctx.clone();
        h.state_mut().request(Pending::New(640, 480), &ctx);
        h.run();
        assert_eq!(h.state().session.doc().width, 640);
        assert_eq!(h.state().popover, None);
    }

    fn key(h: &mut Harness<'_, App>, key: egui::Key, modifiers: egui::Modifiers) {
        h.event(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        h.step();
        h.event(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
        h.step();
    }

    fn click(h: &mut Harness<'_, App>, p: egui::Pos2) {
        h.event(Event::PointerMoved(p));
        h.step();
        press(h, p, true);
        press(h, p, false);
    }

    #[test]
    fn rect_select_then_paint_is_clipped_and_delete_clears() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::M, egui::Modifiers::NONE);
        assert_eq!(h.state().session.tool(), Tool::SelectRect);
        drag_stroke(&mut h, pos2(600.0, 300.0), pos2(700.0, 500.0));
        let sel = h.state().session.selection().cloned().expect("a selection");
        let inside = h.state().canvas.view.screen_to_doc([650.0, 400.0]);
        let outside = h.state().canvas.view.screen_to_doc([500.0, 400.0]);
        assert_eq!(sel.get(inside[0] as u32, inside[1] as u32), 255);
        assert_eq!(sel.get(outside[0] as u32, outside[1] as u32), 0);
        key(&mut h, egui::Key::B, egui::Modifiers::NONE);
        drag_stroke(&mut h, pos2(450.0, 400.0), pos2(800.0, 400.0));
        let app = h.state();
        let layer = app.session.doc().layer(app.session.active()).unwrap();
        assert!(layer.pixels.get(inside[0] as i32, inside[1] as i32)[3] > 0.5);
        assert_eq!(layer.pixels.get(outside[0] as i32, outside[1] as i32)[3], 0.0);
        key(&mut h, egui::Key::Delete, egui::Modifiers::NONE);
        let app = h.state();
        assert_eq!(app.session.doc().layer(app.session.active()).unwrap().pixels.get(inside[0] as i32, inside[1] as i32)[3], 0.0);
        key(&mut h, egui::Key::D, egui::Modifiers::COMMAND);
        assert!(h.state().session.selection().is_none());
    }

    #[test]
    fn polygon_select_closes_with_enter() {
        let mut h = harness();
        h.run();
        h.state_mut().set_tool(Tool::SelectPoly);
        for p in [pos2(500.0, 300.0), pos2(700.0, 300.0), pos2(700.0, 500.0)] {
            click(&mut h, p);
        }
        assert!(h.state().session.selection().is_none(), "not committed yet");
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        let d = h.state().canvas.view.screen_to_doc([660.0, 350.0]);
        assert_eq!(h.state().session.selection().expect("polygon").get(d[0] as u32, d[1] as u32), 255);
    }

    #[test]
    fn fill_tool_and_eyedropper() {
        let mut h = harness();
        h.run();
        let bg = h.state().session.doc().root[0].id().to_string();
        let _ = h.state_mut().session.execute(Command::SetActiveLayer { id: bg });
        h.state_mut().set_color(Rgba::new(1.0, 0.0, 0.0, 1.0));
        key(&mut h, egui::Key::G, egui::Modifiers::NONE);
        click(&mut h, pos2(600.0, 400.0));
        let d = h.state().canvas.view.screen_to_doc([600.0, 400.0]);
        let px = h.state().session.doc().layer(h.state().session.active()).unwrap().pixels.get(d[0] as i32, d[1] as i32);
        assert!(px[0] > 0.99 && px[1] < 0.01, "filled red {px:?}");
        h.state_mut().set_color(Rgba::BLACK);
        key(&mut h, egui::Key::I, egui::Modifiers::NONE);
        click(&mut h, pos2(600.0, 400.0));
        assert_eq!(h.state().session.color().to_hex(), "#ff0000");
    }

    /// Whether the composite (including any floating transform) shows the red-ish paint
    /// rather than the white background at a screen point.
    fn active_alpha(h: &Harness<'_, App>, screen: [f32; 2]) -> f32 {
        let app = h.state();
        let d = app.canvas.view.screen_to_doc([screen[0] as f64, screen[1] as f64]);
        let c = app.session.sample_color(d[0] as i32, d[1] as i32).unwrap();
        1.0 - c.g
    }

    #[test]
    fn move_tool_drags_commits_and_cancels() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(560.0, 400.0), pos2(640.0, 400.0));
        assert!(active_alpha(&h, [600.0, 400.0]) > 0.5);
        key(&mut h, egui::Key::V, egui::Modifiers::NONE);
        assert!(h.state().session.transform_active());
        // Drag the body (clear of the edge/corner handles) down by 100 screen px.
        drag_stroke(&mut h, pos2(578.0, 400.0), pos2(578.0, 500.0));
        assert!(h.state().canvas.xform.dy > 0.0);
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        assert!(active_alpha(&h, [600.0, 500.0]) > 0.5, "moved");
        assert!(active_alpha(&h, [600.0, 400.0]) < 1e-3, "left its old place");
        assert!(h.state().session.transform_active(), "a new session starts after Apply");
        // Esc abandons a pending move.
        drag_stroke(&mut h, pos2(578.0, 500.0), pos2(578.0, 300.0));
        key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(active_alpha(&h, [600.0, 500.0]) > 0.5);
        // Switching tools commits.
        drag_stroke(&mut h, pos2(578.0, 500.0), pos2(678.0, 500.0));
        key(&mut h, egui::Key::B, egui::Modifiers::NONE);
        assert!(!h.state().session.transform_active());
        assert!(active_alpha(&h, [700.0, 500.0]) > 0.5);
    }

    #[test]
    fn transform_bar_apply_and_cancel_buttons() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(560.0, 400.0), pos2(640.0, 400.0));
        h.get_by_label("Move / transform (V)").click();
        h.run();
        assert!(h.state().session.transform_active());
        let n = h.state().session.history_len();
        // Apply commits the move as one undo step and starts a fresh box.
        drag_stroke(&mut h, pos2(578.0, 400.0), pos2(578.0, 500.0));
        assert!(active_alpha(&h, [600.0, 400.0]) < 1e-3, "lifted content follows the drag");
        h.get_by_label("Apply").click();
        h.run();
        assert!(active_alpha(&h, [600.0, 500.0]) > 0.5, "moved");
        assert!(active_alpha(&h, [600.0, 400.0]) < 1e-3, "left its old place");
        assert_eq!(h.state().session.history_len(), n + 1);
        assert!(h.state().session.transform_active(), "the Move tool keeps a box on the layer");
        assert_eq!(h.state().canvas.xform, onepaint_engine::transform::TransformParams::default(), "the new box starts clean");
        // Cancel puts a pending move back and records nothing.
        drag_stroke(&mut h, pos2(578.0, 500.0), pos2(578.0, 300.0));
        h.get_by_label("Rotate +15°").click();
        h.run();
        h.get_by_label("Cancel").click();
        h.run();
        assert!(active_alpha(&h, [600.0, 500.0]) > 0.5, "back where Apply left it");
        assert!(active_alpha(&h, [600.0, 300.0]) < 1e-3);
        assert_eq!(h.state().session.history_len(), n + 1, "cancel adds no undo step");
        // Cancel with nothing pending is harmless; undo reverts the applied move.
        h.get_by_label("Cancel").click();
        h.run();
        assert!(active_alpha(&h, [600.0, 500.0]) > 0.5);
        key(&mut h, egui::Key::Z, egui::Modifiers::COMMAND);
        assert!(active_alpha(&h, [600.0, 400.0]) > 0.5, "undo puts the stroke back");
        assert!(active_alpha(&h, [600.0, 500.0]) < 1e-3);
    }

    #[test]
    fn status_bar_symmetry_zoom_and_fill_mode() {
        let mut h = harness();
        h.run();
        use onepaint_engine::SymmetryMode;
        assert!(h.query_by_label("Move symmetry axis — drag the canvas to reposition (Shift+Y)").is_none(), "only with symmetry on");
        h.get_by_label("Vertical symmetry — mirror left/right (Y toggles symmetry)").click();
        h.run();
        assert_eq!(h.state().session.guides().symmetry.mode, SymmetryMode::Vertical);
        h.get_by_label("Horizontal symmetry — mirror up/down (Y toggles symmetry)").click();
        h.run();
        assert_eq!(h.state().session.guides().symmetry.mode, SymmetryMode::Both);
        h.get_by_label("Move symmetry axis — drag the canvas to reposition (Shift+Y)").click();
        h.run();
        assert!(h.state().canvas.sym_editing);
        // Dragging the canvas now moves the centre, not paint.
        let n = h.state().session.history_len();
        drag_stroke(&mut h, pos2(500.0, 300.0), pos2(400.0, 250.0));
        assert_eq!(h.state().session.history_len(), n, "no paint while moving the axis");
        assert!(h.state().session.guides().symmetry.cx < 0.5);
        h.get_by_label("Vertical symmetry — mirror left/right (Y toggles symmetry)").click();
        h.run();
        assert_eq!(h.state().session.guides().symmetry.mode, SymmetryMode::Horizontal);
        let z = h.state().canvas.view.scale;
        h.get_by_label("Zoom in").click();
        h.run();
        assert!((h.state().canvas.view.scale / z - 1.2).abs() < 1e-6);
        h.get_by_label("Reset zoom to 100%").click();
        h.run();
        assert!((h.state().canvas.view.scale - 1.0).abs() < 1e-9);
        h.get_by_label("Fill mode — brush strokes fill their silhouette (F)").click();
        h.run();
        assert!(h.state().canvas.options.fill_shape);
    }

    #[test]
    fn color_drop_from_the_status_bar_fills_on_release() {
        let mut h = harness();
        h.run();
        let n = h.state().session.history_len();
        h.state_mut().set_color(Rgba::new(0.0, 0.0, 1.0, 1.0));
        h.run();
        let swatch = h.get_by_label("Drag onto the canvas to fill (ColorDrop) · hold + slide to set threshold").rect().center();
        let to = pos2(600.0, 400.0);
        h.event(Event::PointerMoved(swatch));
        h.step();
        press(&mut h, swatch, true);
        for i in 1..=10 {
            h.event(Event::PointerMoved(swatch + (to - swatch) * (i as f32 / 10.0)));
            h.step();
        }
        assert!(h.state().session.color_drop_active(), "previewing over the canvas");
        press(&mut h, to, false);
        h.run();
        assert!(!h.state().session.color_drop_active());
        assert_eq!(h.state().session.history_len(), n + 1, "one undo step");
        let app = h.state();
        let d = app.canvas.screen_doc(to);
        let c = app.session.sample_color(d[0] as i32, d[1] as i32).unwrap();
        assert!(c.b > 0.99 && c.r < 0.01, "filled blue {c:?}");
    }

    #[test]
    fn copy_paste_and_layer_via_copy() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(560.0, 400.0), pos2(640.0, 400.0));
        let n = h.state().session.doc().root.len();
        key(&mut h, egui::Key::C, egui::Modifiers::COMMAND);
        key(&mut h, egui::Key::V, egui::Modifiers::COMMAND);
        let app = h.state();
        assert_eq!(app.session.doc().root.len(), n + 1);
        assert_eq!(app.session.doc().find(app.session.active()).unwrap().name(), "Pasted");
        key(&mut h, egui::Key::J, egui::Modifiers::COMMAND);
        assert_eq!(h.state().session.doc().root.len(), n + 2, "Ctrl+J without a selection duplicates");
    }

    #[test]
    fn layers_reorder_by_drag_and_drop() {
        let mut h = harness();
        h.run();
        h.get_by_label("Layers").click();
        h.run();
        let bg = h.state().session.doc().root[0].id().to_string();
        let from = h.get_by_label("Background").rect().center();
        let to = h.get_by_label("Layer 1").rect().center() - egui::vec2(0.0, 12.0);
        h.event(Event::PointerMoved(from));
        h.step();
        press(&mut h, from, true);
        for i in 1..=8 {
            h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 8.0)));
            h.step();
        }
        press(&mut h, to, false);
        h.run();
        let root = &h.state().session.doc().root;
        assert_eq!(root.last().unwrap().id(), bg, "Background dropped above Layer 1");
    }

    #[test]
    fn adjustments_apply_and_make_layers() {
        let mut h = harness();
        h.run();
        let bg = h.state().session.doc().root[0].id().to_string();
        let _ = h.state_mut().session.execute(Command::SetActiveLayer { id: bg });
        h.get_by_label("Adjustments").click();
        h.run();
        h.get_by_label("Invert").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Filter));
        h.get_by_label("Apply").click();
        h.run();
        let c = h.state().session.pick_color(10, 10);
        assert!(c.r < 0.01, "white background inverted to black {c:?}");
        h.get_by_label("Adjustments").click();
        h.run();
        h.get_by_label("Invert").click();
        h.run();
        h.get_by_label("As adjustment layer").click();
        h.run();
        let app = h.state();
        assert!(app.session.doc().layer(app.session.active()).unwrap().adjustment.is_some());
        assert!(app.session.pick_color(10, 10).r > 0.99, "the invert layer flips it back to white");
    }

    #[test]
    fn generators_and_fill_layers_from_the_menu() {
        let mut h = harness();
        h.run();
        let layers = h.state().session.doc().layer_ids().len();
        h.get_by_label("Adjustments").click();
        h.run();
        h.get_by_label("Clouds").scroll_to_me();
        h.run();
        h.get_by_label("Clouds").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Filter));
        h.get_by_label("Re-roll").click();
        h.run();
        assert_eq!(h.state().session.filter_state().unwrap().1.gen_seed, Some(2.0));
        h.get_by_label("Apply").click();
        h.run();
        assert_eq!(h.state().session.doc().layer_ids().len(), layers + 1, "clouds land on a new layer");
        h.get_by_label("Adjustments").click();
        h.run();
        h.get_by_label("Solid Color Fill").scroll_to_me();
        h.run();
        h.get_by_label("Solid Color Fill").click();
        h.run();
        let app = h.state();
        assert_eq!(app.popover, None, "the settings open in the Properties panel, docked right");
        assert!(dock::panel_open(app, layout::PanelId::Properties));
        let adj = app.session.doc().layer(app.session.active()).unwrap().adjustment.clone().unwrap();
        assert_eq!(adj.kind, "solid-fill");
    }

    #[test]
    fn gradient_and_shape_tools() {
        let mut h = harness();
        h.run();
        h.state_mut().set_color(Rgba::new(0.0, 0.0, 1.0, 1.0));
        h.get_by_label("Gradient").click();
        h.run();
        drag_stroke(&mut h, pos2(400.0, 400.0), pos2(800.0, 400.0));
        let app = h.state();
        let at = |x: f32| {
            let d = app.canvas.view.screen_to_doc([x as f64, 400.0]);
            app.session.doc().layer(app.session.active()).unwrap().pixels.get(d[0] as i32, d[1] as i32)[3]
        };
        assert!(at(300.0) > 0.98 && at(900.0) < 0.02 && at(600.0) > 0.3 && at(600.0) < 0.7, "{} {} {}", at(300.0), at(600.0), at(900.0));
        let n = h.state().session.history_len();
        h.state_mut().set_tool(Tool::Shape);
        h.state_mut().canvas.options.shape_kind = onepaint_stroke::shape::ShapeKind::Rect;
        h.state_mut().canvas.options.shape_fill = true;
        drag_stroke(&mut h, pos2(500.0, 200.0), pos2(700.0, 300.0));
        assert_eq!(h.state().session.history_len(), n + 1, "one step for fill + outline");
        let app = h.state();
        let d = app.canvas.view.screen_to_doc([600.0, 250.0]);
        assert!(app.session.doc().layer(app.session.active()).unwrap().pixels.get(d[0] as i32, d[1] as i32)[3] > 0.99);
    }

    #[test]
    fn text_tool_creates_edits_and_moves_text() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::T, egui::Modifiers::NONE);
        click(&mut h, pos2(500.0, 300.0));
        h.run();
        assert!(h.state().canvas.text_edit.is_some(), "editor opened");
        h.event(Event::Text("Hello".into()));
        h.run();
        let id = h.state().session.active().to_string();
        assert_eq!(h.state().session.texts(&id).first().map(|t| t.text.clone()), Some("Hello".into()));
        h.get_by_label("Done").click();
        h.run();
        assert!(h.state().canvas.text_edit.is_none());
        // Drag the text by its body to move it.
        let x0 = h.state().session.texts(&id)[0].x;
        drag_stroke(&mut h, pos2(505.0, 310.0), pos2(605.0, 310.0));
        assert!(h.state().session.texts(&id)[0].x > x0 + 100.0);
    }

    #[test]
    fn text_editor_styles_a_selection_and_adds_a_balloon() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::T, egui::Modifiers::NONE);
        click(&mut h, pos2(500.0, 300.0));
        h.run();
        h.event(Event::Text("Hello world".into()));
        h.run();
        // Select "world" with Shift+Left and bold just that.
        for _ in 0..5 {
            key(&mut h, egui::Key::ArrowLeft, egui::Modifiers::SHIFT);
        }
        h.get_by_label("B").click();
        h.run();
        let id = h.state().session.active().to_string();
        let spec = h.state().session.texts(&id)[0].clone();
        let runs = spec.runs.clone().expect("rich runs");
        assert_eq!(runs.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["Hello ", "world"]);
        assert_eq!(runs[1].bold, Some(true));
        assert!(!spec.bold, "only the selection");
        // Typing keeps the runs in step.
        key(&mut h, egui::Key::End, egui::Modifiers::NONE);
        h.event(Event::Text("!".into()));
        h.run();
        assert_eq!(h.state().session.texts(&id)[0].runs.clone().unwrap()[1].text, "world!");
        // A speech balloon with a tail.
        h.get_by_label("Balloon").click();
        h.run();
        h.get_by_label("Speech").click();
        h.run();
        let b = h.state().session.texts(&id)[0].balloon.clone().expect("balloon");
        assert_eq!(b.shape, onepaint_text::balloon::BalloonShape::Oval);
        assert_eq!((b.fill.as_str(), b.stroke.as_deref(), b.stroke_width), ("#ffffff", Some("#000000"), Some(3.0)));
        assert!(b.tail.is_none(), "no tail until it is aimed");
        // Back to none.
        h.get_by_label("Balloon").click();
        h.run();
        h.get_by_label("None").click();
        h.run();
        assert!(h.state().session.texts(&id)[0].balloon.is_none());
    }

    #[test]
    fn ctrl_y_is_the_value_check_and_the_guides_panel_holds_print_preview() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::Y, egui::Modifiers::COMMAND);
        assert!(h.state().canvas.view_modes().grayscale);
        key(&mut h, egui::Key::Y, egui::Modifiers::COMMAND);
        assert!(!h.state().canvas.view_modes().grayscale);
        h.state_mut().popover = Some(Popover::Guides);
        h.run();
        h.get_by_label("Print preview (CMYK proof)").click();
        h.run();
        assert!(h.state().canvas.view_modes().proof);
        h.get_by_label("ΔE > 5").click();
        h.run();
        assert_eq!(h.state().canvas.view_modes().gamut_warn, 5.0);
        assert!(h.query_by_label_contains("Press: Generic coated (built-in)").is_some());
        // A loaded press is named there, and clicking it goes back to the built-in.
        let Ok(icc) = std::fs::read("/usr/share/color/icc/colord/FOGRA39L_coated.icc") else { return };
        let p = press::load(&icc, None).unwrap();
        let name = p.name.clone();
        h.state_mut().press = Some(p.clone());
        h.state_mut().canvas.set_press(Some(p));
        h.run();
        h.get_by_label_contains(&format!("Press: {name}")).click();
        h.run();
        assert!(h.state().press.is_none());
    }

    #[test]
    fn symmetry_mirrors_strokes_and_s_picks_smudge() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::Y, egui::Modifiers::NONE);
        assert_eq!(h.state().session.guides().symmetry.mode, onepaint_engine::SymmetryMode::Vertical);
        drag_stroke(&mut h, pos2(500.0, 420.0), pos2(560.0, 440.0));
        let app = h.state();
        let layer = app.session.doc().layer(app.session.active()).unwrap();
        let d = app.canvas.view.screen_to_doc([530.0, 430.0]);
        let w = app.session.doc().width as f64;
        assert!(layer.pixels.get(d[0] as i32, d[1] as i32)[3] > 0.5);
        assert!(layer.pixels.get((w - d[0]) as i32, d[1] as i32)[3] > 0.5, "mirrored copy");
        key(&mut h, egui::Key::Y, egui::Modifiers::NONE);
        assert!(!h.state().session.guides().symmetry.is_on());
        key(&mut h, egui::Key::S, egui::Modifiers::NONE);
        assert_eq!(h.state().session.tool(), Tool::Smudge);
        h.get_by_label("Smudge (S)").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::SmudgeOptions));
        key(&mut h, egui::Key::B, egui::Modifiers::SHIFT);
        assert_eq!(h.state().session.tool(), Tool::Mixer);
    }

    #[test]
    fn smudge_and_liquify_follow_the_whole_drag() {
        for tool in [Tool::Smudge, Tool::Liquify] {
            let mut h = harness();
            h.run();
            drag_stroke(&mut h, pos2(500.0, 430.0), pos2(700.0, 430.0));
            h.state_mut().set_tool(tool);
            let _ = h.state_mut().session.execute(Command::SetBrushSize { size: 60.0 });
            let before = {
                let app = h.state();
                app.session.doc().layer(app.session.active()).unwrap().pixels.read_rect(app.session.doc().bounds())
            };
            // Drag across the painted line, top to bottom.
            drag_stroke(&mut h, pos2(600.0, 380.0), pos2(600.0, 480.0));
            let app = h.state();
            let after = app.session.doc().layer(app.session.active()).unwrap().pixels.read_rect(app.session.doc().bounds());
            let changed = before.iter().zip(&after).filter(|(a, b)| a != b).count();
            assert!(changed > 400, "{tool:?} changed only {changed} values");
            assert!(app.session.can_undo());
        }
    }

    fn set_guides(h: &mut Harness<'_, App>, f: impl FnOnce(&mut onepaint_engine::Guides)) {
        let mut g = h.state().session.guides().clone();
        f(&mut g);
        let _ = h.state_mut().session.execute(Command::SetGuides { guides: Box::new(g) });
        h.step();
    }

    fn alpha_at(h: &Harness<'_, App>, screen: [f64; 2]) -> f32 {
        let app = h.state();
        let d = app.canvas.view.screen_to_doc(screen);
        app.session.doc().layer(app.session.active()).unwrap().pixels.get(d[0] as i32, d[1] as i32)[3]
    }

    #[test]
    fn hv_snap_straightens_a_wobbly_stroke() {
        let mut h = harness();
        h.run();
        set_guides(&mut h, |g| g.snap_hv = true);
        drag_stroke(&mut h, pos2(500.0, 420.0), pos2(700.0, 445.0));
        assert!(alpha_at(&h, [690.0, 420.0]) > 0.5, "the stroke stayed on the start row");
        assert!(alpha_at(&h, [690.0, 445.0]) < 0.01, "not where the pointer drifted");
    }

    #[test]
    fn hard_perspective_draws_one_straight_line() {
        let mut h = harness();
        h.run();
        set_guides(&mut h, |g| {
            g.perspective.enabled = true;
            g.perspective.points = onepaint_engine::guides::perspective_preset(2);
        });
        let start = h.state().session.history_len();
        drag_stroke(&mut h, pos2(520.0, 300.0), pos2(700.0, 320.0));
        assert_eq!(h.state().session.history_len(), start + 1, "one undo step");
        let app = h.state();
        let (o, e) = (app.canvas.view.screen_to_doc([520.0, 300.0]), app.canvas.view.screen_to_doc([700.0, 320.0]));
        let (w, hh) = (app.session.doc().width as f64, app.session.doc().height as f64);
        // The line runs from the origin toward a vanishing point, not to the pointer.
        let vp = [1.8 * w, 0.5 * hh];
        let dir = [(vp[0] - o[0]), (vp[1] - o[1])];
        let len = dir[0].hypot(dir[1]);
        let t = (e[0] - o[0]) * dir[0] / len + (e[1] - o[1]) * dir[1] / len;
        let on = [o[0] + dir[0] / len * t * 0.8, o[1] + dir[1] / len * t * 0.8];
        let layer = app.session.doc().layer(app.session.active()).unwrap();
        assert!(layer.pixels.get(on[0] as i32, on[1] as i32)[3] > 0.5, "paint along the VP line");
    }

    #[test]
    fn editing_perspective_drags_a_vanishing_point() {
        let mut h = harness();
        h.run();
        set_guides(&mut h, |g| {
            g.perspective.enabled = true;
            g.perspective.points = onepaint_engine::guides::perspective_preset(1);
        });
        key(&mut h, egui::Key::P, egui::Modifiers::SHIFT);
        assert!(h.state().canvas.persp_editing);
        let app = h.state();
        let (w, hh) = (app.session.doc().width as f64, app.session.doc().height as f64);
        let s = app.canvas.view.doc_to_screen([0.5 * w, 0.5 * hh]);
        drag_stroke(&mut h, pos2(s[0] as f32, s[1] as f32), pos2(s[0] as f32 + 100.0, s[1] as f32));
        let vp = h.state().session.guides().perspective.points[0];
        assert!(vp.x > 0.55, "moved right: {vp:?}");
        assert_eq!(h.state().session.doc().layer(h.state().session.active()).unwrap().pixels.tile_count(), 0, "no paint while editing");
        key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!h.state().canvas.persp_editing);
    }

    #[test]
    fn rulers_drag_out_guides_and_dropping_back_deletes() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::R, egui::Modifiers::SHIFT);
        assert!(h.state().session.guides().rulers);
        let h0 = h.state().session.history_len();
        drag_stroke(&mut h, pos2(600.0, 9.0), pos2(600.0, 400.0));
        let app = h.state();
        assert_eq!(app.session.guides().lines.len(), 1);
        let g = &app.session.guides().lines[0];
        let want = app.canvas.view.screen_to_doc([600.0, 400.0])[1];
        assert!(g.angle == 0.0 && (g.y - want).abs() < 1.0, "horizontal guide at the drop: {g:?}");
        assert_eq!(app.session.history_len(), h0 + 1, "one undo step");
        assert_eq!(app.session.doc().layer(app.session.active()).unwrap().pixels.tile_count(), 0, "no paint");
        // Out of the left bar and straight back onto it: nothing remains, nothing recorded.
        drag_stroke(&mut h, pos2(9.0, 300.0), pos2(9.0, 320.0));
        assert_eq!(h.state().session.guides().lines.len(), 1);
        assert_eq!(h.state().session.history_len(), h0 + 1);
        key(&mut h, egui::Key::Z, egui::Modifiers::COMMAND);
        assert!(h.state().session.guides().lines.is_empty());
    }

    #[test]
    fn edit_guides_draws_moves_and_deletes() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::G, egui::Modifiers::SHIFT);
        assert!(h.state().canvas.lines_editing);
        assert_eq!(h.state().session.tool(), Tool::Brush, "Shift+G is not the fill tool");
        drag_stroke(&mut h, pos2(500.0, 300.0), pos2(600.0, 400.0));
        let g = h.state().session.guides().lines[0].clone();
        assert!((g.angle - std::f64::consts::FRAC_PI_4).abs() < 1e-6, "drawn along the drag: {g:?}");
        // Grab it at its anchor and throw it far off the document.
        let app = h.state();
        let s = app.canvas.view.doc_to_screen([g.x, g.y]);
        let off = app.canvas.view.doc_to_screen([-50_000.0, g.y]);
        drag_stroke(&mut h, pos2(s[0] as f32, s[1] as f32), pos2(off[0].max(-30000.0) as f32, s[1] as f32 - 2000.0));
        assert!(h.state().session.guides().lines.is_empty(), "dragged off the canvas: deleted");
        key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
        assert!(!h.state().canvas.lines_editing);
    }

    #[test]
    fn measure_reports_without_painting() {
        let mut h = harness();
        h.run();
        h.state_mut().canvas.measuring = true;
        drag_stroke(&mut h, pos2(500.0, 400.0), pos2(600.0, 400.0));
        let app = h.state();
        let (a, b) = app.canvas.measure.unwrap();
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        assert!((len - 100.0 / app.canvas.view.scale).abs() < 1.0);
        assert_eq!(app.session.doc().layer(app.session.active()).unwrap().pixels.tile_count(), 0);
    }

    #[test]
    fn save_and_load_a_selection_from_the_popover() {
        let mut h = harness();
        h.run();
        let _ = h.state_mut().session.execute(Command::SelectShape {
            contours: vec![onepaint_selection::rect_polygon([100.0, 100.0], [300.0, 300.0])],
            mode: onepaint_selection::Mode::Replace,
        });
        h.state_mut().popover = Some(Popover::Selection);
        h.run_steps(4);
        h.get_by_label("Save selection").click();
        h.run_steps(4);
        assert_eq!(h.state().session.saved_selections().len(), 1);
        let _ = h.state_mut().session.execute(Command::Deselect);
        h.run_steps(4);
        assert!(h.state().session.selection().is_none());
        h.get_by_label("Load (replace)").click();
        h.run_steps(4);
        assert!(h.state().session.selection().is_some_and(|m| m.coverage(200, 200) > 0.5));
    }

    #[test]
    fn colour_popover_adds_to_the_active_palette() {
        let mut h = harness();
        h.run();
        assert_eq!(h.state().palettes.active().colors.len(), 12, "first-run default palette");
        h.state_mut().set_color(Rgba::from_hex("#123456").unwrap());
        h.state_mut().popover = Some(Popover::Color);
        h.run();
        h.get_by_label("Add the current colour").click();
        h.run();
        assert_eq!(h.state().palettes.active().colors.last().map(String::as_str), Some("#123456"));
        h.get_by_label("Add the current colour").click();
        h.run();
        assert_eq!(h.state().palettes.active().colors.len(), 13, "no duplicates");
    }

    #[test]
    fn pen_builds_a_path_layer_and_edits_anchors() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::P, egui::Modifiers::NONE);
        assert_eq!(h.state().session.tool(), Tool::Pen);
        for p in [pos2(500.0, 300.0), pos2(700.0, 300.0), pos2(700.0, 450.0)] {
            click(&mut h, p);
            h.run();
        }
        assert_eq!(h.state().canvas.pen.building.as_ref().map(Vec::len), Some(3));
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        h.run();
        let app = h.state();
        let id = app.session.active().to_string();
        assert!(app.session.is_path_layer(&id), "a path layer was created");
        assert!(app.canvas.pen.building.is_none());
        let mid = app.canvas.view.screen_to_doc([600.0, 300.0]);
        assert!(app.session.doc().layer(&id).unwrap().pixels.get(mid[0] as i32, mid[1] as i32)[3] > 0.5, "stroked");
        // Drag the corner anchor down.
        let h0 = app.session.history_len();
        drag_stroke(&mut h, pos2(700.0, 450.0), pos2(700.0, 520.0));
        let app = h.state();
        let a = app.session.paths(&id)[0].subpaths[0].anchors[2];
        let want = app.canvas.view.screen_to_doc([700.0, 520.0]);
        assert!((a.y - want[1]).abs() < 1.0, "anchor moved: {a:?}");
        assert_eq!(app.session.history_len(), h0 + 1, "one step per drag");
        assert!(app.session.paths(&id)[0].subpaths[0].anchors.len() == 3);
    }

    #[test]
    fn animation_timeline_adds_frames_and_steps() {
        let mut h = harness();
        h.run();
        h.get_by_label("Animation").click();
        h.run();
        assert!(h.state().session.anim_settings().enabled);
        let frames = h.state().session.anim_frames().len();
        h.get_by_label("Add frame").click();
        h.run();
        assert_eq!(h.state().session.anim_frames().len(), frames + 1);
        let at = h.state().session.anim_slot();
        key(&mut h, egui::Key::Comma, egui::Modifiers::NONE);
        h.run();
        assert_eq!(h.state().session.anim_slot(), at - 1);
        key(&mut h, egui::Key::Period, egui::Modifiers::NONE);
        h.run();
        assert_eq!(h.state().session.anim_slot(), at);
        h.get_by_label("Play").click();
        h.run_steps(3);
        assert!(h.state().playback.playing);
        h.get_by_label("Pause").click();
        h.run_steps(2);
        assert!(!h.state().playback.playing);
        // The readout switches between frames and seconds.
        h.get_by_label("timeline metric: frames").click();
        h.run();
        assert_eq!(h.state().settings.timeline_metric, settings::TimelineMetric::Seconds);
        assert!(h.query_by_label("timeline metric: seconds").is_some());
    }

    #[test]
    fn frames_retime_in_a_batch_and_ghosts_slide_off_their_pegs() {
        let mut h = harness();
        h.run();
        h.get_by_label("Animation").click();
        h.run();
        for _ in 0..2 {
            h.get_by_label("Add frame").click();
            h.run();
        }
        let ids: Vec<String> = h.state().session.anim_frames().into_iter().map(|f| f.id).collect();
        assert!(ids.len() >= 3);
        // Select mode, two frames, Set duration (2 by default): one undo step.
        h.get_by_label("Select frames to retime (or Ctrl/Shift-click a frame)").click();
        h.run();
        assert!(h.state().timing.select_mode);
        h.state_mut().timing.selected = vec![ids[0].clone(), ids[2].clone()];
        h.run();
        let undo = h.state().session.history_len();
        h.get_by_label("Set duration").click();
        h.run();
        let holds: Vec<usize> = h.state().session.anim_frames().iter().map(|f| f.hold).collect();
        assert_eq!((holds[0], holds[1], holds[2]), (2, 1, 2));
        assert_eq!(h.state().session.history_len(), undo + 1);
        h.get_by_label("Done").click();
        h.run();
        assert!(h.state().timing.selected.is_empty(), "leaving select mode drops the batch");

        // Out of pegs: drag the canvas and the previous frame's ghost moves,
        // with no paint and no undo step.
        let _ = h.state_mut().session.execute(Command::SetActiveLayer { id: ids[1].clone() });
        h.state_mut().timing.set_peg_mode(true);
        h.state_mut().timing.peg_target = Some(ids[0].clone());
        h.run();
        let (undo, scale) = (h.state().session.history_len(), h.state().canvas.view.scale);
        drag_stroke(&mut h, pos2(600.0, 400.0), pos2(660.0, 430.0));
        h.run();
        let peg = h.state().session.anim_settings().out_of_pegs[&ids[0]];
        assert!((peg.dx - 60.0 / scale).abs() < 1.0 && (peg.dy - 30.0 / scale).abs() < 1.0, "{peg:?}");
        assert_eq!(h.state().session.history_len(), undo, "display only");
    }

    #[test]
    fn markers_and_the_play_range_from_the_keyboard() {
        let mut h = harness();
        h.run();
        h.get_by_label("Animation").click();
        h.run();
        for _ in 0..3 {
            h.get_by_label("Add frame").click();
            h.run();
        }
        let total = h.state().session.anim_total_slots();
        assert!(total >= 4);
        let goto = |h: &mut Harness<'_, App>, slot: usize| {
            let _ = h.state_mut().session.execute(Command::SetAnimSlot { slot });
        };
        goto(&mut h, 1);
        key(&mut h, egui::Key::M, egui::Modifiers::SHIFT);
        key(&mut h, egui::Key::I, egui::Modifiers::SHIFT);
        goto(&mut h, 2);
        key(&mut h, egui::Key::O, egui::Modifiers::SHIFT);
        let a = h.state().session.anim_settings().clone();
        assert_eq!((a.markers.len(), a.mark_in, a.mark_out), (1, Some(1), Some(2)));
        // Playing from outside the range starts at the in-point and stays in it.
        goto(&mut h, total - 1);
        timeline::set_playing(h.state_mut(), true);
        assert_eq!(h.state().session.anim_slot(), 1);
        for _ in 0..20 {
            h.step();
            std::thread::sleep(std::time::Duration::from_millis(15));
            assert!((1..=2).contains(&h.state().session.anim_slot()));
        }
        timeline::set_playing(h.state_mut(), false);
        // Alt+, jumps back to the marker.
        goto(&mut h, total - 1);
        key(&mut h, egui::Key::Comma, egui::Modifiers::ALT);
        assert_eq!(h.state().session.anim_slot(), 1);
        h.get_by_label("Clear range").click();
        h.run();
        assert!(!h.state().session.anim_settings().has_mark_range());
    }

    #[test]
    fn the_track_grid_converts_retimes_and_adds() {
        use onepaint_engine::anim::tracks_of;
        let mut h = harness();
        h.run();
        h.get_by_label("Animation").click();
        h.run();
        for _ in 0..2 {
            h.get_by_label("Add frame").click();
            h.run();
        }
        h.get_by_label("Convert to tracks").click();
        h.run();
        let starts = |h: &Harness<'_, App>| {
            tracks_of(&h.state().session.doc().root).iter().filter_map(|t| t.sheets.clone()).flatten().map(|f| f.start).collect::<Vec<_>>()
        };
        let before = starts(&h);
        let n = before.len();
        assert!(n >= 3, "{before:?}");
        // Drag the last sheet two columns right: one undo step.
        let cell = h.get_by_label(&format!("Sheet {n} of Track 1")).rect();
        let col = h.get_by_label("Timeline ruler").rect().width();
        let per = (col - 132.0) / h.state().session.anim_total_slots() as f32;
        let from = cell.center();
        h.event(egui::Event::PointerMoved(from));
        h.step();
        press(&mut h, from, true);
        for i in 1..=6 {
            h.event(egui::Event::PointerMoved(from + egui::vec2(per * 2.0 * i as f32 / 6.0, 0.0)));
            h.step();
        }
        press(&mut h, from + egui::vec2(per * 2.0, 0.0), false);
        let after = starts(&h);
        assert_eq!(after[n - 1], before[n - 1] + 2, "{after:?}");
        let _ = h.state_mut().session.execute(Command::Undo);
        assert_eq!(starts(&h), before, "the drag undoes in one step");
        // A gap takes a new drawing; + Track adds a row.
        let _ = h.state_mut().session.execute(Command::Redo);
        h.run();
        let gap = h.get_by_label(&format!("Track 1 column {}", before[n - 1] + 1)).rect().center();
        h.event(egui::Event::PointerMoved(gap));
        h.step();
        // A gap click moves the playhead there; + Drawing fills it.
        press(&mut h, gap, true);
        press(&mut h, gap, false);
        assert_eq!(h.state().session.anim_slot(), before[n - 1]);
        h.get_by_label("+ Drawing").click();
        h.run();
        assert_eq!(starts(&h).len(), n + 1);
        h.get_by_label("+ Track").click();
        h.run();
        assert_eq!(tracks_of(&h.state().session.doc().root).iter().filter(|t| t.sheets.is_some()).count(), 2);
    }

    #[test]
    fn export_dialog_switches_formats() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::E, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT);
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Export));
        h.get_by_label("JPEG").click();
        h.run();
        assert_eq!(h.state().export_opts.format, onepaint_formats::export::ExportFormat::Jpeg);
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Slider, "Quality").is_some());
        h.get_by_label("PSD").click();
        h.run();
        assert!(h.state().export_psd);
        h.get_by_label("Cancel").click();
        h.run();
        assert_eq!(h.state().popover, None);
    }

    #[test]
    fn the_adjustment_eyedropper_samples_the_canvas() {
        let mut h = harness();
        h.run();
        let red = onepaint_color::Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let _ = h.state_mut().session.execute(Command::SetColor { rgba: red });
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 420.0));
        let _ = h.state_mut().session.execute(Command::SetColor { rgba: onepaint_color::Rgba::BLACK });
        let _ = h.state_mut().session.execute(Command::BeginFilter { kind: "color-to-alpha".into() });
        h.state_mut().popover = Some(Popover::Filter);
        h.run();
        h.get_by_label("Sample Colour").click();
        h.run();
        click(&mut h, pos2(640.0, 420.0));
        h.run();
        let target = h.state().session.filter_state().unwrap().1.to_json().get("target").cloned().unwrap();
        let t: [f32; 3] = serde_json::from_value(target).unwrap();
        assert!(t[0] > 0.9 && t[1] < 0.1 && t[2] < 0.1, "sampled red: {t:?}");
        assert_eq!(h.state().popover, Some(Popover::Filter));
        assert!(h.state().session.filter_active(), "sampling doesn't end the session");
    }

    #[test]
    fn shortcuts_can_be_rebound_from_settings() {
        let mut h = harness();
        h.run();
        h.state_mut().popover = Some(Popover::Settings);
        // The shortcut rows are below the fold of the dialog's scroll area.
        h.step();
        h.step();
        let mut last = egui::Rect::NOTHING;
        for _ in 0..30 {
            h.get_by_label("Smudge shortcut").scroll_to_me();
            h.step();
            let r = h.get_by_label("Smudge shortcut").rect();
            if r == last {
                break;
            }
            last = r;
        }
        h.get_by_label("Smudge shortcut").click();
        h.step();
        assert_eq!(h.state().capture, Some(Action::ToolSmudge));
        // Shift+S becomes Smudge; plain S no longer is.
        key(&mut h, egui::Key::S, egui::Modifiers::SHIFT);
        assert_eq!(h.state().settings.keybindings.get("tool-smudge").map(String::as_str), Some("shift+s"));
        assert_eq!(h.state().capture, None);
        // A reserved key is refused.
        h.get_by_label("Smudge shortcut").click();
        h.step();
        key(&mut h, egui::Key::Z, egui::Modifiers::COMMAND);
        assert_eq!(h.state().settings.keybindings.get("tool-smudge").map(String::as_str), Some("shift+s"));
        h.state_mut().popover = None;
        h.run();
        key(&mut h, egui::Key::S, egui::Modifiers::NONE);
        assert_ne!(h.state().session.tool(), Tool::Smudge);
        key(&mut h, egui::Key::S, egui::Modifiers::SHIFT);
        assert_eq!(h.state().session.tool(), Tool::Smudge);
        // Reset all brings S back.
        h.state_mut().popover = Some(Popover::Settings);
        for _ in 0..10 {
            h.step();
        }
        h.get_by_label("Reset all shortcuts").click();
        h.run();
        assert!(h.state().settings.keybindings.is_empty());
        h.state_mut().popover = None;
        h.run();
        key(&mut h, egui::Key::B, egui::Modifiers::NONE);
        key(&mut h, egui::Key::S, egui::Modifiers::NONE);
        assert_eq!(h.state().session.tool(), Tool::Smudge);
    }

    #[test]
    fn settings_dialog_drives_the_engine_and_the_tab_cap() {
        let mut h = harness();
        h.run();
        h.state_mut().popover = Some(Popover::Menu);
        h.run();
        h.get_by_label("Settings…").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Settings));
        // The centred dialog settles over a few frames.
        let mut last = egui::Rect::NOTHING;
        for _ in 0..20 {
            h.step();
            let r = h.get_by_label("Always use pressure for size").rect();
            if r == last {
                break;
            }
            last = r;
        }
        h.get_by_label("Always use pressure for size").click();
        h.run();
        assert!(h.state().settings.force_size_pressure);
        assert!(h.state().session.preferences().force_size_pressure);
        h.get_by_label("Relative to screen").click();
        h.run();
        let zoom = h.state().canvas.view.scale;
        assert_eq!(h.state().session.preferences().screen_zoom, Some(zoom));
        // Clean tabs past the cap are closed, oldest first.
        h.state_mut().settings.max_open_tabs = 3;
        for _ in 0..4 {
            h.state_mut().new_tab();
        }
        assert_eq!(h.state().tabs.len(), 3);
        assert_eq!(h.state().active_tab, 2);
    }

    #[test]
    fn a_filter_becomes_a_smart_filter_and_bakes() {
        let mut h = harness();
        h.run();
        let _ = h.state_mut().session.execute(Command::BeginFilter { kind: "invert".into() });
        h.state_mut().popover = Some(Popover::Filter);
        h.run();
        h.get_by_label("As smart filter").click();
        h.run();
        let id = h.state().session.active().to_string();
        assert_eq!(h.state().session.smart_filters(&id).len(), 1);
        assert_eq!(h.state().popover, Some(Popover::SmartFilters));
        assert!(!h.state().session.filter_active(), "the destructive preview was cancelled");
        h.get_by_label("Bake into pixels").click();
        h.run();
        assert!(h.state().session.smart_filters(&id).is_empty());
    }

    #[test]
    fn a_recorded_session_replays_on_the_canvas() {
        let mut h = harness();
        h.run();
        h.state_mut().popover = Some(Popover::Menu);
        h.run();
        h.get_by_label("Start recording").click();
        h.run();
        assert!(h.state().session.is_recording());
        drag_stroke(&mut h, pos2(500.0, 420.0), pos2(720.0, 440.0));
        let rec = h.state_mut().session.stop_recording().unwrap();
        let want = h.state().session.flatten();
        let (session, events) = onepaint_engine::recording::Replay::new(&rec).unwrap().into_parts();
        h.state_mut().session = session;
        h.state_mut().replaying = Some((events, 0));
        for _ in 0..300 {
            h.step();
            if h.state().replaying.is_none() {
                break;
            }
        }
        assert!(h.state().replaying.is_none());
        assert_eq!(h.state().session.flatten(), want);
    }

    #[test]
    fn distort_from_the_move_popover() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(500.0, 420.0), pos2(700.0, 420.0));
        key(&mut h, egui::Key::V, egui::Modifiers::NONE);
        h.run();
        assert!(h.state().session.transform_active());
        h.get_by_label("Move / transform (V)").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::MoveOptions));
        h.get_by_label("Distort").click();
        h.run();
        let q = match h.state().session.transform_warp() {
            Some(onepaint_engine::transform::WarpMode::Quad(q)) => *q,
            _ => panic!("distort mode"),
        };
        h.state_mut().popover = None;
        h.run();
        // Drag the bottom-right corner down 80 px.
        let br = h.state().canvas.view.doc_to_screen(q[2]);
        drag_stroke(&mut h, pos2(br[0] as f32, br[1] as f32), pos2(br[0] as f32, br[1] as f32 + 80.0));
        let q2 = match h.state().session.transform_warp() {
            Some(onepaint_engine::transform::WarpMode::Quad(q)) => *q,
            _ => panic!(),
        };
        assert!(q2[2][1] > q[2][1] + 50.0, "corner moved: {q:?} -> {q2:?}");
        assert_eq!(q2[0], q[0], "other corners stay");
    }

    #[test]
    fn gradient_can_make_a_fill_layer() {
        let mut h = harness();
        h.run();
        h.state_mut().set_tool(Tool::Gradient);
        h.state_mut().canvas.options.gradient_layer = true;
        let layers = h.state().session.doc().root.len();
        drag_stroke(&mut h, pos2(400.0, 300.0), pos2(800.0, 300.0));
        let app = h.state();
        assert_eq!(app.session.doc().root.len(), layers + 1);
        let l = app.session.doc().layer(app.session.active()).unwrap();
        let adj = l.adjustment.as_ref().unwrap();
        assert_eq!(adj.kind, "gradient-fill");
        let a = app.canvas.view.screen_to_doc([400.0, 300.0]);
        assert!((adj.params.grad_ax.unwrap() as f64 - a[0] / 2048.0).abs() < 0.01);
        assert!(adj.params.grad_bx.unwrap() > adj.params.grad_ax.unwrap());
    }

    #[test]
    fn happypaint_shortcuts() {
        let mut h = harness();
        h.run();
        h.state_mut().set_color(Rgba::from_hex("#ff0000").unwrap());
        key(&mut h, egui::Key::X, egui::Modifiers::NONE);
        assert_eq!(h.state().session.color().to_hex(), "#ffffff", "X swaps with the background");
        assert_eq!(h.state().canvas.options.background.to_hex(), "#ff0000");
        key(&mut h, egui::Key::D, egui::Modifiers::NONE);
        assert_eq!(h.state().session.color().to_hex(), "#000000");
        let s0 = h.state().canvas.view.scale;
        key(&mut h, egui::Key::Equals, egui::Modifiers::NONE);
        assert!(h.state().canvas.view.scale > s0);
        key(&mut h, egui::Key::H, egui::Modifiers::NONE);
        assert_eq!(h.state().session.tool(), Tool::Hand);
        let off = h.state().canvas.view.doc_to_screen([0.0, 0.0]);
        drag_stroke(&mut h, pos2(500.0, 400.0), pos2(560.0, 400.0));
        let moved = h.state().canvas.view.doc_to_screen([0.0, 0.0]);
        assert!((moved[0] - off[0] - 60.0).abs() < 1.0, "hand drag pans");
        assert_eq!(h.state().session.doc().layer(h.state().session.active()).unwrap().pixels.tile_count(), 0);
        key(&mut h, egui::Key::Questionmark, egui::Modifiers::SHIFT);
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Help));
        key(&mut h, egui::Key::A, egui::Modifiers::SHIFT);
        assert!(h.state().session.anim_settings().enabled, "Shift+A toggles animation");
    }

    #[test]
    fn fill_mode_fills_the_outline() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::F, egui::Modifiers::NONE);
        assert!(h.state().canvas.options.fill_shape);
        h.event(Event::PointerMoved(pos2(500.0, 300.0)));
        h.step();
        press(&mut h, pos2(500.0, 300.0), true);
        for p in [pos2(600.0, 300.0), pos2(700.0, 300.0), pos2(650.0, 400.0), pos2(550.0, 400.0)] {
            h.event(Event::PointerMoved(p));
            h.step();
        }
        press(&mut h, pos2(550.0, 400.0), false);
        h.run();
        assert!(alpha_at(&h, [600.0, 350.0]) > 0.9, "inside filled");
        assert_eq!(alpha_at(&h, [450.0, 350.0]), 0.0);
    }

    #[test]
    fn focus_lines_make_an_editable_path_layer() {
        let mut h = harness();
        h.run();
        h.state_mut().popover = Some(Popover::Adjustments);
        h.run();
        assert!(h.query_by_label("Focus lines").is_some(), "listed under Effect lines");
        crate::adjust::effect_lines(h.state_mut(), true);
        h.run();
        let app = h.state();
        let id = app.session.active().to_string();
        assert!(app.session.is_path_layer(&id));
        assert_eq!(app.session.paths(&id)[0].subpaths.len(), 90);
        assert!(app.session.doc().layer(&id).unwrap().pixels.tile_count() > 0, "stroked");
    }

    #[test]
    fn reference_window_toggles_and_snapshots_the_canvas() {
        let mut h = harness();
        h.run();
        key(&mut h, egui::Key::R, egui::Modifiers::ALT);
        h.run();
        assert!(h.state().reference.open);
        assert!(h.query_by_label("Use a snapshot of the canvas").is_some());
        h.get_by_label("Use a snapshot of the canvas").click();
        h.run();
        let r = h.state().reference.image.as_ref().unwrap();
        assert_eq!((r.width, r.height), (2048, 1536));
        assert_eq!(r.color_at(10, 10).unwrap().to_hex(), "#ffffff");
    }

    #[test]
    fn opened_files_appear_in_the_gallery() {
        let mut h = harness();
        h.run();
        let dir = std::env::temp_dir().join(format!("onepaint-galtest-{}", onepaint_doc::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("sky.png");
        std::fs::write(&png, onepaint_formats::encode_png_rgba8(4, 4, &[90u8; 64]).unwrap()).unwrap();
        h.state_mut().open_path(png.clone());
        h.run();
        assert_eq!(h.state().gallery.recent.first(), Some(&png.canonicalize().unwrap()));
        h.state_mut().popover = Some(Popover::Gallery);
        h.run();
        assert!(h.query_by_label("New drawing").is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_book_opens_as_tabs() {
        let mut h = harness();
        h.run();
        let pages: Vec<Vec<u8>> = [(320u32, 200u32), (640, 480)]
            .iter()
            .map(|&(w, hh)| Session::new(w, hh, Some(Rgba::WHITE)).save_happypaint().unwrap())
            .collect();
        let book = onepaint_formats::book::encode_book("Ch", &["A".into(), "B".into()], &pages, "t").unwrap();
        let dir = std::env::temp_dir().join(format!("onepaint-book-{}", onepaint_doc::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ch.happypaint-book");
        std::fs::write(&path, book).unwrap();
        h.state_mut().open_path(path);
        h.run();
        assert_eq!(h.state().tabs.len(), 2, "the blank start drawing was reused");
        assert_eq!(h.state().session.doc().width, 640);
        key(&mut h, egui::Key::ArrowLeft, egui::Modifiers::COMMAND | egui::Modifiers::ALT);
        assert_eq!(h.state().session.doc().width, 320);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn touch(h: &mut Harness<'_, App>, id: u64, phase: egui::TouchPhase, p: egui::Pos2) {
        h.event(Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(id), phase, pos: p, force: None });
    }

    #[test]
    fn multi_finger_taps_undo_and_redo() {
        // Real touch timing: 60 fps frames, so a quick tap is a tap.
        let mut h = Harness::builder()
            .with_size(vec2(1200.0, 800.0))
            .with_max_steps(400)
            .with_step_dt(1.0 / 60.0)
            .build_eframe(|cc| App::new(cc, None));
        h.run();
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 440.0));
        let painted = h.state().session.history_len();
        assert!(h.state().session.can_undo());
        for (n, want_undo) in [(2u64, true), (3, false)] {
            for i in 0..n {
                touch(&mut h, i, egui::TouchPhase::Start, pos2(300.0 + i as f32 * 60.0, 300.0));
            }
            h.step();
            for i in 0..n {
                touch(&mut h, i, egui::TouchPhase::End, pos2(300.0 + i as f32 * 60.0, 300.0));
            }
            h.step();
            h.step();
            assert_eq!(h.state().session.can_redo(), want_undo, "{n}-finger tap");
        }
        assert_eq!(h.state().session.history_len(), painted);
    }

    #[test]
    fn touch_and_hold_picks_the_colour_under_the_finger() {
        let mut h = Harness::builder()
            .with_size(vec2(1200.0, 800.0))
            .with_max_steps(400)
            .with_step_dt(1.0 / 60.0)
            .build_eframe(|cc| App::new(cc, None));
        h.run();
        let red = onepaint_color::Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        let _ = h.state_mut().session.execute(Command::SetColor { rgba: red });
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 420.0));
        let blue = onepaint_color::Rgba { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
        let _ = h.state_mut().session.execute(Command::SetColor { rgba: blue });
        let steps = h.state().session.history_len();
        touch(&mut h, 1, egui::TouchPhase::Start, pos2(640.0, 420.0));
        for _ in 0..30 {
            h.step();
        }
        assert!(h.state().canvas.gestures_holding(), "half a second still is a hold");
        touch(&mut h, 1, egui::TouchPhase::End, pos2(640.0, 420.0));
        h.step();
        let c = h.state().session.color();
        assert!(c.r > 0.9 && c.b < 0.1, "picked red: {c:?}");
        assert_eq!(h.state().session.history_len(), steps, "the hold leaves no stroke");
    }
}
