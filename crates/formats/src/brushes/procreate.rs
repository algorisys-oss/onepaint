//! Procreate `.brush` / `.brushset` (happypaint
//! `storage/brush-import/procreate.ts`, and the parameter mapping in
//! `state/brush-import.ts:367-416`). Both are zips: a `.brush` holds
//! `Brush.archive` (an NSKeyedArchiver bplist), `Shape.png` and `Grain.png`;
//! a `.brushset` holds one such folder per brush.

use super::bplist::{Plist, archive_root, parse_bplist};
use crate::FormatError;
use onepaint_brush::{BrushPreset, Dynamics, RotationDynamics};
use onepaint_stroke::{Binding, CurvePoint, Sensor};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct ProcreateBrush {
    pub name: String,
    /// `Shape.png`: the tip.
    pub shape_png: Option<Vec<u8>>,
    /// `Grain.png`: the texture.
    pub grain_png: Option<Vec<u8>>,
    /// The archive root with values dereferenced one level (empty when absent).
    pub params: BTreeMap<String, Plist>,
}

/// The file name without its extension, or `fallback`.
pub(crate) fn stem(filename: &str, fallback: &str) -> String {
    let s = match filename.rfind('.') {
        Some(i) if i + 1 < filename.len() && filename[i + 1..].bytes().all(|c| c.is_ascii_alphanumeric()) => &filename[..i],
        _ => filename,
    };
    if s.is_empty() { fallback.to_string() } else { s.to_string() }
}

fn uuid_like(s: &str) -> bool {
    s.len() >= 32 && s.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-')
}

/// Parse `.brush` / `.brushset` bytes (errors when it is not a zip).
pub fn parse_procreate(bytes: &[u8], filename: &str) -> Result<Vec<ProcreateBrush>, FormatError> {
    // Group entries by folder, in first-seen order.
    let mut groups: Vec<(String, BTreeMap<String, Vec<u8>>)> = Vec::new();
    for (name, data) in crate::zip::read_zip(bytes)? {
        let (dir, base) = match name.rfind('/') {
            Some(i) => (name[..i].to_string(), name[i + 1..].to_string()),
            None => (String::new(), name),
        };
        match groups.iter_mut().find(|g| g.0 == dir) {
            Some(g) => {
                g.1.insert(base, data);
            }
            None => groups.push((dir, BTreeMap::from([(base, data)]))),
        }
    }
    let fallback = stem(filename, "Imported brush");
    let mut out = Vec::new();
    for (dir, mut files) in groups {
        let (shape, archive) = (files.remove("Shape.png"), files.remove("Brush.archive"));
        if shape.is_none() && archive.is_none() {
            continue; // thumbnails, signatures, metadata
        }
        let params = archive.and_then(|a| parse_bplist(&a).ok()).and_then(|p| archive_root(&p)).unwrap_or_default();
        let name = params.get("name").and_then(Plist::as_str).map(str::trim).unwrap_or("").to_string();
        let leaf = dir.rsplit('/').next().unwrap_or("");
        let dir_name = if uuid_like(leaf) {
            String::new()
        } else if leaf.len() >= 6 && leaf.get(leaf.len() - 6..).is_some_and(|e| e.eq_ignore_ascii_case(".brush")) {
            leaf[..leaf.len() - 6].to_string()
        } else {
            leaf.to_string()
        };
        let name = [name, dir_name].into_iter().find(|s| !s.is_empty()).unwrap_or_else(|| fallback.clone());
        out.push(ProcreateBrush { name, shape_png: shape, grain_png: files.remove("Grain.png"), params });
    }
    Ok(out)
}

/// `clamp(round(px), 8, 512)`.
pub(crate) fn clamp_size(px: f64) -> f64 {
    px.round().clamp(8.0, 512.0)
}

fn random_binding(jitter: f64) -> Binding {
    Binding {
        sensor: Sensor::Random,
        curve: vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)],
        interp: None,
        min: Some((1.0 - jitter).max(0.05)),
        max: Some(1.0),
    }
}

/// Archive keys → preset fields (best effort, community-documented keys).
pub fn map_procreate_params(params: &BTreeMap<String, Plist>, p: &mut BrushPreset) {
    let n = |k: &str| params.get(k).and_then(Plist::as_f64);
    if let Some(v) = n("paintSize") {
        // A 0..1 fraction of Procreate's size slider; 250 px reads close.
        p.size = clamp_size(v * 250.0);
    }
    if let Some(v) = n("plotSpacing").filter(|v| *v > 0.0) {
        p.spacing = v.clamp(0.02, 4.0);
    }
    if let Some(v) = n("paintOpacity") {
        p.opacity = v.clamp(0.05, 1.0);
    }
    if let Some(v) = n("dynamicsPressureSize") {
        p.size_pressure = v.clamp(0.0, 1.0);
    }
    if let Some(v) = n("dynamicsPressureOpacity") {
        p.flow_pressure = v.clamp(0.0, 1.0);
    }
    let mut d = Dynamics::default();
    let mut any = false;
    if let Some(v) = n("dynamicsJitterSize").filter(|v| *v > 0.0) {
        d.size = vec![random_binding(v)];
        any = true;
    }
    if let Some(v) = n("dynamicsJitterOpacity").filter(|v| *v > 0.0) {
        d.flow = vec![random_binding(v)];
        any = true;
    }
    // StrokePath jitter is a positional offset; shape scatter is rotational.
    if let Some(v) = n("plotJitter").filter(|v| *v > 0.0) {
        d.scatter = Some(v.min(2.0));
        any = true;
    }
    let jitter = n("shapeScatter").filter(|v| *v > 0.0).map(|v| v.min(1.0));
    let follow = n("shapeRotation").is_some_and(|v| v.abs() > 0.25);
    if follow || jitter.is_some() {
        d.rotation = Some(RotationDynamics { source: if follow { "angle" } else { "none" }.into(), jitter, offset: None });
        any = true;
    }
    if any {
        p.dynamics = Some(d);
    }
}
