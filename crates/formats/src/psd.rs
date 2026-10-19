//! PSD import/export (spec 15), following ag-psd's byte layout (big-endian).

use crate::FormatError;
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

const KEYS: [(BlendMode, &[u8; 4]); 28] = [
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
    (BlendMode::VividLight, b"vLit"),
    (BlendMode::LinearLight, b"lLit"),
    (BlendMode::PinLight, b"pLit"),
    (BlendMode::HardMix, b"hMix"),
    (BlendMode::Subtract, b"fsub"),
    (BlendMode::DarkerColor, b"dkCl"),
    (BlendMode::LighterColor, b"lgCl"),
    (BlendMode::Dissolve, b"diss"),
    (BlendMode::PassThrough, b"pass"),
];

/// A layer's key; Pigment and a layer's Pass Through are Normal.
fn key_of(m: BlendMode) -> &'static [u8; 4] {
    KEYS.iter().find(|k| k.0 == m && m != BlendMode::PassThrough).map_or(b"norm", |k| k.1)
}

/// A group's `lsct` key, Pass Through included.
fn group_key_of(m: BlendMode) -> &'static [u8; 4] {
    if m == BlendMode::PassThrough { b"pass" } else { key_of(m) }
}

/// A layer record's key (Pass Through only means something on a group).
fn mode_of(k: &[u8]) -> BlendMode {
    KEYS.iter().find(|e| &e.1[..] == k).map_or(BlendMode::Normal, |e| e.0).effective()
}

/// A group's mode from its `lsct` key.
fn group_mode_of(k: &[u8]) -> BlendMode {
    if k == b"pass" { BlendMode::PassThrough } else { mode_of(k) }
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
    /// The user mask's density and feather (written when not at default).
    mask_look: (f32, f32),
    /// Blend If, as Photoshop's blend ranges (empty: none).
    ranges: Vec<u8>,
    extra: Vec<([u8; 4], Vec<u8>)>,
    /// An untouched layer's original record and channel data, written as they are.
    raw: Option<(Vec<u8>, Vec<Vec<u8>>)>,
}

impl Record {
    fn raw(src: &crate::psd_source::PsdSource, r: &crate::psd_source::RawRecord) -> Self {
        let b = &src.bytes;
        Record {
            rect: Rect::default(),
            channels: Vec::new(),
            key: *b"norm",
            opacity: 255,
            clipping: 0,
            flags: 0,
            name: String::new(),
            mask: None,
            mask_look: (1.0, 0.0),
            ranges: Vec::new(),
            extra: Vec::new(),
            raw: Some((b[r.record.clone()].to_vec(), r.channels.iter().map(|c| b[c.clone()].to_vec()).collect())),
        }
    }
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

/// A fill layer's picture over the document (it has no pixels of its own).
fn fill_pixels(l: &Layer, frame: onepaint_filters::Frame) -> Option<Surface> {
    use rayon::prelude::*;
    let adj = l.adjustment.as_ref()?;
    let kind = onepaint_filters::Kind::parse(&adj.kind).filter(|k| k.is_fill())?;
    let p = onepaint_filters::prepare_in(kind, &adj.params, frame);
    let (w, h) = (frame.doc_w as usize, frame.doc_h as usize);
    let mut px = vec![0.0f32; w * h * 4];
    px.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            row[x * 4..x * 4 + 4].copy_from_slice(&p.color_px([0.0; 4], x as i32, y as i32));
        }
    });
    let mut s = Surface::new();
    s.write_rect(Rect::new(0, 0, w as i32, h as i32), &px);
    Some(s)
}

fn layer_record(l: &Layer, doc: Rect, frame: onepaint_filters::Frame) -> Record {
    let space = frame.space;
    // Fill layers export their picture; adjustments carry none (they're kept as recipes).
    let fill = fill_pixels(l, frame);
    let pixels = fill.as_ref().unwrap_or(&l.pixels);
    let rect = pixels.content_bounds().and_then(|b| b.intersect(&doc)).filter(|_| l.adjustment.is_none() || fill.is_some());
    let mut channels = Vec::new();
    let rect = match rect {
        Some(r) => {
            let px = pixels.read_rect(r);
            let (w, h) = (r.w as usize, r.h as usize);
            let srgb: Vec<[u8; 4]> = px.chunks_exact(4).map(|p| space.to_srgb8([p[0], p[1], p[2], p[3]])).collect();
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
            // Solid and gradient fills as live fill blocks, beside their picture.
            "solid-fill" | "gradient-fill" => extra.extend(crate::psd_fill::write(a, f64::from(doc.w), f64::from(doc.h))),
            _ => extra.extend(crate::psd_adjust::write(a)),
        }
    }
    // Knockout: one byte (1 shallow, 2 deep) padded to four, written only when set.
    if let Some(k) = l.knockout() {
        extra.push((*b"knko", vec![if k == onepaint_doc::Knockout::Deep { 2 } else { 1 }, 0, 0, 0]));
    }
    extra.extend(style_blocks(l.styles()));
    extra.push(lock_block(l));
    extra.extend(label_block(&l.extra));
    let ranges = l.blend_if().map_or_else(Vec::new, |b| b.0.iter().flat_map(|c| c.src.into_iter().chain(c.dst)).collect());
    if let Some(v) = l.vector_mask().filter(|v| !v.paths.is_empty()) {
        extra.push((*b"vmsk", write_vmsk(&v, doc.w as f64, doc.h as f64)));
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
        mask_look: (l.mask_density(), l.mask_feather()),
        ranges,
        extra,
        raw: None,
    }
}

/// The document's Global Light for resources 1037/1049 (spec 56): the
/// first linked effect's, or the default; `None` when nothing has styles.
fn global_light(nodes: &[Node]) -> Option<(f32, f32)> {
    fn styled(nodes: &[Node], out: &mut Vec<onepaint_doc::styles::LayerStyles>) {
        for n in nodes {
            match n {
                Node::Layer(l) => out.extend(l.styles().filter(|s| !s.effects.is_empty())),
                Node::Group(g) => {
                    out.extend(g.styles().filter(|s| !s.effects.is_empty()));
                    styled(&g.children, out);
                }
            }
        }
    }
    let mut all = Vec::new();
    styled(nodes, &mut all);
    if all.is_empty() {
        return None;
    }
    let linked = |s: &&onepaint_doc::styles::LayerStyles| s.effects.iter().any(|e| e.use_global_light);
    Some(all.iter().find(linked).map_or(crate::psd_effects::DEFAULT_LIGHT, |s| s.global_light()))
}

/// A node's layer styles as blocks (spec 56): its effects (`lfx2` or
/// `lmfx`) and, below 100%, its Fill opacity (`iOpa`, truncated as
/// happypaint's `setUint8`).
fn style_blocks(styles: Option<onepaint_doc::styles::LayerStyles>) -> Vec<([u8; 4], Vec<u8>)> {
    let Some(s) = styles else { return Vec::new() };
    let mut out: Vec<([u8; 4], Vec<u8>)> = crate::psd_effects::write(&s).into_iter().collect();
    if s.fill_opacity < 1.0 {
        out.push((*b"iOpa", vec![(s.fill_opacity.clamp(0.0, 1.0) * 255.0) as u8, 0, 0, 0]));
    }
    out
}

/// A node's colour label from an `lclr` index (0 = none).
fn set_label(extra: &mut onepaint_doc::Extra, i: u16) {
    if let Some(l) = LABELS.get(i as usize).filter(|_| i > 0) {
        extra.insert("colorLabel".into(), serde_json::Value::String((*l).into()));
    }
}

/// Photoshop's layer colours (`lclr`), by index.
const LABELS: [&str; 8] = ["none", "red", "orange", "yellow", "green", "blue", "violet", "gray"];

/// A node's colour label as an `lclr` block (none written for none).
fn label_block(extra: &onepaint_doc::Extra) -> Option<([u8; 4], Vec<u8>)> {
    let label = extra.get("colorLabel")?.as_str()?;
    let i = LABELS.iter().position(|l| *l == label).filter(|i| *i > 0)? as u16;
    Some((*b"lclr", [i.to_be_bytes().to_vec(), vec![0; 6]].concat()))
}

/// A layer's locks as `lspf` flags (transparency 1, composite 2, position
/// 4): a locked layer locks all three, alpha lock the transparency.
fn lock_block(l: &Layer) -> ([u8; 4], Vec<u8>) {
    let flags: u32 = if l.locked {
        7
    } else if l.alpha_lock {
        1
    } else {
        0
    };
    (*b"lspf", flags.to_be_bytes().to_vec())
}

/// Guides in the document (`extra.guides.lines`) as Photoshop's: horizontal
/// and vertical lines only, (horizontal, location px).
fn psd_guides(doc: &Document) -> Vec<(bool, f64)> {
    let lines = doc.extra.get("guides").and_then(|g| g.get("lines")).and_then(|l| l.as_array()).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for g in lines {
        let num = |k: &str| g.get(k).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        let pi = std::f64::consts::PI;
        let a = num("angle").rem_euclid(pi);
        if a.abs() < 1e-6 || (a - pi).abs() < 1e-6 {
            out.push((true, num("y")));
        } else if (a - pi / 2.0).abs() < 1e-6 {
            out.push((false, num("x")));
        }
    }
    out
}

/// Guides from resource 1032: version, grid, count, then per guide its
/// location ×32 and direction (1 horizontal).
fn read_guides(d: &[u8]) -> Option<Vec<(bool, f64)>> {
    let mut c = Cur { b: d, p: 0 };
    c.take(12).ok()?;
    let n = c.u32().ok()? as usize;
    let mut out = Vec::with_capacity(n.min(4096));
    for _ in 0..n {
        let at = f64::from(c.u32().ok()? as i32) / 32.0;
        out.push((c.u8().ok()? != 0, at));
    }
    Some(out)
}

/// The image resources in `res`: (id, the whole block, its data).
fn resource_blocks(res: &[u8]) -> Vec<(u16, &[u8], &[u8])> {
    let mut out = Vec::new();
    let mut p = 0;
    while p + 12 <= res.len() && &res[p..p + 4] == b"8BIM" {
        let id = u16::from_be_bytes([res[p + 4], res[p + 5]]);
        let nl = res[p + 6] as usize;
        let q = p + 6 + nl + 1 + (nl + 1) % 2;
        let Some(sz) = res.get(q..q + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize) else { break };
        let end = (q + 4 + sz + sz % 2).min(res.len());
        out.push((id, &res[p..end], &res[(q + 4).min(end)..(q + 4 + sz).min(end)]));
        p = end;
    }
    out
}

/// The guides and grid an opened file's resources hold.
fn source_guides(res: &[u8]) -> (Vec<(bool, f64)>, (u32, u32)) {
    let Some((_, _, d)) = resource_blocks(res).into_iter().find(|b| b.0 == 1032) else { return (Vec::new(), (576, 576)) };
    let grid = (d.len() >= 12).then(|| (u32::from_be_bytes([d[4], d[5], d[6], d[7]]), u32::from_be_bytes([d[8], d[9], d[10], d[11]])));
    (read_guides(d).unwrap_or_default(), grid.unwrap_or((576, 576)))
}

fn put_resource(r: &mut W, id: u16, data: &[u8]) {
    r.bytes(b"8BIM");
    r.u16(id);
    r.u16(0);
    r.u32(data.len() as u32);
    r.bytes(data);
    if data.len() % 2 == 1 {
        r.u8(0);
    }
}

/// Resource 1032 for `guides`, Photoshop's grid as the file had it.
fn guides_resource(guides: &[(bool, f64)], grid: (u32, u32)) -> Vec<u8> {
    let mut w = W(Vec::new());
    w.u32(1);
    w.u32(grid.0);
    w.u32(grid.1);
    w.u32(guides.len() as u32);
    for (h, at) in guides {
        w.u32((at * 32.0).round() as i32 as u32);
        w.u8(u8::from(*h));
    }
    w.0
}

/// The same guides, in any order, at Photoshop's 1/32 px.
fn same_guides(a: &[(bool, f64)], b: &[(bool, f64)]) -> bool {
    let key = |g: &[(bool, f64)]| {
        let mut k: Vec<(bool, i64)> = g.iter().map(|(h, at)| (*h, (at * 32.0).round() as i64)).collect();
        k.sort();
        k
    };
    key(a) == key(b)
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
        mask_look: (1.0, 0.0),
        ranges: Vec::new(),
        extra: vec![(*b"lsct", lsct)],
        raw: None,
    }
}

/// The records for `nodes`, bottom up; a node `src` still matches reuses
/// its original record.
fn flatten(nodes: &[Node], doc: Rect, frame: onepaint_filters::Frame, src: Option<&crate::psd_source::PsdSource>, out: &mut Vec<Record>) {
    for n in nodes {
        let raw = src.and_then(|s| s.record(n).map(|r| Record::raw(s, r)));
        match n {
            Node::Layer(l) => out.push(raw.unwrap_or_else(|| layer_record(l, doc, frame))),
            Node::Group(g) => {
                out.push(empty_record("</Layer group>", *b"norm", 255, 0x08 | 0x10, 3u32.to_be_bytes().to_vec()));
                flatten(&g.children, doc, frame, src, out);
                if let Some(r) = raw {
                    out.push(r);
                    continue;
                }
                let mut lsct = 1u32.to_be_bytes().to_vec();
                lsct.extend_from_slice(b"8BIM");
                lsct.extend_from_slice(group_key_of(g.blend_mode));
                lsct.extend_from_slice(&0u32.to_be_bytes());
                let flags = 0x08 | 0x10 | if g.visible { 0 } else { 2 };
                // Photoshop writes a pass-through group's own record as Normal.
                let mut rec = empty_record(&g.name, *key_of(g.blend_mode), (g.opacity.clamp(0.0, 1.0) * 255.0).round() as u8, flags, lsct);
                rec.extra.extend(style_blocks(g.styles()));
                rec.extra.extend(label_block(&g.extra));
                out.push(rec);
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
    // Opened from a PSD and not changed at all: the file as it was.
    let src = crate::psd_source::PsdSource::of(doc);
    let guides = psd_guides(doc);
    let (src_guides, grid) = src.map_or((Vec::new(), (576, 576)), |s| source_guides(&s.bytes[s.resources.clone()]));
    let guides_kept = same_guides(&guides, &src_guides);
    if let Some(s) = src
        && guides_kept
        && s.untouched(doc)
    {
        return Ok(s.bytes.to_vec());
    }
    let bounds = doc.bounds();
    let mut records = Vec::new();
    let frame = onepaint_filters::Frame::region(0, 0, doc.width, doc.height, doc.dpi as f32).in_space(doc.space());
    flatten(&doc.root, bounds, frame, src, &mut records);
    let (w, h) = (doc.width as usize, doc.height as usize);
    let comp: Vec<[u8; 4]> = composite.chunks_exact(4).map(|p| doc.space().to_srgb8([p[0], p[1], p[2], p[3]])).collect();
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
    // Image resources: the original ones (guides, slices, ICC…) while the
    // resolution is unchanged, else 1005 resolution info.
    let resources = src.filter(|s| s.same_dpi(doc)).map(|s| &s.bytes[s.resources.clone()]);
    let light = global_light(&doc.root);
    o.section(1, false, |r| {
        if let Some(b) = resources {
            if guides_kept {
                r.bytes(b);
            } else {
                // Every resource as it was but the guides.
                for (_, block, _) in resource_blocks(b).into_iter().filter(|b| b.0 != 1032) {
                    r.bytes(block);
                }
                if !guides.is_empty() {
                    put_resource(r, 1032, &guides_resource(&guides, grid));
                }
            }
            return;
        }
        if !guides.is_empty() {
            put_resource(r, 1032, &guides_resource(&guides, grid));
        }
        // Global Light, when some layer's styles take it (spec 56).
        if let Some((angle, altitude)) = light {
            for (id, v) in [(1037u16, angle.round() as i32 as u32), (1049, altitude.round() as u32)] {
                r.bytes(b"8BIM");
                r.u16(id);
                r.u16(0);
                r.u32(4);
                r.u32(v);
            }
        }
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
                if let Some((bytes, _)) = &rec.raw {
                    li.bytes(bytes);
                    continue;
                }
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
                            // Mask parameters (flag 0x10): user density (bit 0)
                            // as a byte, user feather (bit 1) as an f64.
                            let (density, feather) = rec.mask_look;
                            let which = u8::from(density < 1.0) | u8::from(feather > 0.0) << 1;
                            m.u8(if which != 0 { fl | 0x10 } else { fl });
                            if which != 0 {
                                m.u8(which);
                                if density < 1.0 {
                                    m.u8((density.max(0.0) * 255.0).round() as u8);
                                }
                                if feather > 0.0 {
                                    m.bytes(&f64::from(feather).to_be_bytes());
                                }
                            }
                            m.u16(0);
                        }
                    });
                    ex.u32(rec.ranges.len() as u32);
                    ex.bytes(&rec.ranges);
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
                if let Some((_, chans)) = &rec.raw {
                    chans.iter().for_each(|c| li.bytes(c));
                    continue;
                }
                for (_, data) in &rec.channels {
                    li.bytes(data);
                }
            }
        });
        // The global layer mask and file-wide additional info, kept from the original.
        match src {
            Some(s) => lm.bytes(&s.bytes[s.tail.clone()]),
            None => lm.u32(0),
        }
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
    fn f64(&mut self) -> Result<f64, FormatError> {
        self.u64().map(f64::from_bits)
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
    knockout: u8,
    mask_look: (f32, f32),
    blend_if: Option<onepaint_doc::BlendIf>,
    vector_mask: Option<onepaint_doc::VectorMask>,
    /// Layer styles (spec 56): the last effects block's data, and Fill opacity.
    effects: Option<Vec<u8>>,
    fill: Option<f32>,
    /// A Solid Color or Gradient fill read as a live fill layer.
    live_fill: Option<onepaint_doc::Adjustment>,
    /// `lspf` flags and the `lclr` colour index.
    locks: u32,
    label: u16,
    /// Where the record and its channel data sit in the file.
    raw: crate::psd_source::RawRecord,
}

/// One channel's samples at the file's depth (spec 61): 1-bit rows packed
/// MSB first and padded to a byte, 8/16-bit big-endian integers, 32-bit
/// big-endian floats.
pub(crate) struct Plane {
    data: Vec<u8>,
    depth: u16,
    w: usize,
}

impl Plane {
    /// Sample `i` (row-major) as 0..1 (32-bit as stored); 1-bit 1 is black.
    fn sample(&self, i: usize) -> Option<f32> {
        match self.depth {
            8 => self.data.get(i).map(|v| f32::from(*v) / 255.0),
            16 => self.data.get(i * 2..i * 2 + 2).map(|b| f32::from(u16::from_be_bytes([b[0], b[1]])) / 65535.0),
            32 => self.data.get(i * 4..i * 4 + 4).map(|b| f32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            _ => {
                let (x, y) = (i % self.w.max(1), i / self.w.max(1));
                let byte = self.data.get(y * self.w.div_ceil(8) + x / 8)?;
                Some(if (byte >> (7 - x % 8)) & 1 == 1 { 0.0 } else { 1.0 })
            }
        }
    }
}

/// Bytes per row of a channel at `depth`.
fn row_bytes(w: usize, depth: u16) -> usize {
    if depth == 1 { w.div_ceil(8) } else { w * (depth as usize / 8) }
}

fn decode_channel(c: &mut Cur, len: usize, w: usize, h: usize, depth: u16, psb: bool) -> Result<Plane, FormatError> {
    let start = c.p;
    let row = row_bytes(w, depth);
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
                    unpredict(&mut out[y * row..(y + 1) * row], w, depth);
                }
            }
            out
        }
        _ => return Err(bad("unknown compression")),
    };
    c.p = start + len;
    Ok(Plane { data: out, depth, w })
}

/// Undo ZIP prediction on one row: byte deltas at 8-bit, big-endian u16
/// deltas at 16-bit; at 32-bit the row is byte-planar (every float's first
/// byte, then the second…) and delta-coded bytewise across it.
fn unpredict(r: &mut [u8], w: usize, depth: u16) {
    match depth {
        16 => {
            for x in 1..w {
                let prev = u16::from_be_bytes([r[(x - 1) * 2], r[(x - 1) * 2 + 1]]);
                let cur = u16::from_be_bytes([r[x * 2], r[x * 2 + 1]]).wrapping_add(prev);
                r[x * 2..x * 2 + 2].copy_from_slice(&cur.to_be_bytes());
            }
        }
        32 => {
            for x in 1..r.len() {
                r[x] = r[x].wrapping_add(r[x - 1]);
            }
            let planar = r.to_vec();
            for x in 0..w {
                for b in 0..4 {
                    r[x * 4 + b] = planar[b * w + x];
                }
            }
        }
        _ => {
            for x in 1..r.len() {
                r[x] = r[x].wrapping_add(r[x - 1]);
            }
        }
    }
}

/// How a file's colour channels become linear RGB (happypaint
/// `channels.ts` `converter`; spec 61).
pub(crate) enum ModeConv {
    Rgb,
    /// Grayscale, Duotone (its grey), Multichannel (its first channel), Bitmap.
    Gray,
    Cmyk(Box<onepaint_color::cmyk::CmykToRgb>),
    Lab,
    /// The colour table (planar R, G, B), and the transparent index.
    Indexed(Vec<u8>, Option<u8>),
}

pub(crate) struct Conv {
    mode: ModeConv,
    /// 32-bit colour is linear light already.
    linear: bool,
}

impl Conv {
    /// Colour channels per pixel.
    fn count(&self) -> usize {
        match self.mode {
            ModeConv::Rgb | ModeConv::Lab => 3,
            ModeConv::Cmyk(_) => 4,
            _ => 1,
        }
    }

    /// Linear RGB and straight alpha from the samples.
    fn px(&self, v: [f32; 4], alpha: f32) -> ([f32; 4], f32) {
        let s2l = srgb_to_linear;
        let from_srgb = |c: [f64; 3]| [s2l(c[0] as f32), s2l(c[1] as f32), s2l(c[2] as f32), 0.0];
        let rgb = match &self.mode {
            ModeConv::Rgb if self.linear => v.map(|c| c.clamp(0.0, 1.0)),
            ModeConv::Rgb => v.map(s2l),
            ModeConv::Gray => {
                let g = if self.linear { v[0].clamp(0.0, 1.0) } else { s2l(v[0]) };
                [g, g, g, 0.0]
            }
            ModeConv::Cmyk(c) => from_srgb(c.rgb([0, 1, 2, 3].map(|k| f64::from(1.0 - v[k])))),
            ModeConv::Lab => from_srgb(onepaint_color::cmyk::lab_to_srgb(
                f64::from(v[0]) * 100.0,
                f64::from(v[1]) * 255.0 - 128.0,
                f64::from(v[2]) * 255.0 - 128.0,
            )),
            ModeConv::Indexed(t, tidx) => {
                let i = (v[0] * 255.0).round() as usize;
                if *tidx == Some(i as u8) {
                    return ([0.0; 4], 0.0);
                }
                let at = |k: usize| f32::from(t.get(k * 256 + i).copied().unwrap_or(0)) / 255.0;
                [s2l(at(0)), s2l(at(1)), s2l(at(2)), 0.0]
            }
        };
        (rgb, alpha)
    }
}

/// A layer's pixels from its planes (colour ids 0.., alpha −1), as linear
/// premultiplied light.
fn surface_from(planes: &[(i16, Plane)], rect: Rect, conv: &Conv, doc: Rect) -> Surface {
    let mut s = Surface::new();
    let Some(ix) = rect.intersect(&doc) else { return s };
    let get = |id: i16| planes.iter().find(|p| p.0 == id).map(|p| &p.1);
    let n = conv.count();
    let colour: Vec<Option<&Plane>> = (0..4).map(|k| if k < n { get(k as i16) } else { None }).collect();
    let alpha = get(-1);
    let mut data = Vec::with_capacity((ix.w * ix.h * 4) as usize);
    for y in ix.y..ix.bottom() {
        for x in ix.x..ix.right() {
            let i = ((y - rect.y) * rect.w + (x - rect.x)) as usize;
            let v: [f32; 4] = std::array::from_fn(|k| colour[k].and_then(|p| p.sample(i)).unwrap_or(0.0));
            let a = alpha.and_then(|p| p.sample(i)).unwrap_or(1.0).clamp(0.0, 1.0);
            let (rgb, a) = conv.px(v, a);
            data.extend_from_slice(&[rgb[0] * a, rgb[1] * a, rgb[2] * a, a]);
        }
    }
    s.write_rect(ix, &data);
    s.prune_empty();
    s
}

/// Read a PSD/PSB into a document.
/// Path operations in Photoshop's order (ag-psd `booleanOperations`).
const VMSK_OPS: [onepaint_doc::VectorOp; 4] =
    [onepaint_doc::VectorOp::Exclude, onepaint_doc::VectorOp::Combine, onepaint_doc::VectorOp::Subtract, onepaint_doc::VectorOp::Intersect];

/// A `vmsk` / `vsms` block: version 3, flags (1 invert, 2 not linked,
/// 4 disabled), then 26-byte path records. Knots are three points, y before
/// x, in 8.24 fixed point of the document's height and width.
fn read_vmsk(d: &[u8], w: f64, h: f64) -> Option<onepaint_doc::VectorMask> {
    use onepaint_doc::{FillRule, VectorMask, VectorOp, VectorPath};
    let be32 = |o: usize| d.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    if be32(0)? != 3 {
        return None;
    }
    let flags = be32(4)?;
    let mut v = VectorMask { invert: flags & 1 != 0, unlinked: flags & 2 != 0, disabled: flags & 4 != 0, paths: Vec::new() };
    let mut at = 8;
    while at + 26 <= d.len() {
        let r = &d[at..at + 26];
        let sel = u16::from_be_bytes([r[0], r[1]]);
        match sel {
            0 | 3 => {
                let op = i16::from_be_bytes([r[4], r[5]]);
                let rule = u16::from_be_bytes([r[6], r[7]]);
                v.paths.push(VectorPath {
                    open: sel == 3,
                    op: usize::try_from(op).ok().and_then(|i| VMSK_OPS.get(i).copied()).unwrap_or(VectorOp::Combine),
                    rule: if rule == 2 { FillRule::NonZero } else { FillRule::EvenOdd },
                    knots: Vec::new(),
                });
            }
            1 | 2 | 4 | 5 => {
                let f =
                    |i: usize| i32::from_be_bytes([r[2 + i * 4], r[3 + i * 4], r[4 + i * 4], r[5 + i * 4]]) as f64 / (1u32 << 24) as f64;
                if let Some(p) = v.paths.last_mut() {
                    p.knots.push([f(1) * w, f(0) * h, f(3) * w, f(2) * h, f(5) * w, f(4) * h]);
                }
            }
            _ => {}
        }
        at += 26;
    }
    v.paths.retain(|p| !p.knots.is_empty());
    (!v.paths.is_empty()).then_some(v)
}

fn write_vmsk(v: &onepaint_doc::VectorMask, w: f64, h: f64) -> Vec<u8> {
    use onepaint_doc::VectorOp;
    let mut o = W(Vec::new());
    o.u32(3);
    o.u32(u32::from(v.invert) | u32::from(v.unlinked) << 1 | u32::from(v.disabled) << 2);
    // The path fill rule record, then the initial fill.
    o.u16(6);
    o.bytes(&[0; 24]);
    o.u16(8);
    o.u16(u16::from(matches!(v.paths.first().map(|p| p.op), Some(VectorOp::Subtract | VectorOp::Intersect))));
    o.bytes(&[0; 22]);
    let fixed = |x: f64, size: f64| ((x / size) * (1u32 << 24) as f64).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32;
    for p in &v.paths {
        o.u16(if p.open { 3 } else { 0 });
        o.u16(p.knots.len() as u16);
        o.i16(VMSK_OPS.iter().position(|&x| x == p.op).unwrap_or(1) as i16);
        o.u16(if p.rule == onepaint_doc::FillRule::NonZero { 2 } else { 1 });
        o.bytes(&[0; 18]);
        for k in &p.knots {
            o.u16(if p.open { 4 } else { 1 });
            for i in [0, 2, 4] {
                o.i32(fixed(k[i + 1], h));
                o.i32(fixed(k[i], w));
            }
        }
    }
    o.0
}

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
    if ![1, 8, 16, 32].contains(&depth) {
        return Err(bad("only 1-, 8-, 16- and 32-bit PSDs are supported"));
    }
    // Bitmap, Grayscale, Indexed, RGB, CMYK, Multichannel, Duotone, Lab (spec 61).
    if ![0, 1, 2, 3, 4, 7, 8, 9].contains(&mode) {
        return Err(bad("this PSD's colour mode isn't supported"));
    }
    let rgb_mode = mode == 3;
    let cm = c.u32()? as usize;
    let colour_data = c.take(cm)?.to_vec();
    let (mut icc, mut transparent): (Option<Vec<u8>>, Option<u16>) = (None, None);
    let mut doc = Document::new(w, h, None);
    doc.root.clear();
    doc.name = "Imported PSD".into();
    let ir = c.u32()? as usize;
    let mut light = crate::psd_effects::DEFAULT_LIGHT;
    let ir_start = c.p;
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
        // Global Light for layer styles (spec 56): angle, then altitude.
        // Guides: horizontal at angle 0, vertical at a quarter turn.
        if id == 1032
            && let Some(g) = c.b.get(start..start + sz).and_then(read_guides).filter(|g| !g.is_empty())
        {
            let lines: Vec<serde_json::Value> = g
                .iter()
                .map(|(h, at)| {
                    let (x, y, angle) = if *h { (0.0, *at, 0.0) } else { (*at, 0.0, std::f64::consts::FRAC_PI_2) };
                    serde_json::json!({"id": onepaint_doc::new_id(), "x": x, "y": y, "angle": angle})
                })
                .collect();
            doc.extra.insert("guides".into(), serde_json::json!({ "lines": lines }));
        }
        // The embedded ICC profile (CMYK files read through it), and an
        // indexed file's transparent colour.
        if id == 1039 {
            icc = c.b.get(start..start + sz).map(<[u8]>::to_vec);
        }
        if id == 1047 && sz >= 2 {
            transparent = c.b.get(start..start + 2).map(|b| u16::from_be_bytes([b[0], b[1]]));
        }
        if id == 1037 && sz >= 4 {
            light.0 = c.u32()? as i32 as f32;
        }
        if id == 1049 && sz >= 4 {
            light.1 = c.u32()? as f32;
        }
        c.p = start + sz + sz % 2;
    }
    c.p = ir_end;
    let conv = Conv {
        mode: match mode {
            3 => ModeConv::Rgb,
            4 => ModeConv::Cmyk(Box::new(onepaint_color::cmyk::CmykToRgb::new(icc.as_deref()))),
            9 => ModeConv::Lab,
            2 => ModeConv::Indexed(colour_data, transparent.and_then(|t| u8::try_from(t).ok())),
            _ => ModeConv::Gray,
        },
        linear: depth == 32,
    };
    let mut merged_alpha = false;
    let lm_len = c.len(psb)?;
    let lm_end = c.p + lm_len;
    let bounds = doc.bounds();
    // An 8-bit RGB PSD is what onepaint writes: keep its records to save
    // untouched layers back as they were.
    let mut src = (depth == 8 && mode == 3 && !psb)
        .then(|| crate::psd_source::PsdSource::new(std::sync::Arc::new(bytes.to_vec()), &doc, ir_start..ir_end, lm_end..lm_end));
    if lm_len > 0 {
        let li_len = c.len(psb)?;
        let li_end = c.p + li_len;
        if li_len > 0 {
            // A negative count: the merged image's first extra channel is its alpha.
            let raw_count = c.i16()?;
            merged_alpha = raw_count < 0;
            let count = raw_count.unsigned_abs() as usize;
            let mut recs = Vec::with_capacity(count);
            for _ in 0..count {
                let rec_start = c.p;
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
                let mut mask_look = (1.0f32, 0.0f32);
                if mlen >= 20 {
                    let (t, l, b, r) = (c.i32()?, c.i32()?, c.i32()?, c.i32()?);
                    let (dc, fl) = (c.u8()?, c.u8()?);
                    mask = Some((Rect::new(l, t, r - l, b - t), dc, fl));
                    // Mask parameters (`mask-params.ts`): density bytes, feather f64s.
                    if fl & 0x10 != 0 {
                        let which = c.u8()?;
                        if which & 1 != 0 {
                            mask_look.0 = c.u8()? as f32 / 255.0;
                        }
                        if which & 2 != 0 {
                            mask_look.1 = c.f64()? as f32;
                        }
                    }
                }
                c.p = mstart + mlen;
                let br = c.u32()? as usize;
                // Blend ranges: gray then each channel, This Layer then Underlying (RGB only).
                let ranges = c.take(br)?;
                let blend_if = (rgb_mode && br >= 8).then(|| {
                    let mut b = onepaint_doc::BlendIf::default();
                    for (i, ch) in ranges.chunks_exact(8).take(4).enumerate() {
                        b.0[i].src.copy_from_slice(&ch[..4]);
                        b.0[i].dst.copy_from_slice(&ch[4..]);
                    }
                    b
                });
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
                let mut knockout = 0;
                let mut effects = None;
                let mut fill = None;
                let (mut locks, mut label) = (0u32, 0u16);
                let mut vector_mask = None;
                let mut vector_fill = false;
                let mut fill_block: Option<([u8; 4], Vec<u8>)> = None;
                let mut vector_stroke = false;
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
                        b"knko" => knockout = d.u8()?,
                        b"lspf" => locks = d.u32()?,
                        b"lclr" => label = d.u16()?,
                        // The last effects block wins, as ag-psd reads them.
                        b"lfx2" | b"lmfx" => effects = Some(data.to_vec()),
                        b"iOpa" => fill = Some(d.u8()? as f32 / 255.0),
                        b"vmsk" | b"vsms" => vector_mask = vector_mask.or_else(|| read_vmsk(data, w as f64, h as f64)),
                        // A shape layer: its pixels are already cut to the path.
                        b"SoCo" | b"GdFl" => {
                            vector_fill = true;
                            fill_block = Some((k, data.to_vec()));
                        }
                        b"PtFl" | b"vscg" => vector_fill = true,
                        b"vstk" => vector_stroke = true,
                        b"nvrt" => adjustment = adj("invert", Default::default()),
                        b"expA" | b"vibA" | b"phfl" | b"mixr" | b"levl" | b"curv" | b"hue2" | b"blnc" | b"blwh" | b"selc" | b"grdm" => {
                            adjustment = crate::psd_adjust::read(&k, data)
                        }
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
                    knockout,
                    mask_look,
                    blend_if,
                    // A fill without a path or stroke comes in live (a shape
                    // layer keeps Photoshop's pixels, as happypaint).
                    live_fill: fill_block
                        .filter(|_| vector_mask.is_none() && !vector_stroke)
                        .and_then(|(k, d)| crate::psd_fill::read_block(&k, &d, f64::from(w), f64::from(h))),
                    vector_mask: vector_mask.filter(|_| !vector_fill),
                    effects,
                    fill,
                    locks,
                    label,
                    raw: crate::psd_source::RawRecord { record: rec_start..ex_end, channels: Vec::new() },
                });
            }
            // Channel image data, record by record.
            let mut nodes_flat: Vec<(RRec, Option<Surface>, Option<Surface>)> = Vec::new();
            progress.set_total(recs.len() as u64);
            for mut rec in recs {
                progress.check()?;
                let mut planes = Vec::new();
                let mut mask_px = None;
                for &(id, l) in &rec.channels {
                    let (cw, ch) = if id == -2 {
                        rec.mask.map_or((0, 0), |m| (m.0.w.max(0) as usize, m.0.h.max(0) as usize))
                    } else {
                        (rec.rect.w.max(0) as usize, rec.rect.h.max(0) as usize)
                    };
                    let at = c.p;
                    let data = decode_channel(&mut c, l, cw, ch, depth, psb)?;
                    rec.raw.channels.push(at..at + l);
                    if id == -2 {
                        mask_px = Some(data);
                    } else {
                        planes.push((id, data));
                    }
                }
                let pixels = (rec.rect.w > 0 && rec.rect.h > 0).then(|| surface_from(&planes, rec.rect, &conv, bounds));
                let mask = rec.mask.map(|(mr, dc, _)| {
                    let mut m = Document::filled(w, h, if dc >= 128 { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] });
                    if let Some(px) = &mask_px
                        && let Some(ix) = mr.intersect(&bounds)
                    {
                        for y in ix.y..ix.bottom() {
                            for x in ix.x..ix.right() {
                                let v =
                                    px.sample(((y - mr.y) * mr.w + (x - mr.x)) as usize).unwrap_or(f32::from(dc) / 255.0).clamp(0.0, 1.0);
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
                    Some(3) => {
                        if let Some(s) = src.as_mut() {
                            s.divider();
                        }
                        stack.push(Vec::new())
                    }
                    Some(1) | Some(2) => {
                        let children = if stack.len() > 1 { stack.pop().unwrap_or_default() } else { Vec::new() };
                        let mut g = Group::new(&rec.name);
                        g.children = children;
                        g.visible = rec.flags & 2 == 0;
                        g.opacity = rec.opacity as f32 / 255.0;
                        g.blend_mode = group_mode_of(&rec.lsct.and_then(|l| l.1).unwrap_or(rec.key));
                        g.set_styles(crate::psd_effects::styles_of(
                            rec.effects.as_deref().and_then(|d| crate::psd_effects::read_block(d, light)),
                            rec.fill,
                        ));
                        set_label(&mut g.extra, rec.label);
                        if let Some(s) = src.as_mut() {
                            s.node(&g.id, rec.raw.clone());
                        }
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
                        // Density and feather only come in with a real mask.
                        if mask.is_some() {
                            l.set_mask_density(rec.mask_look.0);
                            l.set_mask_feather(rec.mask_look.1);
                        }
                        l.set_blend_if(rec.blend_if);
                        l.set_vector_mask(rec.vector_mask);
                        l.mask = mask;
                        l.adjustment = rec.adjustment.or(rec.live_fill).map(Box::new);
                        if l.adjustment.as_ref().is_some_and(|a| a.kind.ends_with("-fill")) {
                            // A fill draws itself; its record's pixels were the rendered picture.
                            l.pixels = Surface::new();
                        }
                        l.set_knockout(match rec.knockout {
                            1 => Some(onepaint_doc::Knockout::Shallow),
                            2 => Some(onepaint_doc::Knockout::Deep),
                            _ => None,
                        });
                        if l.adjustment.is_none() {
                            l.set_styles(crate::psd_effects::styles_of(
                                rec.effects.as_deref().and_then(|d| crate::psd_effects::read_block(d, light)),
                                rec.fill,
                            ));
                        }
                        // Locked: composite, or position and transparency (as
                        // Photoshop's Background has it); else transparency alone
                        // is the alpha lock.
                        let (transparency, composite, position) = (rec.locks & 1 != 0, rec.locks & 2 != 0, rec.locks & 4 != 0);
                        l.locked = composite || (position && transparency);
                        l.alpha_lock = !l.locked && transparency;
                        set_label(&mut l.extra, rec.label);
                        if let Some(s) = src.as_mut() {
                            s.node(&l.id, rec.raw.clone());
                        }
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
        if let Some(s) = src.as_mut() {
            s.tail = li_end..lm_end;
        }
    }
    c.p = lm_end;
    if doc.root.is_empty() {
        // No layers: import the merged composite. Its colour channels come
        // first; the next is alpha only when the layer count said so.
        let comp = c.u16()?;
        let row = row_bytes(w as usize, depth);
        let colours = conv.count().min(nch as usize);
        let planes_n = if nch as usize > colours && merged_alpha { colours + 1 } else { colours };
        let counts: Vec<usize> = if comp == 1 {
            (0..h as usize * nch as usize)
                .map(|_| if psb { c.u32().map(|v| v as usize) } else { c.u16().map(|v| v as usize) })
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        let mut planes = Vec::new();
        for ch in 0..planes_n {
            let mut data = Vec::with_capacity(row * h as usize);
            for y in 0..h as usize {
                if comp == 1 {
                    data.extend(unpackbits(c.take(counts[ch * h as usize + y])?, row)?);
                } else {
                    data.extend_from_slice(c.take(row)?);
                }
            }
            let id = if ch == colours { -1 } else { ch as i16 };
            planes.push((id, Plane { data, depth, w: w as usize }));
        }
        let mut l = Layer::new("Background");
        l.pixels = surface_from(&planes, bounds, &conv, bounds);
        doc.root.push(Node::Layer(l));
    }
    if let Some(mut s) = src.filter(|s| s.has_records()) {
        s.fingerprint(&doc.root);
        doc.source = Some(std::sync::Arc::new(s));
    }
    Ok(doc)
}
