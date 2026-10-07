//! Swatch palettes (happypaint `state/color-store.ts`, spec 21): any number of
//! named palettes, one active, persisted as JSON in the config folder.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_SWATCHES: [&str; 12] =
    ["#000000", "#ffffff", "#7f8c8d", "#e74c3c", "#e67e22", "#f1c40f", "#2ecc71", "#1abc9c", "#3498db", "#9b59b6", "#8d6e63", "#f8c8dc"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub id: String,
    pub name: String,
    pub colors: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Palettes {
    pub palettes: Vec<Palette>,
    pub active_id: String,
    /// Where they persist (`None` in tests and scripted runs).
    #[serde(skip)]
    path: Option<PathBuf>,
}

pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint").map(|d| d.config_dir().join("palettes.json"))
}

fn new_id() -> String {
    onepaint_doc::new_id()
}

fn default_palette() -> Palette {
    Palette { id: new_id(), name: "My palette".into(), colors: DEFAULT_SWATCHES.iter().map(|s| s.to_string()).collect() }
}

impl Palettes {
    pub fn load(path: Option<PathBuf>) -> Self {
        let stored = path.as_ref().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Palettes>(&b).ok());
        let mut s = match stored {
            Some(s) if !s.palettes.is_empty() => s,
            _ => {
                let p = default_palette();
                Self { active_id: p.id.clone(), palettes: vec![p], path: None }
            }
        };
        if !s.palettes.iter().any(|p| p.id == s.active_id) {
            s.active_id = s.palettes[0].id.clone();
        }
        s.path = path;
        s
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    pub fn active(&self) -> &Palette {
        self.palettes.iter().find(|p| p.id == self.active_id).unwrap_or(&self.palettes[0])
    }

    fn active_mut(&mut self) -> &mut Palette {
        let i = self.palettes.iter().position(|p| p.id == self.active_id).unwrap_or(0);
        &mut self.palettes[i]
    }

    pub fn set_active(&mut self, id: &str) {
        if self.palettes.iter().any(|p| p.id == id) {
            self.active_id = id.to_string();
            self.save();
        }
    }

    /// The next free "Palette N" name.
    fn auto_name(&self) -> String {
        (self.palettes.len() + 1..)
            .map(|n| format!("Palette {n}"))
            .find(|n| !self.palettes.iter().any(|p| &p.name == n))
            .unwrap_or_default()
    }

    /// Create a palette (empty unless colours are given) and make it active.
    pub fn create(&mut self, name: Option<&str>, colors: Vec<String>) -> String {
        let name = name.map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| self.auto_name(), String::from);
        let p = Palette { id: new_id(), name, colors };
        let id = p.id.clone();
        self.palettes.push(p);
        self.active_id = id.clone();
        self.save();
        id
    }

    pub fn rename(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if let Some(p) = self.palettes.iter_mut().find(|p| p.id == id).filter(|_| !name.is_empty()) {
            p.name = name.to_string();
            self.save();
        }
    }

    pub fn duplicate(&mut self, id: &str) {
        let Some(at) = self.palettes.iter().position(|p| p.id == id) else { return };
        let src = &self.palettes[at];
        let copy = Palette { id: new_id(), name: format!("{} copy", src.name), colors: src.colors.clone() };
        self.active_id = copy.id.clone();
        self.palettes.insert(at + 1, copy);
        self.save();
    }

    /// Delete a palette; the last one is replaced by a fresh default.
    pub fn delete(&mut self, id: &str) {
        self.palettes.retain(|p| p.id != id);
        if self.palettes.is_empty() {
            self.palettes.push(default_palette());
        }
        if self.active_id == id || !self.palettes.iter().any(|p| p.id == self.active_id) {
            self.active_id = self.palettes[0].id.clone();
        }
        self.save();
    }

    /// Add a colour to the active palette (no duplicates).
    pub fn add_swatch(&mut self, hex: &str) {
        let p = self.active_mut();
        if !p.colors.iter().any(|c| c.eq_ignore_ascii_case(hex)) {
            p.colors.push(hex.to_ascii_lowercase());
            self.save();
        }
    }

    pub fn remove_swatch(&mut self, index: usize) {
        let p = self.active_mut();
        if index < p.colors.len() {
            p.colors.remove(index);
            self.save();
        }
    }

    pub fn move_swatch(&mut self, from: usize, to: usize) {
        let p = self.active_mut();
        if from == to || from >= p.colors.len() || to >= p.colors.len() {
            return;
        }
        let c = p.colors.remove(from);
        p.colors.insert(to, c);
        self.save();
    }

    /// Add imported palettes (the last becomes active).
    pub fn import(&mut self, list: Vec<onepaint_formats::palette::ImportedPalette>) -> usize {
        let n = list.len();
        for p in list {
            self.create(Some(&p.name), p.colors);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_and_persistence() {
        let dir = std::env::temp_dir().join(format!("onepaint-pal-{}", new_id()));
        let path = dir.join("palettes.json");
        let mut p = Palettes::load(Some(path.clone()));
        assert_eq!(p.active().name, "My palette");
        assert_eq!(p.active().colors.len(), 12);
        let id = p.create(None, vec!["#123456".into()]);
        assert_eq!(p.active().name, "Palette 2");
        p.add_swatch("#ABCDEF");
        p.add_swatch("#abcdef");
        assert_eq!(p.active().colors, ["#123456", "#abcdef"], "no duplicates");
        p.move_swatch(1, 0);
        p.rename(&id, "  Skin  ");
        let back = Palettes::load(Some(path.clone()));
        assert_eq!(back.active().name, "Skin");
        assert_eq!(back.active().colors, ["#abcdef", "#123456"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_and_delete_last() {
        let mut p = Palettes::load(None);
        let first = p.active_id.clone();
        p.duplicate(&first);
        assert_eq!(p.palettes[1].name, "My palette copy");
        assert_eq!(p.active_id, p.palettes[1].id);
        let ids: Vec<_> = p.palettes.iter().map(|x| x.id.clone()).collect();
        for id in ids {
            p.delete(&id);
        }
        assert_eq!(p.palettes.len(), 1, "always keeps one");
        assert_eq!(p.active().colors.len(), 12);
        p.remove_swatch(0);
        assert_eq!(p.active().colors[0], "#ffffff");
    }
}
