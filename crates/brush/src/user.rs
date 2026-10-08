//! The user brush library: imported presets with their image tips and grains
//! (happypaint `state/custom-brushes.ts`, `state/custom-grains.ts`; spec 23).
//!
//! One process-wide registry, so `preset()`, `tip_for()` and `grain_for()`
//! resolve imported ids exactly like built-in ones. The registry holds data
//! only; loading and saving it is the app's job (`formats::brushes`).

use crate::{BrushPreset, tips::Mask};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

/// The category every imported preset is listed under.
pub const IMPORTED_CATEGORY: &str = "Imported";
/// Id prefix of user tips (happypaint's `custom:<id>`).
pub const USER_TIP_PREFIX: &str = "custom:";
/// Id prefix of user grains (happypaint's `customgrain:<id>`).
pub const USER_GRAIN_PREFIX: &str = "customgrain:";

#[derive(Clone, Debug, PartialEq)]
pub struct UserGrain {
    pub name: String,
    pub mask: Arc<Mask>,
}

/// Imported presets plus the tips and grains they reference.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserLibrary {
    pub presets: Vec<BrushPreset>,
    pub tips: BTreeMap<String, Arc<Mask>>,
    pub grains: BTreeMap<String, UserGrain>,
}

impl UserLibrary {
    pub fn is_empty(&self) -> bool {
        self.presets.is_empty() && self.grains.is_empty()
    }

    /// Add `other`'s entries. A preset whose id already exists is replaced
    /// in place (ids are content hashes, so this is the same brush).
    pub fn merge(&mut self, other: UserLibrary) {
        for p in other.presets {
            match self.presets.iter_mut().find(|q| q.id == p.id) {
                Some(q) => *q = p,
                None => self.presets.push(p),
            }
        }
        self.tips.extend(other.tips);
        self.grains.extend(other.grains);
    }

    /// Remove a preset, and its tip if no other preset uses it. Grains stay:
    /// like happypaint's custom grains they are a library of their own.
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(at) = self.presets.iter().position(|p| p.id == id) else { return false };
        let gone = self.presets.remove(at);
        if !self.presets.iter().any(|p| p.tip_id == gone.tip_id) {
            self.tips.remove(&gone.tip_id);
        }
        true
    }

    pub fn preset(&self, id: &str) -> Option<&BrushPreset> {
        self.presets.iter().find(|p| p.id == id)
    }
}

fn registry() -> &'static RwLock<UserLibrary> {
    static R: OnceLock<RwLock<UserLibrary>> = OnceLock::new();
    R.get_or_init(Default::default)
}

fn read<T>(f: impl FnOnce(&UserLibrary) -> T) -> Option<T> {
    registry().read().ok().map(|l| f(&l))
}

fn write(f: impl FnOnce(&mut UserLibrary)) {
    if let Ok(mut l) = registry().write() {
        f(&mut l);
    }
}

/// A snapshot of the registered library (tips and grains are shared `Arc`s).
pub fn user_library() -> UserLibrary {
    read(Clone::clone).unwrap_or_default()
}

/// Replace the whole registry (startup load).
pub fn set_user_library(lib: UserLibrary) {
    write(|l| *l = lib);
}

/// Register imported brushes and grains.
pub fn add_user_brushes(lib: UserLibrary) {
    write(|l| l.merge(lib));
}

/// Delete an imported preset; false when the id is not a user preset.
pub fn remove_user_brush(id: &str) -> bool {
    let mut removed = false;
    write(|l| removed = l.remove(id));
    removed
}

/// Imported presets in import order.
pub fn user_presets() -> Vec<BrushPreset> {
    read(|l| l.presets.clone()).unwrap_or_default()
}

pub(crate) fn user_preset(id: &str) -> Option<BrushPreset> {
    read(|l| l.preset(id).cloned()).flatten()
}

pub(crate) fn user_tip(id: &str) -> Option<Arc<Mask>> {
    read(|l| l.tips.get(id).cloned()).flatten()
}

pub(crate) fn user_grain(id: &str) -> Option<Arc<Mask>> {
    read(|l| l.grains.get(id).map(|g| g.mask.clone())).flatten()
}
