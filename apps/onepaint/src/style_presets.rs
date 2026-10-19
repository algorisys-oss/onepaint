//! The user's saved layer styles (happypaint `state/style-presets-store.ts`,
//! spec 56): shared across drawings, persisted as JSON in the config folder.

use onepaint_doc::styles::{LayerStyles, StylePreset, parse_style_presets, preset_name, style_presets_json};
use std::path::PathBuf;

pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("art", "onepaint", "onepaint").map(|d| d.config_dir().join("styles.json"))
}

#[derive(Clone, Debug, Default)]
pub struct StylePresets {
    pub list: Vec<StylePreset>,
    /// Where they persist (`None` in tests and scripted runs).
    path: Option<PathBuf>,
}

impl StylePresets {
    pub fn load(path: Option<PathBuf>) -> Self {
        let raw = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok());
        Self { list: parse_style_presets(raw.as_deref()), path }
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // A failed write keeps the presets for this session only, as happypaint.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, style_presets_json(&self.list)).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Save `styles` as a new preset; its id.
    pub fn add(&mut self, name: &str, styles: &LayerStyles) -> String {
        let id = format!("s{}", onepaint_doc::new_id());
        self.list.push(StylePreset { id: id.clone(), name: preset_name(name, self.list.len()), styles: styles.clone() });
        self.save();
        id
    }

    pub fn remove(&mut self, id: &str) {
        let n = self.list.len();
        self.list.retain(|p| p.id != id);
        if self.list.len() != n {
            self.save();
        }
    }

    /// Every style in a `.asl` library becomes a preset (no dedupe, as
    /// happypaint); the notice to show.
    pub fn import_asl(&mut self, bytes: &[u8], file_name: &str) -> String {
        match onepaint_formats::asl::read_asl(bytes) {
            Err(_) => format!("{file_name} isn't a Photoshop style library (.asl)"),
            Ok(lib) if lib.is_empty() => format!("No styles we can use in {file_name}"),
            Ok(lib) => {
                for s in &lib {
                    self.list.push(StylePreset {
                        id: format!("s{}", onepaint_doc::new_id()),
                        name: preset_name(&s.name, self.list.len()),
                        styles: s.styles.clone(),
                    });
                }
                self.save();
                let n = lib.len();
                format!("Imported {n} style{} from {file_name}", if n == 1 { "" } else { "s" })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use onepaint_doc::styles::{EffectKind, default_effect};
    use onepaint_formats::asl::{NamedStyle, write_asl};

    fn stroke() -> LayerStyles {
        LayerStyles { fill_opacity: 1.0, effects: vec![default_effect(EffectKind::Stroke)] }
    }

    #[test]
    fn presets_persist_and_reload() {
        let dir = std::env::temp_dir().join(format!("onepaint-styles-{}", std::process::id()));
        let path = dir.join("styles.json");
        let mut p = StylePresets::load(Some(path.clone()));
        let id = p.add(" Outline ", &stroke());
        p.add("", &stroke());
        let again = StylePresets::load(Some(path.clone()));
        assert_eq!(again.list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Outline", "Style 2"]);
        p.remove(&id);
        assert_eq!(StylePresets::load(Some(path)).list.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_library_imports_as_presets_with_a_notice() {
        let mut p = StylePresets::default();
        let lib = write_asl(&[NamedStyle { name: "Edge".into(), styles: stroke() }]);
        assert_eq!(p.import_asl(&lib, "a.asl"), "Imported 1 style from a.asl");
        assert_eq!(p.import_asl(&lib, "a.asl"), "Imported 1 style from a.asl");
        assert_eq!(p.list.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Edge", "Edge"]);
        assert_ne!(p.list[0].id, p.list[1].id);
        assert_eq!(p.import_asl(&[0; 16], "b.asl"), "b.asl isn't a Photoshop style library (.asl)");
        assert_eq!(p.import_asl(&write_asl(&[]), "c.asl"), "No styles we can use in c.asl");
    }
}
