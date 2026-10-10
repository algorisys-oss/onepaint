//! Text layers: happypaint's `TextSpec` rendered with cosmic-text (spec 14).
//!
//! A spec is laid out in its own block (top-left at `x, y`, rotated about that
//! point), drawn as background → outline → fill → underline into a local
//! premultiplied-linear buffer, then resampled into the document.

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, LetterSpacing, Metrics, Shaping, Style, SwashCache, Weight, Wrap};
use onepaint_color::Rgba;
use onepaint_raster::{Rect, Surface};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

pub mod balloon;
pub mod runs;

use balloon::{Balloon, balloon_extra_local_points, balloon_pad, balloon_paths, spec_font_size};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// A rich-text run; unset fields fall back to the spec (TR:19-30).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRun {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size_px: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
}

fn d_family() -> String {
    "system-ui, sans-serif".into()
}
fn d_size() -> f64 {
    32.0
}
fn d_color() -> String {
    "#000000".into()
}
fn d_line() -> f64 {
    1.2
}

/// happypaint `TextSpec` (TR:62-108); unknown fields are kept in `extra`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextSpec {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<Vec<TextRun>>,
    #[serde(default = "d_family")]
    pub font_family: String,
    #[serde(default = "d_size")]
    pub font_size_px: f64,
    #[serde(default = "d_color")]
    pub color: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_color: Option<String>,
    /// Speech balloon behind the block (spec 28).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balloon: Option<Balloon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(default)]
    pub align: Align,
    #[serde(default = "d_line")]
    pub line_height: f64,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub letter_spacing: f64,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_width: Option<f64>,
    #[serde(default)]
    pub rotation: f64,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for TextSpec {
    fn default() -> Self {
        Self {
            text: String::new(),
            runs: None,
            font_family: d_family(),
            font_size_px: d_size(),
            color: d_color(),
            bg_color: None,
            balloon: None,
            outline_width: None,
            outline_color: None,
            underline: None,
            align: Align::Left,
            line_height: d_line(),
            bold: false,
            italic: false,
            letter_spacing: 0.0,
            x: 0.0,
            y: 0.0,
            box_width: None,
            rotation: 0.0,
            extra: serde_json::Map::new(),
        }
    }
}

/// Built-in font stacks (happypaint `state/text-store.ts:42-52`).
pub const FONT_STACKS: [(&str, &str); 7] = [
    ("Sans", "system-ui, sans-serif"),
    ("Serif", "Georgia, \"Times New Roman\", serif"),
    ("Mono", "\"SFMono-Regular\", Menlo, Consolas, monospace"),
    ("Rounded", "\"Segoe UI\", \"Helvetica Neue\", Arial, sans-serif"),
    ("Condensed", "\"Arial Narrow\", \"Roboto Condensed\", sans-serif"),
    ("Slab", "\"Rockwell\", \"Roboto Slab\", Georgia, serif"),
    ("Cursive", "\"Dancing Script\", \"Brush Script MT\", cursive"),
];

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
}

fn fonts() -> &'static Mutex<Fonts> {
    static F: OnceLock<Mutex<Fonts>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(Fonts { system: FontSystem::new(), cache: SwashCache::new() }))
}

/// Pick the first installed family of a CSS stack.
fn family_name(system: &FontSystem, stack: &str) -> Option<String> {
    for raw in stack.split(',') {
        let name = raw.trim().trim_matches(|c| c == '"' || c == '\'');
        match name.to_ascii_lowercase().as_str() {
            "sans-serif" | "system-ui" | "-apple-system" | "serif" | "monospace" | "cursive" | "fantasy" => {
                return Some(name.to_ascii_lowercase());
            }
            _ => {}
        }
        if system.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))) {
            return Some(name.to_string());
        }
    }
    None
}

fn to_family(name: &str) -> Family<'_> {
    match name {
        "sans-serif" | "system-ui" | "-apple-system" => Family::SansSerif,
        "serif" => Family::Serif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        other => Family::Name(other),
    }
}

fn hex(c: &str) -> Rgba {
    Rgba::from_hex(&runs::normalize_color(c)).unwrap_or(Rgba::BLACK)
}

/// Local premultiplied-linear canvas for one spec.
struct Local {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Local {
    fn over(&mut self, x: i64, y: i64, c: [f32; 3], a: f32) {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h || a <= 0.0 {
            return;
        }
        let p = &mut self.px[y as usize * self.w + x as usize];
        let k = 1.0 - a;
        *p = [c[0] * a + p[0] * k, c[1] * a + p[1] * k, c[2] * a + p[2] * k, a + p[3] * k];
    }

    fn sample(&self, x: f64, y: f64) -> [f32; 4] {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
        let (tx, ty) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let at = |xx: i64, yy: i64| {
            if xx < 0 || yy < 0 || xx as usize >= self.w || yy as usize >= self.h {
                [0.0; 4]
            } else {
                self.px[yy as usize * self.w + xx as usize]
            }
        };
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        [0, 1, 2, 3].map(|i| {
            let top = a[i] + (b[i] - a[i]) * tx;
            let bot = c[i] + (d[i] - c[i]) * tx;
            top + (bot - top) * ty
        })
    }
}

/// A laid-out spec: its local canvas, where the block origin sits on it, and
/// the block size (doc px).
struct Laid {
    local: Local,
    ox: f64,
    oy: f64,
    block_w: f64,
    block_h: f64,
}

/// A block-space rect `[x0, x1) × [y0, y1)`.
#[derive(Clone, Copy)]
struct Bar {
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

/// Fill a decoration bar into the glyph layer and its coverage.
fn deco_rect(fill: &mut Local, cover: &mut [f32], (ox, oy): (i64, i64), b: Bar, c: [f32; 3]) {
    for yy in b.y0.floor() as i64..b.y1.ceil() as i64 {
        for xx in b.x0.floor() as i64..b.x1.ceil() as i64 {
            let (px, py) = (xx + ox, yy + oy);
            fill.over(px, py, c, 1.0);
            if px >= 0 && py >= 0 && (px as usize) < fill.w && (py as usize) < fill.h {
                cover[py as usize * fill.w + px as usize] = 1.0;
            }
        }
    }
}

/// Shared text renderer (system fonts are loaded once per process).
#[derive(Clone, Copy, Default)]
pub struct Renderer {
    /// The document's space: text colours enter as its pixel values (spec 50).
    space: onepaint_color::Space,
}

impl Renderer {
    pub fn new() -> Self {
        Renderer::default()
    }

    /// Render for a document in `space`.
    pub fn in_space(space: onepaint_color::Space) -> Self {
        Renderer { space }
    }

    fn lin(&self, c: Rgba) -> [f32; 3] {
        [c.r, c.g, c.b].map(|v| self.space.from_srgb(v))
    }

    /// Render all specs (later on top) into a `w × h` document surface.
    pub fn render(&self, specs: &[TextSpec], w: u32, h: u32) -> Surface {
        let mut out = Surface::new();
        let doc = Rect::new(0, 0, w as i32, h as i32);
        for s in specs {
            let Some(Laid { local, ox, oy, .. }) = self.layout(s) else { continue };
            let (c, sn) = (s.rotation.cos(), s.rotation.sin());
            // Doc-space bounds of the rotated local canvas.
            let corners = [[0.0, 0.0], [local.w as f64, 0.0], [0.0, local.h as f64], [local.w as f64, local.h as f64]].map(|p| {
                let (lx, ly) = (p[0] - ox, p[1] - oy);
                [s.x + lx * c - ly * sn, s.y + lx * sn + ly * c]
            });
            let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in corners {
                (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
            }
            let Some(r) = Rect::covering(x0 as f32, y0 as f32, x1 as f32, y1 as f32).intersect(&doc) else { continue };
            let mut dst = out.read_rect(r);
            for yy in 0..r.h {
                for xx in 0..r.w {
                    let (dx, dy) = ((r.x + xx) as f64 + 0.5 - s.x, (r.y + yy) as f64 + 0.5 - s.y);
                    let (lx, ly) = (dx * c + dy * sn + ox, -dx * sn + dy * c + oy);
                    let p = local.sample(lx, ly);
                    if p[3] <= 0.0 {
                        continue;
                    }
                    let i = ((yy * r.w + xx) * 4) as usize;
                    let k = 1.0 - p[3];
                    for ch in 0..4 {
                        dst[i + ch] = p[ch] + dst[i + ch] * k;
                    }
                }
            }
            out.write_rect(r, &dst);
        }
        out.prune_empty();
        out
    }

    /// Lay out and draw one spec into its local canvas.
    fn layout(&self, s: &TextSpec) -> Option<Laid> {
        let rich = s.runs.as_ref().is_some_and(|r| !r.is_empty());
        let runs: Vec<TextRun> = match &s.runs {
            Some(r) if !r.is_empty() => r.clone(),
            _ => vec![TextRun { text: s.text.clone(), ..Default::default() }],
        };
        if runs.iter().all(|r| r.text.is_empty()) {
            return None;
        }
        let size = s.font_size_px.max(1.0) as f32;
        let line_h = size * s.line_height.max(0.1) as f32;
        let mut guard = fonts().lock().ok()?;
        let Fonts { system, cache } = &mut *guard;
        let base_family = family_name(system, &s.font_family).unwrap_or_else(|| "sans-serif".into());
        let run_families: Vec<String> = runs
            .iter()
            .map(|r| r.font_family.as_deref().and_then(|f| family_name(system, f)).unwrap_or_else(|| base_family.clone()))
            .collect();
        let mut buffer = Buffer::new(system, Metrics::new(size, line_h));
        buffer.set_wrap(if s.box_width.is_some_and(|w| w > 0.0) { Wrap::Word } else { Wrap::None });
        buffer.set_size(s.box_width.filter(|w| *w > 0.0).map(|w| w as f32), None);
        let spacing = if s.letter_spacing != 0.0 { Some(LetterSpacing((s.letter_spacing / s.font_size_px.max(1.0)) as f32)) } else { None };
        let attrs_for = |i: usize| {
            let r = &runs[i];
            let bold = r.bold.unwrap_or(s.bold);
            let italic = r.italic.unwrap_or(s.italic);
            let c = hex(r.color.as_deref().unwrap_or(&s.color));
            let rs = r.font_size_px.unwrap_or(s.font_size_px).max(1.0) as f32;
            let mut a = Attrs::new()
                .family(to_family(&run_families[i]))
                .weight(if bold { Weight::BOLD } else { Weight::NORMAL })
                .style(if italic { Style::Italic } else { Style::Normal })
                .color(Color::rgba((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, 255))
                .metadata(i);
            if (rs - size).abs() > 1e-3 {
                a = a.metrics(Metrics::new(rs, rs * s.line_height.max(0.1) as f32));
            }
            a.letter_spacing_opt = spacing;
            a
        };
        let align = Some(match s.align {
            Align::Left => cosmic_text::Align::Left,
            Align::Center => cosmic_text::Align::Center,
            Align::Right => cosmic_text::Align::Right,
        });
        let spans: Vec<(&str, Attrs)> = runs.iter().enumerate().map(|(i, r)| (r.text.as_str(), attrs_for(i))).collect();
        buffer.set_rich_text(spans, &attrs_for(0), Shaping::Advanced, align);
        buffer.shape_until_scroll(system, false);
        let mut block_w: f32 = 0.0;
        let mut block_h: f32 = 0.0;
        let mut lines = Vec::new();
        // Per-run decoration spans: (x0, x1, baseline, run index).
        let mut frags: Vec<(f32, f32, f32, usize)> = Vec::new();
        for run in buffer.layout_runs() {
            block_w = block_w.max(run.line_w);
            block_h = block_h.max(run.line_top + run.line_height);
            let x0 = run.glyphs.first().map_or(0.0, |g| g.x);
            lines.push((run.line_top, run.line_height, x0, run.line_w));
            for g in run.glyphs {
                match frags.last_mut() {
                    Some(f) if f.3 == g.metadata && f.2 == run.line_y && (g.x - f.1).abs() < 0.5 => f.1 = g.x + g.w,
                    _ => frags.push((g.x, g.x + g.w, run.line_y, g.metadata)),
                }
            }
        }
        if let Some(bw) = s.box_width.filter(|w| *w > 0.0) {
            block_w = bw as f32;
        }
        let (bw, bh) = (block_w as f64, block_h as f64);
        let max_size = spec_font_size(s);
        let outline = s.outline_width.unwrap_or(0.0).max(0.0);
        let pad = (outline.max(max_size * 0.18) + 2.0).ceil();
        // Local extent in block coordinates: the padded block plus the balloon.
        let (mut lo, mut hi) = ([-pad, -pad], [bw + pad, bh + pad]);
        let paths = balloon_paths(s, bw, bh);
        let stroke_w = s.balloon.as_ref().map_or(0.0, Balloon::stroke_px);
        for p in paths.iter().flatten() {
            let m = stroke_w / 2.0 + 2.0;
            lo = [lo[0].min(p[0] - m), lo[1].min(p[1] - m)];
            hi = [hi[0].max(p[0] + m), hi[1].max(p[1] + m)];
        }
        let (ox, oy) = ((-lo[0]).ceil(), (-lo[1]).ceil());
        let (w, h) = ((ox + hi[0]).ceil() as usize + 1, (oy + hi[1]).ceil() as usize + 1);
        let mut fill = Local { w, h, px: vec![[0.0; 4]; w * h] };
        let mut cover = vec![0.0f32; w * h];
        let off = (ox as i64, oy as i64);
        buffer.draw(system, cache, Color::rgb(0, 0, 0), |x, y, gw, gh, color| {
            let a = color.a() as f32 / 255.0;
            if a <= 0.0 {
                return;
            }
            let c = self.lin(Rgba::new(color.r() as f32 / 255.0, color.g() as f32 / 255.0, color.b() as f32 / 255.0, 1.0));
            for yy in 0..gh as i64 {
                for xx in 0..gw as i64 {
                    let (px, py) = (x as i64 + xx + off.0, y as i64 + yy + off.1);
                    fill.over(px, py, c, a);
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                        let i = py as usize * w + px as usize;
                        cover[i] = cover[i].max(a);
                    }
                }
            }
        });
        if rich {
            // Per-fragment decorations in the run's colour (TR:319-323).
            for &(x0, x1, base, i) in &frags {
                let Some(r) = runs.get(i) else { continue };
                let rs = r.font_size_px.unwrap_or(s.font_size_px).max(1.0) as f32;
                let th = (rs * 0.06).max(1.0);
                let c = self.lin(hex(r.color.as_deref().unwrap_or(&s.color)));
                if r.underline == Some(true) {
                    let y0 = base + rs * 0.10;
                    deco_rect(&mut fill, &mut cover, off, Bar { x0, x1, y0, y1: y0 + th }, c);
                }
                if r.strikethrough == Some(true) {
                    let y0 = base - rs * 0.28;
                    deco_rect(&mut fill, &mut cover, off, Bar { x0, x1, y0, y1: y0 + th }, c);
                }
            }
        } else if s.underline.unwrap_or(false) {
            let fc = self.lin(hex(&s.color));
            for &(top, lh, x0, lw) in &lines {
                let half = (lh - size) / 2.0;
                let y0 = top + half + size * 0.92;
                let th = (size * 0.06).max(1.0);
                deco_rect(&mut fill, &mut cover, off, Bar { x0, x1: x0 + lw, y0, y1: y0 + th }, fc);
            }
        }
        // Compose: background, balloon, then the outline under the fill.
        let mut local = Local { w, h, px: vec![[0.0; 4]; w * h] };
        if let Some(bg) = s.bg_color.as_deref() {
            let c = self.lin(hex(bg));
            let bp = max_size * 0.18;
            let (bx0, by0) = ((ox - bp).floor() as i64, (oy - bp).floor() as i64);
            let (bx1, by1) = ((ox + bw + bp).ceil() as i64, (oy + bh + bp).ceil() as i64);
            for yy in by0..by1 {
                for xx in bx0..bx1 {
                    local.over(xx, yy, c, 1.0);
                }
            }
        }
        if let Some(b) = &s.balloon {
            let cov = balloon::coverage(&paths, stroke_w, w, h, ox, oy);
            let fc = self.lin(hex(&b.fill));
            let sc = b.stroke.as_deref().map(|c| self.lin(hex(c)));
            for i in 0..w * h {
                let (x, y) = ((i % w) as i64, (i / w) as i64);
                local.over(x, y, fc, cov.fill[i]);
                if let Some(sc) = sc {
                    local.over(x, y, sc, cov.stroke[i]);
                }
            }
        }
        if outline > 0.0 {
            let oc = self.lin(hex(s.outline_color.as_deref().unwrap_or("#000000")));
            let r = outline.ceil() as i64;
            let r2 = outline * outline;
            let mut dil = vec![0.0f32; w * h];
            for y in 0..h as i64 {
                for x in 0..w as i64 {
                    let mut m = 0.0f32;
                    for dy in -r..=r {
                        for dx in -r..=r {
                            if (dx * dx + dy * dy) as f64 > r2 {
                                continue;
                            }
                            let (xx, yy) = (x + dx, y + dy);
                            if xx >= 0 && yy >= 0 && (xx as usize) < w && (yy as usize) < h {
                                m = m.max(cover[yy as usize * w + xx as usize]);
                            }
                        }
                    }
                    dil[y as usize * w + x as usize] = m;
                }
            }
            for (i, a) in dil.iter().enumerate() {
                local.over((i % w) as i64, (i / w) as i64, oc, *a);
            }
        }
        for (i, f) in fill.px.iter().enumerate() {
            if f[3] > 0.0 {
                let d = &mut local.px[i];
                let k = 1.0 - f[3];
                *d = [f[0] + d[0] * k, f[1] + d[1] * k, f[2] + d[2] * k, f[3] + d[3] * k];
            }
        }
        Some(Laid { local, ox, oy, block_w: bw, block_h: bh })
    }

    /// The rotated box (doc px) a spec occupies, for hit-testing and handles:
    /// corners TL, TR, BR, BL. With a balloon it grows to the balloon pad and
    /// the tail (TR:681-725); otherwise it hugs the block, even with `bgColor`.
    pub fn quad(&self, s: &TextSpec) -> Option<[[f64; 2]; 4]> {
        let Laid { block_w: bw, block_h: bh, .. } = self.layout(s)?;
        let (mut lo, mut hi) = ([0.0, 0.0], [bw, bh]);
        if s.balloon.is_some() {
            let pad = balloon_pad(s, spec_font_size(s));
            (lo, hi) = ([-pad, -pad], [bw + pad, bh + pad]);
            for p in balloon_extra_local_points(s) {
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
        }
        let (c, sn) = (s.rotation.cos(), s.rotation.sin());
        Some(
            [[lo[0], lo[1]], [hi[0], lo[1]], [hi[0], hi[1]], [lo[0], hi[1]]]
                .map(|p| [s.x + p[0] * c - p[1] * sn, s.y + p[0] * sn + p[1] * c]),
        )
    }
}

/// Point-in-quad test with `slack` px around the edges.
pub fn quad_contains(q: &[[f64; 2]; 4], p: [f64; 2], slack: f64) -> bool {
    let (ux, uy) = (q[1][0] - q[0][0], q[1][1] - q[0][1]);
    let (vx, vy) = (q[3][0] - q[0][0], q[3][1] - q[0][1]);
    let (lu, lv) = (ux.hypot(uy).max(1e-9), vx.hypot(vy).max(1e-9));
    let (dx, dy) = (p[0] - q[0][0], p[1] - q[0][1]);
    let a = (dx * ux + dy * uy) / lu;
    let b = (dx * vx + dy * vy) / lv;
    a >= -slack && a <= lu + slack && b >= -slack && b <= lv + slack
}
