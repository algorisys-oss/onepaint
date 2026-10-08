//! Batch retiming (spec 24, happypaint `animation-store.ts:589-760`): set
//! the duration of, split, in-between, duplicate or delete a set of frames
//! (flipbook frames, or the active track's sheets). Each op is one undo
//! step; happypaint's take one per frame.

use super::*;
use anim::{hold_of, is_tracks_doc, tracks_of};

fn extra_mut(n: &mut Node) -> &mut onepaint_doc::Extra {
    match n {
        Node::Layer(l) => &mut l.extra,
        Node::Group(g) => &mut g.extra,
    }
}

fn set_hold(n: &mut Node, hold: usize) {
    let e = extra_mut(n);
    if hold <= 1 {
        e.remove("holdFrames");
    } else {
        e.insert("holdFrames".into(), serde_json::json!(hold.min(99)));
    }
}

fn start_of(n: &Node) -> Option<usize> {
    let e = match n {
        Node::Layer(l) => &l.extra,
        Node::Group(g) => &g.extra,
    };
    e.get("animStart").and_then(|v| v.as_f64()).map(|s| s.round().max(0.0) as usize)
}

impl Session {
    /// The frames the ops may touch, in timeline order.
    fn frame_ids(&self) -> Vec<NodeId> {
        self.anim_frames().into_iter().map(|f| f.id).collect()
    }

    /// Hold every frame in `ids` for `hold` slots (`setFramesDuration`).
    pub(crate) fn set_frames_duration(&mut self, ids: &[NodeId], hold: usize) -> Result<(), EngineError> {
        let ids = self.known(ids);
        if ids.is_empty() {
            return Ok(());
        }
        self.tree_edit(None, |s| {
            for id in &ids {
                if let Some(n) = s.doc.find_mut(id) {
                    set_hold(n, hold.clamp(1, 99));
                }
            }
            s.ripple_tracks();
            Ok(())
        })?;
        self.after_frames_changed();
        Ok(())
    }

    /// Cut each frame into sheets of `duration` slots, keeping its content
    /// and total timing; the last piece takes the remainder (`splitFrames`).
    /// Groups and frames already short enough are skipped.
    pub(crate) fn split_frames(&mut self, ids: &[NodeId], duration: usize) -> Result<(), EngineError> {
        let step = duration.max(1);
        let ids: Vec<NodeId> =
            self.known(ids).into_iter().filter(|id| self.doc.find(id).is_some_and(|n| !n.is_group() && hold_of(n) > step)).collect();
        if ids.is_empty() {
            return Ok(());
        }
        self.tree_edit(None, |s| {
            for id in &ids {
                let Some(src) = s.doc.find(id).cloned() else { continue };
                let (hold, start) = (hold_of(&src), start_of(&src));
                let pieces = hold.div_ceil(step);
                let parent = s.doc.parent_of(id).flatten();
                let idx = s.doc.path_of(id).and_then(|p| p.last().copied()).unwrap_or(0);
                for i in 0..pieces {
                    let last = i + 1 == pieces;
                    let mut piece = if i == 0 { src.clone() } else { copy_of(&src) };
                    set_hold(&mut piece, if last { hold - step * (pieces - 1) } else { step });
                    if let Some(st) = start {
                        extra_mut(&mut piece).insert("animStart".into(), serde_json::json!(st + i * step));
                    }
                    if i == 0 {
                        if let Some(n) = s.doc.find_mut(id) {
                            *n = piece;
                        }
                    } else {
                        s.doc.insert(parent.as_deref(), idx + i, piece);
                    }
                }
            }
            Ok(())
        })?;
        self.after_frames_changed();
        Ok(())
    }

    /// An empty frame after each of `ids`, to draw the in-between into
    /// (`addInbetweens`). The last one added is selected.
    pub(crate) fn add_inbetweens(&mut self, ids: &[NodeId]) -> Result<(), EngineError> {
        self.insert_after(ids, |src, n| {
            let mut l = Layer::new(&format!("Frame {n}"));
            if let Some(st) = start_of(src) {
                l.extra.insert("animStart".into(), serde_json::json!(st + hold_of(src)));
            }
            Some(Node::Layer(l))
        })
    }

    /// Duplicate each of `ids`, content and all; groups are skipped
    /// (`duplicateFrames`).
    pub(crate) fn duplicate_frames(&mut self, ids: &[NodeId]) -> Result<(), EngineError> {
        self.insert_after(ids, |src, _| {
            if src.is_group() {
                return None;
            }
            let mut c = copy_of(src);
            if let Some(st) = start_of(src) {
                extra_mut(&mut c).insert("animStart".into(), serde_json::json!(st + hold_of(src)));
            }
            Some(c)
        })
    }

    /// Delete `ids`, always leaving one frame (`deleteFrames`).
    pub(crate) fn delete_frames(&mut self, ids: &[NodeId]) -> Result<(), EngineError> {
        let all = self.frame_ids();
        let mut ids = self.known(ids);
        ids.truncate(all.len().saturating_sub(1));
        if ids.is_empty() {
            return Ok(());
        }
        let keep: Vec<NodeId> = all.into_iter().filter(|f| !ids.contains(f)).collect();
        self.tree_edit(None, |s| {
            for id in &ids {
                s.doc.remove(id);
            }
            if s.doc.find(&s.active).is_none() {
                s.active = keep[0].clone();
            }
            Ok(())
        })?;
        self.after_frames_changed();
        Ok(())
    }

    /// `ids` that are frames here, in timeline order.
    fn known(&self, ids: &[NodeId]) -> Vec<NodeId> {
        self.frame_ids().into_iter().filter(|f| ids.contains(f)).collect()
    }

    /// Insert `make(src, n)` after each of `ids` in one step; later sheets
    /// ripple right where the new one would overlap them.
    fn insert_after(&mut self, ids: &[NodeId], make: impl Fn(&Node, usize) -> Option<Node>) -> Result<(), EngineError> {
        let ids = self.known(ids);
        let base = self.frame_ids().len();
        let mut added = Vec::new();
        for id in &ids {
            if let Some(src) = self.doc.find(id)
                && let Some(n) = make(src, base + added.len() + 1)
            {
                added.push((id.clone(), n));
            }
        }
        let Some(last) = added.last().map(|(_, n)| n.id().to_string()) else { return Ok(()) };
        self.tree_edit(None, |s| {
            for (after, n) in added {
                let parent = s.doc.parent_of(&after).flatten();
                let idx = s.doc.path_of(&after).and_then(|p| p.last().copied()).unwrap_or(0);
                s.doc.insert(parent.as_deref(), idx + 1, n);
            }
            s.ripple_tracks();
            s.active = last;
            Ok(())
        })?;
        self.after_frames_changed();
        Ok(())
    }

    /// Push sheets with explicit starts right until none overlaps the one
    /// before it.
    fn ripple_tracks(&mut self) {
        if !is_tracks_doc(&self.doc.root) {
            return;
        }
        for t in tracks_of(&self.doc.root) {
            if t.sheets.is_none() {
                continue;
            }
            let Some(Node::Group(g)) = self.doc.find_mut(&t.id) else { continue };
            let mut end = 0;
            for c in &mut g.children {
                let start = match start_of(c) {
                    Some(st) if st < end => {
                        extra_mut(c).insert("animStart".into(), serde_json::json!(end));
                        end
                    }
                    Some(st) => st,
                    None => end,
                };
                end = start + hold_of(c);
            }
        }
    }

    /// `afterStructureChange`: keep the playhead on a real slot.
    fn after_frames_changed(&mut self) {
        let total = self.anim_total_slots();
        self.anim_slot = self.anim_slot.min(total.saturating_sub(1));
        self.sync_playhead_to_active();
        self.mark_all_dirty();
    }
}

/// A copy with fresh ids, named like Duplicate does.
fn copy_of(n: &Node) -> Node {
    let mut c = n.clone();
    c.reassign_ids();
    let name = format!("{} copy", c.name());
    c.set_name(name);
    c
}
