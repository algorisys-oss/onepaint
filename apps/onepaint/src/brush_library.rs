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

    /// Recent picks show above the list, per tool; sections fold; the
    /// library actions sit above the list (happypaint v0.205.1). Both themes.
    #[test]
    fn recent_brushes_and_folding_sections() {
        for light in [false, true] {
            let mut h = Harness::builder().with_size(vec2(1200.0, 900.0)).with_max_steps(400).build_eframe(|cc| App::new(cc, None));
            h.run();
            if light {
                let ctx = h.ctx.clone();
                h.state_mut().toggle_theme(&ctx);
            }
            h.state_mut().settings.brush_recent.clear();
            h.state_mut().settings.brush_folded.clear();
            h.state_mut().popover = Some(Popover::Brushes);
            h.run();
            assert!(h.query_by_label("Recent").is_none(), "no picks yet, no Recent section");
            let import = h.get_by_label("Import brushes…").rect().min.y;
            let cat = h.state().brush_category.clone();
            // The category is a chip above and the section header below.
            let header = h.get_all_by_label(&cat).last().unwrap().rect().min.y;
            assert!(import < header, "the actions sit above the list");

            let names: Vec<String> = onepaint_brush::presets().into_iter().filter(|p| p.category == cat).map(|p| p.name).take(2).collect();
            for n in &names {
                h.get_all_by_label(n).last().unwrap().click();
                h.run();
            }
            let recent = h.state().settings.brush_recent["brush"].clone();
            assert_eq!(recent.len(), 2);
            assert_eq!(h.state().session.current_brush().id, recent[0], "newest first");
            assert_eq!(h.query_all_by_label(&names[0]).count(), 2, "in Recent and in its category");

            // Fold Recent: its rows go; the category list stays.
            h.get_by_label("Recent").click();
            h.run();
            assert_eq!(h.query_all_by_label(&names[0]).count(), 1);
            // Fold the category too: a search still finds the brush.
            h.get_all_by_label(&cat).last().unwrap().click();
            h.run();
            assert!(h.query_by_label(&names[0]).is_none());
            h.state_mut().brush_query = names[0].clone();
            h.run();
            assert!(h.query_by_label(&names[0]).is_some(), "a search shows folded matches");
            h.state_mut().brush_query.clear();

            // The eraser keeps its own list.
            h.state_mut().set_tool(onepaint_engine::Tool::Eraser);
            h.state_mut().popover = Some(Popover::Brushes);
            h.run();
            assert!(h.query_by_label("Recent").is_none(), "the eraser's list is empty");
        }
    }

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

/// Whether a brush shows for a library search (happypaint's
/// `brush-library.tsx`): the lowercased, trimmed `query` in its name or
/// category; an empty query matches everything.
pub fn matches(name: &str, category: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || name.to_lowercase().contains(&q) || category.to_lowercase().contains(&q)
}

#[cfg(test)]
mod search_tests {
    #[test]
    fn search_matches_name_or_category() {
        use super::matches;
        assert!(matches("Soft Round", "Airbrush", ""));
        assert!(matches("Soft Round", "Airbrush", "  ROUND "));
        assert!(matches("Soft Round", "Airbrush", "airb"));
        assert!(!matches("Soft Round", "Airbrush", "ink"));
    }
}

/// Brushes picked from the library, newest first (happypaint
/// `brush-library-prefs.ts`, v0.205.1).
pub const RECENT_MAX: usize = 8;

/// The recent list a tool keeps: erasers and smudges apart from brushes.
pub fn recent_key(tool: onepaint_engine::Tool) -> &'static str {
    match tool {
        onepaint_engine::Tool::Eraser => "eraser",
        onepaint_engine::Tool::Smudge => "smudge",
        _ => "brush",
    }
}

/// `id` moved to the front of `list`, once, capped at [`RECENT_MAX`].
pub fn push_recent(list: &mut Vec<String>, id: &str) {
    list.retain(|x| x != id);
    list.insert(0, id.to_string());
    list.truncate(RECENT_MAX);
}

/// Settings read from disk: each list deduplicated and capped.
pub fn tidy_recent(map: &mut std::collections::BTreeMap<String, Vec<String>>) {
    for list in map.values_mut() {
        let mut seen = std::collections::HashSet::new();
        list.retain(|x| seen.insert(x.clone()));
        list.truncate(RECENT_MAX);
    }
}

/// The fold key of a library section.
pub fn section_key(name: &str) -> String {
    format!("section:{name}")
}

#[cfg(test)]
mod recent_tests {
    use super::*;
    use onepaint_engine::Tool;

    // brush-library-prefs.test.ts
    #[test]
    fn newest_first_no_duplicates() {
        let mut l = vec!["a".to_string(), "b".into()];
        push_recent(&mut l, "b");
        assert_eq!(l, ["b", "a"]);
        push_recent(&mut l, "c");
        assert_eq!(l, ["c", "b", "a"]);
    }

    #[test]
    fn capped_dropping_the_oldest() {
        let mut l = Vec::new();
        for i in 0..10 {
            push_recent(&mut l, &format!("p{i}"));
        }
        assert_eq!(l.len(), RECENT_MAX);
        assert_eq!((l[0].as_str(), l[7].as_str()), ("p9", "p2"));
    }

    #[test]
    fn eraser_and_smudge_keep_their_own_lists() {
        assert_eq!(recent_key(Tool::Eraser), "eraser");
        assert_eq!(recent_key(Tool::Smudge), "smudge");
        assert_eq!(recent_key(Tool::Brush), "brush");
        assert_eq!(recent_key(Tool::Fill), "brush", "painting tools share one");
    }

    #[test]
    fn stored_lists_are_deduped_and_capped() {
        let mut m = std::collections::BTreeMap::new();
        m.insert("brush".to_string(), ["a", "a", "b"].iter().map(|s| s.to_string()).chain((0..20).map(|i| format!("x{i}"))).collect());
        tidy_recent(&mut m);
        assert_eq!(m["brush"].len(), RECENT_MAX);
        assert_eq!(&m["brush"][..2], ["a", "b"]);
    }
}
