//! Progress and cancellation for long reads (spec 41). A reader on a worker
//! thread counts its steps; the UI thread shows the fraction and may cancel.
//! The reader also lists the layers it could only read in part.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// A cloneable handle shared by the reader and the UI.
#[derive(Clone, Debug, Default)]
pub struct Progress(Arc<Inner>);

#[derive(Debug, Default)]
struct Inner {
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
    damaged: Mutex<Vec<String>>,
}

impl Progress {
    /// Set the number of steps (replaces any earlier total).
    pub fn set_total(&self, total: u64) {
        self.0.total.store(total, Ordering::Relaxed);
    }

    /// Count `n` steps done.
    pub fn advance(&self, n: u64) {
        self.0.done.fetch_add(n, Ordering::Relaxed);
    }

    /// Done so far, 0 to 1; `None` while the total is unknown.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.0.total.load(Ordering::Relaxed);
        (total > 0).then(|| (self.0.done.load(Ordering::Relaxed) as f64 / total as f64).min(1.0) as f32)
    }

    pub fn cancel(&self) {
        self.0.cancel.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.cancel.load(Ordering::Relaxed)
    }

    /// Note a layer that lost some of its pixels to a part that wouldn't
    /// decode (happypaint v0.205.2, review F03). Each name is kept once.
    pub fn damaged(&self, layer: &str) {
        let mut d = self.0.damaged.lock().unwrap_or_else(|e| e.into_inner());
        if !d.iter().any(|n| n == layer) {
            d.push(layer.to_string());
        }
    }

    /// The layers noted by [`Progress::damaged`], in the order first noted.
    pub fn damaged_layers(&self) -> Vec<String> {
        self.0.damaged.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// `Err(Cancelled)` once cancelled, for readers to `?` between steps.
    pub fn check(&self) -> Result<(), crate::FormatError> {
        if self.is_cancelled() { Err(crate::FormatError::Cancelled) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_steps_and_cancels_across_clones() {
        let p = Progress::default();
        assert_eq!(p.fraction(), None);
        p.set_total(4);
        let q = p.clone();
        q.advance(1);
        assert_eq!(p.fraction(), Some(0.25));
        q.advance(10);
        assert_eq!(p.fraction(), Some(1.0));
        assert!(p.check().is_ok());
        p.cancel();
        assert!(matches!(q.check(), Err(crate::FormatError::Cancelled)));
    }
}
