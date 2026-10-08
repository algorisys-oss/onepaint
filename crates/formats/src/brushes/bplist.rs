//! Apple binary plist (`bplist00`) reader and writer, enough for Procreate's
//! `Brush.archive` (happypaint `storage/brush-import/bplist.ts`).

use crate::FormatError;
use std::collections::BTreeMap;

/// A decoded plist value. NSKeyedArchiver object references are `Uid`.
#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    /// Seconds since 2001-01-01.
    Date(f64),
    Data(Vec<u8>),
    String(String),
    Uid(u64),
    Array(Vec<Plist>),
    Dict(BTreeMap<String, Plist>),
}

impl Plist {
    pub fn as_f64(&self) -> Option<f64> {
        match *self {
            Plist::Int(v) => Some(v as f64),
            Plist::Real(v) if v.is_finite() => Some(v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Plist::String(s) => Some(s),
            _ => None,
        }
    }

    /// The JSON shape happypaint's parser produces (data → `{bytes: n}`,
    /// UIDs → `{__uid: n}`, dates → seconds); for tests and debugging.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::{Value, json};
        match self {
            Plist::Null => Value::Null,
            Plist::Bool(b) => json!(b),
            Plist::Int(v) => json!(v),
            Plist::Real(v) | Plist::Date(v) => json!(v),
            Plist::Data(d) => json!({ "bytes": d.len() }),
            Plist::String(s) => json!(s),
            Plist::Uid(u) => json!({ "__uid": u }),
            Plist::Array(a) => Value::Array(a.iter().map(Plist::to_json).collect()),
            Plist::Dict(d) => Value::Object(d.iter().map(|(k, v)| (k.clone(), v.to_json())).collect()),
        }
    }
}

fn bad(s: &str) -> FormatError {
    FormatError::Corrupt(format!("bplist: {s}"))
}

struct Parser<'a> {
    b: &'a [u8],
    offset_size: usize,
    ref_size: usize,
    count: usize,
    table: usize,
    memo: Vec<Option<Plist>>,
    busy: Vec<bool>,
}

impl Parser<'_> {
    fn uint(&self, pos: usize, size: usize) -> Result<u64, FormatError> {
        let s = self.b.get(pos..pos + size).ok_or_else(|| bad("truncated"))?;
        Ok(s.iter().fold(0u64, |v, &x| (v << 8) | x as u64))
    }

    fn decode(&mut self, r: usize, depth: usize) -> Result<Plist, FormatError> {
        if r >= self.count {
            return Err(bad("object ref out of range"));
        }
        if let Some(v) = &self.memo[r] {
            return Ok(v.clone());
        }
        // A reference cycle (legal in plists, impossible in owned values) reads as null.
        if self.busy[r] || depth > 512 {
            return Ok(Plist::Null);
        }
        self.busy[r] = true;
        let v = self.decode_at(r, depth);
        self.busy[r] = false;
        let v = v?;
        self.memo[r] = Some(v.clone());
        Ok(v)
    }

    fn decode_at(&mut self, r: usize, depth: usize) -> Result<Plist, FormatError> {
        let mut p = self.uint(self.table + r * self.offset_size, self.offset_size)? as usize;
        let marker = *self.b.get(p).ok_or_else(|| bad("truncated"))?;
        p += 1;
        let (kind, n) = (marker >> 4, (marker & 0x0f) as usize);
        let mut len = || -> Result<usize, FormatError> {
            if n != 0x0f {
                return Ok(n);
            }
            let m = *self.b.get(p).ok_or_else(|| bad("truncated"))?;
            if m >> 4 != 1 {
                return Err(bad("bad extended length"));
            }
            let size = 1usize << (m & 0x0f);
            let v = self.uint(p + 1, size)? as usize;
            p += 1 + size;
            Ok(v)
        };
        let bytes = |p: usize, n: usize| p.checked_add(n).and_then(|e| self.b.get(p..e)).ok_or_else(|| bad("truncated"));
        Ok(match kind {
            0x0 => match n {
                0x8 => Plist::Bool(false),
                0x9 => Plist::Bool(true),
                _ => Plist::Null,
            },
            0x1 => {
                // 8-byte ints are signed; a 16-byte int keeps its low 8 bytes.
                let size = 1usize << n;
                Plist::Int(self.uint(p + size.saturating_sub(8), size.min(8))? as i64)
            }
            0x2 => {
                let s = bytes(p, if n == 2 { 4 } else { 8 })?;
                Plist::Real(if n == 2 { f32::from_be_bytes([s[0], s[1], s[2], s[3]]) as f64 } else { f64_be(s) })
            }
            0x3 => Plist::Date(f64_be(bytes(p, 8)?)),
            0x4 => {
                let n = len()?;
                Plist::Data(bytes(p, n)?.to_vec())
            }
            0x5 => {
                let n = len()?;
                Plist::String(bytes(p, n)?.iter().map(|&c| c as char).collect())
            }
            0x6 => {
                let n = len()?;
                let units: Vec<u16> = bytes(p, n * 2)?.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                Plist::String(String::from_utf16_lossy(&units))
            }
            0x8 => Plist::Uid(self.uint(p, n + 1)?),
            0xa | 0xc => {
                let n = len()?;
                let mut out = Vec::with_capacity(n.min(4096));
                for i in 0..n {
                    let r = self.uint(p + i * self.ref_size, self.ref_size)? as usize;
                    out.push(self.decode(r, depth + 1)?);
                }
                Plist::Array(out)
            }
            0xd => {
                let n = len()?;
                let mut out = BTreeMap::new();
                for i in 0..n {
                    let k = self.uint(p + i * self.ref_size, self.ref_size)? as usize;
                    let v = self.uint(p + (n + i) * self.ref_size, self.ref_size)? as usize;
                    let (k, v) = (self.decode(k, depth + 1)?, self.decode(v, depth + 1)?);
                    if let Plist::String(k) = k {
                        out.insert(k, v);
                    }
                }
                Plist::Dict(out)
            }
            _ => return Err(bad(&format!("unsupported marker 0x{marker:02x}"))),
        })
    }
}

fn f64_be(s: &[u8]) -> f64 {
    f64::from_be_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]])
}

/// Parse `bplist00` bytes (errors on anything malformed).
pub fn parse_bplist(b: &[u8]) -> Result<Plist, FormatError> {
    if b.len() < 40 || !b.starts_with(b"bplist0") {
        return Err(bad("bad magic"));
    }
    let t = b.len() - 32;
    let be64 = |p: usize| u64::from_be_bytes([b[p], b[p + 1], b[p + 2], b[p + 3], b[p + 4], b[p + 5], b[p + 6], b[p + 7]]);
    let (offset_size, ref_size) = (b[t + 6] as usize, b[t + 7] as usize);
    let (count, top, table) = (be64(t + 8) as usize, be64(t + 16) as usize, be64(t + 24) as usize);
    if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) || count > b.len() {
        return Err(bad("bad trailer"));
    }
    let mut p = Parser { b, offset_size, ref_size, count, table, memo: vec![None; count], busy: vec![false; count] };
    p.decode(top, 0)
}

/// NSKeyedArchiver: the root object (`$objects[$top.root]`) with each value
/// dereferenced one level (`$null` → null), or `None` when not an archive.
pub fn archive_root(plist: &Plist) -> Option<BTreeMap<String, Plist>> {
    let Plist::Dict(d) = plist else { return None };
    let Some(Plist::Array(objects)) = d.get("$objects") else { return None };
    let Some(Plist::Dict(top)) = d.get("$top") else { return None };
    let deref = |v: &Plist| match v {
        Plist::Uid(u) => match objects.get(*u as usize) {
            Some(Plist::String(s)) if s == "$null" => Plist::Null,
            Some(o) => o.clone(),
            None => v.clone(),
        },
        _ => v.clone(),
    };
    let Plist::Dict(root) = deref(top.get("root")?) else { return None };
    Some(root.iter().map(|(k, v)| (k.clone(), deref(v))).collect())
}

/// Write a `bplist00` (one object per value, 4-byte offsets and refs).
pub fn write_bplist(top: &Plist) -> Vec<u8> {
    fn len(out: &mut Vec<u8>, kind: u8, n: usize) {
        if n < 15 {
            out.push((kind << 4) | n as u8);
        } else {
            out.push((kind << 4) | 0x0f);
            out.push(0x13);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    fn add(objs: &mut Vec<Vec<u8>>, v: &Plist) -> u32 {
        let idx = objs.len();
        objs.push(Vec::new());
        let mut o = Vec::new();
        match v {
            Plist::Null => o.push(0x00),
            Plist::Bool(b) => o.push(if *b { 0x09 } else { 0x08 }),
            Plist::Int(i) => {
                o.push(0x13);
                o.extend_from_slice(&i.to_be_bytes());
            }
            Plist::Real(r) => {
                o.push(0x23);
                o.extend_from_slice(&r.to_be_bytes());
            }
            Plist::Date(d) => {
                o.push(0x33);
                o.extend_from_slice(&d.to_be_bytes());
            }
            Plist::Data(d) => {
                len(&mut o, 4, d.len());
                o.extend_from_slice(d);
            }
            Plist::String(s) if s.is_ascii() => {
                len(&mut o, 5, s.len());
                o.extend_from_slice(s.as_bytes());
            }
            Plist::String(s) => {
                let units: Vec<u16> = s.encode_utf16().collect();
                len(&mut o, 6, units.len());
                units.iter().for_each(|u| o.extend_from_slice(&u.to_be_bytes()));
            }
            Plist::Uid(u) => {
                o.push(0x87);
                o.extend_from_slice(&u.to_be_bytes());
            }
            Plist::Array(a) => {
                let refs: Vec<u32> = a.iter().map(|x| add(objs, x)).collect();
                len(&mut o, 0xa, refs.len());
                refs.iter().for_each(|r| o.extend_from_slice(&r.to_be_bytes()));
            }
            Plist::Dict(d) => {
                let keys: Vec<u32> = d.keys().map(|k| add(objs, &Plist::String(k.clone()))).collect();
                let vals: Vec<u32> = d.values().map(|x| add(objs, x)).collect();
                len(&mut o, 0xd, keys.len());
                keys.iter().chain(&vals).for_each(|r| o.extend_from_slice(&r.to_be_bytes()));
            }
        }
        objs[idx] = o;
        idx as u32
    }
    let mut objs = Vec::new();
    add(&mut objs, top);
    let mut out = b"bplist00".to_vec();
    let mut offsets = Vec::with_capacity(objs.len());
    for o in &objs {
        offsets.push(out.len() as u32);
        out.extend_from_slice(o);
    }
    let table = out.len() as u64;
    offsets.iter().for_each(|o| out.extend_from_slice(&o.to_be_bytes()));
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 4, 4]);
    out.extend_from_slice(&(objs.len() as u64).to_be_bytes());
    out.extend_from_slice(&0u64.to_be_bytes());
    out.extend_from_slice(&table.to_be_bytes());
    out
}
