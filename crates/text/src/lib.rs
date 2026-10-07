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

/// happypaint `TextSpec` (TR:62-108); unknown fields (balloons…) are kept.
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
    Rgba::from_hex(c).unwrap_or(Rgba::BLACK)
}

fn lin(c: Rgba) -> [f32; 3] {
    [c.r, c.g, c.b].map(onepaint_color::srgb_to_linear)
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

/// Shared text renderer (system fonts are loaded once per process).
#[derive(Clone, Copy, Default)]
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Renderer
    }

    /// Render all specs (later on top) into a `w × h` document surface.
    pub fn render(&self, specs: &[TextSpec], w: u32, h: u32) -> Surface {
        let mut out = Surface::new();
        let doc = Rect::new(0, 0, w as i32, h as i32);
        for s in specs {
            let Some((local, pad)) = self.layout(s) else { continue };
            let (c, sn) = (s.rotation.cos(), s.rotation.sin());
            // Doc-space bounds of the rotated local canvas.
            let corners = [[0.0, 0.0], [local.w as f64, 0.0], [0.0, local.h as f64], [local.w as f64, local.h as f64]].map(|p| {
                let (lx, ly) = (p[0] - pad, p[1] - pad);
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
                    let (lx, ly) = (dx * c + dy * sn + pad, -dx * sn + dy * c + pad);
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

    /// Lay out and draw one spec into its local canvas; returns it and its padding.
    fn layout(&self, s: &TextSpec) -> Option<(Local, f64)> {
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
        for run in buffer.layout_runs() {
            block_w = block_w.max(run.line_w);
            block_h = block_h.max(run.line_top + run.line_height);
            let x0 = run.glyphs.first().map_or(0.0, |g| g.x);
            lines.push((run.line_top, run.line_height, x0, run.line_w));
        }
        if let Some(bw) = s.box_width.filter(|w| *w > 0.0) {
            block_w = bw as f32;
        }
        let outline = s.outline_width.unwrap_or(0.0).max(0.0);
        let pad = (outline.max(s.font_size_px * 0.18) + 2.0).ceil();
        let (w, h) = ((block_w as f64 + 2.0 * pad).ceil() as usize + 1, (block_h as f64 + 2.0 * pad).ceil() as usize + 1);
        let mut fill = Local { w, h, px: vec![[0.0; 4]; w * h] };
        let mut cover = vec![0.0f32; w * h];
        let p = pad as i64;
        buffer.draw(system, cache, Color::rgb(0, 0, 0), |x, y, gw, gh, color| {
            let a = color.a() as f32 / 255.0;
            if a <= 0.0 {
                return;
            }
            let c = lin(Rgba::new(color.r() as f32 / 255.0, color.g() as f32 / 255.0, color.b() as f32 / 255.0, 1.0));
            for yy in 0..gh as i64 {
                for xx in 0..gw as i64 {
                    let (px, py) = (x as i64 + xx + p, y as i64 + yy + p);
                    fill.over(px, py, c, a);
                    if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                        let i = py as usize * w + px as usize;
                        cover[i] = cover[i].max(a);
                    }
                }
            }
        });
        // Underline (object-level; runs' underline handled the same way when set).
        let underline = s.underline.unwrap_or(false) || runs.iter().any(|r| r.underline == Some(true));
        if underline {
            let fc = lin(hex(&s.color));
            for &(top, lh, x0, lw) in &lines {
                let half = (lh - size) / 2.0;
                let uy = top + half + size * 0.92;
                let th = (size * 0.06).max(1.0);
                for yy in uy as i64..(uy + th).ceil() as i64 {
                    for xx in x0 as i64..(x0 + lw).ceil() as i64 {
                        fill.over(xx + p, yy + p, fc, 1.0);
                        let (px, py) = (xx + p, yy + p);
                        if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                            cover[py as usize * w + px as usize] = 1.0;
                        }
                    }
                }
            }
        }
        // Compose: background, then the outline under the fill.
        let mut local = Local { w, h, px: vec![[0.0; 4]; w * h] };
        if let Some(bg) = s.bg_color.as_deref() {
            let c = lin(hex(bg));
            let bp = s.font_size_px * 0.18;
            let (bx0, by0) = ((pad - bp).floor() as i64, (pad - bp).floor() as i64);
            let (bx1, by1) = ((pad + block_w as f64 + bp).ceil() as i64, (pad + block_h as f64 + bp).ceil() as i64);
            for yy in by0..by1 {
                for xx in bx0..bx1 {
                    local.over(xx, yy, c, 1.0);
                }
            }
        }
        if outline > 0.0 {
            let oc = lin(hex(s.outline_color.as_deref().unwrap_or("#000000")));
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
        Some((local, pad))
    }

    /// The rotated block (doc px) a spec occupies, for hit-testing and handles:
    /// corners TL, TR, BR, BL.
    pub fn quad(&self, s: &TextSpec) -> Option<[[f64; 2]; 4]> {
        let (local, pad) = self.layout(s)?;
        let (bw, bh) = (local.w as f64 - 2.0 * pad - 1.0, local.h as f64 - 2.0 * pad - 1.0);
        let (c, sn) = (s.rotation.cos(), s.rotation.sin());
        Some([[0.0, 0.0], [bw, 0.0], [bw, bh], [0.0, bh]].map(|p| [s.x + p[0] * c - p[1] * sn, s.y + p[0] * sn + p[1] * c]))
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
