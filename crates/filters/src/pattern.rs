//! Built-in fill patterns (happypaint `utils/patterns.ts`): seamless 32 px
//! coverage functions baked into sRGB byte tiles with 4×4 supersampling.

/// Tile size in px at 100 % scale (`PATTERN_TILE`).
pub const PATTERN_TILE: usize = 32;

/// Built-in pattern ids and labels, in menu order.
pub const PATTERNS: [(&str, &str); 8] = [
    ("checker", "Checker"),
    ("stripes", "Stripes"),
    ("dots", "Dots"),
    ("grid", "Grid"),
    ("brick", "Brick"),
    ("crosshatch", "Crosshatch"),
    ("halftone", "Halftone"),
    ("weave", "Canvas weave"),
];

fn wrap(v: f64, m: f64) -> f64 {
    ((v % m) + m) % m
}

/// Coverage of colour A at (x, y), any real coordinates (`patternCoverage`).
pub fn pattern_coverage(id: &str, x: f64, y: f64) -> f64 {
    let s = PATTERN_TILE as f64;
    let u = wrap(x, s);
    let v = wrap(y, s);
    let b = |c: bool| if c { 1.0 } else { 0.0 };
    match id {
        "checker" => b(((u / (s / 2.0)).floor() + (v / (s / 2.0)).floor()) % 2.0 == 0.0),
        "stripes" => b(v < s / 4.0 || (v >= s / 2.0 && v < 3.0 * s / 4.0)),
        "dots" => b((u - s / 2.0).hypot(v - s / 2.0) < s * 0.22),
        "grid" => b(u < 2.0 || v < 2.0),
        "brick" => {
            let course = (v / (s / 2.0)).floor();
            let bu = wrap(u + (course % 2.0) * (s / 2.0), s);
            b(wrap(v, s / 2.0) < 2.0 || bu < 2.0)
        }
        "crosshatch" => b(wrap(u + v, s / 2.0) < 2.0 || wrap(u - v, s / 2.0) < 2.0),
        "halftone" => {
            let corner = u.hypot(v).min((s - u).hypot(v)).min(u.hypot(s - v)).min((s - u).hypot(s - v));
            let centre = (u - s / 2.0).hypot(v - s / 2.0);
            b(corner.min(centre) < s * 0.16)
        }
        "weave" => {
            let bx = (u / (s / 2.0)).floor();
            let by = (v / (s / 2.0)).floor();
            let along = if (bx + by) % 2.0 == 0.0 { v } else { u };
            b(wrap(along, 8.0) < 6.0)
        }
        _ => 0.0,
    }
}

/// One tile: straight sRGB bytes (built-ins are opaque; imported images may not be).
#[derive(Clone, Debug, PartialEq)]
pub struct PatternTile {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl PatternTile {
    /// Bilinear with REPEAT wrap at uv (one tile per unit), straight 0..1.
    pub fn sample_repeat(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.w as i64, self.h as i64);
        let x = u * self.w as f32 - 0.5;
        let y = v * self.h as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |xx: i64, yy: i64| {
            let i = ((yy.rem_euclid(h) * w + xx.rem_euclid(w)) * 4) as usize;
            [0, 1, 2, 3].map(|k| self.data[i + k] as f32 / 255.0)
        };
        let (xi, yi) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (at(xi, yi), at(xi + 1, yi), at(xi, yi + 1), at(xi + 1, yi + 1));
        [0, 1, 2, 3].map(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bot = c[k] + (d[k] - c[k]) * fx;
            top + (bot - top) * fy
        })
    }
}

/// Bake a tile: A where covered, B elsewhere (`patternPixels`). Unknown ids use checker.
pub fn pattern_pixels(id: &str, a: [f32; 3], b: [f32; 3]) -> PatternTile {
    let id = if PATTERNS.iter().any(|(p, _)| *p == id) { id } else { PATTERNS[0].0 };
    // Colours come from JSON decimals: widen through their shortest decimal form
    // so a tie like 0.1·255 = 25.5 rounds as it does in happypaint's doubles.
    let (ad, bd) = (a.map(crate::tex::widen), b.map(crate::tex::widen));
    let s = PATTERN_TILE;
    let n = 4;
    let mut data = vec![0u8; s * s * 4];
    for y in 0..s {
        for x in 0..s {
            let mut cov = 0.0;
            for j in 0..n {
                for i in 0..n {
                    cov += pattern_coverage(id, x as f64 + (i as f64 + 0.5) / n as f64, y as f64 + (j as f64 + 0.5) / n as f64);
                }
            }
            cov /= (n * n) as f64;
            let k = (y * s + x) * 4;
            for c in 0..3 {
                let v = (bd[c] + (ad[c] - bd[c]) * cov) * 255.0;
                // `Math.round`: halves round up.
                data[k + c] = (v + 0.5).floor().clamp(0.0, 255.0) as u8;
            }
            data[k + 3] = 255;
        }
    }
    PatternTile { w: s, h: s, data }
}

/// Imported tiles are kept inside the params as PNG data URLs (happypaint
/// `utils/pattern-import.ts`), at most this long on the longer side.
pub const PATTERN_IMPORT_MAX: u32 = 512;

/// The size an imported image is stored at (`fitWithin`).
pub fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    let k = (max as f64 / w.max(h) as f64).min(1.0);
    (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1))
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                out.push(B64[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in s.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if ch == b'=' {
            break;
        }
        let v = B64.iter().position(|&b| b == ch)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A PNG data URL for straight RGBA8 pixels.
pub fn png_data_url(rgba: &[u8], w: u32, h: u32) -> Option<String> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().ok()?;
        wr.write_image_data(rgba).ok()?;
    }
    Some(format!("data:image/png;base64,{}", base64_encode(&out)))
}

fn decode_png_url(url: &str) -> Option<PatternTile> {
    let (head, body) = url.split_once(',')?;
    if !head.starts_with("data:image/png") || !head.ends_with(";base64") {
        return None;
    }
    let bytes = base64_decode(body)?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let data: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::GrayscaleAlpha => buf[..w * h * 2].chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => return None,
    };
    Some(PatternTile { w, h, data })
}

/// An imported tile from its data URL, decoded once and kept in a small
/// cache (`patternTex`, 16 entries). `None` for an undecodable image: the
/// layer stays empty, as in happypaint.
pub fn imported_tile(url: &str) -> Option<std::sync::Arc<PatternTile>> {
    use std::sync::{Arc, Mutex, OnceLock};
    type Entries = Vec<(u64, Option<Arc<PatternTile>>)>;
    static CACHE: OnceLock<Mutex<Entries>> = OnceLock::new();
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        url.hash(&mut h);
        h.finish()
    };
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(c) = cache.lock()
        && let Some((_, t)) = c.iter().find(|(k, _)| *k == key)
    {
        return t.clone();
    }
    let tile = decode_png_url(url).map(Arc::new);
    if let Ok(mut c) = cache.lock() {
        c.push((key, tile.clone()));
        if c.len() > 16 {
            c.remove(0);
        }
    }
    tile
}
