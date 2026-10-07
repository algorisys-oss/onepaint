//! onepaint desktop app: a thin egui shell over `onepaint_engine::Session`.

mod adjust;
mod autosave;
mod canvas;
mod chrome;
mod clip;
mod debug;
mod progressive;
mod text_ui;
mod thumbs;

use canvas::Canvas;
use egui::{Key, KeyboardShortcut, Modifiers};
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
}

/// What a file opens as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Native,
    Psd,
    Image,
}

/// How a file opens, by extension: `.happypaint`, PSD/PSB, or any other image.
pub fn kind_of(p: &std::path::Path) -> FileKind {
    match p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("happypaint") => FileKind::Native,
        Some("psd" | "psb") => FileKind::Psd,
        _ => FileKind::Image,
    }
}

/// An action that would discard unsaved work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    New(u32, u32),
    Open,
    Close,
}

pub struct App {
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
    pub thumbs: thumbs::Thumbs,
    pub toast: Option<(String, f64)>,
    pub new_doc: [u32; 2],
    pub gpu_ok: bool,
    pub debug: debug::Debug,
    title: String,
    pub pending: Option<Pending>,
    pub last_select_tool: Tool,
    pub last_symmetry: onepaint_engine::SymmetryMode,
    xform_layer: Option<String>,
    pub clipboard: clip::Clipboard,
    /// Layer being dragged in the layers panel.
    pub layer_drag: Option<String>,
    /// Canvas/image size dialog state: size, anchor (col, row), keep aspect.
    pub size_dialog: ([u32; 2], (u8, u8), bool),
    pub autosave: autosave::Autosave,
    /// A leftover autosave offered for recovery at launch.
    pub recover: Option<PathBuf>,
    /// Selected tab in the curves editor.
    pub curve_tab: usize,
    /// Category shown in the brush library.
    pub brush_category: String,
    /// A file chosen by drag-and-drop, opened once unsaved work is dealt with.
    pub open_later: Option<PathBuf>,
    allow_close: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext, open: Option<PathBuf>) -> Self {
        let debug = debug::Debug::from_env();
        let theme = if debug.light { ThemeKind::Light } else { ThemeKind::Dark };
        onepaint_ui_kit::install(&cc.egui_ctx, theme);
        let gpu_ok = canvas::init_gpu(cc);
        let session = Session::new(2048, 1536, Some(Rgba::WHITE));
        let size = [session.doc().width, session.doc().height];
        let mut app = Self {
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
            thumbs: thumbs::Thumbs::default(),
            toast: None,
            new_doc: [2048, 1536],
            gpu_ok,
            debug,
            title: String::new(),
            pending: None,
            last_select_tool: Tool::SelectRect,
            last_symmetry: onepaint_engine::SymmetryMode::Vertical,
            xform_layer: None,
            clipboard: clip::Clipboard::default(),
            layer_drag: None,
            size_dialog: ([0, 0], (1, 1), true),
            autosave: autosave::Autosave::new(if cfg!(test) { None } else { autosave::dir() }),
            recover: None,
            curve_tab: 0,
            brush_category: String::new(),
            open_later: None,
            allow_close: false,
        };
        if let Some(p) = open {
            app.open_path(p);
        } else if !app.debug.scripted()
            && let Some(p) = app.autosave.leftover()
        {
            app.recover = Some(p);
            app.popover = Some(Popover::Recover);
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
        let c = self.session.color();
        if self.recent.first() != Some(&c) {
            self.recent.retain(|r| r.to_hex() != c.to_hex());
            self.recent.insert(0, c);
            self.recent.truncate(12);
        }
    }

    pub fn new_document(&mut self, w: u32, h: u32) {
        let id = self.session.doc().id.clone();
        self.autosave.clear(&id);
        self.session.replace_document(onepaint_doc::Document::new(w, h, Some(Rgba::WHITE)));
        self.canvas.reset_for([w, h]);
        self.path = None;
        self.saved_history_len = Some(0);
        self.thumbs.clear();
    }

    pub fn open_path(&mut self, p: PathBuf) {
        let now = 0.0;
        let kind = kind_of(&p);
        let native = kind == FileKind::Native;
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("Image").to_string();
        match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| {
            match kind {
                FileKind::Native => self.session.open_happypaint(&b),
                FileKind::Psd => self.session.open_psd(&b, &name),
                FileKind::Image => self.session.open_image(&b, &name),
            }
            .map_err(|e| e.to_string())
        }) {
            Ok(()) => {
                let d = self.session.doc();
                self.canvas.reset_for([d.width, d.height]);
                // An imported image saves as a new .happypaint, never over the source file.
                self.path = native.then_some(p);
                self.saved_history_len = Some(0);
                self.thumbs.clear();
            }
            Err(e) => self.notify(format!("Couldn't open {}: {e}", p.display()), now),
        }
    }

    /// Run `action`, or ask first when there are unsaved changes.
    pub fn request(&mut self, action: Pending, ctx: &egui::Context) {
        if self.dirty() {
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
        }
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Images and documents", &["happypaint", "psd", "psb", "png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"])
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
                self.path = Some(path);
            }
            Err(e) => self.notify(format!("Save failed: {e}"), now),
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
        if sc(cmd | Modifiers::SHIFT, Key::Z) || sc(cmd, Key::Y) {
            let _ = self.session.execute(Command::Redo);
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
        if sc(cmd | Modifiers::SHIFT, Key::E) {
            self.export_png(now);
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
            if sc(Modifiers::ALT, Key::Backspace) {
                let _ = self.session.execute(Command::FillSelection);
            }
            if sc(Modifiers::NONE, Key::Delete) || sc(Modifiers::NONE, Key::Backspace) {
                let _ = self.session.execute(Command::ClearSelected);
            }
            for (key, tool) in [
                (Key::M, Tool::SelectRect),
                (Key::L, Tool::SelectLasso),
                (Key::W, Tool::SelectWand),
                (Key::G, Tool::Fill),
                (Key::I, Tool::Eyedropper),
            ] {
                if sc(Modifiers::NONE, key) {
                    self.set_tool(tool);
                }
            }
            // Shift+B first: a plain-B shortcut also matches with Shift held.
            if sc(Modifiers::SHIFT, Key::B) {
                self.set_tool(Tool::Mixer);
            }
            if sc(Modifiers::NONE, Key::B) {
                self.set_tool(Tool::Brush);
            }
            if sc(Modifiers::NONE, Key::S) {
                self.set_tool(Tool::Smudge);
            }
            if sc(Modifiers::NONE, Key::Y) {
                self.toggle_symmetry();
            }
            if sc(Modifiers::SHIFT, Key::P) && self.session.guides().perspective.enabled {
                self.canvas.persp_editing = !self.canvas.persp_editing;
            }
            if sc(Modifiers::NONE, Key::E) {
                self.set_tool(Tool::Eraser);
            }
            if sc(Modifiers::NONE, Key::T) {
                self.set_tool(Tool::Text);
            }
            if sc(Modifiers::NONE, Key::U) {
                self.set_tool(Tool::Shape);
            }
            if sc(Modifiers::NONE, Key::V) || sc(cmd, Key::T) {
                self.set_tool(Tool::Move);
            }
            if self.session.transform_active() && sc(Modifiers::NONE, Key::Enter) {
                self.commit_transform();
            }
            if sc(Modifiers::NONE, Key::OpenBracket) {
                let s = self.session.current_brush().size;
                let _ = self.session.execute(Command::SetBrushSize { size: (s / 1.15).floor().max(1.0) });
                self.canvas.size_flash = 1.0;
            }
            if sc(Modifiers::NONE, Key::CloseBracket) {
                let s = self.session.current_brush().size;
                let _ = self.session.execute(Command::SetBrushSize { size: (s * 1.15).ceil() });
                self.canvas.size_flash = 1.0;
            }
            if sc(Modifiers::NONE, Key::Tab) {
                self.hide_ui = !self.hide_ui;
            }
            if sc(Modifiers::NONE, Key::Escape) && !self.canvas.cancel_poly() {
                if self.canvas.persp_editing || self.canvas.sym_editing {
                    self.canvas.persp_editing = false;
                    self.canvas.sym_editing = false;
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
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        debug::tick(self, &ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Pending::Close, &ctx);
        }
        let (space, r) = self.shortcuts(&ctx);
        let rect = ui.max_rect();
        let was_stroking = self.session.is_stroking();
        let t = onepaint_ui_kit::Tokens::get(&ctx);
        ui.painter().rect_filled(rect, 0.0, t.stage);
        self.canvas.show(ui, rect, &mut self.session, space, r, self.gpu_ok);
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
    fn unsaved_work_asks_before_new_canvas() {
        let mut h = harness();
        h.run();
        drag_stroke(&mut h, pos2(560.0, 420.0), pos2(720.0, 440.0));
        let ctx = h.ctx.clone();
        h.state_mut().request(Pending::New(800, 600), &ctx);
        h.run();
        assert_eq!(h.state().popover, Some(Popover::Confirm));
        assert_eq!(h.state().session.doc().width, 2048, "nothing discarded yet");
        h.get_by_label("Cancel").click();
        h.run();
        assert_eq!(h.state().pending, None);
        assert!(h.state().dirty());
        h.state_mut().request(Pending::New(800, 600), &ctx);
        h.run();
        h.get_by_label("Discard").click();
        h.run();
        assert_eq!(h.state().session.doc().width, 800);
        assert!(!h.state().dirty());
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
}
