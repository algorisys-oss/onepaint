//! Brush tips and paper grain. Image tips and grains are happypaint's own
//! generators, baked to PNG by `scripts/export-brushes`.

use crate::assets::{COLOR_TIPS, GRAINS, TIPS};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// A square coverage mask, 0..1, row-major.
#[derive(Debug, PartialEq)]
pub struct Mask {
    pub size: usize,
    pub data: Vec<f32>,
}

impl Mask {
    /// A square mask from 8-bit coverage (255 = full).
    pub fn from_u8(size: usize, data: &[u8]) -> Self {
        Self { size, data: data.iter().map(|&v| v as f32 / 255.0).collect() }
    }

    /// 8-bit coverage, rounded.
    pub fn to_u8(&self) -> Vec<u8> {
        self.data.iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect()
    }

    /// Bilinear, clamp-to-edge sample at uv in 0..1.
    pub fn sample(&self, u: f64, v: f64) -> f32 {
        let n = self.size;
        let x = (u * n as f64 - 0.5).clamp(0.0, (n - 1) as f64);
        let y = (v * n as f64 - 0.5).clamp(0.0, (n - 1) as f64);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
        let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
        let at = |xx: usize, yy: usize| self.data[yy * n + xx];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bot = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        top + (bot - top) * fy
    }

    /// Bilinear with wrap-around (tileable grain).
    pub fn sample_repeat(&self, u: f64, v: f64) -> f32 {
        let n = self.size as f64;
        let x = (u * n - 0.5).rem_euclid(n);
        let y = (v * n - 0.5).rem_euclid(n);
        let (x0, y0) = (x as usize % self.size, y as usize % self.size);
        let (x1, y1) = ((x0 + 1) % self.size, (y0 + 1) % self.size);
        let (fx, fy) = ((x - x.floor()) as f32, (y - y.floor()) as f32);
        let at = |xx: usize, yy: usize| self.data[yy * self.size + xx];
        let top = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let bot = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        top + (bot - top) * fy
    }
}

fn decode(bytes: &[u8]) -> Option<Mask> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let n = info.width as usize;
    let ch = info.color_type.samples();
    Some(Mask { size: n, data: buf[..info.buffer_size()].chunks(ch).map(|p| p[0] as f32 / 255.0).collect() })
}

fn cache() -> &'static Mutex<HashMap<String, Arc<Mask>>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<Mask>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn cached(key: String, bytes: impl FnOnce() -> Option<&'static [u8]>) -> Option<Arc<Mask>> {
    if let Some(m) = cache().lock().ok()?.get(&key) {
        return Some(m.clone());
    }
    let m = Arc::new(decode(bytes()?)?);
    cache().lock().ok()?.insert(key, m.clone());
    Some(m)
}

/// How a dab's coverage is shaped (happypaint `BRUSH_TIPS`).
#[derive(Clone, Debug, PartialEq)]
pub enum Tip {
    Circle,
    Gauss,
    Square,
    Image(Arc<Mask>),
}

/// Unknown ids fall back to round, like happypaint's `getTip`. User tips
/// (`custom:…`) come from the user library.
pub fn tip_for(id: &str) -> Tip {
    match id {
        "round" => Tip::Circle,
        "soft" => Tip::Gauss,
        "square" => Tip::Square,
        _ if id.starts_with(crate::USER_TIP_PREFIX) => crate::user::user_tip(id).map_or(Tip::Circle, Tip::Image),
        _ => cached(format!("tip:{id}"), || TIPS.iter().find(|t| t.0 == id).map(|t| t.2)).map_or(Tip::Circle, Tip::Image),
    }
}

/// A colour (RGBA) tip's pixels: straight sRGB, plus the coverage-weighted
/// mean luma the tint formula is relative to (`brush-tips.ts:369-373`).
#[derive(Debug, PartialEq)]
pub struct ColorTip {
    pub size: usize,
    pub rgb: Vec<[f32; 3]>,
    pub lum: f32,
}

impl ColorTip {
    /// Bilinear, clamp-to-edge sample at uv in 0..1 (as `Mask::sample`).
    pub fn sample(&self, u: f64, v: f64) -> [f32; 3] {
        let n = self.size;
        let x = (u * n as f64 - 0.5).clamp(0.0, (n - 1) as f64);
        let y = (v * n as f64 - 0.5).clamp(0.0, (n - 1) as f64);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
        let (fx, fy) = ((x - x0 as f64) as f32, (y - y0 as f64) as f32);
        let at = |xx: usize, yy: usize| self.rgb[yy * n + xx];
        std::array::from_fn(|k| {
            let top = at(x0, y0)[k] + (at(x1, y0)[k] - at(x0, y0)[k]) * fx;
            let bot = at(x0, y1)[k] + (at(x1, y1)[k] - at(x0, y1)[k]) * fx;
            top + (bot - top) * fy
        })
    }
}

/// The colour data of a built-in colour tip (`oil-daub`, `impasto-daub`).
pub fn color_tip_for(id: &str) -> Option<Arc<ColorTip>> {
    static C: OnceLock<Mutex<HashMap<String, Arc<ColorTip>>>> = OnceLock::new();
    let cache = C.get_or_init(Default::default);
    if let Some(t) = cache.lock().ok()?.get(id) {
        return Some(t.clone());
    }
    let (_, lum, bytes) = COLOR_TIPS.iter().find(|t| t.0 == id)?;
    let mut dec = png::Decoder::new(std::io::Cursor::new(*bytes));
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = dec.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let ch = info.color_type.samples();
    let rgb = buf[..info.buffer_size()].chunks(ch).map(|p| [p[0], p[1], p[2]].map(|v| v as f32 / 255.0)).collect();
    let t = Arc::new(ColorTip { size: info.width as usize, rgb, lum: *lum });
    cache.lock().ok()?.insert(id.to_string(), t.clone());
    Some(t)
}

/// A tileable grayscale paper grain, built in or from the user library.
pub fn grain_for(id: &str) -> Option<Arc<Mask>> {
    if id.starts_with(crate::USER_GRAIN_PREFIX) {
        return crate::user::user_grain(id);
    }
    cached(format!("grain:{id}"), || GRAINS.iter().find(|g| g.0 == id).map(|g| g.3))
}

/// Built-in tip ids for the UI: the procedural tips, then the image tips.
pub fn tip_ids() -> Vec<&'static str> {
    ["round", "soft", "square"].into_iter().chain(TIPS.iter().map(|t| t.0)).collect()
}

/// Grain names for the UI.
pub fn grain_names() -> Vec<(&'static str, &'static str)> {
    GRAINS.iter().map(|g| (g.0, g.1)).collect()
}

/// Per-stroke grain parameters: canvas-fixed texture, offset and rotation.
pub struct GrainSampler {
    tex: Arc<Mask>,
    scale: f64,
    strength: f32,
    off: [f64; 2],
    rot: (f64, f64),
}

impl GrainSampler {
    pub fn new(g: &crate::Grain, off: [f64; 2], rot: f64) -> Option<Self> {
        Some(Self { tex: grain_for(&g.id)?, scale: g.scale.max(1.0), strength: g.strength as f32, off, rot: (rot.cos(), rot.sin()) })
    }

    /// Coverage multiplier at doc px (x, y): `max(smoothstep(s−.25, s+.05, g), .12)`.
    #[inline]
    pub fn factor(&self, x: f64, y: f64) -> f32 {
        let (gx, gy) = (x - self.off[0], y - self.off[1]);
        let (c, s) = self.rot;
        let (rx, ry) = (c * gx - s * gy, s * gx + c * gy);
        let g = self.tex.sample_repeat(rx / self.scale, ry / self.scale);
        let (e0, e1) = (self.strength - 0.25, self.strength + 0.05);
        let t = ((g - e0) / (e1 - e0)).clamp(0.0, 1.0);
        (t * t * (3.0 - 2.0 * t)).max(0.12)
    }
}
