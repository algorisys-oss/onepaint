//! Brush import and export (spec 23): Photoshop `.abr`, Procreate `.brush`
//! and `.brushset`, and happypaint's `.happybrush`, turned into a
//! [`UserLibrary`] the brush registry can serve (happypaint
//! `state/brush-import.ts`).

pub mod abr;
pub mod abr_mapping;
pub mod bplist;
pub mod happybrush;
mod library;
pub mod procreate;
pub mod tip_mask;

pub use abr::{AbrComputed, AbrFile, AbrPattern, AbrTip, packbits, parse_abr};
pub use abr_mapping::{MappedAbr, TipTransform, map_abr_descriptor};
pub use happybrush::{DecodedHappyBrush, decode_happybrush, encode_happybrush};
pub use library::{load_user_library, save_user_library};
pub use procreate::{ProcreateBrush, map_procreate_params, parse_procreate};

use crate::FormatError;
use onepaint_brush::{BrushPreset, Grain, IMPORTED_CATEGORY, Tip, TipMask, USER_GRAIN_PREFIX, USER_TIP_PREFIX, UserGrain, UserLibrary};
use procreate::{clamp_size, stem};
use serde_json::{Map, Value, json};
use std::sync::Arc;
use tip_mask::{GRAIN_SIZE, Polarity, TIP_SIZE};

/// The extensions the importer reads.
pub const BRUSH_EXTENSIONS: [&str; 4] = ["abr", "brush", "brushset", "happybrush"];

pub fn is_brush_file(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| BRUSH_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

/// Neutral settings every imported brush starts from (`state/brush-import.ts:320-334`).
pub fn import_defaults() -> BrushPreset {
    BrushPreset {
        id: String::new(),
        name: String::new(),
        category: IMPORTED_CATEGORY.into(),
        size: 60.0,
        spacing: 0.12,
        spacing_jitter: None,
        hardness: 0.9,
        flow: 0.9,
        opacity: 1.0,
        size_pressure: 0.85,
        flow_pressure: 0.3,
        speed_size: 0.0,
        speed_flow: 0.0,
        smoothing: 0.15,
        stabilizer: None,
        wash: false,
        tilt_strength: 0.0,
        pressure_curve: 1.0,
        tip_id: "round".into(),
        tip_roundness: None,
        tip_angle_deg: None,
        dynamics: None,
        opaque_linearize: Some(0.9),
        taper: None,
        paper_response: None,
        grain: None,
        airbrush: None,
        extra: Map::new(),
    }
}

/// Set the id (empty until hashed), name and category on raw preset JSON.
pub(crate) fn user_preset_fields(m: &mut Map<String, Value>, name: &str) {
    m.insert("id".into(), json!(""));
    m.insert("name".into(), json!(name));
    m.insert("category".into(), json!(IMPORTED_CATEGORY));
}

/// FNV-1a 64: stable, content-derived ids.
fn hash(parts: &[&[u8]]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for &b in p.iter().chain(&[0xff]) {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

/// What one file import produced.
#[derive(Clone, Debug, Default)]
pub struct BrushImport {
    pub library: UserLibrary,
    pub brushes: usize,
    pub grains: usize,
    /// Parse and mapping notes (skipped tips, unmapped features…).
    pub warnings: Vec<String>,
}

impl BrushImport {
    fn add_tip(&mut self, data: Vec<u8>) -> String {
        let id = format!("{USER_TIP_PREFIX}{}", hash(&[&data]));
        self.library.tips.entry(id.clone()).or_insert_with(|| Arc::new(TipMask::from_u8(TIP_SIZE, &data)));
        id
    }

    fn add_grain(&mut self, name: String, data: Vec<u8>) -> String {
        let id = format!("{USER_GRAIN_PREFIX}{}", hash(&[&data]));
        self.library.grains.entry(id.clone()).or_insert_with(|| UserGrain { name, mask: Arc::new(TipMask::from_u8(GRAIN_SIZE, &data)) });
        self.grains += 1;
        id
    }

    /// Name it, give it its content id and add it (`addImportedBrushes`).
    fn add_preset(&mut self, mut p: BrushPreset, name: &str) {
        let name = name.trim();
        p.name = if name.is_empty() { "Brush".into() } else { name.to_string() };
        p.category = IMPORTED_CATEGORY.into();
        p.id = String::new();
        let body = serde_json::to_vec(&p).unwrap_or_default();
        p.id = format!("imported-{}", hash(&[p.name.as_bytes(), &body]));
        self.brushes += 1;
        self.library.merge(UserLibrary { presets: vec![p], ..Default::default() });
    }
}

/// Import every brush and grain in an `.abr`, `.brush`, `.brushset` or
/// `.happybrush` file. Errors when the file is wholly unreadable; malformed
/// brushes inside a readable file are skipped with a warning.
pub fn import_brush_file(filename: &str, bytes: &[u8]) -> Result<BrushImport, FormatError> {
    let ext = filename.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let stem = stem(filename, "Imported");
    match ext.as_str() {
        "abr" => import_abr(bytes, &stem),
        "happybrush" => import_happybrush(bytes),
        _ => import_procreate(bytes, filename),
    }
}

fn import_abr(bytes: &[u8], stem: &str) -> Result<BrushImport, FormatError> {
    let abr = parse_abr(bytes)?;
    let mut out = BrushImport { warnings: abr.warnings, ..Default::default() };
    // Presets in order, with their samp UUID and the dual tip they want.
    let mut items: Vec<(BrushPreset, String, Option<String>, Option<String>)> = Vec::new();
    let mut unmapped: Vec<String> = Vec::new();
    for (i, tip) in abr.tips.iter().enumerate() {
        let mut p = import_defaults();
        // The sampled bitmap's size is its authored diameter; Dmtr overrides it.
        p.size = clamp_size(tip.width.max(tip.height) as f64);
        let mapped = tip.desc.as_ref().map(|d| map_abr_descriptor(d, &mut p));
        if let Some(m) = &mapped {
            for u in &m.unmapped {
                if !unmapped.contains(u) {
                    unmapped.push(u.clone());
                }
            }
        }
        let (mut cov, mut w, mut h) = (tip.data.clone(), tip.width, tip.height);
        if let Some(t) = mapped.as_ref().and_then(|m| m.tip_transform) {
            (cov, w, h) = tip_mask::transform_coverage(&cov, w, h, &t);
        }
        let Some(mask) = tip_mask::coverage_to_tip_mask(&cov, w, h) else {
            out.warnings.push(format!("tip #{} was empty", i + 1));
            continue;
        };
        p.tip_id = out.add_tip(mask);
        let name = tip.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("{stem} {}", i + 1));
        items.push((p, name, tip.uuid.clone(), mapped.and_then(|m| m.dual_tip_uuid)));
    }
    if !unmapped.is_empty() {
        out.warnings.push(format!("unmapped descriptor features: {}", unmapped.join("; ")));
    }
    // Dual brush: the descriptor names another sampled tip of this pack.
    let tip_by_uuid: Vec<(String, String)> = items.iter().filter_map(|(p, _, u, _)| Some((u.clone()?, p.tip_id.clone()))).collect();
    for (p, _, _, dual) in &mut items {
        let Some(dual) = dual else { continue };
        match tip_by_uuid.iter().find(|(u, _)| u == dual) {
            Some((_, tip)) => {
                p.extra.insert("maskTip".into(), json!({ "id": tip, "strength": 0.7 }));
            }
            None => out.warnings.push(format!("dual-brush tip {dual} not in this pack (maskTip skipped)")),
        }
    }
    for (p, name, _, _) in items {
        out.add_preset(p, &name);
    }
    for (i, c) in abr.computed.iter().enumerate() {
        let mut p = import_defaults();
        p.size = clamp_size(c.diameter);
        p.hardness = c.hardness;
        p.spacing = c.spacing.min(1.0);
        let name = c.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| format!("{stem} round {}", i + 1));
        out.add_preset(p, &name);
    }
    for pat in &abr.patterns {
        if let Some(g) = tip_mask::gray_to_grain(&pat.data, pat.width, pat.height) {
            out.add_grain(pat.name.clone(), g);
        }
    }
    Ok(out)
}

fn import_procreate(bytes: &[u8], filename: &str) -> Result<BrushImport, FormatError> {
    let mut out = BrushImport::default();
    for b in parse_procreate(bytes, filename)? {
        let mut p = import_defaults();
        map_procreate_params(&b.params, &mut p);
        if let Some(png) = &b.grain_png {
            match crate::decode_image(png).ok().and_then(|i| tip_mask::image_to_grain(&i.rgba, i.width as usize, i.height as usize)) {
                Some(g) => {
                    let id = out.add_grain(format!("{} grain", b.name), g);
                    let n = |k: &str| b.params.get(k).and_then(bplist::Plist::as_f64);
                    p.grain = Some(Grain {
                        id,
                        scale: n("textureScale").map_or(90.0, |v| (30.0 + v.clamp(0.0, 1.0) * 170.0).round()),
                        strength: n("textureDepth").map_or(0.35, |v| v.clamp(0.05, 1.0)),
                        moves: Some(0.0),
                        random: None,
                    });
                }
                None => out.warnings.push(format!("could not decode Grain.png for \"{}\"", b.name)),
            }
        }
        if let Some(png) = &b.shape_png {
            // Shape.png ships in both polarities, so pick per image.
            match crate::decode_image(png)
                .ok()
                .map(|i| tip_mask::image_to_tip_mask(&i.rgba, i.width as usize, i.height as usize, Polarity::Auto))
            {
                Some(Some(mask)) => p.tip_id = out.add_tip(mask),
                Some(None) => {}
                None => out.warnings.push(format!("could not decode Shape.png for \"{}\"", b.name)),
            }
        }
        out.add_preset(p, &b.name);
    }
    Ok(out)
}

/// Settings restore verbatim; embedded bitmaps get fresh ids; custom
/// references with no bitmap degrade to safe built-ins.
fn import_happybrush(bytes: &[u8]) -> Result<BrushImport, FormatError> {
    let d = decode_happybrush(bytes)?;
    let mut out = BrushImport::default();
    let mut p = d.settings;
    match (d.grain, p.grain.as_mut()) {
        (Some(g), Some(pg)) => pg.id = out.add_grain(format!("{} grain", d.name), g),
        (_, Some(pg)) if pg.id.starts_with(USER_GRAIN_PREFIX) => p.grain = None,
        _ => {}
    }
    match d.tip {
        Some(t) => p.tip_id = out.add_tip(t),
        None if p.tip_id.starts_with(USER_TIP_PREFIX) => p.tip_id = "round".into(),
        None => {}
    }
    out.add_preset(p, &d.name);
    Ok(out)
}

/// The preset as a `.happybrush`, its user tip and grain taken from the registry.
pub fn export_happybrush(p: &BrushPreset) -> Result<Vec<u8>, FormatError> {
    let tip = match onepaint_brush::tip_for(&p.tip_id) {
        Tip::Image(m) if p.tip_id.starts_with(USER_TIP_PREFIX) => Some(m),
        _ => None,
    };
    let grain = p.grain.as_ref().filter(|g| g.id.starts_with(USER_GRAIN_PREFIX)).and_then(|g| onepaint_brush::grain_for(&g.id));
    encode_happybrush(p, tip.as_deref(), grain.as_deref())
}
