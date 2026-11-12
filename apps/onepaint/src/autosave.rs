//! Background autosave and crash recovery (happypaint keeps an IndexedDB autosave).
//!
//! Unsaved work is written to the app data dir from a cheap document clone
//! (tiles are shared `Arc`s) on a worker thread, when the [`Mode`] says so. A
//! clean save, New or Open removes it; on launch a leftover file is offered
//! for recovery.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// "When you pause": this long after the last edit (happypaint `AUTOSAVE_DELAY_MS`).
pub const PAUSE_S: f64 = 2.0;

/// When the recovery copy is written (Settings ▸ Documents ▸ Auto-save,
/// happypaint `AutosaveMode`, v0.205.0).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(from = "String", into = "String")]
pub enum Mode {
    /// A couple of seconds after the edits stop.
    #[default]
    Pause,
    /// At most this often while there are unsaved edits, timed from the
    /// first one, so continuous painting is still saved.
    Every2,
    Every5,
    Every10,
    Every15,
    /// Never on a timer: only Save writes anything.
    Manual,
}

/// happypaint's ids; anything else is the default.
impl From<String> for Mode {
    fn from(s: String) -> Self {
        Mode::ALL.into_iter().find(|m| m.id() == s).unwrap_or_default()
    }
}

impl From<Mode> for String {
    fn from(m: Mode) -> Self {
        m.id().to_string()
    }
}

impl Mode {
    pub const ALL: [Mode; 6] = [Mode::Pause, Mode::Every2, Mode::Every5, Mode::Every10, Mode::Every15, Mode::Manual];

    /// The settings value (happypaint `AutosaveMode`).
    pub fn id(self) -> &'static str {
        match self {
            Mode::Pause => "pause",
            Mode::Every2 => "2m",
            Mode::Every5 => "5m",
            Mode::Every10 => "10m",
            Mode::Every15 => "15m",
            Mode::Manual => "manual",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Pause => "When you pause",
            Mode::Every2 => "Every 2 minutes",
            Mode::Every5 => "Every 5 minutes",
            Mode::Every10 => "Every 10 minutes",
            Mode::Every15 => "Every 15 minutes",
            Mode::Manual => "Manual (Ctrl+S)",
        }
    }

    /// Minutes between interval saves; `None` for Pause and Manual.
    pub fn minutes(self) -> Option<f64> {
        match self {
            Mode::Every2 => Some(2.0),
            Mode::Every5 => Some(5.0),
            Mode::Every10 => Some(10.0),
            Mode::Every15 => Some(15.0),
            Mode::Pause | Mode::Manual => None,
        }
    }
}

pub fn dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint").map(|d| d.data_local_dir().join("autosave"))
}

pub struct Autosave {
    dir: Option<PathBuf>,
    /// The edit counter last seen, and when it last moved.
    seen: u64,
    last_edit: f64,
    /// When the first edit not yet written was seen.
    pending_since: Option<f64>,
    /// The edit counter the last write covered.
    written: u64,
    busy: Arc<AtomicBool>,
    /// The last write's encoded tiles: a save re-encodes only what changed.
    chunks: Arc<std::sync::Mutex<onepaint_formats::ChunkCache>>,
}

impl Autosave {
    pub fn new(dir: Option<PathBuf>) -> Self {
        Self {
            dir,
            seen: 0,
            last_edit: 0.0,
            pending_since: None,
            written: 0,
            busy: Arc::new(AtomicBool::new(false)),
            chunks: Arc::default(),
        }
    }

    fn path_for(&self, doc_id: &str) -> Option<PathBuf> {
        let safe: String = doc_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        self.dir.as_ref().map(|d| d.join(format!("{safe}.happypaint")))
    }

    /// Note the edit counter (bumped on every history change) at `now`.
    fn observe(&mut self, now: f64, edits: u64) {
        if edits != self.seen {
            self.seen = edits;
            self.last_edit = now;
            self.pending_since.get_or_insert(now);
        }
    }

    /// When the next write is due under `mode`, if one is: the caller wakes
    /// the UI then.
    pub fn due_at(&self, mode: Mode) -> Option<f64> {
        self.dir.as_ref()?;
        let since = self.pending_since?;
        match mode {
            Mode::Manual => None,
            Mode::Pause => Some(self.last_edit + PAUSE_S),
            m => m.minutes().map(|min| since + min * 60.0),
        }
    }

    /// Write `doc` in the background if `mode` says it's time. `edits`
    /// counts history changes; `dirty` is unsaved work. True if a write started.
    pub fn tick(&mut self, now: f64, mode: Mode, dirty: bool, edits: u64, doc: impl FnOnce() -> Option<onepaint_doc::Document>) -> bool {
        self.observe(now, edits);
        if !dirty || self.seen == self.written {
            self.pending_since = None;
            return false;
        }
        if self.due_at(mode).is_none_or(|t| now < t) || self.busy.load(Ordering::Acquire) {
            return false;
        }
        let Some(doc) = doc() else { return false };
        let Some(path) = self.path_for(&doc.id) else {
            return false;
        };
        self.written = self.seen;
        self.pending_since = None;
        self.busy.store(true, Ordering::Release);
        let (busy, chunks) = (self.busy.clone(), self.chunks.clone());
        std::thread::spawn(move || {
            let mut cache = chunks.lock().unwrap_or_else(|e| e.into_inner());
            if let Err(e) = write_cached(&path, &doc, &mut cache) {
                log::warn!("autosave failed: {e}");
            }
            busy.store(false, Ordering::Release);
        });
        true
    }

    /// Write `doc` now, on this thread (the GPU is gone and the app may not
    /// last another frame). The path written, or why not.
    pub fn save_now(&mut self, doc: &onepaint_doc::Document) -> Result<PathBuf, String> {
        let path = self.path_for(&doc.id).ok_or("no autosave folder")?;
        write(&path, doc)?;
        Ok(path)
    }

    /// Forget the autosave for this document (after a real save, New or Open).
    pub fn clear(&mut self, doc_id: &str) {
        if let Some(p) = self.path_for(doc_id) {
            let _ = std::fs::remove_file(p);
        }
        self.written = self.seen;
        self.pending_since = None;
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
    write_cached(path, doc, &mut onepaint_formats::ChunkCache::default())
}

fn write_cached(path: &Path, doc: &onepaint_doc::Document, cache: &mut onepaint_formats::ChunkCache) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let bytes = onepaint_formats::write_happypaint_cached(doc, None, onepaint_engine::APP_VERSION, cache).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("happypaint.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_now_on_the_calling_thread() {
        let dir = std::env::temp_dir().join(format!("onepaint-autosave-now-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = Autosave::new(Some(dir.clone()));
        let doc = onepaint_doc::Document::new(64, 64, None);
        let path = a.save_now(&doc).unwrap();
        assert!(path.exists(), "written before it returns");
        assert!(Autosave::new(None).save_now(&doc).is_err(), "no folder, no save");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn wait_idle(a: &Autosave) {
        for _ in 0..200 {
            if !a.busy.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn writes_in_background_and_offers_the_leftover() {
        let dir = std::env::temp_dir().join(format!("onepaint-autosave-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut a = Autosave::new(Some(dir.clone()));
        let doc = onepaint_doc::Document::new(64, 64, None);
        let id = doc.id.clone();
        assert!(!a.tick(100.0, Mode::Pause, false, 1, || Some(doc.clone())), "clean docs are not autosaved");
        assert!(!a.tick(101.0, Mode::Pause, true, 2, || Some(doc.clone())), "still editing");
        assert!(a.tick(103.0, Mode::Pause, true, 2, || Some(doc.clone())), "two seconds after the last edit");
        wait_idle(&a);
        assert!(!a.tick(110.0, Mode::Pause, true, 2, || Some(doc.clone())), "nothing new since");
        let left = a.leftover().expect("autosave written");
        let back = onepaint_formats::read_happypaint(&std::fs::read(&left).unwrap()).unwrap();
        assert_eq!(back.id, id);
        a.clear(&id);
        assert!(a.leftover().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // happypaint v0.205.0 (`autosave-setting.test.ts`).
    #[test]
    fn modes_decide_when_to_write() {
        let doc = || Some(onepaint_doc::Document::new(8, 8, None));
        assert_eq!(Autosave::new(None).due_at(Mode::Pause), None, "no folder, nothing due");
        // A folder that's never written (each tick is before the save is due).
        let dir = || Some(std::env::temp_dir().join("onepaint-autosave-modes-unused"));
        // Pause: waits for two quiet seconds, however long the painting goes on.
        let mut a = Autosave::new(dir());
        for i in 0..100 {
            a.tick(i as f64, Mode::Pause, true, i + 1, doc);
            assert_eq!(a.due_at(Mode::Pause), Some(i as f64 + PAUSE_S));
        }
        // An interval: timed from the first unsaved edit, even while painting.
        let mut a = Autosave::new(dir());
        a.tick(10.0, Mode::Every2, true, 1, doc);
        for i in 0..30 {
            a.tick(11.0 + i as f64, Mode::Every2, true, 2 + i, doc);
        }
        assert_eq!(a.due_at(Mode::Every2), Some(130.0));
        // Manual: never on a timer.
        assert_eq!(a.due_at(Mode::Manual), None);
        assert!(!a.tick(10_000.0, Mode::Manual, true, 99, doc));
        // A clean document has nothing due.
        a.tick(10_001.0, Mode::Pause, false, 99, doc);
        assert_eq!(a.due_at(Mode::Pause), None);
    }

    #[test]
    fn modes_list_and_settings_names() {
        assert_eq!(Mode::default(), Mode::Pause, "the default keeps happypaint's timing");
        let names: Vec<String> = Mode::ALL.iter().map(|m| serde_json::to_string(m).unwrap()).collect();
        assert_eq!(names, ["\"pause\"", "\"2m\"", "\"5m\"", "\"10m\"", "\"15m\"", "\"manual\""]);
        assert_eq!(Mode::ALL.map(Mode::minutes), [None, Some(2.0), Some(5.0), Some(10.0), Some(15.0), None]);
        assert_eq!(serde_json::from_str::<Mode>("\"weekly\"").unwrap(), Mode::Pause, "unknown falls back");
    }
}
