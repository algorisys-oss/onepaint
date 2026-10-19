//! PSD export (spec 15), following ag-psd's byte layout (big-endian).

use crate::FormatError;
use crate::psd::{Cur, bad, group_key_of, key_of, packbits, write_vmsk};
use onepaint_doc::{Document, Layer, Node};
use onepaint_raster::{Rect, Surface};

pub(crate) struct W(pub(crate) Vec<u8>);

impl W {
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(crate) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }
    /// Length-prefixed section padded to `round` (ag-psd `writeSection`): the
    /// stored length includes the padding only when `total` is set.
    pub(crate) fn section(&mut self, round: usize, total: bool, f: impl FnOnce(&mut W)) {
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
pub(crate) fn set_label(extra: &mut onepaint_doc::Extra, i: u16) {
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
pub(crate) fn read_guides(d: &[u8]) -> Option<Vec<(bool, f64)>> {
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
