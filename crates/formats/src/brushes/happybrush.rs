//! `.happybrush`, happypaint's open brush format (`storage/happybrush.ts`):
//! a stored zip of `manifest.json` (`{format, version, name, settings}`),
//! an optional 128² `tip.png` and an optional 256² `grain.png`.

use super::tip_mask::{GRAIN_SIZE, TIP_SIZE, encode_gray_png, image_red_square};
use super::{import_defaults, user_preset_fields};
use crate::FormatError;
use onepaint_brush::{BrushPreset, TipMask, USER_GRAIN_PREFIX, USER_TIP_PREFIX};
use serde_json::{Value, json};

pub const HAPPYBRUSH_EXT: &str = "happybrush";
pub const HAPPYBRUSH_VERSION: u64 = 1;
/// The ids an exported file uses for its embedded bitmaps.
pub const EMBEDDED_TIP: &str = "custom:embedded";
pub const EMBEDDED_GRAIN: &str = "customgrain:embedded";

fn bad(s: &str) -> FormatError {
    FormatError::Corrupt(format!("happybrush: {s}"))
}

/// happypaint's `BrushSettings` for a preset: everything but id, name and category.
pub fn preset_settings(p: &BrushPreset) -> Result<serde_json::Map<String, Value>, FormatError> {
    let Value::Object(mut m) = serde_json::to_value(p)? else { return Err(bad("preset is not an object")) };
    for k in ["id", "name", "category"] {
        m.remove(k);
    }
    Ok(m)
}

/// Encode a preset. `tip` and `grain` are embedded only when the preset uses
/// a user tip (`custom:…`) or grain (`customgrain:…`); built-in ids stay ids.
pub fn encode_happybrush(p: &BrushPreset, tip: Option<&TipMask>, grain: Option<&TipMask>) -> Result<Vec<u8>, FormatError> {
    let mut settings = preset_settings(p)?;
    let mut files: Vec<(&str, Vec<u8>)> = Vec::new();
    if let Some(t) = tip.filter(|_| p.tip_id.starts_with(USER_TIP_PREFIX)) {
        files.push(("tip.png", encode_gray_png(t.size, &t.to_u8())?));
        settings.insert("tipId".into(), json!(EMBEDDED_TIP));
    }
    if let (Some(g), Some(pg)) = (grain, p.grain.as_ref().filter(|g| g.id.starts_with(USER_GRAIN_PREFIX))) {
        files.push(("grain.png", encode_gray_png(g.size, &g.to_u8())?));
        let mut gv = serde_json::to_value(pg)?;
        gv["id"] = json!(EMBEDDED_GRAIN);
        settings.insert("grain".into(), gv);
    }
    let manifest = json!({ "format": "happybrush", "version": HAPPYBRUSH_VERSION, "name": p.name, "settings": settings });
    let manifest = serde_json::to_vec_pretty(&manifest)?;
    let mut entries: Vec<(&str, &[u8])> = vec![("manifest.json", &manifest)];
    entries.extend(files.iter().map(|(n, d)| (*n, d.as_slice())));
    Ok(crate::zip::write_zip(&entries))
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedHappyBrush {
    pub name: String,
    /// The settings over the import defaults; id empty, category `Imported`.
    pub settings: BrushPreset,
    /// 128² coverage when the file embeds a tip.
    pub tip: Option<Vec<u8>>,
    /// 256² grain when the file embeds one.
    pub grain: Option<Vec<u8>>,
}

fn square_png(bytes: &[u8], size: usize) -> Result<Vec<u8>, FormatError> {
    let img = crate::decode_image(bytes)?;
    image_red_square(&img.rgba, img.width as usize, img.height as usize, size).ok_or_else(|| bad("bad image"))
}

/// Decode a `.happybrush` (errors on a malformed one).
pub fn decode_happybrush(bytes: &[u8]) -> Result<DecodedHappyBrush, FormatError> {
    let entries = crate::zip::read_zip(bytes)?;
    let entry = |n: &str| entries.iter().find(|e| e.0.rsplit('/').next() == Some(n)).map(|e| e.1.as_slice());
    let manifest: Value = serde_json::from_slice(entry("manifest.json").ok_or_else(|| bad("no manifest.json"))?)?;
    let Some(Value::Object(settings)) = manifest.get("settings") else { return Err(bad("bad manifest")) };
    if manifest.get("format").and_then(Value::as_str) != Some("happybrush") {
        return Err(bad("bad manifest"));
    }
    if let Some(v) = manifest.get("version").and_then(Value::as_f64)
        && v > HAPPYBRUSH_VERSION as f64
    {
        return Err(bad(&format!("unsupported version {v}")));
    }
    let name = manifest.get("name").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).unwrap_or("Imported brush");
    let Value::Object(mut merged) = serde_json::to_value(import_defaults())? else { return Err(bad("defaults")) };
    merged.extend(settings.clone());
    user_preset_fields(&mut merged, name);
    let settings: BrushPreset = serde_json::from_value(Value::Object(merged))?;
    let tip = entry("tip.png").map(|b| square_png(b, TIP_SIZE)).transpose()?;
    let grain = entry("grain.png").map(|b| square_png(b, GRAIN_SIZE)).transpose()?;
    Ok(DecodedHappyBrush { name: name.to_string(), settings, tip, grain })
}
