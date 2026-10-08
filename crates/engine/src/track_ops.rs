//! Exposure-sheet editing (spec 24, happypaint `animation-store.ts:398-520`):
//! convert a flipbook to tracks, retime sheets, add sheets and tracks.
//! Every edit is one undo step (happypaint's retiming is not undoable at
//! all); a live sheet drag coalesces into one.

use super::*;
use anim::{flipbook_frames, is_tracks_doc, tracks_of};

fn set_meta(n: &mut Node, key: &str, v: serde_json::Value) {
    let e = match n {
        Node::Layer(l) => &mut l.extra,
        Node::Group(g) => &mut g.extra,
    };
    e.insert(key.into(), v);
}

impl Session {
    /// Wrap the flipbook frames into one track, keeping their timing
    /// (`convertToTracks`). Background / foreground frames stay as static
    /// tracks. Needs two or more layer frames.
    pub(crate) fn convert_to_tracks(&mut self) -> Result<(), EngineError> {
        if is_tracks_doc(&self.doc.root) {
            return Err(EngineError::Invalid("Already in tracks".into()));
        }
        let frames = flipbook_frames(&self.doc.root);
        if frames.len() < 2 {
            return Err(EngineError::Invalid("Needs two frames or more".into()));
        }
        if frames.iter().any(|f| self.doc.find(&f.id).is_some_and(|n| n.is_group())) {
            return Err(EngineError::Invalid("Group frames can't become sheets".into()));
        }
        self.tree_edit(None, |s| {
            let ids: Vec<NodeId> = frames.iter().map(|f| f.id.clone()).collect();
            let at = s.doc.root.iter().position(|n| ids.iter().any(|i| i == n.id())).unwrap_or(0);
            let mut sheets = Vec::new();
            for f in &frames {
                if let Some(i) = s.doc.root.iter().position(|n| n.id() == f.id) {
                    let mut n = s.doc.root.remove(i);
                    set_meta(&mut n, "animStart", serde_json::json!(f.start));
                    sheets.push(n);
                }
            }
            let mut g = onepaint_doc::Group::new("Track 1");
            g.extra.insert("animTrack".into(), serde_json::json!(true));
            g.children = sheets;
            s.doc.root.insert(at.min(s.doc.root.len()), Node::Group(g));
            Ok(())
        })
    }

    /// Retime a sheet: its start, clamped between its neighbours so sheets
    /// never cross (`moveSheet`). A drag's moves coalesce into one step.
    pub(crate) fn move_sheet(&mut self, id: &str, column: i64) -> Result<(), EngineError> {
        let track = tracks_of(&self.doc.root)
            .into_iter()
            .find(|t| t.sheets.as_ref().is_some_and(|s| s.iter().any(|f| f.id == id)))
            .ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let sheets = track.sheets.unwrap_or_default();
        let i = sheets.iter().position(|f| f.id == id).unwrap_or(0);
        let me = &sheets[i];
        let lo = if i == 0 { 0 } else { sheets[i - 1].start + sheets[i - 1].hold } as i64;
        let hi = sheets.get(i + 1).map_or(i64::MAX, |n| n.start as i64 - me.hold as i64);
        let start = column.clamp(lo, hi.max(lo)) as usize;
        if start == me.start {
            return Ok(());
        }
        let id = id.to_string();
        self.tree_edit(Some(format!("move-sheet:{id}")), |s| {
            if let Some(n) = s.doc.find_mut(&id) {
                set_meta(n, "animStart", serde_json::json!(start));
            }
            Ok(())
        })?;
        self.anim_slot = start;
        Ok(())
    }

    /// A blank drawing at `column` of `track`, between its neighbours
    /// (`addSheetAtColumn`); refused where a sheet already shows.
    pub(crate) fn add_sheet(&mut self, track: &str, column: usize) -> Result<(), EngineError> {
        let sheets = tracks_of(&self.doc.root)
            .into_iter()
            .find(|t| t.id == track)
            .and_then(|t| t.sheets)
            .ok_or_else(|| EngineError::Invalid("Not a track".into()))?;
        if sheets.iter().any(|f| column >= f.start && column < f.start + f.hold) {
            return Err(EngineError::Invalid("There's already a drawing here".into()));
        }
        let index = sheets.iter().filter(|f| f.start < column).count();
        let mut l = Layer::new(&format!("Drawing {}", sheets.len() + 1));
        l.extra.insert("animStart".into(), serde_json::json!(column));
        let (id, track) = (l.id.clone(), track.to_string());
        self.tree_edit(None, |s| {
            if !s.doc.insert(Some(&track), index, Node::Layer(l)) {
                return Err(EngineError::InvalidMove);
            }
            s.active = id;
            Ok(())
        })?;
        self.anim_slot = column;
        Ok(())
    }

    /// A new track on top holding one blank sheet, selected (`addTrack`).
    pub(crate) fn add_track(&mut self) -> Result<(), EngineError> {
        let n = tracks_of(&self.doc.root).iter().filter(|t| t.sheets.is_some()).count();
        let sheet = Layer::new("Drawing 1");
        let id = sheet.id.clone();
        let mut g = onepaint_doc::Group::new(&format!("Track {}", n + 1));
        g.extra.insert("animTrack".into(), serde_json::json!(true));
        g.children.push(Node::Layer(sheet));
        self.tree_edit(None, |s| {
            s.doc.root.push(Node::Group(g));
            s.active = id;
            Ok(())
        })
    }

    /// Delete a track; the last sheet track stays (`deleteTrack`).
    pub(crate) fn delete_track(&mut self, id: &str) -> Result<(), EngineError> {
        let tracks = tracks_of(&self.doc.root);
        let t = tracks.iter().find(|t| t.id == id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        if t.sheets.is_some() && tracks.iter().filter(|t| t.sheets.is_some()).count() <= 1 {
            return Err(EngineError::Invalid("A sheet track has to stay".into()));
        }
        self.delete(id)
    }

    /// Move a track one row toward the front (+1) or back (−1) (`moveTrack`).
    pub(crate) fn move_track(&mut self, id: &str, delta: i32) -> Result<(), EngineError> {
        let i = self.doc.root.iter().position(|n| n.id() == id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let j = i as i64 + delta.signum() as i64;
        if j < 0 || j >= self.doc.root.len() as i64 {
            return Err(EngineError::Invalid("Already at the end".into()));
        }
        self.tree_edit(None, |s| {
            s.doc.root.swap(i, j as usize);
            Ok(())
        })
    }
}
