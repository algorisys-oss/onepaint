//! Progress and cancellation for long reads (spec 41). A reader on a worker
//! thread counts its steps; the UI thread shows the fraction and may cancel.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A cloneable handle shared by the reader and the UI.
#[derive(Clone, Debug, Default)]
pub struct Progress(Arc<Inner>);

#[derive(Debug, Default)]
struct Inner {
    done: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
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
