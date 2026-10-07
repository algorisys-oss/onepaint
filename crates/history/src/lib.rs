//! Undo/redo stacks with an entry cap and a byte budget
//! (happypaint `engine/history/history.ts`).
//!
//! Generic over the entry type: the engine decides what an entry holds and how
//! to apply it. A transaction groups several entries into one undo step.

pub const DEFAULT_MAX_ENTRIES: usize = 200;
pub const DEFAULT_BUDGET: usize = 192 * 1024 * 1024;

struct Step<E> {
    entries: Vec<E>,
    bytes: usize,
}

pub struct History<E> {
    undo: Vec<Step<E>>,
    redo: Vec<Step<E>>,
    max_entries: usize,
    budget: usize,
    bytes: usize,
    memory_limited: bool,
    txn: Option<Step<E>>,
}

impl<E> Default for History<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// Collects the entries of one transaction.
pub struct Txn<'a, E> {
    step: &'a mut Step<E>,
}

impl<E> Txn<'_, E> {
    pub fn push(&mut self, e: E, bytes: usize) {
        self.step.entries.push(e);
        self.step.bytes += bytes;
    }
}

impl<E> History<E> {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_entries: DEFAULT_MAX_ENTRIES,
            budget: DEFAULT_BUDGET,
            bytes: 0,
            memory_limited: false,
            txn: None,
        }
    }

    pub fn set_max_entries(&mut self, n: usize) {
        self.max_entries = n.max(1);
        self.evict();
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
        self.evict();
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn memory_limited(&self) -> bool {
        self.memory_limited
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
        self.memory_limited = false;
    }

    pub fn push(&mut self, e: E, bytes: usize) {
        if let Some(t) = &mut self.txn {
            t.entries.push(e);
            t.bytes += bytes;
            return;
        }
        self.push_step(Step {
            entries: vec![e],
            bytes,
        });
    }

    fn push_step(&mut self, step: Step<E>) {
        if step.entries.is_empty() {
            return;
        }
        for s in self.redo.drain(..) {
            self.bytes -= s.bytes;
        }
        self.bytes += step.bytes;
        self.undo.push(step);
        self.evict();
    }

    fn evict(&mut self) {
        while self.undo.len() > 1
            && (self.bytes > self.budget || self.undo.len() > self.max_entries)
        {
            if self.bytes > self.budget && self.undo.len() <= self.max_entries {
                self.memory_limited = true;
            }
            let s = self.undo.remove(0);
            self.bytes -= s.bytes;
        }
    }

    /// Group every push inside `f` into one undo step (nesting collapses).
    pub fn transact(&mut self, f: impl FnOnce(&mut Txn<E>)) {
        let mut step = Step {
            entries: Vec::new(),
            bytes: 0,
        };
        f(&mut Txn { step: &mut step });
        self.push_step(step);
    }

    /// Start collecting pushes into one step until [`History::end_transaction`].
    pub fn begin_transaction(&mut self) {
        if self.txn.is_none() {
            self.txn = Some(Step {
                entries: Vec::new(),
                bytes: 0,
            });
        }
    }

    pub fn end_transaction(&mut self) {
        if let Some(s) = self.txn.take() {
            self.push_step(s);
        }
    }

    /// Move the newest step to the redo stack; returns its entries newest-first, to apply in that order.
    pub fn undo(&mut self) -> Option<Vec<&E>> {
        let s = self.undo.pop()?;
        self.redo.push(s);
        self.redo.last().map(|s| s.entries.iter().rev().collect())
    }

    /// Move the newest redo step back; returns its entries oldest-first.
    pub fn redo(&mut self) -> Option<Vec<&E>> {
        let s = self.redo.pop()?;
        self.undo.push(s);
        self.undo.last().map(|s| s.entries.iter().collect())
    }

    /// The newest entry of the newest undo step (for coalescing edits).
    pub fn top_mut(&mut self) -> Option<&mut E> {
        if !self.redo.is_empty() {
            return None;
        }
        self.undo.last_mut()?.entries.last_mut()
    }
}
