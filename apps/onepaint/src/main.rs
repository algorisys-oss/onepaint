//! onepaint desktop app: a thin egui shell over `onepaint_engine::Session`.

mod canvas;
mod chrome;
mod debug;
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
    eframe::run_native(
        "onepaint",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, open)))),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Popover {
    Menu,
    Brushes,
    Layers,
    Color,
    NewDoc,
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
}

impl App {
    fn new(cc: &eframe::CreationContext, open: Option<PathBuf>) -> Self {
        let debug = debug::Debug::from_env();
        let theme = if debug.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        };
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
            hsv: Hsv {
                h: 0.0,
                s: 0.0,
                v: 0.0,
            },
            hex: "#000000".into(),
            recent: Vec::new(),
            thumbs: thumbs::Thumbs::default(),
            toast: None,
            new_doc: [2048, 1536],
            gpu_ok,
            debug,
        };
        if let Some(p) = open {
            app.open_path(p);
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
        self.session
            .replace_document(onepaint_doc::Document::new(w, h, Some(Rgba::WHITE)));
        self.canvas.reset_for([w, h]);
        self.path = None;
        self.saved_history_len = Some(0);
        self.thumbs.clear();
    }

    pub fn open_path(&mut self, p: PathBuf) {
        let now = 0.0;
        match std::fs::read(&p)
            .map_err(|e| e.to_string())
            .and_then(|b| self.session.open_happypaint(&b).map_err(|e| e.to_string()))
        {
            Ok(()) => {
                let d = self.session.doc();
                self.canvas.reset_for([d.width, d.height]);
                self.path = Some(p);
                self.saved_history_len = Some(0);
                self.thumbs.clear();
            }
            Err(e) => self.notify(format!("Couldn't open {}: {e}", p.display()), now),
        }
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("happypaint", &["happypaint"])
            .pick_file()
        {
            self.open_path(p);
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
        match self
            .session
            .save_happypaint()
            .map_err(|e| e.to_string())
            .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()))
        {
            Ok(()) => {
                self.saved_history_len = Some(self.session.history_len());
                self.notify(format!("Saved {}", path.display()), now);
                self.path = Some(path);
            }
            Err(e) => self.notify(format!("Save failed: {e}"), now),
        }
    }

    pub fn export_png(&mut self, now: f64) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .set_file_name(format!("{}.png", self.session.doc().name))
            .save_file()
        else {
            return;
        };
        match self
            .session
            .export_png()
            .map_err(|e| e.to_string())
            .and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string()))
        {
            Ok(()) => self.notify(format!("Exported {}", path.display()), now),
            Err(e) => self.notify(format!("Export failed: {e}"), now),
        }
    }

    pub fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.theme = self.theme.toggled();
        onepaint_ui_kit::theme::apply(ctx, self.theme);
    }

    fn shortcuts(&mut self, ctx: &egui::Context) -> (bool, bool) {
        let now = ctx.input(|i| i.time);
        let cmd = Modifiers::COMMAND;
        let sc = |m: Modifiers, k: Key| {
            ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)))
        };
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
            self.open_dialog();
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
            let c = [
                self.canvas.view.view_size[0] / 2.0,
                self.canvas.view.view_size[1] / 2.0,
            ];
            self.canvas.view.zoom_at(c, 1.25);
        }
        if sc(cmd, Key::Minus) {
            let c = [
                self.canvas.view.view_size[0] / 2.0,
                self.canvas.view.view_size[1] / 2.0,
            ];
            self.canvas.view.zoom_at(c, 0.8);
        }
        if !typing {
            if sc(Modifiers::NONE, Key::B) {
                let _ = self.session.execute(Command::SetTool { tool: Tool::Brush });
            }
            if sc(Modifiers::NONE, Key::E) {
                let _ = self
                    .session
                    .execute(Command::SetTool { tool: Tool::Eraser });
            }
            if sc(Modifiers::NONE, Key::OpenBracket) {
                let s = self.session.current_brush().size;
                let _ = self.session.execute(Command::SetBrushSize {
                    size: (s / 1.15).floor().max(1.0),
                });
                self.canvas.size_flash = 1.0;
            }
            if sc(Modifiers::NONE, Key::CloseBracket) {
                let s = self.session.current_brush().size;
                let _ = self.session.execute(Command::SetBrushSize {
                    size: (s * 1.15).ceil(),
                });
                self.canvas.size_flash = 1.0;
            }
            if sc(Modifiers::NONE, Key::Tab) {
                self.hide_ui = !self.hide_ui;
            }
            if sc(Modifiers::NONE, Key::Escape) {
                self.popover = None;
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
        let (space, r) = self.shortcuts(&ctx);
        let rect = ui.max_rect();
        let was_stroking = self.session.is_stroking();
        let t = onepaint_ui_kit::Tokens::get(&ctx);
        ui.painter().rect_filled(rect, 0.0, t.stage);
        if self.gpu_ok {
            self.canvas.show(ui, rect, &mut self.session, space, r);
        } else {
            ui.centered_and_justified(|ui| ui.label("onepaint needs a GPU (wgpu) renderer."));
        }
        if was_stroking && !self.session.is_stroking() {
            self.push_recent();
        }
        for e in self.session.drain_events() {
            match e {
                Event::LayersChanged | Event::DocumentReplaced => self.thumbs.clear(),
                Event::HistoryChanged { .. } => {
                    self.thumbs.invalidate_active(self.session.active())
                }
                _ => {}
            }
        }
        if !self.hide_ui {
            chrome::show(self, &ctx);
        }
        let title = format!(
            "{}{} — onepaint",
            if self.dirty() { "• " } else { "" },
            self.session.doc().name
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if self.session.is_stroking() {
            ctx.request_repaint();
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }
}
