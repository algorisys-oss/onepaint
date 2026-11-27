//! Saving in the background: the UI thread settles and snapshots the
//! drawing (`Session::prepare_save`); a worker composites the thumbnail,
//! encodes (reusing the last save's chunks) and writes the file atomically.
//! Painting goes on meanwhile; edits made during the save stay unsaved.

use onepaint_engine::SaveJob;
use std::path::PathBuf;
use std::sync::mpsc;

/// What is being saved.
pub enum What {
    /// One drawing, and the history step it was at when snapshotted. `then`
    /// runs once it is saved (the unsaved-changes prompt's Save).
    Drawing { doc_id: String, history_len: usize, then: Option<crate::Pending> },
    /// Every open tab as a book, with this many pages.
    Book { pages: usize },
}

pub struct Saving {
    pub path: PathBuf,
    pub what: What,
    rx: mpsc::Receiver<Result<(), String>>,
}

impl Saving {
    /// Save one drawing to `path`.
    pub fn drawing(job: SaveJob, path: PathBuf, what: What) -> Self {
        let target = path.clone();
        Self::spawn(path, what, move || {
            let bytes = job.run().map_err(|e| e.to_string())?;
            onepaint_formats::write_atomic(&target, &bytes).map_err(|e| e.to_string())
        })
    }

    /// Save `pages` (each tab's name and job, in order) as a book named
    /// after `path`.
    pub fn book(pages: Vec<(String, SaveJob)>, path: PathBuf) -> Self {
        let target = path.clone();
        let what = What::Book { pages: pages.len() };
        Self::spawn(path, what, move || {
            let name = target.file_stem().and_then(|s| s.to_str()).unwrap_or("Chapter").to_string();
            let (mut names, mut files) = (Vec::new(), Vec::new());
            for (n, job) in pages {
                files.push(job.run().map_err(|e| e.to_string())?);
                names.push(n);
            }
            let bytes = onepaint_formats::book::encode_book(&name, &names, &files, concat!("onepaint ", env!("CARGO_PKG_VERSION")))
                .map_err(|e| e.to_string())?;
            onepaint_formats::write_atomic(&target, &bytes).map_err(|e| e.to_string())
        })
    }

    fn spawn(path: PathBuf, what: What, work: impl FnOnce() -> Result<(), String> + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            // The receiver is gone only if the app moved on.
            let _ = tx.send(work());
        });
        Self { path, what, rx }
    }

    /// The result, once the worker is done.
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("the save stopped".into())),
        }
    }

    /// Block until it is done (closing the app, tests).
    pub fn wait(&self) -> Result<(), String> {
        self.rx.recv().unwrap_or_else(|_| Err("the save stopped".into()))
    }
}
