//! Photoshop `.abr` parser (happypaint `storage/brush-import/abr.ts`):
//! sampled tips (`samp`, or v1/v2 records), computed round brushes (v1/v2),
//! patterns (`patt` → grains) and the `desc` Action Descriptor tree that
//! carries names and parameters. Big-endian throughout; see spec 23.

use crate::FormatError;
use serde_json::{Map, Value, json};
use std::io::Read;

#[derive(Clone, Debug, PartialEq)]
pub struct AbrTip {
    pub name: Option<String>,
    pub width: usize,
    pub height: usize,
    /// 8-bit coverage, 255 = opaque.
    pub data: Vec<u8>,
    /// The samp record's UUID, which links it to its `desc` preset.
    pub uuid: Option<String>,
    /// The preset's descriptor (`Brsh` list entry), when linked.
    pub desc: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AbrComputed {
    pub name: Option<String>,
    pub diameter: f64,
    /// 0..1
    pub hardness: f64,
    /// Fraction of the diameter.
    pub spacing: f64,
    pub angle: f64,
    pub roundness: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AbrPattern {
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// 8-bit luminance.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AbrFile {
    pub tips: Vec<AbrTip>,
    pub computed: Vec<AbrComputed>,
    pub patterns: Vec<AbrPattern>,
    /// Notes about skipped content, never silent.
    pub warnings: Vec<String>,
}

/// PackBits RLE decode of `src` into exactly `out.len()` bytes (`abr.ts:60-74`).
pub fn packbits(src: &[u8], out: &mut [u8]) {
    let (mut s, mut d) = (0, 0);
    while s < src.len() && d < out.len() {
        let n = src[s] as i8;
        s += 1;
        if n >= 0 {
            for _ in 0..=n as usize {
                if d >= out.len() || s >= src.len() {
                    break;
                }
                out[d] = src[s];
                d += 1;
                s += 1;
            }
        } else if n != -128 {
            let v = src.get(s).copied().unwrap_or(0);
            s += 1;
            for _ in 0..(1 - n as i32) {
                if d >= out.len() {
                    break;
                }
                out[d] = v;
                d += 1;
            }
        }
    }
}

/// Bounds-checked big-endian cursor; `None` is happypaint's thrown RangeError.
#[derive(Clone)]
struct Reader<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p.checked_add(n)?)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|s| u16::from_be_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|v| v as i16)
    }
    fn i32(&mut self) -> Option<i32> {
        self.take(4).map(|s| i32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn f64(&mut self) -> Option<f64> {
        self.take(8).map(|s| f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
    }
    fn fourcc(&mut self) -> Option<String> {
        self.take(4).map(|s| s.iter().map(|&c| c as char).collect())
    }
    /// i32 UTF-16 unit count (incl. a trailing null), then UTF-16BE; nulls dropped.
    fn unicode(&mut self) -> Option<String> {
        let n = self.i32()?.max(0) as usize;
        let s = self.take(n.checked_mul(2)?)?;
        let units: Vec<u16> = s.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).filter(|&c| c != 0).collect();
        Some(String::from_utf16_lossy(&units))
    }
    fn pascal(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        self.take(n).map(|s| s.iter().map(|&c| c as char).collect())
    }
}

/// Sampled bitmap after its bounds: depth, compression, data (`abr.ts:130-161`).
/// `None` = malformed (throws in happypaint), `Some(None)` = skipped with a warning.
fn read_sampled(r: &mut Reader, w: i64, h: i64, warnings: &mut Vec<String>) -> Option<Option<Vec<u8>>> {
    let depth = r.i16()?;
    let compressed = r.u8()?;
    if w <= 0 || h <= 0 || w * h > 8192 * 8192 {
        warnings.push(format!("skipped tip with bad dimensions {w}×{h}"));
        return Some(None);
    }
    if depth != 8 && depth != 16 {
        warnings.push(format!("skipped tip with unsupported depth {depth}"));
        return Some(None);
    }
    let (w, h, bpp) = (w as usize, h as usize, (depth >> 3) as usize);
    let mut out = vec![0u8; w * h];
    if compressed == 0 {
        // Out-of-range bytes read as 0, like a typed-array read past the end.
        for (i, o) in out.iter_mut().enumerate() {
            *o = r.b.get(r.p + i * bpp).copied().unwrap_or(0);
        }
        r.p += w * h * bpp;
        return Some(Some(out));
    }
    let mut lens = Vec::with_capacity(h);
    for _ in 0..h {
        lens.push(r.u16()? as usize);
    }
    let mut row = vec![0u8; w * bpp];
    for (y, &n) in lens.iter().enumerate() {
        row.fill(0);
        let end = (r.p + n).min(r.b.len());
        packbits(r.b.get(r.p.min(end)..end).unwrap_or(&[]), &mut row);
        r.p += n;
        for x in 0..w {
            out[y * w + x] = row[x * bpp];
        }
    }
    Some(Some(out))
}

fn parse_v12(r: &mut Reader, version: i16) -> AbrFile {
    let mut out = AbrFile::default();
    let count = r.i16().unwrap_or(0);
    let mut i = 0;
    while i < count && r.p + 6 <= r.b.len() {
        let (Some(kind), Some(size)) = (r.i16(), r.i32()) else { break };
        let next = (r.p as i64 + size as i64).max(0) as usize;
        let ok = (|| -> Option<()> {
            match kind {
                1 => {
                    r.i32()?;
                    let spacing = r.i16()? as f64;
                    let name = if version == 2 { Some(r.unicode()?) } else { None };
                    r.u8()?;
                    let (diameter, roundness, angle, hardness) = (r.i16()?, r.i16()?, r.i16()?, r.i16()?);
                    out.computed.push(AbrComputed {
                        name,
                        diameter: diameter as f64,
                        hardness: (hardness as f64 / 100.0).clamp(0.0, 1.0),
                        spacing: (spacing / 100.0).max(0.02),
                        angle: angle as f64,
                        roundness: roundness as f64,
                    });
                }
                2 => {
                    r.i32()?;
                    r.i16()?;
                    let name = if version == 2 { Some(r.unicode()?) } else { None };
                    r.u8()?;
                    r.take(8)?;
                    let (top, left, bottom, right) = (r.i32()? as i64, r.i32()? as i64, r.i32()? as i64, r.i32()? as i64);
                    let (w, h) = (right - left, bottom - top);
                    if let Some(data) = read_sampled(r, w, h, &mut out.warnings)? {
                        out.tips.push(AbrTip { name, width: w as usize, height: h as usize, data, uuid: None, desc: None });
                    }
                }
                _ => out.warnings.push(format!("skipped unknown v{version} brush type {kind}")),
            }
            Some(())
        })();
        if ok.is_none() {
            out.warnings.push(format!("skipped malformed v{version} brush #{}", i + 1));
        }
        r.p = next;
        i += 1;
    }
    out
}

fn padded_next(p: usize, len: i32) -> usize {
    p + ((len as usize + 3) & !3)
}

fn parse_samp(mut r: Reader, end: usize, subversion: i16, out: &mut AbrFile) {
    while r.p + 4 <= end {
        let Some(len) = r.i32() else { break };
        if len <= 0 || r.p + len as usize > end + 4 {
            break;
        }
        let next = padded_next(r.p, len);
        let ok = (|| -> Option<()> {
            // A 37-byte id field: a Pascal string holding the UUID the desc
            // presets carry as `sampledData`.
            let start = r.p;
            let uuid = r.pascal()?;
            r.p = start + 37;
            r.p += if subversion == 1 { 10 } else { 264 };
            let (top, left, bottom, right) = (r.i32()? as i64, r.i32()? as i64, r.i32()? as i64, r.i32()? as i64);
            let (w, h) = (right - left, bottom - top);
            if let Some(data) = read_sampled(&mut r, w, h, &mut out.warnings)? {
                out.tips.push(AbrTip { name: None, width: w as usize, height: h as usize, data, uuid: Some(uuid), desc: None });
            }
            Some(())
        })();
        if ok.is_none() {
            out.warnings.push(format!("skipped malformed sampled brush #{}", out.tips.len() + 1));
        }
        r.p = next;
    }
}

/// zlib, or raw deflate as a fallback (`abr.ts:355-365`).
fn inflate(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut v = Vec::new();
    if flate2::read::ZlibDecoder::new(bytes).read_to_end(&mut v).is_ok() {
        return Some(v);
    }
    v.clear();
    flate2::read::DeflateDecoder::new(bytes).read_to_end(&mut v).ok().map(|_| v)
}

/// `patt` block: Photoshop patterns (`abr.ts:375-450`).
fn parse_patt(mut r: Reader, end: usize, out: &mut AbrFile) {
    while r.p + 4 <= end {
        let Some(len) = r.i32() else { break };
        if len <= 0 || r.p + len as usize > end + 4 {
            break;
        }
        let next = padded_next(r.p, len);
        let n = out.patterns.len();
        let ok = (|| -> Option<()> {
            let version = r.i32()?;
            let mode = r.i32()?;
            let (h, w) = (r.i16()? as i64, r.i16()? as i64);
            let name = Some(r.unicode()?).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("Pattern {}", n + 1));
            r.pascal()?;
            if version != 1 || w <= 0 || h <= 0 || w * h > 4096 * 4096 {
                out.warnings.push(format!("skipped pattern \"{name}\" (unsupported header)"));
                return Some(());
            }
            if mode == 2 {
                r.p += 768;
            }
            let vma = r.i32()?;
            r.i32()?;
            r.p += 16;
            let channels = r.i32()?;
            if vma != 3 {
                out.warnings.push(format!("skipped pattern \"{name}\" (VMA v{vma})"));
                return Some(());
            }
            let (w, h) = (w as usize, h as usize);
            let mut planes: Vec<Vec<u8>> = Vec::new();
            let mut c = 0i64;
            while c < channels as i64 + 2 && r.p + 8 <= next {
                c += 1;
                let written = r.i32()?;
                let ch_len = r.i32()?;
                if written == 0 || ch_len <= 0 {
                    continue;
                }
                let ch_end = r.p + ch_len as usize;
                let pixel_depth = r.i32()?;
                r.p += 16;
                let depth = r.i16()?;
                let compression = r.u8()?;
                let raw = r.b.get(r.p.min(ch_end).min(r.b.len())..ch_end.min(r.b.len())).unwrap_or(&[]);
                r.p = ch_end;
                if !matches!(pixel_depth, 8 | 16) || !matches!(depth, 8 | 16) {
                    continue;
                }
                let mut plane = match compression {
                    0 => raw.to_vec(),
                    1 => inflate(raw)?,
                    _ => continue,
                };
                let bpp = (depth >> 3) as usize;
                if plane.len() < w * h * bpp {
                    continue;
                }
                if bpp == 2 {
                    plane = (0..w * h).map(|i| plane[i * 2]).collect();
                } else {
                    plane.truncate(w * h);
                }
                planes.push(plane);
                if planes.len() == 3 {
                    break;
                }
            }
            if planes.is_empty() {
                out.warnings.push(format!("skipped pattern \"{name}\" (no readable channel)"));
                return Some(());
            }
            let data = if planes.len() >= 3 {
                (0..w * h)
                    .map(|i| (0.299 * planes[0][i] as f64 + 0.587 * planes[1][i] as f64 + 0.114 * planes[2][i] as f64).round() as u8)
                    .collect()
            } else {
                planes.swap_remove(0)
            };
            out.patterns.push(AbrPattern { name, width: w, height: h, data });
            Some(())
        })();
        if ok.is_none() {
            out.warnings.push(format!("skipped malformed pattern #{}", out.patterns.len() + 1));
        }
        r.p = next;
    }
}

// ── Action Descriptors (`abr.ts:245-320`) ──────────────────────────────────

impl Reader<'_> {
    /// u32 length; 0 → a 4-char code, else that many ASCII bytes.
    fn key_id(&mut self) -> Option<String> {
        let n = self.i32()?;
        if n == 0 {
            return self.fourcc();
        }
        self.take(n.max(0) as usize).map(|s| s.iter().map(|&c| c as char).collect())
    }

    fn value(&mut self, kind: &str, depth: usize) -> Option<Value> {
        if depth > 64 {
            return None;
        }
        Some(match kind {
            "Objc" | "GlbO" => self.descriptor(depth + 1)?,
            "VlLs" => {
                let n = self.i32()?.max(0);
                let mut a = Vec::new();
                for _ in 0..n {
                    let t = self.fourcc()?;
                    a.push(self.value(&t, depth + 1)?);
                }
                Value::Array(a)
            }
            "doub" => num(self.f64()?),
            "UntF" => {
                let unit = self.fourcc()?;
                json!({ "unit": unit, "value": num(self.f64()?) })
            }
            "TEXT" => Value::String(self.unicode()?),
            "enum" => {
                let t = self.key_id()?;
                json!({ "enumType": t, "value": self.key_id()? })
            }
            "long" => json!(self.i32()?),
            "comp" => {
                let s = self.take(8)?;
                json!(i64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
            }
            "bool" => Value::Bool(self.u8()? != 0),
            "type" | "GlbC" => {
                self.unicode()?;
                Value::String(self.key_id()?)
            }
            "alis" | "tdta" => {
                let n = self.i32()?.max(0) as usize;
                self.take(n)?;
                Value::Null
            }
            _ => return None,
        })
    }

    /// Name, class id and keyed items; keys trimmed (`'Nm  '` → `Nm`).
    fn descriptor(&mut self, depth: usize) -> Option<Value> {
        self.unicode()?;
        self.key_id()?;
        let n = self.i32()?.max(0);
        let mut obj = Map::new();
        for _ in 0..n {
            let key = self.key_id()?.trim().to_string();
            let t = self.fourcc()?;
            obj.insert(key, self.value(&t, depth)?);
        }
        Some(Value::Object(obj))
    }
}

fn num(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

/// Flat `Nm  TEXT` name scan, the fallback when the descriptor walk fails.
fn scan_desc_names(b: &[u8], start: usize, end: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut i = start;
    while i + 8 <= end {
        if &b[i..i + 8] == b"Nm  TEXT" {
            let mut r = Reader { b, p: i + 8 };
            if let Some(s) = r.unicode().filter(|s| s.encode_utf16().count() <= 128) {
                names.push(s);
            }
            i = r.p.max(i + 1);
            continue;
        }
        i += 1;
    }
    names
}

/// `String(v ?? '')` for a `sampledData` value.
fn js_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Attach each preset descriptor (and name) to its tip: by UUID, then by
/// position when the counts match (`abr.ts:501-539`). False when the walk fails.
fn link_descriptors(b: &[u8], start: usize, out: &mut AbrFile) -> bool {
    let mut r = Reader { b, p: start };
    if b.get(start..start + 4) == Some(&16i32.to_be_bytes()[..]) {
        r.p += 4;
    }
    let Some(root) = r.descriptor(0) else { return false };
    let Some(Value::Array(list)) = root.get("Brsh") else { return false };
    let mut sampled: Vec<&Value> = Vec::new();
    let mut matches = 0;
    for preset in list.iter().filter(|v| v.is_object()) {
        let Some(tip) = preset.get("Brsh").filter(|t| t.is_object()) else { continue };
        let Some(sd) = tip.get("sampledData") else { continue };
        sampled.push(preset);
        let sd = js_string(sd).trim().to_string();
        if sd.is_empty() {
            continue;
        }
        // Later tips with the same UUID win, like a JS Map built from the list.
        if let Some(t) = out.tips.iter_mut().rev().find(|t| t.uuid.as_deref() == Some(sd.as_str())) {
            t.desc = Some(preset.clone());
            if let Some(Value::String(nm)) = preset.get("Nm") {
                t.name = Some(nm.clone());
            }
            matches += 1;
        }
    }
    if matches < out.tips.len() && sampled.len() == out.tips.len() {
        for (t, preset) in out.tips.iter_mut().zip(&sampled) {
            if t.desc.is_some() {
                continue;
            }
            t.desc = Some((*preset).clone());
            if let Some(Value::String(nm)) = preset.get("Nm") {
                t.name = Some(nm.clone());
            }
        }
    }
    out.tips.iter().any(|t| t.desc.is_some()) || matches > 0
}

fn parse_v6(mut r: Reader, subversion: i16) -> AbrFile {
    let mut out = AbrFile::default();
    let mut desc = None;
    let n = r.b.len();
    while r.p + 12 <= n {
        let Some(sig) = r.take(4) else { break };
        if sig != b"8BIM" {
            // Lost alignment: scan forward for the next block signature.
            let Some(idx) = r.b[r.p..].iter().position(|&c| c == b'8').map(|i| i + r.p) else { break };
            r.p = idx;
            if r.b.get(idx..idx + 4) != Some(b"8BIM") {
                r.p += 1;
            }
            continue;
        }
        let (Some(key), Some(len)) = (r.take(4), r.i32()) else { break };
        if len < 0 {
            break;
        }
        let end = (r.p + len as usize).min(n);
        match key {
            b"samp" => parse_samp(r.clone(), end, subversion, &mut out),
            b"patt" => parse_patt(r.clone(), end, &mut out),
            b"desc" => desc = Some((r.p, end)),
            _ => {}
        }
        r.p = end;
    }
    if let Some((start, end)) = desc
        && !out.tips.is_empty()
        && !link_descriptors(r.b, start, &mut out)
    {
        let names = scan_desc_names(r.b, start, end);
        if names.len() == out.tips.len() {
            for (t, nm) in out.tips.iter_mut().zip(names) {
                t.name = Some(nm);
            }
        } else {
            out.warnings.push("brush names not recovered from desc block".into());
        }
    }
    out
}

/// Parse a Photoshop `.abr`. Errors only on a wholly unreadable file.
pub fn parse_abr(bytes: &[u8]) -> Result<AbrFile, FormatError> {
    if bytes.len() < 4 {
        return Err(FormatError::Corrupt("abr: file too small".into()));
    }
    let mut r = Reader { b: bytes, p: 0 };
    let version = r.i16().unwrap_or(0);
    if version == 1 || version == 2 {
        return Ok(parse_v12(&mut r, version));
    }
    let sub = r.i16().unwrap_or(0);
    if (6..=10).contains(&version) && (sub == 1 || sub == 2) {
        return Ok(parse_v6(r, sub));
    }
    Err(FormatError::Corrupt(format!("abr: unsupported version {version}.{sub}")))
}
