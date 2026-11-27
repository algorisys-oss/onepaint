//! Saving in the background: the UI thread settles and snapshots the
//! drawing (`Session::prepare_save`); a worker composites the thumbnail,
//! encodes (reusing the last save's chunks) and writes the file atomically.
//! Painting goes on meanwhile; edits made during the save stay unsaved.

use onepaint_engine::SaveJob;
use std::path::PathBuf;
use std::sync::mpsc;

pub struct Saving {
    pub path: PathBuf,
    /// The drawing saved, and the history step it was at when snapshotted.
    pub doc_id: String,
    pub history_len: usize,
    /// What to do once it is saved (the unsaved-changes prompt's Save).
    pub then: Option<crate::Pending>,
    rx: mpsc::Receiver<Result<(), String>>,
}

impl Saving {
    pub fn start(job: SaveJob, path: PathBuf, doc_id: String, history_len: usize) -> Self {
        let (tx, rx) = mpsc::channel();
        let target = path.clone();
        std::thread::spawn(move || {
            let r =
                job.run().map_err(|e| e.to_string()).and_then(|b| onepaint_formats::write_atomic(&target, &b).map_err(|e| e.to_string()));
            // The receiver is gone only if the app moved on.
            let _ = tx.send(r);
        });
        Self { path, doc_id, history_len, then: None, rx }
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
