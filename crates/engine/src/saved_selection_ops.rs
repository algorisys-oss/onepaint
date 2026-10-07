//! Saved selections (spec 20): named masks kept with the document.

use super::*;
use onepaint_selection::saved::{decode_rle, encode_rle, resample_mask, shift_mask};
use onepaint_selection::{Mask, Mode};

#[derive(Clone, Debug, PartialEq)]
pub struct SavedSelection {
    pub id: String,
    pub name: String,
    pub mask: Arc<Mask>,
}

/// happypaint's `StoredSavedSelection`.
#[derive(Serialize, Deserialize)]
struct Stored {
    id: String,
    name: String,
    w: u32,
    h: u32,
    rle: String,
}

impl Session {
    pub fn saved_selections(&self) -> &[SavedSelection] {
        &self.saved
    }

    /// Restore from the document's `savedSelections`, skipping other sizes.
    pub(crate) fn load_saved_selections(&mut self) {
        let (w, h) = (self.doc.width, self.doc.height);
        let stored: Vec<Stored> =
            self.doc.extra.get("savedSelections").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        self.saved = stored
            .into_iter()
            .filter(|s| s.w == w && s.h == h)
            .filter_map(|s| {
                Some(SavedSelection { id: s.id, name: s.name, mask: Arc::new(Mask::from_vec(w, h, decode_rle(&s.rle, w, h)?)) })
            })
            .collect();
    }

    pub(crate) fn store_saved_selections(&mut self) {
        let (w, h) = (self.doc.width, self.doc.height);
        let stored: Vec<Stored> = self
            .saved
            .iter()
            .filter_map(|s| Some(Stored { id: s.id.clone(), name: s.name.clone(), w, h, rle: encode_rle(s.mask.data())? }))
            .collect();
        if stored.is_empty() {
            self.doc.extra.remove("savedSelections");
        } else {
            self.doc.extra.insert("savedSelections".into(), serde_json::to_value(stored).unwrap_or_default());
        }
    }

    fn push_saved(&mut self, before: Vec<SavedSelection>) {
        let bytes = 64 + self.saved.iter().map(|s| s.mask.data().len()).sum::<usize>() / 8;
        self.history.push(Change::SavedSelections { before, after: self.saved.clone() }, bytes);
        self.events.push(Event::SavedSelectionsChanged);
        self.history_changed();
    }

    pub(crate) fn save_selection(&mut self, name: String) -> Result<(), EngineError> {
        let Some(m) = self.selection.clone().filter(|m| m.has_content()) else {
            return Err(EngineError::Invalid("no selection to save".into()));
        };
        if encode_rle(m.data()).is_none() {
            return Err(EngineError::Invalid("this selection is too detailed to save".into()));
        }
        let before = self.saved.clone();
        self.saved.push(SavedSelection { id: format!("sel-{}", onepaint_doc::new_id()), name, mask: m });
        self.push_saved(before);
        Ok(())
    }

    pub(crate) fn delete_saved_selection(&mut self, id: &str) {
        if !self.saved.iter().any(|s| s.id == id) {
            return;
        }
        let before = self.saved.clone();
        self.saved.retain(|s| s.id != id);
        self.push_saved(before);
    }

    pub(crate) fn load_selection(&mut self, id: &str, mode: Mode) {
        let Some(s) = self.saved.iter().find(|s| s.id == id).cloned() else { return };
        let m = self.region_combined(&s.mask, mode);
        self.commit_selection(Some(m));
    }

    /// Canvas Size: shift every saved mask with the content.
    pub(crate) fn shift_saved(&mut self, nw: u32, nh: u32, dx: i32, dy: i32) {
        let (w, h) = (self.doc.width, self.doc.height);
        for s in &mut self.saved {
            s.mask = Arc::new(Mask::from_vec(nw, nh, shift_mask(s.mask.data(), w, h, nw, nh, dx, dy)));
        }
    }

    /// Image Size: resample every saved mask.
    pub(crate) fn resample_saved(&mut self, nw: u32, nh: u32) {
        let (w, h) = (self.doc.width, self.doc.height);
        for s in &mut self.saved {
            s.mask = Arc::new(Mask::from_vec(nw, nh, resample_mask(s.mask.data(), w, h, nw, nh)));
        }
    }
}
