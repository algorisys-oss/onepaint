//! Palette import/export (happypaint `storage/palette-io.ts`, spec 21):
//! Procreate `.swatches`, Photoshop `.aco`, Adobe `.ase`, GIMP `.gpl`.

use crate::FormatError;
use crate::zip::{read_zip, write_zip};
use onepaint_color::Rgba;

#[derive(Clone, Debug, PartialEq)]
pub struct ImportedPalette {
    pub name: String,
    pub colors: Vec<String>,
}

fn err(s: impl Into<String>) -> FormatError {
    FormatError::Corrupt(s.into())
}

fn hex8(r: f64, g: f64, b: f64) -> String {
    let c = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

fn hsv_hex(h: f64, s: f64, v: f64) -> String {
    Rgba::from_hsv(h as f32, s as f32, v as f32, 1.0).to_hex()
}

fn cmyk_hex(c: f64, m: f64, y: f64, k: f64) -> String {
    let k1 = 1.0 - k.clamp(0.0, 1.0);
    hex8(255.0 * (1.0 - c.clamp(0.0, 1.0)) * k1, 255.0 * (1.0 - m.clamp(0.0, 1.0)) * k1, 255.0 * (1.0 - y.clamp(0.0, 1.0)) * k1)
}

fn base_name(file: &str) -> String {
    let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem.is_empty() { "Imported palette".into() } else { stem.into() }
}

fn be16(b: &[u8], p: usize) -> Option<u16> {
    b.get(p..p + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn be32(b: &[u8], p: usize) -> Option<u32> {
    b.get(p..p + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn bef32(b: &[u8], p: usize) -> f64 {
    be32(b, p).map_or(0.0, |v| f32::from_bits(v) as f64)
}

fn parse_swatches(file: &str, bytes: &[u8]) -> Result<Vec<ImportedPalette>, FormatError> {
    let entries = read_zip(bytes)?;
    let (_, json) = entries
        .iter()
        .find(|(n, _)| n.to_ascii_lowercase().ends_with("swatches.json"))
        .ok_or_else(|| err(".swatches: no Swatches.json inside"))?;
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| err(format!(".swatches: {e}")))?;
    let list = match v {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    let base = base_name(file);
    let num = |o: &serde_json::Value, k: &str| o.get(k).and_then(serde_json::Value::as_f64);
    let out: Vec<ImportedPalette> = list
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let sw = p.get("swatches")?.as_array()?;
            let colors: Vec<String> = sw
                .iter()
                .filter_map(|s| {
                    let h = num(s, "hue")?;
                    Some(hsv_hex(
                        h.clamp(0.0, 1.0) * 360.0,
                        num(s, "saturation").unwrap_or(0.0).clamp(0.0, 1.0),
                        num(s, "brightness").unwrap_or(0.0).clamp(0.0, 1.0),
                    ))
                })
                .collect();
            let name = p.get("name").and_then(|n| n.as_str()).map(str::trim).filter(|n| !n.is_empty()).map(String::from);
            let name = name.unwrap_or_else(|| if i == 0 { base.clone() } else { format!("{base} {}", i + 1) });
            (!colors.is_empty()).then_some(ImportedPalette { name, colors })
        })
        .collect();
    Ok(out)
}

/// A Procreate-compatible `.swatches` file (one palette, 30 slots).
pub fn export_swatches(name: &str, colors: &[String]) -> Vec<u8> {
    let mut sw: Vec<serde_json::Value> = colors
        .iter()
        .take(30)
        .filter_map(|h| Rgba::from_hex(h))
        .map(|c| {
            let (h, s, v) = c.to_hsv();
            serde_json::json!({"hue": h as f64 / 360.0, "saturation": s, "brightness": v, "alpha": 1, "colorSpace": 0})
        })
        .collect();
    while sw.len() < 30 {
        sw.push(serde_json::Value::Null);
    }
    let json = serde_json::json!([{"name": name, "swatches": sw}]).to_string();
    write_zip(&[("Swatches.json", json.as_bytes())])
}

fn parse_aco(file: &str, b: &[u8]) -> Result<Vec<ImportedPalette>, FormatError> {
    let version = be16(b, 0).ok_or_else(|| err(".aco: file too short"))?;
    if version != 1 && version != 2 {
        return Err(err(format!(".aco: unsupported version {version}")));
    }
    let count = be16(b, 2).unwrap_or(0) as usize;
    let mut colors = Vec::new();
    let mut p = 4;
    for _ in 0..count {
        let (Some(space), Some(w), Some(x), Some(y), Some(z)) =
            (be16(b, p), be16(b, p + 2), be16(b, p + 4), be16(b, p + 6), be16(b, p + 8))
        else {
            break;
        };
        let (w, x, y, z) = (w as f64, x as f64, y as f64, z as f64);
        p += 10;
        if version == 2 {
            p += 4 + be16(b, p + 2).unwrap_or(0) as usize * 2;
        }
        match space {
            0 => colors.push(hex8(w / 257.0, x / 257.0, y / 257.0)),
            1 => colors.push(hsv_hex(w / 65535.0 * 360.0, x / 65535.0, y / 65535.0)),
            2 => colors.push(cmyk_hex(1.0 - w / 65535.0, 1.0 - x / 65535.0, 1.0 - y / 65535.0, 1.0 - z / 65535.0)),
            8 => {
                let g = (1.0 - w / 10000.0) * 255.0;
                colors.push(hex8(g, g, g));
            }
            _ => {}
        }
    }
    if colors.is_empty() {
        return Err(err(".aco: no supported colours found"));
    }
    Ok(vec![ImportedPalette { name: base_name(file), colors }])
}

fn parse_ase(file: &str, b: &[u8]) -> Result<Vec<ImportedPalette>, FormatError> {
    if b.len() < 12 || &b[0..4] != b"ASEF" {
        return Err(err(".ase: not an Adobe Swatch Exchange file"));
    }
    let blocks = be32(b, 8).unwrap_or(0) as usize;
    let mut colors = Vec::new();
    let mut p = 12;
    for _ in 0..blocks {
        let (Some(kind), Some(len)) = (be16(b, p), be32(b, p + 2)) else { break };
        let body = p + 6;
        p = body + len as usize;
        if kind != 1 {
            continue;
        }
        let name_len = be16(b, body).unwrap_or(0) as usize;
        let q = body + 2 + name_len * 2;
        let Some(model) = b.get(q..q + 4) else { break };
        let q = q + 4;
        match model {
            b"RGB " => colors.push(hex8(
                bef32(b, q).clamp(0.0, 1.0) * 255.0,
                bef32(b, q + 4).clamp(0.0, 1.0) * 255.0,
                bef32(b, q + 8).clamp(0.0, 1.0) * 255.0,
            )),
            b"CMYK" => colors.push(cmyk_hex(bef32(b, q), bef32(b, q + 4), bef32(b, q + 8), bef32(b, q + 12))),
            b"Gray" => {
                let g = bef32(b, q).clamp(0.0, 1.0) * 255.0;
                colors.push(hex8(g, g, g));
            }
            _ => {}
        }
    }
    if colors.is_empty() {
        return Err(err(".ase: no supported colours found"));
    }
    Ok(vec![ImportedPalette { name: base_name(file), colors }])
}

fn parse_gpl(file: &str, b: &[u8]) -> Result<Vec<ImportedPalette>, FormatError> {
    let text = String::from_utf8_lossy(b);
    if !text.lines().any(|l| l.starts_with("GIMP Palette")) {
        return Err(err(".gpl: missing \"GIMP Palette\" header"));
    }
    let mut name = base_name(file);
    let mut colors = Vec::new();
    for line in text.lines() {
        if let Some(n) = line.strip_prefix("Name:") {
            if !n.trim().is_empty() {
                name = n.trim().to_string();
            }
            continue;
        }
        if line.starts_with("GIMP Palette") || line.starts_with("Columns:") || line.starts_with('#') {
            continue;
        }
        let nums: Vec<u32> = line.split_whitespace().take(3).map_while(|t| t.parse::<u32>().ok().filter(|_| t.len() <= 3)).collect();
        if nums.len() == 3 {
            colors.push(hex8(nums[0].min(255) as f64, nums[1].min(255) as f64, nums[2].min(255) as f64));
        }
    }
    if colors.is_empty() {
        return Err(err(".gpl: no colours found"));
    }
    Ok(vec![ImportedPalette { name, colors }])
}

/// Parse a palette file by extension, with a readable error.
pub fn parse_palette(file: &str, bytes: &[u8]) -> Result<Vec<ImportedPalette>, FormatError> {
    let ext = std::path::Path::new(file).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    match ext.as_str() {
        "swatches" => parse_swatches(file, bytes),
        "aco" => parse_aco(file, bytes),
        "ase" => parse_ase(file, bytes),
        "gpl" => parse_gpl(file, bytes),
        _ => Err(err(format!("Unsupported palette format \".{ext}\" (use .swatches, .aco, .ase or .gpl)"))),
    }
}
