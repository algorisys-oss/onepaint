//! The user brush library on disk (spec 23): `library.json` plus one
//! grayscale PNG per tip (`tips/`) and grain (`grains/`). happypaint keeps
//! the same data in localStorage and IndexedDB.

use super::tip_mask::encode_gray_png;
use crate::FormatError;
use onepaint_brush::{BrushPreset, TipMask, UserGrain, UserLibrary};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Asset {
    id: String,
    file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Index {
    version: u32,
    presets: Vec<BrushPreset>,
    #[serde(default)]
    tips: Vec<Asset>,
    #[serde(default)]
    grains: Vec<Asset>,
}

/// `custom:abc` → `tips/abc.png` (anything outside `[A-Za-z0-9_-]` becomes `_`).
fn file_for(dir: &str, id: &str) -> String {
    let leaf = id.rsplit(':').next().unwrap_or(id);
    let safe: String = leaf.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    format!("{dir}/{safe}.png")
}

fn write_png(root: &Path, file: &str, mask: &TipMask) -> Result<(), FormatError> {
    let path = root.join(file);
    if !path.exists() {
        std::fs::write(&path, encode_gray_png(mask.size, &mask.to_u8())?)?;
    }
    Ok(())
}

/// Remove PNGs in `root/dir` that the index no longer lists.
fn prune(root: &Path, dir: &str, keep: &BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(root.join(dir)) else { return };
    for e in entries.flatten() {
        let name = format!("{dir}/{}", e.file_name().to_string_lossy());
        if name.ends_with(".png") && !keep.contains(&name) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Write the library into `root` (created if needed); the index is replaced atomically.
pub fn save_user_library(root: &Path, lib: &UserLibrary) -> Result<(), FormatError> {
    std::fs::create_dir_all(root.join("tips"))?;
    std::fs::create_dir_all(root.join("grains"))?;
    let mut index = Index { version: VERSION, presets: lib.presets.clone(), tips: Vec::new(), grains: Vec::new() };
    for (id, mask) in &lib.tips {
        let file = file_for("tips", id);
        write_png(root, &file, mask)?;
        index.tips.push(Asset { id: id.clone(), file, name: None });
    }
    for (id, g) in &lib.grains {
        let file = file_for("grains", id);
        write_png(root, &file, &g.mask)?;
        index.grains.push(Asset { id: id.clone(), file, name: Some(g.name.clone()) });
    }
    let tmp = root.join("library.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&index)?)?;
    std::fs::rename(&tmp, root.join("library.json"))?;
    prune(root, "tips", &index.tips.iter().map(|a| a.file.clone()).collect());
    prune(root, "grains", &index.grains.iter().map(|a| a.file.clone()).collect());
    Ok(())
}

fn read_mask(root: &Path, file: &str) -> Option<TipMask> {
    let img = crate::decode_image(&std::fs::read(root.join(file)).ok()?).ok()?;
    if img.width != img.height || img.width == 0 {
        return None;
    }
    let red: Vec<u8> = img.rgba.chunks_exact(4).map(|p| p[0]).collect();
    Some(TipMask::from_u8(img.width as usize, &red))
}

/// Read the library from `root`; a missing or unreadable library is empty,
/// and a missing bitmap just drops that tip or grain (presets fall back to round).
pub fn load_user_library(root: &Path) -> UserLibrary {
    let Some(index) = std::fs::read(root.join("library.json")).ok().and_then(|b| serde_json::from_slice::<Index>(&b).ok()) else {
        return UserLibrary::default();
    };
    let mut lib = UserLibrary { presets: index.presets, ..Default::default() };
    for a in index.tips {
        if let Some(m) = read_mask(root, &a.file) {
            lib.tips.insert(a.id, Arc::new(m));
        }
    }
    for a in index.grains {
        if let Some(m) = read_mask(root, &a.file) {
            lib.grains.insert(a.id, UserGrain { name: a.name.unwrap_or_default(), mask: Arc::new(m) });
        }
    }
    lib
}
