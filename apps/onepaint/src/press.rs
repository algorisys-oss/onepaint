//! The app-level CMYK press (happypaint `state/press-profile-store.ts`): a
//! loaded `.icc` drives the print preview, the gamut warning and the CMYK
//! TIFF separation; without one the built-in coated press does. A loaded
//! profile is kept next to the settings file and restored at launch.

use onepaint_color::cmyk::{Press, press_from_icc};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the profile is kept, beside `settings.json`.
pub fn saved_path(settings: Option<&Path>) -> Option<PathBuf> {
    Some(settings?.parent()?.join("press.icc"))
}

/// The persisted profile; a corrupt or unusable one is dropped.
pub fn restore(path: Option<&Path>) -> Option<Arc<Press>> {
    let path = path?;
    let bytes = std::fs::read(path).ok()?;
    match press_from_icc(&bytes) {
        Ok(p) => Some(Arc::new(p)),
        Err(_) => {
            let _ = std::fs::remove_file(path);
            None
        }
    }
}

/// Bake `bytes` as the press and persist it. Errors read as sentences.
pub fn load(bytes: &[u8], path: Option<&Path>) -> Result<Arc<Press>, String> {
    let p = press_from_icc(bytes).map_err(|e| e.0)?;
    if let Some(path) = path {
        let _ = path.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(path, bytes);
    }
    Ok(Arc::new(p))
}

pub fn forget(path: Option<&Path>) {
    if let Some(path) = path {
        let _ = std::fs::remove_file(path);
    }
}

/// The press everything prints through.
pub fn active(p: &Option<Arc<Press>>) -> &Press {
    p.as_deref().unwrap_or_else(|| onepaint_color::cmyk::builtin_press())
}
