//! happypaint session recordings (`.happypaint-rec`; happypaint
//! `storage/recording-file.ts`, spec 62): the layers when recording began
//! and the command log, for onepaint to replay.
//!
//! Layout: `HPRC`, u16 LE version (1), u16 LE flags, u32 LE manifest
//! length, the manifest as raw-deflated JSON, then the PNG chunks the
//! manifest's nodes point into (offsets from the end of the manifest).

use crate::{FormatError, corrupt, inflate_raw, read_node};
use onepaint_doc::Document;
use serde_json::Value;

/// One recorded command: milliseconds from the start, its name and args.
#[derive(Clone, Debug, PartialEq)]
pub struct HappyCommand {
    pub t: f64,
    pub op: String,
    pub args: Value,
}

pub struct HappyRecording {
    /// The document when recording began (no paper, dpi or depth: happypaint
    /// doesn't record them; later `setPaper` commands do).
    pub doc: Document,
    pub active: Option<String>,
    pub commands: Vec<HappyCommand>,
    pub duration_ms: f64,
}

pub fn read_happypaint_rec(bytes: &[u8]) -> Result<HappyRecording, FormatError> {
    if bytes.len() < 12 || &bytes[..4] != b"HPRC" {
        return Err(FormatError::BadMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version > 1 {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let end = 12usize.checked_add(len).filter(|e| *e <= bytes.len()).ok_or_else(|| corrupt("recording manifest past the end"))?;
    let manifest: Value = serde_json::from_slice(&inflate_raw(&bytes[12..end])?)?;
    let chunks = &bytes[end..];
    let d = manifest.get("doc").ok_or_else(|| corrupt("recording without doc"))?;
    let dim = |k: &str| d.get(k).and_then(Value::as_u64).filter(|v| *v > 0).ok_or_else(|| corrupt("recording without a size"));
    let mut doc = Document::new(dim("width")? as u32, dim("height")? as u32, None);
    doc.root.clear();
    doc.name = "Replay".into();
    let bounds = doc.bounds();
    for n in manifest.get("nodes").and_then(Value::as_array).into_iter().flatten() {
        doc.root.push(read_node(n, chunks, bounds, &crate::Progress::default(), doc.space())?);
    }
    let commands = manifest
        .get("commands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(HappyCommand {
                t: c.get("t").and_then(Value::as_f64).unwrap_or(0.0),
                op: c.get("op")?.as_str()?.to_string(),
                args: c.get("args").cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    Ok(HappyRecording {
        doc,
        active: d.get("activeLayerId").and_then(Value::as_str).map(String::from),
        commands,
        duration_ms: manifest.get("durationMs").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

/// A recording file of `manifest` and `chunks` (tests, and round trips).
pub fn write_happypaint_rec(manifest: &Value, chunks: &[u8]) -> Vec<u8> {
    let m = crate::deflate_raw(manifest.to_string().as_bytes());
    let mut o = Vec::with_capacity(12 + m.len() + chunks.len());
    o.extend(b"HPRC");
    o.extend(1u16.to_le_bytes());
    o.extend(0u16.to_le_bytes());
    o.extend((m.len() as u32).to_le_bytes());
    o.extend(m);
    o.extend(chunks);
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_opens_with_its_layers_and_commands() {
        // A 2 × 1 red layer as a PNG chunk at (3, 4), and two commands.
        let mut png = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png, 2, 1);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[255, 0, 0, 255, 255, 0, 0, 255]).unwrap();
        }
        let manifest = serde_json::json!({
            "version": 1, "durationMs": 1200,
            "doc": {"width": 16, "height": 8, "activeLayerId": "a"},
            "nodes": [{"id": "a", "name": "Ink", "visible": true, "opacity": 1, "blendMode": "normal",
                       "bbox": {"x": 3, "y": 4, "width": 2, "height": 1}, "chunk": {"offset": 0, "len": png.len()}}],
            "commands": [{"t": 10, "op": "addLayer", "args": {"id": "b"}}, {"t": 20, "op": "undo", "args": {}}],
        });
        let r = read_happypaint_rec(&write_happypaint_rec(&manifest, &png)).unwrap();
        assert_eq!((r.doc.width, r.doc.height, r.active.as_deref(), r.duration_ms), (16, 8, Some("a"), 1200.0));
        let l = r.doc.layer("a").unwrap();
        assert!(l.pixels.get(3, 4)[3] > 0.99 && l.pixels.get(5, 4)[3] == 0.0);
        assert_eq!(r.commands.iter().map(|c| c.op.as_str()).collect::<Vec<_>>(), ["addLayer", "undo"]);
        assert_eq!(r.commands[0].args["id"], "b");
        assert!(matches!(read_happypaint_rec(b"nope"), Err(FormatError::BadMagic)));
    }
}
