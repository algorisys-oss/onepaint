//! Background autosave and crash recovery (happypaint keeps an IndexedDB autosave).
//!
//! Every minute, unsaved work is written to the app data dir from a cheap
//! document clone (tiles are shared `Arc`s) on a worker thread. A clean save,
//! New or Open removes it; on launch a leftover file is offered for recovery.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const INTERVAL_S: f64 = 60.0;

pub fn dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint")
        .map(|d| d.data_local_dir().join("autosave"))
}

pub struct Autosave {
    dir: Option<PathBuf>,
    last: f64,
    busy: Arc<AtomicBool>,
}

impl Autosave {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            last: 0.0,
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    fn path_for(&self, doc_id: &str) -> Option<PathBuf> {
        let safe: String = doc_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.dir
            .as_ref()
            .map(|d| d.join(format!("{safe}.happypaint")))
    }

    /// Write `doc` in the background if it's time; returns true if a write started.
    pub fn tick(
        &mut self,
        now: f64,
        dirty: bool,
        doc: impl FnOnce() -> Option<onepaint_doc::Document>,
    ) -> bool {
        if !dirty || now - self.last < INTERVAL_S || self.busy.load(Ordering::Acquire) {
            return false;
        }
        let Some(doc) = doc() else { return false };
        let Some(path) = self.path_for(&doc.id) else {
            return false;
        };
        self.last = now;
        self.busy.store(true, Ordering::Release);
        let busy = self.busy.clone();
        std::thread::spawn(move || {
            if let Err(e) = write(&path, &doc) {
                log::warn!("autosave failed: {e}");
            }
            busy.store(false, Ordering::Release);
        });
        true
    }

    /// Forget the autosave for this document (after a real save, New or Open).
    pub fn clear(&mut self, doc_id: &str) {
        if let Some(p) = self.path_for(doc_id) {
            let _ = std::fs::remove_file(p);
        }
        self.last = 0.0;
    }

    /// The newest leftover autosave, if any.
    pub fn leftover(&self) -> Option<PathBuf> {
        let rd = std::fs::read_dir(self.dir.as_ref()?).ok()?;
        rd.flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "happypaint"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.path())
    }
}

/// Write atomically: a temp file, then rename.
pub fn write(path: &Path, doc: &onepaint_doc::Document) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let bytes = onepaint_formats::write_happypaint(doc, None, onepaint_engine::APP_VERSION)
        .map_err(|e| e.to_string())?;
    let tmp = path.with_extension("happypaint.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_in_background_and_offers_the_leftover() {
        let dir =
            std::env::temp_dir().join(format!("onepaint-autosave-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = Autosave::new(Some(dir.clone()));
        let doc = onepaint_doc::Document::new(64, 64, None);
        let id = doc.id.clone();
        assert!(
            !a.tick(100.0, false, || Some(doc.clone())),
            "clean docs are not autosaved"
        );
        assert!(a.tick(100.0, true, || Some(doc.clone())));
        assert!(
            !a.tick(110.0, true, || Some(doc.clone())),
            "not again within the interval"
        );
        for _ in 0..200 {
            if !a.busy.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let left = a.leftover().expect("autosave written");
        let back = onepaint_formats::read_happypaint(&std::fs::read(&left).unwrap()).unwrap();
        assert_eq!(back.id, id);
        a.clear(&id);
        assert!(a.leftover().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
