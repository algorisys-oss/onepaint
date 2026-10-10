//! Opening files in the background (spec 41): the file is read and decoded
//! on a worker thread while the UI keeps running, with a progress card and
//! Cancel. The decoded document is installed on the UI thread.

use crate::FileKind;
use onepaint_doc::Document;
use onepaint_engine::Progress;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Show the card only for loads that take longer than this, so quick opens don't flash.
const SHOW_AFTER: Duration = Duration::from_millis(150);

pub struct Loading {
    pub path: PathBuf,
    pub kind: FileKind,
    pub name: String,
    pub progress: Progress,
    /// An autosave being recovered: it opens as unsaved work.
    pub recovered: bool,
    /// The tab and document the load replaces; if either changed meanwhile
    /// (a book opened, a new drawing, a tab switch), the result is dropped.
    pub target: (usize, String),
    started: Instant,
    rx: mpsc::Receiver<Result<Document, String>>,
}

/// How a load ended.
pub enum Outcome {
    Loaded(Document),
    Cancelled,
    Failed(String),
}

impl Loading {
    /// Start reading `path` on a worker thread. Books open on the UI thread.
    pub fn start(path: PathBuf, kind: FileKind, target: (usize, String)) -> Self {
        let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Image").to_string();
        let progress = Progress::default();
        let (tx, rx) = mpsc::channel();
        let (p, n, job) = (progress.clone(), name.clone(), path.clone());
        std::thread::spawn(move || {
            let doc = std::fs::read(&job).map_err(|e| e.to_string()).and_then(|b| {
                p.check().map_err(|e| e.to_string())?;
                match kind {
                    FileKind::Native => onepaint_engine::decode_happypaint(&b, &p),
                    FileKind::Psd => onepaint_engine::decode_psd(&b, &n, &p),
                    FileKind::Image | FileKind::Book => onepaint_engine::decode_image(&b, &n),
                }
                .map_err(|e| e.to_string())
            });
            // The receiver is gone if the app moved on; nothing to do then.
            let _ = tx.send(doc);
        });
        Self { path, kind, name, progress, recovered: false, target, started: Instant::now(), rx }
    }

    /// The result, once the worker is done.
    pub fn poll(&self) -> Option<Outcome> {
        match self.rx.try_recv() {
            Ok(r) => Some(self.outcome(r)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(self.outcome(Err("the reader stopped".into()))),
        }
    }

    /// Block until the worker is done (tests).
    #[cfg(test)]
    pub fn wait(&self) -> Outcome {
        match self.rx.recv() {
            Ok(r) => self.outcome(r),
            Err(_) => self.outcome(Err("the reader stopped".into())),
        }
    }

    /// A load that only ends when the returned sender sends or is dropped (tests).
    #[cfg(test)]
    pub fn stalled(name: &str) -> (Self, mpsc::Sender<Result<Document, String>>) {
        let (tx, rx) = mpsc::channel();
        let path = PathBuf::from(format!("{name}.happypaint"));
        let l = Self {
            path,
            kind: FileKind::Native,
            name: name.into(),
            progress: Progress::default(),
            recovered: false,
            target: (0, String::new()),
            started: Instant::now() - SHOW_AFTER,
            rx,
        };
        (l, tx)
    }

    fn outcome(&self, r: Result<Document, String>) -> Outcome {
        match r {
            Ok(doc) if !self.progress.is_cancelled() => Outcome::Loaded(doc),
            Ok(_) => Outcome::Cancelled,
            Err(_) if self.progress.is_cancelled() => Outcome::Cancelled,
            Err(e) => Outcome::Failed(e),
        }
    }

    pub fn visible(&self) -> bool {
        self.started.elapsed() >= SHOW_AFTER
    }
}

/// The modal progress card; it blocks the canvas until the load ends.
pub fn show(app: &mut crate::App, ctx: &egui::Context) {
    let Some(l) = &app.loading else { return };
    ctx.request_repaint_after(Duration::from_millis(30));
    if !l.visible() {
        return;
    }
    let t = onepaint_ui_kit::Tokens::get(ctx);
    let mut cancel = false;
    egui::Modal::new(egui::Id::new("loading")).backdrop_color(t.shadow).frame(egui::Frame::NONE).show(ctx, |ui| {
        onepaint_ui_kit::widgets::popover_frame(ui).show(ui, |ui| {
            ui.set_width(300.0);
            ui.label(egui::RichText::new(format!("Opening {}", l.name)).font(onepaint_ui_kit::theme::medium(14.0)).color(t.fg));
            ui.add_space(10.0);
            onepaint_ui_kit::widgets::progress_bar(ui, l.progress.fraction(), 300.0);
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel = onepaint_ui_kit::widgets::secondary_button(ui, "Cancel", 88.0).clicked();
            });
        });
    });
    if cancel {
        l.progress.cancel();
    }
}
