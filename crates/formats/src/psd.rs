//! PSD import/export (spec 15), following ag-psd's byte layout (big-endian).

use crate::{FormatError, premul_linear_to_srgb8};
use onepaint_color::srgb_to_linear;
use onepaint_doc::{BlendMode, Document, Group, Layer, Node};
use onepaint_raster::{Rect, Surface};
use std::io::Read;

fn bad(s: &str) -> FormatError {
    FormatError::Corrupt(format!("psd: {s}"))
}

// ── PackBits ──

/// PackBits-encode one row (ag-psd's encoder: runs of ≥3 → repeat packets).
pub fn packbits(row: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(row.len() + row.len() / 64 + 2);
    let n = row.len();
    let mut i = 0;
    while i < n {
        let mut run = 1;
        while i + run < n && run < 128 && row[i + run] == row[i] {
            run += 1;
        }
        if run >= 3 {
            out.push((1i16 - run as i16) as i8 as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        let start = i;
        let mut len = 0;
        while i < n && len < 128 {
            if i + 2 < n && row[i] == row[i + 1] && row[i] == row[i + 2] {
                break;
            }
            i += 1;
            len += 1;
        }
        out.push((len - 1) as u8);
        out.extend_from_slice(&row[start..start + len]);
    }
    out
}

pub fn unpackbits(data: &[u8], len: usize) -> Result<Vec<u8>, FormatError> {
    let mut out = Vec::with_capacity(len);
    let mut i = 0;
    while out.len() < len && i < data.len() {
        let h = data[i] as i8;
        i += 1;
        if h >= 0 {
            let n = h as usize + 1;
            out.extend_from_slice(data.get(i..i + n).ok_or_else(|| bad("rle literal"))?);
            i += n;
        } else if h != -128 {
            let n = (1 - h as i16) as usize;
            let b = *data.get(i).ok_or_else(|| bad("rle run"))?;
            out.extend(std::iter::repeat_n(b, n));
            i += 1;
        }
    }
    out.resize(len, 0);
    Ok(out)
}

// ── Blend keys (psd-blend.ts) ──

const KEYS: [(BlendMode, &[u8; 4]); 19] = [
    (BlendMode::Normal, b"norm"),
    (BlendMode::Multiply, b"mul "),
    (BlendMode::Screen, b"scrn"),
    (BlendMode::Overlay, b"over"),
    (BlendMode::Add, b"lddg"),
    (BlendMode::Darken, b"dark"),
    (BlendMode::Lighten, b"lite"),
    (BlendMode::ColorDodge, b"div "),
    (BlendMode::ColorBurn, b"idiv"),
    (BlendMode::HardLight, b"hLit"),
    (BlendMode::SoftLight, b"sLit"),
    (BlendMode::Difference, b"diff"),
    (BlendMode::Exclusion, b"smud"),
    (BlendMode::LinearBurn, b"lbrn"),
    (BlendMode::Divide, b"fdiv"),
    (BlendMode::Hue, b"hue "),
    (BlendMode::Saturation, b"sat "),
    (BlendMode::Color, b"colr"),
    (BlendMode::Luminosity, b"lum "),
];

fn key_of(m: BlendMode) -> &'static [u8; 4] {
    KEYS.iter().find(|k| k.0 == m).map_or(b"norm", |k| k.1)
}

fn mode_of(k: &[u8]) -> BlendMode {
    KEYS.iter().find(|e| &e.1[..] == k).map_or(BlendMode::Normal, |e| e.0)
}

// ── Writer ──

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
    /// Length-prefixed section padded to `round` (ag-psd `writeSection`): the
    /// stored length includes the padding only when `total` is set.
    fn section(&mut self, round: usize, total: bool, f: impl FnOnce(&mut W)) {
        let at = self.0.len();
        self.u32(0);
        let start = self.0.len();
        f(self);
        let content = self.0.len() - start;
        while !(self.0.len() - start).is_multiple_of(round) {
            self.u8(0);
        }
        let stored = if total { self.0.len() - start } else { content };
        self.0[at..at + 4].copy_from_slice(&(stored as u32).to_be_bytes());
    }
}

struct Record {
    rect: Rect,
    channels: Vec<(i16, Vec<u8>)>,
    key: [u8; 4],
    opacity: u8,
    clipping: u8,
    flags: u8,
    name: String,
    mask: Option<(Rect, u8, u8)>,
    extra: Vec<([u8; 4], Vec<u8>)>,
}

fn rle_channel(rows: &[Vec<u8>]) -> Vec<u8> {
    let enc: Vec<Vec<u8>> = rows.iter().map(|r| packbits(r)).collect();
    let mut w = W(Vec::new());
    w.u16(1);
    for e in &enc {
        w.u16(e.len() as u16);
    }
    for e in enc {
        w.bytes(&e);
    }
    w.0
}

fn layer_record(l: &Layer, doc: Rect) -> Record {
    let rect = l.pixels.content_bounds().and_then(|b| b.intersect(&doc)).filter(|_| l.adjustment.is_none());
    let mut channels = Vec::new();
    let rect = match rect {
        Some(r) => {
            let px = l.pixels.read_rect(r);
            let (w, h) = (r.w as usize, r.h as usize);
            let srgb: Vec<[u8; 4]> = px.chunks_exact(4).map(|p| premul_linear_to_srgb8([p[0], p[1], p[2], p[3]])).collect();
            // Channel order: alpha (-1), then R, G, B.
            for (id, ch) in [(-1i16, 3usize), (0, 0), (1, 1), (2, 2)] {
                let rows: Vec<Vec<u8>> = (0..h).map(|y| srgb[y * w..(y + 1) * w].iter().map(|c| c[ch]).collect()).collect();
                channels.push((id, rle_channel(&rows)));
            }
            r
        }
        None => {
            for id in [-1i16, 0, 1, 2] {
                channels.push((id, vec![0, 0]));
            }
            Rect::default()
        }
    };
    let mut mask = None;
    if let Some(m) = &l.mask {
        let rows: Vec<Vec<u8>> =
            (0..doc.h).map(|y| (0..doc.w).map(|x| (m.get(x, y)[0].clamp(0.0, 1.0) * 255.0).round() as u8).collect()).collect();
        channels.push((-2, rle_channel(&rows)));
        mask = Some((doc, 255, if l.mask_enabled { 0 } else { 2 }));
    }
    let mut extra = Vec::new();
    if let Some(a) = &l.adjustment {
        let p = &a.params;
        match a.kind.as_str() {
            "invert" => extra.push((*b"nvrt", Vec::new())),
            "posterize" => {
                let v = p.levels.unwrap_or(4.0).clamp(2.0, 255.0).round() as u16;
                extra.push((*b"post", [v.to_be_bytes().to_vec(), vec![0, 0]].concat()));
            }
            "threshold" => {
                let v = (p.threshold.unwrap_or(0.5).clamp(0.0, 1.0) * 255.0).round().clamp(1.0, 255.0) as u16;
                extra.push((*b"thrs", [v.to_be_bytes().to_vec(), vec![0, 0]].concat()));
            }
            "brightness-contrast" => {
                let b = (p.brightness.unwrap_or(0.0).clamp(-1.0, 1.0) * 150.0).round() as i16;
                let c = p.contrast.unwrap_or(0.0);
                let c = if c >= 0.0 { (c * 100.0).round() } else { (c * 50.0).round() } as i16;
                let mut w = W(Vec::new());
                w.i16(b);
                w.i16(c);
                w.i16(127);
                w.u8(0);
                w.u8(0);
                extra.push((*b"brit", w.0));
            }
            _ => {}
        }
    }
    let hidden = if l.visible { 0 } else { 2 };
    Record {
        rect,
        channels,
        key: *key_of(l.blend_mode),
        opacity: (l.opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
        clipping: l.clipped as u8,
        flags: 0x08 | hidden | if l.adjustment.is_some() { 0x10 } else { 0 },
        name: l.name.clone(),
        mask,
        extra,
    }
}

fn empty_record(name: &str, key: [u8; 4], opacity: u8, flags: u8, lsct: Vec<u8>) -> Record {
    Record {
        rect: Rect::default(),
        channels: [-1i16, 0, 1, 2].into_iter().map(|id| (id, vec![0, 0])).collect(),
        key,
        opacity,
        clipping: 0,
        flags,
        name: name.to_string(),
        mask: None,
        extra: vec![(*b"lsct", lsct)],
    }
}

fn flatten(nodes: &[Node], doc: Rect, out: &mut Vec<Record>) {
    for n in nodes {
        match n {
            Node::Layer(l) => out.push(layer_record(l, doc)),
            Node::Group(g) => {
                out.push(empty_record("</Layer group>", *b"norm", 255, 0x08 | 0x10, 3u32.to_be_bytes().to_vec()));
                flatten(&g.children, doc, out);
                let mut lsct = 1u32.to_be_bytes().to_vec();
                lsct.extend_from_slice(b"8BIM");
                lsct.extend_from_slice(key_of(g.blend_mode));
                lsct.extend_from_slice(&0u32.to_be_bytes());
                let flags = 0x08 | 0x10 | if g.visible { 0 } else { 2 };
                out.push(empty_record(&g.name, *key_of(g.blend_mode), (g.opacity.clamp(0.0, 1.0) * 255.0).round() as u8, flags, lsct));
            }
        }
    }
}

fn pascal_padded(w: &mut W, name: &str, round: usize) {
    let bytes: Vec<u8> = name.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' }).take(255).collect();
    w.u8(bytes.len() as u8);
    w.bytes(&bytes);
    let mut n = 1 + bytes.len();
    while !n.is_multiple_of(round) {
        w.u8(0);
        n += 1;
    }
}

/// Write `doc` as an 8-bit RGB PSD; `composite` is the flattened canvas
/// (premultiplied linear, row-major RGBA).
pub fn write_psd(doc: &Document, composite: &[f32]) -> Result<Vec<u8>, FormatError> {
    if doc.width > 30000 || doc.height > 30000 {
        return Err(bad("PSD is limited to 30000 px; PSB export is not supported yet"));
    }
    let bounds = doc.bounds();
    let mut records = Vec::new();
    flatten(&doc.root, bounds, &mut records);
    let (w, h) = (doc.width as usize, doc.height as usize);
    let comp: Vec<[u8; 4]> = composite.chunks_exact(4).map(|p| premul_linear_to_srgb8([p[0], p[1], p[2], p[3]])).collect();
    let has_alpha = comp.iter().any(|c| c[3] < 255);
    let mut o = W(Vec::new());
    o.bytes(b"8BPS");
    o.u16(1);
    o.bytes(&[0; 6]);
    o.u16(if has_alpha { 4 } else { 3 });
    o.u32(doc.height);
    o.u32(doc.width);
    o.u16(8);
    o.u16(3);
    o.u32(0);
    // Image resources: 1005 resolution info.
    o.section(1, false, |r| {
        r.bytes(b"8BIM");
        r.u16(1005);
        r.u16(0);
        r.u32(16);
        let fixed = (doc.dpi * 65536.0).round() as u32;
        r.u32(fixed);
        r.u16(1);
        r.u16(1);
        r.u32(fixed);
        r.u16(1);
        r.u16(1);
    });
    o.section(2, false, |lm| {
        lm.section(4, true, |li| {
            li.i16(-(records.len() as i16));
            for rec in &records {
                li.i32(rec.rect.y);
                li.i32(rec.rect.x);
                li.i32(rec.rect.bottom());
                li.i32(rec.rect.right());
                li.u16(rec.channels.len() as u16);
                for (id, data) in &rec.channels {
                    li.i16(*id);
                    li.u32(data.len() as u32);
                }
                li.bytes(b"8BIM");
                li.bytes(&rec.key);
                li.u8(rec.opacity);
                li.u8(rec.clipping);
                li.u8(rec.flags);
                li.u8(0);
                li.section(1, false, |ex| {
                    ex.section(1, false, |m| {
                        if let Some((r, dc, fl)) = rec.mask {
                            m.i32(r.y);
                            m.i32(r.x);
                            m.i32(r.bottom());
                            m.i32(r.right());
                            m.u8(dc);
                            m.u8(fl);
                            m.u16(0);
                        }
                    });
                    ex.u32(0);
                    pascal_padded(ex, &rec.name, 4);
                    let mut blocks = rec.extra.clone();
                    let utf16: Vec<u16> = rec.name.encode_utf16().collect();
                    let mut luni = (utf16.len() as u32).to_be_bytes().to_vec();
                    for c in utf16 {
                        luni.extend_from_slice(&c.to_be_bytes());
                    }
                    blocks.insert(0, (*b"luni", luni));
                    for (key, data) in blocks {
                        ex.bytes(b"8BIM");
                        ex.bytes(&key);
                        let round = if &key == b"luni" { 4 } else { 2 };
                        ex.section(round, true, |d| d.bytes(&data));
                    }
                });
            }
            for rec in &records {
                for (_, data) in &rec.channels {
                    li.bytes(data);
                }
            }
        });
        lm.u32(0);
    });
    // Merged composite, RLE, partial alpha matted against white.
    o.u16(1);
    let nch = if has_alpha { 4 } else { 3 };
    let mut planes: Vec<Vec<Vec<u8>>> = vec![vec![vec![0u8; w]; h]; nch];
    for y in 0..h {
        for x in 0..w {
            let c = comp[y * w + x];
            let a = c[3] as f32 / 255.0;
            for ch in 0..3 {
                let v = if c[3] == 0 || c[3] == 255 { c[ch] as f32 } else { c[ch] as f32 * a + 255.0 * (1.0 - a) };
                planes[ch][y][x] = v.round() as u8;
            }
            if nch == 4 {
                planes[3][y][x] = c[3];
            }
        }
    }
    let enc: Vec<Vec<Vec<u8>>> = planes.iter().map(|p| p.iter().map(|r| packbits(r)).collect()).collect();
    for p in &enc {
        for r in p {
            o.u16(r.len() as u16);
        }
    }
    for p in enc {
        for r in p {
            o.bytes(&r);
        }
    }
    Ok(o.0)
}

// ── Reader ──

struct Cur<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> Cur<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let s = self.b.get(self.p..self.p.checked_add(n).ok_or_else(|| bad("overflow"))?).ok_or_else(|| bad("truncated"))?;
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, FormatError> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> Result<i16, FormatError> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, FormatError> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> Result<i32, FormatError> {
        Ok(self.u32()? as i32)
    }
    fn u64(&mut self) -> Result<u64, FormatError> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(u64::from_be_bytes(a))
    }
    fn len(&mut self, psb: bool) -> Result<usize, FormatError> {
        Ok(if psb { self.u64()? as usize } else { self.u32()? as usize })
    }
}

struct RRec {
    rect: Rect,
    channels: Vec<(i16, usize)>,
    key: [u8; 4],
    opacity: u8,
    clipping: u8,
    flags: u8,
    name: String,
    mask: Option<(Rect, u8, u8)>,
    lsct: Option<(u32, Option<[u8; 4]>)>,
    adjustment: Option<onepaint_doc::Adjustment>,
}

fn decode_channel(c: &mut Cur, len: usize, w: usize, h: usize, depth: u16, psb: bool) -> Result<Vec<u8>, FormatError> {
    let start = c.p;
    let bpp = (depth / 8).max(1) as usize;
    let row = w * bpp;
    let comp = c.u16()?;
    let out = match comp {
        0 => c.take(row * h)?.to_vec(),
        1 => {
            let counts: Vec<usize> =
                (0..h).map(|_| if psb { c.u32().map(|v| v as usize) } else { c.u16().map(|v| v as usize) }).collect::<Result<_, _>>()?;
            let mut out = Vec::with_capacity(row * h);
            for n in counts {
                out.extend(unpackbits(c.take(n)?, row)?);
            }
            out
        }
        2 | 3 => {
            let body = c.take(len.saturating_sub(2))?;
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(body).read_to_end(&mut out)?;
            out.resize(row * h, 0);
            if comp == 3 {
                for y in 0..h {
                    let r = &mut out[y * row..(y + 1) * row];
                    if bpp == 1 {
                        for x in 1..row {
                            r[x] = r[x].wrapping_add(r[x - 1]);
                        }
                    } else {
                        for x in 1..w {
                            let prev = u16::from_be_bytes([r[(x - 1) * 2], r[(x - 1) * 2 + 1]]);
                            let cur = u16::from_be_bytes([r[x * 2], r[x * 2 + 1]]).wrapping_add(prev);
                            r[x * 2..x * 2 + 2].copy_from_slice(&cur.to_be_bytes());
                        }
                    }
                }
            }
            out
        }
        _ => return Err(bad("unknown compression")),
    };
    c.p = start + len;
    // Down-convert 16-bit to 8-bit values for placement (rounded, not truncated).
    if bpp == 2 {
        return Ok(out.chunks_exact(2).map(|v| ((u16::from_be_bytes([v[0], v[1]]) as u32 * 255 + 32767) / 65535) as u8).collect());
    }
    Ok(out)
}

fn surface_from(planes: &[(i16, Vec<u8>)], rect: Rect, gray: bool, doc: Rect) -> Surface {
    let mut s = Surface::new();
    let Some(ix) = rect.intersect(&doc) else { return s };
    let get = |id: i16| planes.iter().find(|p| p.0 == id).map(|p| &p.1);
    let (r, g, b, a) = (get(0), get(if gray { 0 } else { 1 }), get(if gray { 0 } else { 2 }), get(-1));
    let mut data = Vec::with_capacity((ix.w * ix.h * 4) as usize);
    for y in ix.y..ix.bottom() {
        for x in ix.x..ix.right() {
            let i = ((y - rect.y) * rect.w + (x - rect.x)) as usize;
            let v = |p: Option<&Vec<u8>>, d: u8| p.and_then(|p| p.get(i).copied()).unwrap_or(d);
            let al = v(a, 255) as f32 / 255.0;
            let l = |c: u8| srgb_to_linear(c as f32 / 255.0) * al;
            data.extend_from_slice(&[l(v(r, 0)), l(v(g, 0)), l(v(b, 0)), al]);
        }
    }
    s.write_rect(ix, &data);
    s.prune_empty();
    s
}

/// Read a PSD/PSB into a document.
pub fn read_psd(bytes: &[u8]) -> Result<Document, FormatError> {
    read_psd_with(bytes, &crate::Progress::default())
}

/// [`read_psd`], counting layer records on `progress` and stopping with
/// [`FormatError::Cancelled`] once it is cancelled (spec 41).
pub fn read_psd_with(bytes: &[u8], progress: &crate::Progress) -> Result<Document, FormatError> {
    let mut c = Cur { b: bytes, p: 0 };
    if c.take(4)? != b"8BPS" {
        return Err(FormatError::BadMagic);
    }
    let version = c.u16()?;
    let psb = version == 2;
    if version != 1 && !psb {
        return Err(bad("unknown version"));
    }
    c.take(6)?;
    let nch = c.u16()?;
    let (h, w) = (c.u32()?, c.u32()?);
    let depth = c.u16()?;
    let mode = c.u16()?;
    if depth != 8 && depth != 16 {
        return Err(bad("only 8- and 16-bit PSDs are supported"));
    }
    if mode != 3 && mode != 1 {
        return Err(bad("only RGB and Grayscale PSDs are supported"));
    }
    let gray = mode == 1;
    let cm = c.u32()? as usize;
    c.take(cm)?;
    let mut doc = Document::new(w, h, None);
    doc.root.clear();
    doc.name = "Imported PSD".into();
    let ir = c.u32()? as usize;
    let ir_end = c.p + ir;
    while c.p + 12 <= ir_end {
        if c.take(4)? != b"8BIM" {
            break;
        }
        let id = c.u16()?;
        let nl = c.u8()? as usize;
        c.take(nl + if (nl + 1) % 2 == 1 { 1 } else { 0 })?;
        let sz = c.u32()? as usize;
        let start = c.p;
        if id == 1005 && sz >= 16 {
            doc.dpi = (c.u32()? as f64 / 65536.0).round();
        }
        c.p = start + sz + sz % 2;
    }
    c.p = ir_end;
    let lm_len = c.len(psb)?;
    let lm_end = c.p + lm_len;
    let bounds = doc.bounds();
    if lm_len > 0 {
        let li_len = c.len(psb)?;
        let li_end = c.p + li_len;
        if li_len > 0 {
            let count = c.i16()?.unsigned_abs() as usize;
            let mut recs = Vec::with_capacity(count);
            for _ in 0..count {
                let (top, left, bottom, right) = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
                let n = c.u16()? as usize;
                let mut channels = Vec::new();
                for _ in 0..n {
                    let id = c.i16()?;
                    let l = c.len(psb)?;
                    channels.push((id, l));
                }
                c.take(4)?;
                let mut key = [0u8; 4];
                key.copy_from_slice(c.take(4)?);
                let (opacity, clipping, flags) = (c.u8()?, c.u8()?, c.u8()?);
                c.u8()?;
                let ex_len = c.u32()? as usize;
                let ex_end = c.p + ex_len;
                let mlen = c.u32()? as usize;
                let mstart = c.p;
                let mut mask = None;
                if mlen >= 20 {
                    let (t, l, b, r) = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
                    let (dc, fl) = (c.u8()?, c.u8()?);
                    mask = Some((Rect::new(l, t, r - l, b - t), dc, fl));
                }
                c.p = mstart + mlen;
                let br = c.u32()? as usize;
                c.take(br)?;
                let nl = c.u8()? as usize;
                let raw = c.take(nl)?;
                let mut name = String::from_utf8_lossy(raw).into_owned();
                let mut pad = 1 + nl;
                while !pad.is_multiple_of(4) {
                    c.u8()?;
                    pad += 1;
                }
                let mut lsct = None;
                let mut adjustment = None;
                while c.p + 12 <= ex_end {
                    let sig = c.take(4)?;
                    if sig != b"8BIM" && sig != b"8B64" {
                        break;
                    }
                    let mut k = [0u8; 4];
                    k.copy_from_slice(c.take(4)?);
                    let long = psb
                        && matches!(
                            &k,
                            b"LMsk"
                                | b"Lr16"
                                | b"Lr32"
                                | b"Layr"
                                | b"Mt16"
                                | b"Mt32"
                                | b"Mtrn"
                                | b"Alph"
                                | b"FMsk"
                                | b"lnk2"
                                | b"FEid"
                                | b"FXid"
                                | b"PxSD"
                        );
                    let l = if long { c.u64()? as usize } else { c.u32()? as usize };
                    let start = c.p;
                    let data = c.take(l.min(ex_end.saturating_sub(start)))?;
                    let mut d = Cur { b: data, p: 0 };
                    let adj = |kind: &str, params: onepaint_filters::Params| Some(onepaint_doc::Adjustment { kind: kind.into(), params });
                    match &k {
                        b"luni" => {
                            let n = d.u32()? as usize;
                            let units: Vec<u16> = (0..n).map(|_| d.u16()).collect::<Result<_, _>>()?;
                            name = String::from_utf16_lossy(&units).trim_end_matches('\0').to_string();
                        }
                        b"lsct" => {
                            let t = d.u32()?;
                            let blend = if data.len() >= 12 && &data[4..8] == b"8BIM" {
                                let mut b = [0u8; 4];
                                b.copy_from_slice(&data[8..12]);
                                Some(b)
                            } else {
                                None
                            };
                            lsct = Some((t, blend));
                        }
                        b"nvrt" => adjustment = adj("invert", Default::default()),
                        b"post" => {
                            adjustment = adj(
                                "posterize",
                                onepaint_filters::Params { levels: Some((d.u16()? as f32).clamp(2.0, 32.0)), ..Default::default() },
                            )
                        }
                        b"thrs" => {
                            adjustment = adj(
                                "threshold",
                                onepaint_filters::Params { threshold: Some(d.u16()? as f32 / 255.0), ..Default::default() },
                            )
                        }
                        b"brit" => {
                            let (b, ct) = (d.i16()? as f32, d.i16()? as f32);
                            adjustment = adj(
                                "brightness-contrast",
                                onepaint_filters::Params {
                                    brightness: Some(b / 150.0),
                                    contrast: Some(if ct >= 0.0 { ct / 100.0 } else { ct / 50.0 }),
                                    ..Default::default()
                                },
                            );
                        }
                        _ => {}
                    }
                    c.p = start + l;
                }
                c.p = ex_end;
                recs.push(RRec {
                    rect: Rect::new(left, top, right - left, bottom - top),
                    channels,
                    key,
                    opacity,
                    clipping,
                    flags,
                    name,
                    mask,
                    lsct,
                    adjustment,
                });
            }
            // Channel image data, record by record.
            let mut nodes_flat: Vec<(RRec, Option<Surface>, Option<Surface>)> = Vec::new();
            progress.set_total(recs.len() as u64);
            for rec in recs {
                progress.check()?;
                let mut planes = Vec::new();
                let mut mask_px = None;
                for &(id, l) in &rec.channels {
                    let (cw, ch) = if id == -2 {
                        rec.mask.map_or((0, 0), |m| (m.0.w.max(0) as usize, m.0.h.max(0) as usize))
                    } else {
                        (rec.rect.w.max(0) as usize, rec.rect.h.max(0) as usize)
                    };
                    let data = decode_channel(&mut c, l, cw, ch, depth, psb)?;
                    if id == -2 {
                        mask_px = Some(data);
                    } else {
                        planes.push((id, data));
                    }
                }
                let pixels = (rec.rect.w > 0 && rec.rect.h > 0).then(|| surface_from(&planes, rec.rect, gray, bounds));
                let mask = rec.mask.map(|(mr, dc, _)| {
                    let mut m = Document::filled(w, h, if dc >= 128 { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] });
                    if let Some(px) = &mask_px
                        && let Some(ix) = mr.intersect(&bounds)
                    {
                        for y in ix.y..ix.bottom() {
                            for x in ix.x..ix.right() {
                                let v = px.get(((y - mr.y) * mr.w + (x - mr.x)) as usize).copied().unwrap_or(dc) as f32 / 255.0;
                                m.set(x, y, [v, v, v, 1.0]);
                            }
                        }
                    }
                    m
                });
                nodes_flat.push((rec, pixels, mask));
                progress.advance(1);
            }
            // Rebuild the tree bottom-up: dividers open a child list, folders close it.
            let mut stack: Vec<Vec<Node>> = vec![Vec::new()];
            for (rec, pixels, mask) in nodes_flat {
                match rec.lsct.map(|l| l.0) {
                    Some(3) => stack.push(Vec::new()),
                    Some(1) | Some(2) => {
                        let children = if stack.len() > 1 { stack.pop().unwrap_or_default() } else { Vec::new() };
                        let mut g = Group::new(&rec.name);
                        g.children = children;
                        g.visible = rec.flags & 2 == 0;
                        g.opacity = rec.opacity as f32 / 255.0;
                        g.blend_mode = mode_of(&rec.lsct.and_then(|l| l.1).unwrap_or(rec.key));
                        if let Some(top) = stack.last_mut() {
                            top.push(Node::Group(g));
                        }
                    }
                    _ => {
                        let mut l = Layer::new(&rec.name);
                        l.visible = rec.flags & 2 == 0;
                        l.opacity = rec.opacity as f32 / 255.0;
                        l.blend_mode = mode_of(&rec.key);
                        l.clipped = rec.clipping != 0;
                        l.pixels = pixels.unwrap_or_default();
                        l.mask_enabled = rec.mask.is_none_or(|m| m.2 & 2 == 0);
                        l.mask = mask;
                        l.adjustment = rec.adjustment.map(Box::new);
                        if let Some(top) = stack.last_mut() {
                            top.push(Node::Layer(l));
                        }
                    }
                }
            }
            while stack.len() > 1 {
                let orphan = stack.pop().unwrap_or_default();
                if let Some(top) = stack.last_mut() {
                    top.extend(orphan);
                }
            }
            doc.root = stack.pop().unwrap_or_default();
        }
        c.p = li_end;
    }
    c.p = lm_end;
    if doc.root.is_empty() {
        // No layers: import the merged composite.
        let comp = c.u16()?;
        let row = w as usize * (depth as usize / 8);
        let mut planes = Vec::new();
        let counts: Vec<usize> = if comp == 1 {
            (0..h as usize * nch as usize)
                .map(|_| if psb { c.u32().map(|v| v as usize) } else { c.u16().map(|v| v as usize) })
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        for ch in 0..nch.min(4) as usize {
            let mut plane = Vec::with_capacity(row * h as usize);
            for y in 0..h as usize {
                if comp == 1 {
                    plane.extend(unpackbits(c.take(counts[ch * h as usize + y])?, row)?);
                } else {
                    plane.extend_from_slice(c.take(row)?);
                }
            }
            if depth == 16 {
                plane = plane.chunks_exact(2).map(|v| v[0]).collect();
            }
            planes.push((if ch == 3 || (gray && ch == 1) { -1 } else { ch as i16 }, plane));
        }
        let mut l = Layer::new("Background");
        l.pixels = surface_from(&planes, bounds, gray, bounds);
        doc.root.push(Node::Layer(l));
    }
    Ok(doc)
}
