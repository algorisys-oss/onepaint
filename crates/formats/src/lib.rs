//! File formats: the native `.happypaint` container (v1–v9 read, v9 write) and
//! PNG/JPEG helpers. See `docs/specs/05-file-format.md`.

pub mod book;
pub mod brushes;
pub mod export;
pub mod icc_embed;
mod image;
pub mod palette;
mod progress;
pub mod psd;
pub mod tiff_cmyk;
pub mod video;
pub mod zip;

pub use image::*;
pub use progress::Progress;

use onepaint_color::Space;
use onepaint_doc::{BlendMode, Depth, Document, Extra, Group, Layer, Node};
use onepaint_raster::{Rect, Surface, TILE, pack_key, unpack_key};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::{Read, Write};

pub const MAGIC: &[u8; 4] = b"HPNT";
pub const VERSION: u16 = 9;
pub const HEADER_BYTES: usize = 12;
pub const MIME: &str = "application/x-happypaint";
pub const EXTENSION: &str = "happypaint";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("not a .happypaint file")]
    BadMagic,
    #[error("file version {0} is newer than this app supports")]
    UnsupportedVersion(u16),
    #[error("file is truncated or corrupt: {0}")]
    Corrupt(String),
    #[error("image error: {0}")]
    Image(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cancelled")]
    Cancelled,
}

fn corrupt(s: impl Into<String>) -> FormatError {
    FormatError::Corrupt(s.into())
}

/// The 12-byte header: magic, u16 version, u16 flags (0), u32 manifest length; all little-endian.
pub fn header(version: u16, manifest_len: u32) -> [u8; 12] {
    let mut h = [0u8; 12];
    h[..4].copy_from_slice(MAGIC);
    h[4..6].copy_from_slice(&version.to_le_bytes());
    h[8..12].copy_from_slice(&manifest_len.to_le_bytes());
    h
}

/// Raw RFC 1951 deflate (no zlib header), as `CompressionStream('deflate-raw')`.
pub fn deflate_raw(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    // Writing into a Vec cannot fail.
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

pub fn inflate_raw(data: &[u8]) -> Result<Vec<u8>, FormatError> {
    let mut out = Vec::new();
    flate2::read::DeflateDecoder::new(data).read_to_end(&mut out)?;
    Ok(out)
}

/// happypaint's dedup hash: two FNV-1a passes, `hex(a)-hex(b)` unpadded.
pub fn fnv_pair(bytes: &[u8]) -> String {
    let (mut a, mut b) = (0x811c_9dc5u32, 0xcbf2_9ce4u32);
    for &x in bytes {
        a = (a ^ x as u32).wrapping_mul(0x0100_0193);
        b = (b ^ x as u32).wrapping_mul(0x0100_0197);
    }
    format!("{a:x}-{b:x}")
}

struct Parsed<'a> {
    manifest: Value,
    chunks: &'a [u8],
}

fn parse(bytes: &[u8]) -> Result<Parsed<'_>, FormatError> {
    if bytes.len() < HEADER_BYTES || &bytes[..4] != MAGIC {
        return Err(FormatError::BadMagic);
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version > VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let end = HEADER_BYTES.checked_add(len).filter(|&e| e <= bytes.len()).ok_or_else(|| corrupt("manifest past end of file"))?;
    let manifest = serde_json::from_slice(&inflate_raw(&bytes[HEADER_BYTES..end])?)?;
    Ok(Parsed { manifest, chunks: &bytes[end..] })
}

fn chunk<'a>(chunks: &'a [u8], r: &Value) -> Result<&'a [u8], FormatError> {
    let off = r.get("offset").and_then(Value::as_u64).ok_or_else(|| corrupt("chunk ref without offset"))? as usize;
    let len = r.get("len").and_then(Value::as_u64).ok_or_else(|| corrupt("chunk ref without len"))? as usize;
    let end = off.checked_add(len).filter(|&e| e <= chunks.len()).ok_or_else(|| corrupt("chunk past end of file"))?;
    Ok(&chunks[off..end])
}

/// The JPEG thumbnail, if the file has one.
pub fn read_thumbnail(bytes: &[u8]) -> Result<Option<Vec<u8>>, FormatError> {
    let p = parse(bytes)?;
    match p.manifest.get("thumbnail") {
        Some(r) => Ok(Some(chunk(p.chunks, r)?.to_vec())),
        None => Ok(None),
    }
}

const LEAF_KNOWN: &[&str] = &[
    "id",
    "name",
    "visible",
    "opacity",
    "blendMode",
    "clipped",
    "locked",
    "alphaLock",
    "inheritAlpha",
    "maskEnabled",
    "tiles",
    "bbox",
    "chunk",
    "maskTiles",
    "mask",
    "height",
    "overflow",
];
const GROUP_KNOWN: &[&str] = &["id", "name", "visible", "opacity", "blendMode", "collapsed", "children"];

fn extras(o: &Map<String, Value>, known: &[&str]) -> Extra {
    o.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect()
}

fn s(o: &Map<String, Value>, k: &str) -> String {
    o.get(k).and_then(Value::as_str).unwrap_or_default().to_string()
}

fn b(o: &Map<String, Value>, k: &str, d: bool) -> bool {
    o.get(k).and_then(Value::as_bool).unwrap_or(d)
}

fn f(o: &Map<String, Value>, k: &str, d: f64) -> f64 {
    o.get(k).and_then(Value::as_f64).unwrap_or(d)
}

/// Paste a decoded straight-sRGB RGBA8 image into a surface at (x, y), premultiplied linear.
#[allow(clippy::too_many_arguments)]
fn paste_color(s: &mut Surface, x: i32, y: i32, w: u32, h: u32, rgba: &[u8], clip: Rect, space: Space) {
    let r = Rect::new(x, y, w as i32, h as i32);
    let Some(ix) = r.intersect(&clip) else { return };
    let mut data = vec![0.0f32; (ix.w * ix.h * 4) as usize];
    for yy in 0..ix.h {
        for xx in 0..ix.w {
            let si = (((ix.y - y + yy) as u32 * w + (ix.x - x + xx) as u32) * 4) as usize;
            let p = space.from_srgb8([rgba[si], rgba[si + 1], rgba[si + 2], rgba[si + 3]]);
            let o = ((yy * ix.w + xx) * 4) as usize;
            data[o..o + 4].copy_from_slice(&p);
        }
    }
    s.write_rect(ix, &data);
}

/// The layer's pixels outside the canvas as a trimmed straight-sRGB PNG (zero
/// inside the canvas), or `None` when everything is on the canvas.
fn overflow_png(s: &Surface, doc: Rect, space: Space) -> Result<Option<(Rect, Vec<u8>)>, FormatError> {
    let Some(cb) = s.content_bounds() else { return Ok(None) };
    if cb.x >= doc.x && cb.y >= doc.y && cb.right() <= doc.right() && cb.bottom() <= doc.bottom() {
        return Ok(None);
    }
    let px = s.read_rect(cb);
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for yy in 0..cb.h {
        for xx in 0..cb.w {
            let (x, y) = (cb.x + xx, cb.y + yy);
            if !doc.contains(x, y) && px[((yy * cb.w + xx) * 4 + 3) as usize] > 0.0 {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    if x1 < x0 {
        return Ok(None);
    }
    let r = Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1);
    let data = s.read_rect(r);
    let mut rgba = Vec::with_capacity(data.len());
    for (i, p) in data.chunks_exact(4).enumerate() {
        let (x, y) = (r.x + (i as i32 % r.w), r.y + (i as i32 / r.w));
        rgba.extend_from_slice(&if doc.contains(x, y) { [0; 4] } else { space.to_srgb8([p[0], p[1], p[2], p[3]]) });
    }
    Ok(Some((r, encode_png_rgba8(r.w as u32, r.h as u32, &rgba)?)))
}

/// Masks store raw coverage in R (no transfer curve).
fn paste_mask(s: &mut Surface, x: i32, y: i32, w: u32, h: u32, rgba: &[u8], clip: Rect) {
    let r = Rect::new(x, y, w as i32, h as i32);
    let Some(ix) = r.intersect(&clip) else { return };
    for yy in 0..ix.h {
        for xx in 0..ix.w {
            let si = (((ix.y - y + yy) as u32 * w + (ix.x - x + xx) as u32) * 4) as usize;
            let m = rgba[si] as f32 / 255.0;
            s.set(ix.x + xx, ix.y + yy, [m, m, m, 1.0]);
        }
    }
}

/// Decode a layer's tile PNGs in parallel (spec 41), counting each on `progress`.
fn read_tiles(chunks: &[u8], tiles: &[Value], doc: Rect, mask: bool, progress: &Progress, space: Space) -> Result<Surface, FormatError> {
    use rayon::prelude::*;
    let decoded: Vec<Option<Surface>> = tiles
        .par_iter()
        .map(|t| {
            progress.check()?;
            let key = t.get("key").and_then(Value::as_i64).ok_or_else(|| corrupt("tile without key"))?;
            let (tx, ty) = unpack_key((key & 0xffff_ffff) as u32);
            let Some(clip) = doc.intersect(&Rect::from_tile(tx, ty)) else {
                progress.advance(1);
                return Ok(None);
            };
            let c = chunk(chunks, t.get("chunk").ok_or_else(|| corrupt("tile without chunk"))?)?;
            let (w, h, rgba) = decode_png_rgba8(c)?;
            let mut s = Surface::new();
            if mask {
                paste_mask(&mut s, tx * TILE, ty * TILE, w, h, &rgba, clip);
            } else {
                paste_color(&mut s, tx * TILE, ty * TILE, w, h, &rgba, clip, space);
            }
            progress.advance(1);
            Ok(Some(s))
        })
        .collect::<Result<_, FormatError>>()?;
    // Each part holds one tile; later tiles win, as when pasted in order.
    let mut s = Surface::new();
    for part in decoded.into_iter().flatten() {
        for (tx, ty) in part.tile_keys() {
            s.set_tile(tx, ty, part.tile(tx, ty).cloned());
        }
    }
    Ok(s)
}

/// Tile entries under `nodes`, for the progress total.
fn count_tiles(nodes: &[Value]) -> u64 {
    nodes
        .iter()
        .map(|n| {
            let len = |k: &str| n.get(k).and_then(Value::as_array).map_or(0, |a| a.len() as u64);
            len("tiles") + len("maskTiles") + n.get("children").and_then(Value::as_array).map_or(0, |c| count_tiles(c))
        })
        .sum()
}

/// A mode onepaint can't composite yet keeps its name, so saving writes it
/// back (the extras are written after `blendMode`).
fn keep_raw_blend(o: &Map<String, Value>, parsed: BlendMode, extra: &mut onepaint_doc::Extra) {
    if let Some(raw) = o.get("blendMode").and_then(Value::as_str)
        && raw != parsed.as_str()
    {
        extra.insert(onepaint_doc::RAW_BLEND_KEY.into(), Value::String(raw.into()));
    }
}

fn read_node(v: &Value, chunks: &[u8], doc: Rect, progress: &Progress, space: Space) -> Result<Node, FormatError> {
    let o = v.as_object().ok_or_else(|| corrupt("node is not an object"))?;
    if let Some(children) = o.get("children").and_then(Value::as_array) {
        let mut g = Group::new(&s(o, "name"));
        g.id = s(o, "id");
        g.visible = b(o, "visible", true);
        g.opacity = f(o, "opacity", 1.0) as f32;
        g.blend_mode = BlendMode::parse(&s(o, "blendMode"));
        g.collapsed = b(o, "collapsed", false);
        g.extra = extras(o, GROUP_KNOWN);
        keep_raw_blend(o, g.blend_mode, &mut g.extra);
        for c in children {
            g.children.push(read_node(c, chunks, doc, progress, space)?);
        }
        return Ok(Node::Group(g));
    }
    let mut l = Layer::new(&s(o, "name"));
    l.id = s(o, "id");
    l.visible = b(o, "visible", true);
    l.opacity = f(o, "opacity", 1.0) as f32;
    l.blend_mode = BlendMode::parse(&s(o, "blendMode"));
    l.clipped = b(o, "clipped", false);
    l.locked = b(o, "locked", false);
    l.alpha_lock = b(o, "alphaLock", false);
    l.inherit_alpha = b(o, "inheritAlpha", false);
    l.mask_enabled = b(o, "maskEnabled", true);
    l.extra = extras(o, LEAF_KNOWN);
    keep_raw_blend(o, l.blend_mode, &mut l.extra);
    if let Some(kind) = l.extra.remove("adjustmentKind").and_then(|v| v.as_str().map(str::to_string)) {
        let params = l.extra.remove("adjustmentParams").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        l.adjustment = Some(Box::new(onepaint_doc::Adjustment { kind, params }));
    }
    if let Some(tiles) = o.get("tiles").and_then(Value::as_array) {
        l.pixels = read_tiles(chunks, tiles, doc, false, progress, space)?;
    } else if let (Some(bb), Some(c)) = (o.get("bbox"), o.get("chunk")) {
        let (w, h, rgba) = decode_png_rgba8(chunk(chunks, c)?)?;
        let x = bb.get("x").and_then(Value::as_i64).unwrap_or(0) as i32;
        let y = bb.get("y").and_then(Value::as_i64).unwrap_or(0) as i32;
        paste_color(&mut l.pixels, x, y, w, h, &rgba, doc, space);
    }
    if let Some(tiles) = o.get("maskTiles").and_then(Value::as_array) {
        l.mask = Some(read_tiles(chunks, tiles, doc, true, progress, space)?);
    } else if let Some(c) = o.get("mask") {
        let (w, h, rgba) = decode_png_rgba8(chunk(chunks, c)?)?;
        let mut m = Surface::new();
        paste_mask(&mut m, 0, 0, w, h, &rgba, doc);
        l.mask = Some(m);
    }
    if let Some(c) = o.get("height") {
        let bytes = chunk(chunks, c)?;
        // A height PNG we can't read is kept verbatim and re-emitted.
        match decode_height_png(bytes, doc) {
            Ok(h) => l.height = (h.tile_count() > 0).then_some(h),
            Err(_) => {
                l.chunks.insert("height".into(), bytes.to_vec());
            }
        }
    }
    if let Some(ov) = o.get("overflow") {
        // Off-canvas pixels (layer-overflow.ts): a trimmed rect, zero inside the canvas.
        let c = ov.get("chunk").ok_or_else(|| corrupt("overflow without chunk"))?;
        let r = ov.get("rect").cloned().unwrap_or(Value::Null);
        let n = |k: &str| r.get(k).and_then(Value::as_i64).unwrap_or(0) as i32;
        let (w, h, rgba) = decode_png_rgba8(chunk(chunks, c)?)?;
        let (x0, y0) = (n("x"), n("y"));
        for yy in 0..h as i32 {
            for xx in 0..w as i32 {
                let (x, y) = (x0 + xx, y0 + yy);
                if doc.contains(x, y) {
                    continue;
                }
                let i = ((yy as u32 * w + xx as u32) * 4) as usize;
                if rgba[i + 3] > 0 {
                    l.pixels.set(x, y, space.from_srgb8([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]));
                }
            }
        }
    }
    Ok(Node::Layer(l))
}

/// Read any `.happypaint` (v1–v9) by key presence, never by version number.
pub fn read_happypaint(bytes: &[u8]) -> Result<Document, FormatError> {
    read_happypaint_with(bytes, &Progress::default())
}

/// [`read_happypaint`], counting tiles on `progress` and stopping with
/// [`FormatError::Cancelled`] once it is cancelled (spec 41).
pub fn read_happypaint_with(bytes: &[u8], progress: &Progress) -> Result<Document, FormatError> {
    let p = parse(bytes)?;
    let d = p.manifest.get("doc").and_then(Value::as_object).ok_or_else(|| corrupt("manifest without doc"))?;
    let width = d.get("width").and_then(Value::as_u64).ok_or_else(|| corrupt("doc without width"))? as u32;
    let height = d.get("height").and_then(Value::as_u64).ok_or_else(|| corrupt("doc without height"))? as u32;
    let mut doc = Document::new(width, height, None);
    doc.root.clear();
    doc.id = s(d, "id");
    doc.name = s(d, "name");
    doc.dpi = f(d, "dpi", 300.0);
    doc.color_space = if s(d, "colorSpace") == "display-p3" { "display-p3".into() } else { "srgb".into() };
    doc.depth = match d.get("depth").and_then(Value::as_str) {
        Some("8-bit") => Depth::Rgba8,
        Some("32-bit-float") => Depth::Rgba32F,
        _ => Depth::Rgba16F,
    };
    doc.updated_at = f(d, "updatedAt", 0.0);
    doc.created_at = f(d, "createdAt", doc.updated_at);
    doc.extra = extras(d, &["id", "name", "width", "height", "dpi", "bitDepth", "colorSpace", "depth", "createdAt", "updatedAt"]);
    let bounds = doc.bounds();
    let nodes = p.manifest.get("nodes").and_then(Value::as_array).ok_or_else(|| corrupt("manifest without nodes"))?;
    progress.set_total(count_tiles(nodes));
    for n in nodes {
        doc.root.push(read_node(n, p.chunks, bounds, progress, doc.space())?);
    }
    Ok(doc)
}

/// JSON number: integral values print without a fraction, like JS.
fn num(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 { json!(v as i64) } else { json!((v * 1e6).round() / 1e6) }
}

struct Writer {
    /// The document's space: colour tiles are saved as straight sRGB (spec 50).
    space: Space,
    chunks: Vec<u8>,
    dedup: HashMap<Vec<u8>, Value>,
}

impl Writer {
    fn push(&mut self, data: &[u8]) -> Value {
        let r = json!({"offset": self.chunks.len(), "len": data.len()});
        self.chunks.extend_from_slice(data);
        r
    }

    fn push_dedup(&mut self, png: Vec<u8>) -> (String, Value) {
        let hash = fnv_pair(&png);
        if let Some(r) = self.dedup.get(&png) {
            return (hash, r.clone());
        }
        let r = self.push(&png);
        self.dedup.insert(png, r.clone());
        (hash, r)
    }

    fn tiles(&mut self, s: &Surface, doc: Rect, mask: bool) -> Result<Vec<Value>, FormatError> {
        let mut out = Vec::new();
        for (tx, ty) in doc.tiles() {
            let Some(r) = doc.intersect(&Rect::from_tile(tx, ty)) else {
                continue;
            };
            if !mask && s.tile(tx, ty).is_none_or(|t| t.is_empty()) {
                continue;
            }
            let px = s.read_rect(r);
            let mut rgba = Vec::with_capacity(px.len());
            for p in px.chunks_exact(4) {
                if mask {
                    let m = (p[0].clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
                    rgba.extend_from_slice(&[m, m, m, 255]);
                } else {
                    rgba.extend_from_slice(&self.space.to_srgb8([p[0], p[1], p[2], p[3]]));
                }
            }
            let (hash, chunk) = self.push_dedup(encode_png_rgba8(r.w as u32, r.h as u32, &rgba)?);
            out.push(json!({"key": pack_key(tx, ty), "hash": hash, "chunk": chunk}));
        }
        Ok(out)
    }

    fn node(&mut self, n: &Node, doc: Rect) -> Result<Value, FormatError> {
        let mut o = Map::new();
        let (id, name, visible, opacity, mode) = (n.id(), n.name(), n.visible(), n.opacity(), n.blend_mode());
        o.insert("id".into(), json!(id));
        o.insert("name".into(), json!(name));
        o.insert("visible".into(), json!(visible));
        o.insert("opacity".into(), num(opacity as f64));
        o.insert("blendMode".into(), json!(mode.as_str()));
        match n {
            Node::Group(g) => {
                for (k, v) in &g.extra {
                    o.insert(k.clone(), v.clone());
                }
                if g.collapsed {
                    o.insert("collapsed".into(), json!(true));
                }
                let children = g.children.iter().map(|c| self.node(c, doc)).collect::<Result<Vec<_>, _>>()?;
                o.insert("children".into(), Value::Array(children));
            }
            Node::Layer(l) => {
                o.insert("clipped".into(), json!(l.clipped));
                o.insert("locked".into(), json!(l.locked));
                o.insert("alphaLock".into(), json!(l.alpha_lock));
                // Preserved extras keep their relative order, ahead of maskEnabled and pixels (F:274-294).
                let mut extra = l.extra.clone();
                let metallic = extra.remove("metallic");
                if let Some(m) = metallic {
                    o.insert("metallic".into(), m);
                }
                o.insert("inheritAlpha".into(), json!(l.inherit_alpha));
                for (k, v) in extra {
                    o.insert(k, v);
                }
                if let Some(a) = &l.adjustment {
                    o.insert("adjustmentKind".into(), json!(a.kind));
                    o.insert("adjustmentParams".into(), serde_json::to_value(&a.params)?);
                }
                if !l.mask_enabled {
                    o.insert("maskEnabled".into(), json!(false));
                }
                // Adjustment layers carry no pixels of their own (PE:7403-7412).
                if l.adjustment.is_none() {
                    let tiles = self.tiles(&l.pixels, doc, false)?;
                    o.insert("tiles".into(), Value::Array(tiles));
                }
                if let Some(m) = &l.mask {
                    let mt = self.tiles(m, doc, true)?;
                    o.insert("maskTiles".into(), Value::Array(mt));
                }
                if let Some(png) = l.height.as_ref().map(|h| encode_height_png(h, doc)).transpose()?.flatten() {
                    let r = self.push(&png);
                    o.insert("height".into(), r);
                } else if let Some(h) = l.chunks.get("height") {
                    let r = self.push(h);
                    o.insert("height".into(), r);
                }
                if l.adjustment.is_none()
                    && let Some((rect, png)) = overflow_png(&l.pixels, doc, self.space)?
                {
                    let r = self.push(&png);
                    o.insert("overflow".into(), json!({"rect": {"x": rect.x, "y": rect.y, "width": rect.w, "height": rect.h}, "chunk": r}));
                }
            }
        }
        Ok(Value::Object(o))
    }
}

/// Write a v9 `.happypaint`. `thumbnail_jpeg` becomes the first chunk.
pub fn write_happypaint(doc: &Document, thumbnail_jpeg: Option<&[u8]>, app_version: &str) -> Result<Vec<u8>, FormatError> {
    let mut w = Writer { space: doc.space(), chunks: Vec::new(), dedup: HashMap::new() };
    let thumb = thumbnail_jpeg.map(|t| w.push(t));
    let bounds = doc.bounds();
    let nodes = doc.root.iter().map(|n| w.node(n, bounds)).collect::<Result<Vec<_>, _>>()?;
    let mut d = Map::new();
    d.insert("id".into(), json!(doc.id));
    d.insert("name".into(), json!(doc.name));
    d.insert("width".into(), json!(doc.width));
    d.insert("height".into(), json!(doc.height));
    d.insert("dpi".into(), num(doc.dpi));
    d.insert("bitDepth".into(), json!(doc.depth.bit_depth()));
    d.insert("colorSpace".into(), json!(doc.color_space));
    d.insert("depth".into(), json!(doc.depth.as_str()));
    for (k, v) in &doc.extra {
        d.insert(k.clone(), v.clone());
    }
    d.insert("createdAt".into(), num(doc.created_at));
    d.insert("updatedAt".into(), num(doc.updated_at));
    let mut m = Map::new();
    m.insert("app".into(), json!(app_version));
    m.insert("doc".into(), Value::Object(d));
    if let Some(t) = thumb {
        m.insert("thumbnail".into(), t);
    }
    m.insert("nodes".into(), Value::Array(nodes));
    let manifest = deflate_raw(&serde_json::to_vec(&Value::Object(m))?);
    let mut out = header(VERSION, manifest.len() as u32).to_vec();
    out.extend_from_slice(&manifest);
    out.extend_from_slice(&w.chunks);
    Ok(out)
}

/// Impasto height as a full-canvas PNG (`height-codec.ts`): `q = h/16 · 65535`,
/// R = high byte, G = low byte. `None` when the field is flat.
pub fn encode_height_png(field: &Surface, doc: Rect) -> Result<Option<Vec<u8>>, FormatError> {
    if field.tile_count() == 0 || doc.is_empty() {
        return Ok(None);
    }
    let (w, h) = (doc.w as usize, doc.h as usize);
    let data = field.read_rect(doc);
    let mut rgb = vec![0u8; w * h * 3];
    let mut any = false;
    for (i, px) in rgb.chunks_exact_mut(3).enumerate() {
        let v = data[i * 4];
        any |= v > 0.0;
        let q = ((v / HEIGHT_STORE_MAX).clamp(0.0, 1.0) * 65535.0) as u32;
        px[0] = (q >> 8) as u8;
        px[1] = (q & 0xff) as u8;
    }
    if !any {
        return Ok(None);
    }
    let err = |e: png::EncodingError| FormatError::Image(e.to_string());
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, doc.w as u32, doc.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let mut wr = enc.write_header().map_err(err)?;
        wr.write_image_data(&rgb).map_err(err)?;
    }
    Ok(Some(out))
}

/// The inverse of [`encode_height_png`]; pixels outside the document are dropped.
pub fn decode_height_png(bytes: &[u8], doc: Rect) -> Result<Surface, FormatError> {
    let (w, h, rgba) = decode_png_rgba8(bytes)?;
    let mut s = Surface::new();
    for y in 0..(h as i32).min(doc.h) {
        for x in 0..(w as i32).min(doc.w) {
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            let q = ((rgba[i] as u32) << 8) | rgba[i + 1] as u32;
            if q > 0 {
                let v = q as f32 / 65535.0 * HEIGHT_STORE_MAX;
                s.set(doc.x + x, doc.y + y, [v; 4]);
            }
        }
    }
    Ok(s)
}

const HEIGHT_STORE_MAX: f32 = 16.0;
