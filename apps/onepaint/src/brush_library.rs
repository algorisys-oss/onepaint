//! The user brush library (specs 23, 27): custom brushes from the studio,
//! and imports from `.abr`, `.brush`, `.brushset` and
//! `.happybrush`, export as `.happybrush`, persisted in the app data folder.

use onepaint_brush::{BrushPreset, set_user_library, user_library};
use onepaint_formats::brushes::{self, BrushImport};
use std::path::PathBuf;

pub fn default_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint").map(|d| d.data_dir().join("brushes"))
}

/// Where the user library lives (`None` in tests and scripted runs).
pub struct BrushLibrary {
    dir: Option<PathBuf>,
}

impl BrushLibrary {
    /// Load the saved library into the brush registry.
    pub fn load(dir: Option<PathBuf>) -> Self {
        if let Some(d) = &dir {
            set_user_library(brushes::load_user_library(d));
        }
        Self { dir }
    }

    fn save(&self) -> Result<(), String> {
        match &self.dir {
            Some(d) => brushes::save_user_library(d, &user_library()).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }

    /// Import a brush file into the registry and persist it.
    pub fn import(&self, filename: &str, bytes: &[u8]) -> Result<BrushImport, String> {
        let imp = brushes::import_brush_file(filename, bytes).map_err(|e| e.to_string())?;
        if cfg!(debug_assertions) {
            for w in &imp.warnings {
                eprintln!("[brush-import] {filename}: {w}");
            }
        }
        onepaint_brush::add_user_brushes(imp.library.clone());
        self.save()?;
        Ok(imp)
    }

    /// Save `base` as a new custom brush (spec 27) and persist it.
    pub fn save_custom(&self, base: &BrushPreset, name: &str) -> Result<BrushPreset, String> {
        let p = onepaint_brush::add_custom_brush(base, name);
        self.save()?;
        Ok(p)
    }

    /// Write an edited user brush back over its stored version.
    pub fn update(&self, p: &BrushPreset) -> Result<bool, String> {
        let ok = onepaint_brush::update_user_brush(p);
        if ok {
            self.save()?;
        }
        Ok(ok)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<bool, String> {
        let ok = onepaint_brush::rename_user_brush(id, name);
        if ok {
            self.save()?;
        }
        Ok(ok)
    }

    pub fn remove(&self, id: &str) -> Result<bool, String> {
        let removed = onepaint_brush::remove_user_brush(id);
        if removed {
            self.save()?;
        }
        Ok(removed)
    }
}

/// "Imported 3 brushes and 1 grain".
pub fn summary(imp: &BrushImport) -> String {
    if imp.brushes == 0 && imp.grains == 0 {
        return "No brushes found in that file".into();
    }
    let brushes = if imp.brushes == 1 { "1 brush".to_string() } else { format!("{} brushes", imp.brushes) };
    match imp.grains {
        0 => format!("Imported {brushes}"),
        1 => format!("Imported {brushes} and 1 grain"),
        n => format!("Imported {brushes} and {n} grains"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, Popover};
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use onepaint_brush::IMPORTED_CATEGORY;

    #[test]
    fn summary_reads_naturally() {
        let imp = |brushes, grains| BrushImport { brushes, grains, ..Default::default() };
        assert_eq!(summary(&imp(1, 0)), "Imported 1 brush");
        assert_eq!(summary(&imp(3, 2)), "Imported 3 brushes and 2 grains");
        assert_eq!(summary(&imp(0, 0)), "No brushes found in that file");
    }

    #[test]
    fn imported_brushes_list_and_select_from_the_popover() {
        let mut h = Harness::builder().with_size(vec2(1200.0, 800.0)).with_max_steps(400).build_eframe(|cc| App::new(cc, None));
        h.run();
        // A brush file built the way the app imports it (no dialog in tests).
        let mut p = onepaint_formats::brushes::import_defaults();
        p.name = "Popover Test Brush".into();
        p.size = 33.0;
        let bytes = onepaint_formats::brushes::encode_happybrush(&p, None, None).unwrap();
        let imp = h.state().brush_library.import("popover.happybrush", &bytes).unwrap();
        assert_eq!(summary(&imp), "Imported 1 brush");
        let id = imp.library.presets[0].id.clone();

        h.state_mut().popover = Some(Popover::Brushes);
        h.run();
        h.get_by_label(IMPORTED_CATEGORY).click();
        h.run();
        assert_eq!(h.state().brush_category, IMPORTED_CATEGORY);
        h.get_by_label("Popover Test Brush").click();
        h.run();
        assert_eq!(h.state().session.current_brush().id, id);
        assert_eq!(h.state().session.current_brush().size, 33.0);
        assert!(h.query_by_label("Import brushes…").is_some());
        assert!(h.query_by_label("Export brush…").is_some());
        assert!(h.state().brush_library.remove(&id).unwrap());
    }
}
