//! Seeded random documents for the compositor oracle tests (CPU program
//! interpreter and GPU pass against `composite_tile`). Not for app use.

use crate::{GHOST_TINT_KEY, StrokeMode};
use onepaint_doc::{BlendMode, Document, Group, Knockout, Layer, Node};
use onepaint_raster::{Rect, Surface};

/// A small xorshift generator: deterministic, no dependency.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// Uniform in 0..1.
    pub fn f(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        (self.next_u64() % n as u64) as u32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.f() < p
    }
}

/// A random premultiplied pixel; alpha is often 0 or 1 to hit the edge cases.
fn pixel(r: &mut Rng) -> [f32; 4] {
    let a = match r.below(4) {
        0 => 1.0,
        1 => 0.0,
        _ => r.f(),
    };
    [r.f() * a, r.f() * a, r.f() * a, a]
}

fn rect(r: &mut Rng, w: u32, h: u32) -> Rect {
    let (x0, y0) = (r.below(w) as i32, r.below(h) as i32);
    let (x1, y1) = (r.below(w) as i32, r.below(h) as i32);
    Rect::new(x0.min(x1), y0.min(y1), (x0 - x1).abs() + 1, (y0 - y1).abs() + 1)
}

/// Fill a rect with per-pixel noise.
pub fn noise(s: &mut Surface, r: &mut Rng, area: Rect) {
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            s.set(x, y, pixel(r));
        }
    }
}

/// A layer mode (Pigment if asked); now and then Pass Through, which a
/// layer reads as Normal.
fn mode(r: &mut Rng, pigment: bool) -> BlendMode {
    if pigment && r.chance(0.05) {
        return BlendMode::Pigment;
    }
    if r.chance(0.03) {
        return BlendMode::PassThrough;
    }
    BlendMode::LAYER_MODES[r.below(BlendMode::LAYER_MODES.len() as u32) as usize]
}

fn layer(r: &mut Rng, id: &mut u32, w: u32, h: u32, pigment: bool) -> Layer {
    *id += 1;
    let mut l = Layer::new(&format!("l{id}"));
    l.id = format!("l{id}");
    for _ in 0..1 + r.below(2) {
        let a = rect(r, w, h);
        noise(&mut l.pixels, r, a);
    }
    l.blend_mode = if r.chance(0.4) { BlendMode::Normal } else { mode(r, pigment) };
    l.opacity = if r.chance(0.5) { 1.0 } else { r.f() };
    l.visible = !r.chance(0.08);
    l.clipped = r.chance(0.25);
    l.inherit_alpha = r.chance(0.12);
    if r.chance(0.3) {
        let mut m = Surface::default();
        let a = rect(r, w, h);
        noise(&mut m, r, a);
        l.mask = Some(m);
        l.mask_enabled = !r.chance(0.2);
    }
    l
}

fn nodes(r: &mut Rng, id: &mut u32, depth: u32, w: u32, h: u32, pigment: bool) -> Vec<Node> {
    let n = 1 + r.below(if depth == 0 { 6 } else { 4 });
    (0..n)
        .map(|_| {
            if depth < 3 && r.chance(0.25) {
                *id += 1;
                let mut g = Group::new(&format!("g{id}"));
                g.id = format!("g{id}");
                g.children = nodes(r, id, depth + 1, w, h, pigment);
                g.blend_mode = match r.below(4) {
                    0 | 1 => BlendMode::Normal,
                    2 => BlendMode::PassThrough,
                    _ => mode(r, pigment),
                };
                g.opacity = if r.chance(0.5) { 1.0 } else { r.f() };
                g.visible = !r.chance(0.08);
                if r.chance(0.15) {
                    g.extra.insert(GHOST_TINT_KEY.into(), serde_json::json!([r.f(), r.f(), r.f()]));
                }
                Node::Group(g)
            } else {
                Node::Layer(layer(r, id, w, h, pigment))
            }
        })
        .collect()
}

/// A random document `w × h` mixing every blend mode (Pigment if asked),
/// groups (Pass Through ones too), clip families, masks, inherit alpha,
/// ghost tints and knockouts (some over a locked Background).
pub fn random_doc(seed: u64, w: u32, h: u32, pigment: bool) -> Document {
    let mut r = Rng::new(seed);
    let mut d = Document::new(w, h, None);
    let mut id = 0;
    d.root = nodes(&mut r, &mut id, 0, w, h, pigment);
    // Their own stream, so the rest of the document is as it was.
    let mut k = Rng::new(seed ^ 0x6b6e_6f63);
    knockouts(&mut k, &mut d.root);
    if let Some(Node::Layer(l)) = d.root.first_mut() {
        l.locked = k.chance(0.5);
    }
    looks(&mut Rng::new(seed ^ 0x6c6f_6f6b), &mut d.root, false);
    d
}

/// Mask density and Blend If on some layers (only adjustment layers when
/// `adjust_only`).
fn looks(r: &mut Rng, nodes: &mut [Node], adjust_only: bool) {
    for n in nodes {
        match n {
            Node::Layer(l) if !adjust_only || l.adjustment.is_some() => {
                if l.mask.is_some() && r.chance(0.4) {
                    l.set_mask_density(r.f());
                }
                if l.mask.is_some() && r.chance(0.3) {
                    l.set_mask_feather(0.25 + 12.0 * r.f());
                }
                if r.chance(0.2) {
                    l.set_blend_if(Some(random_blend_if(r)));
                }
                if r.chance(0.15) {
                    l.set_vector_mask(Some(random_vector_mask(r)));
                }
            }
            Node::Layer(_) => {}
            Node::Group(g) => looks(r, &mut g.children, adjust_only),
        }
    }
}

/// One to three random paths (some curved) in random ops and rules, over a
/// 600 × 300 area so some tiles are missed, cut, or covered.
pub fn random_vector_mask(r: &mut Rng) -> onepaint_doc::VectorMask {
    use onepaint_doc::{FillRule, VectorOp, VectorPath};
    let ops = [VectorOp::Combine, VectorOp::Subtract, VectorOp::Intersect, VectorOp::Exclude];
    let paths = (0..1 + r.below(3))
        .map(|_| {
            let n = 3 + r.below(4) as usize;
            let (cx, cy, rad) = (r.f() as f64 * 600.0, r.f() as f64 * 300.0, 20.0 + r.f() as f64 * 200.0);
            let knots = (0..n)
                .map(|k| {
                    let a = k as f64 / n as f64 * std::f64::consts::TAU;
                    let (x, y) = (cx + rad * a.cos() * (0.5 + r.f() as f64), cy + rad * a.sin() * (0.5 + r.f() as f64));
                    let (hx, hy) = if r.chance(0.5) { ((r.f() as f64 - 0.5) * 80.0, (r.f() as f64 - 0.5) * 80.0) } else { (0.0, 0.0) };
                    [x - hx, y - hy, x, y, x + hx, y + hy]
                })
                .collect();
            VectorPath {
                open: r.chance(0.1),
                op: ops[r.below(4) as usize],
                rule: if r.chance(0.5) { FillRule::EvenOdd } else { FillRule::NonZero },
                knots,
            }
        })
        .collect();
    onepaint_doc::VectorMask { paths, invert: r.chance(0.2), disabled: r.chance(0.1), unlinked: false }
}

/// One or two channels with random (often split) sliders.
pub fn random_blend_if(r: &mut Rng) -> onepaint_doc::BlendIf {
    let mut b = onepaint_doc::BlendIf::default();
    for _ in 0..1 + r.below(2) {
        let mut v = [0u8; 4].map(|_| r.below(256) as u8);
        v.sort_unstable();
        let c = &mut b.0[r.below(4) as usize];
        if r.chance(0.5) { c.src = v } else { c.dst = v }
    }
    b
}

fn knockouts(r: &mut Rng, nodes: &mut [Node]) {
    for n in nodes {
        match n {
            Node::Layer(l) => {
                if r.chance(0.15) {
                    l.set_knockout(Some(if r.chance(0.5) { Knockout::Shallow } else { Knockout::Deep }));
                }
            }
            Node::Group(g) => knockouts(r, &mut g.children),
        }
    }
}

/// Every layer id in the tree.
pub fn layer_ids(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .flat_map(|n| match n {
            Node::Layer(l) => vec![l.id.clone()],
            Node::Group(g) => layer_ids(&g.children),
        })
        .collect()
}

/// A random stroke: buffer, target layer, opacity, mode and whether it paints the mask.
pub fn random_stroke(seed: u64, doc: &Document) -> Option<(Surface, String, f32, StrokeMode, bool)> {
    let mut r = Rng::new(seed ^ 0x5157);
    let ids = layer_ids(&doc.root);
    if ids.is_empty() {
        return None;
    }
    let id = ids[r.below(ids.len() as u32) as usize].clone();
    let mut s = Surface::default();
    let a = rect(&mut r, doc.width, doc.height);
    noise(&mut s, &mut r, a);
    let m = mode(&mut r, false);
    let mode = match r.below(3) {
        0 => StrokeMode::Paint(m),
        1 => StrokeMode::Erase,
        _ => StrokeMode::AlphaLock(m),
    };
    Some((s, id, r.f(), mode, r.chance(0.25)))
}

/// The colour adjustment kinds the GPU runs (spec 42), with random params.
pub fn random_adjustment(r: &mut Rng) -> onepaint_doc::Adjustment {
    use serde_json::json;
    let kinds = [
        "brightness-contrast",
        "hue-saturation",
        "black-white",
        "invert",
        "threshold",
        "posterize",
        "levels",
        "curves",
        "color-balance",
        "gradient-map",
        "color-to-alpha",
        "replace-color",
        "selective-color",
        "noise",
        "solid-fill",
        "gradient-fill",
        "tone",
        "pattern-fill",
        "exposure",
        "vibrance",
        "photo-filter",
        "channel-mixer",
        "cmyk-selective-color",
    ];
    let kind = kinds[r.below(kinds.len() as u32) as usize];
    let mut c = || [r.f(), r.f(), r.f()];
    let v = match kind {
        "brightness-contrast" => json!({"brightness": c()[0] - 0.5, "contrast": c()[1] - 0.5}),
        "hue-saturation" => json!({"hue": (c()[0] - 0.5) * 360.0, "saturation": c()[1] * 2.0 - 1.0, "brightness": c()[2] - 0.5}),
        "cmyk-selective-color" => {
            let sc: Vec<f32> = (0..36).map(|_| if r.chance(0.3) { r.f() * 2.0 - 1.0 } else { 0.0 }).collect();
            json!({"scCmyk": sc, "scMethod": if r.chance(0.5) { "absolute" } else { "relative" }})
        }
        "black-white" => {
            let (a, b, t) = (c(), c(), c());
            json!({
                "amount": a[0], "bwReds": a[1] * 5.0 - 2.0, "bwYellows": a[2] * 5.0 - 2.0,
                "bwGreens": b[0] * 5.0 - 2.0, "bwCyans": b[1] * 5.0 - 2.0, "bwBlues": b[2] * 5.0 - 2.0,
                "bwMagentas": t[0] * 5.0 - 2.0, "bwTint": t[1] > 0.5, "bwTintColor": c()
            })
        }
        "threshold" => json!({"threshold": c()[0]}),
        "posterize" => json!({"levels": 2.0 + (c()[0] * 10.0).floor()}),
        "levels" => {
            let k = c();
            json!({"inBlack": k[0] * 0.3, "inWhite": 0.7 + k[1] * 0.3, "gamma": 0.5 + k[2], "outBlack": 0.1, "outWhite": 0.9})
        }
        "curves" => {
            let k = c();
            json!({"curve": [{"x": 0.0, "y": 0.0}, {"x": 0.4, "y": k[0]}, {"x": 1.0, "y": 1.0}],
                   "curveR": [{"x": 0.0, "y": k[1] * 0.3}, {"x": 1.0, "y": 1.0}],
                   "curveMode": if k[2] < 0.5 { "linear" } else { "smooth" }})
        }
        "color-balance" => {
            let k = c();
            let (sh, hi, keep) = (c(), c(), c()[0] > 0.3);
            json!({
                "cyanRed": k[0] * 2.0 - 1.0, "magentaGreen": k[1] * 2.0 - 1.0, "yellowBlue": k[2] * 2.0 - 1.0,
                "cbShadowCR": sh[0] * 2.0 - 1.0, "cbShadowMG": sh[1] * 2.0 - 1.0, "cbShadowYB": sh[2] * 2.0 - 1.0,
                "cbHighCR": hi[0] * 2.0 - 1.0, "cbHighMG": hi[1] * 2.0 - 1.0, "cbHighYB": hi[2] * 2.0 - 1.0,
                "preserveLum": keep
            })
        }
        "gradient-map" => json!({"gradientStops": [{"pos": 0.0, "color": c()}, {"pos": 0.5, "color": c()}, {"pos": 1.0, "color": c()}]}),
        "color-to-alpha" => json!({"target": c(), "threshold": c()[0] * 0.5}),
        "replace-color" => json!({"target": c(), "replace": c(), "threshold": c()[0] * 0.6}),
        "selective-color" => {
            let samples: Vec<_> = (0..3)
                .map(|_| {
                    let k = c();
                    json!({"target": c(), "reach": k[0] * 0.3, "falloff": k[1] * 0.3, "dh": (k[2] - 0.5) * 120.0, "ds": k[0] - 0.5, "dv": k[1] - 0.5})
                })
                .collect();
            json!({"colorSamples": samples, "showRange": c()[0] < 0.2})
        }
        "noise" => json!({"amount": c()[0], "mono": c()[1] < 0.5}),
        "exposure" => {
            let k = c();
            json!({"exposure": (k[0] - 0.5) * 4.0, "expOffset": (k[1] - 0.5) * 0.4, "expGamma": 0.3 + k[2] * 2.0})
        }
        "vibrance" => json!({"vibrance": c()[0] * 2.0 - 1.0, "vibSaturation": c()[1] * 2.0 - 1.0}),
        "photo-filter" => json!({"filterColor": c(), "filterDensity": c()[0], "preserveLum": c()[1] < 0.6}),
        "channel-mixer" => {
            let mut m = serde_json::Map::new();
            for a in ["R", "G", "B", "K"] {
                for b in ["R", "G", "B", "C"] {
                    m.insert(format!("mix{a}{b}"), json!((c()[0] - 0.5) * 2.0));
                }
            }
            m.insert("mixMono".into(), json!(c()[0] < 0.3));
            serde_json::Value::Object(m)
        }
        "pattern-fill" => {
            let k = c();
            let ids = ["checker", "stripes", "dots", "grid", "brick", "crosshatch", "halftone", "weave"];
            json!({"patternId": ids[(k[0] * 8.0) as usize % 8], "patternColorA": c(), "patternColorB": c(),
                   "patternScale": 0.3 + k[1] * 2.0, "patternAngle": k[2] * 90.0, "patternOffX": k[0] * 0.3, "patternOffY": k[1] * 0.2})
        }
        "tone" => {
            let k = c();
            let shape = ["dot", "line", "cross"][(k[0] * 3.0) as usize % 3];
            let density = ["flat", "luminance", "gradient"][(k[1] * 3.0) as usize % 3];
            json!({"toneShape": shape, "toneDensity": density, "toneLevel": k[2], "toneLevelEnd": 1.0 - k[2],
                   "toneGradientAngle": k[0] * 360.0, "lpi": 20.0 + k[1] * 60.0, "angle": k[2] * 90.0,
                   "hardDots": k[0] < 0.2, "target": c()})
        }
        "solid-fill" => json!({"fillColor": c()}),
        "gradient-fill" => {
            let (k, s) = (c(), ["linear", "radial", "angle", "reflected", "diamond"]);
            let stop = |pos: f32, color: [f32; 3], opacity: f32| json!({"pos": pos, "color": color, "opacity": opacity});
            json!({"gradShape": s[(k[0] * 5.0) as usize % 5], "gradAX": k[1], "gradAY": k[2], "gradBX": 1.0 - k[2], "gradBY": k[1] * 0.5,
                   "gradReverse": k[0] < 0.3,
                   "fillStops": [stop(0.0, c(), 1.0), stop(0.4, c(), 0.6), stop(1.0, c(), 0.9)]})
        }
        _ => json!({}),
    };
    onepaint_doc::Adjustment { kind: kind.into(), params: serde_json::from_value(v).unwrap_or_default() }
}

/// `doc` with colour adjustment layers dropped into its top level and its
/// groups (spec 42), some masked, in random modes and opacities.
pub fn with_adjustments(mut doc: Document, seed: u64) -> Document {
    fn walk(nodes: &mut Vec<Node>, r: &mut Rng, id: &mut u32, w: u32, h: u32) {
        for n in nodes.iter_mut() {
            if let Node::Group(g) = n {
                walk(&mut g.children, r, id, w, h);
            }
        }
        for _ in 0..1 + r.below(2) {
            *id += 1;
            let mut l = Layer::new(&format!("adj{id}"));
            l.id = format!("adj{id}");
            l.adjustment = Some(Box::new(random_adjustment(r)));
            l.blend_mode = if r.chance(0.6) { BlendMode::Normal } else { mode(r, false) };
            // Hard Mix flips at b + s = 1, and an inverted backdrop sits exactly
            // there: rounding alone decides every pixel, on either side.
            if l.blend_mode == BlendMode::HardMix && l.adjustment.as_ref().is_some_and(|a| a.kind == "invert") {
                l.blend_mode = BlendMode::Normal;
            }
            // Color Dodge steps at s = 1 (white there, d / (1 − s) just under:
            // black over a black channel), and the channel mixer clamps bright
            // pixels to exactly 1, which a fused multiply-add can miss by an ulp.
            if l.blend_mode == BlendMode::ColorDodge && l.adjustment.as_ref().is_some_and(|a| a.kind == "channel-mixer") {
                l.blend_mode = BlendMode::Normal;
            }
            l.opacity = if r.chance(0.5) { 1.0 } else { r.f() };
            l.visible = !r.chance(0.1);
            if r.chance(0.3) {
                let mut m = Surface::default();
                let a = rect(r, w, h);
                noise(&mut m, r, a);
                l.mask = Some(m);
                l.mask_enabled = !r.chance(0.2);
            }
            let at = r.below(nodes.len() as u32 + 1) as usize;
            nodes.insert(at, Node::Layer(l));
        }
    }
    let mut r = Rng::new(seed ^ 0xad1);
    let (w, h) = (doc.width, doc.height);
    let mut id = 0;
    walk(&mut doc.root, &mut r, &mut id, w, h);
    looks(&mut Rng::new(seed ^ 0xad2), &mut doc.root, true);
    doc
}

/// `doc` with impasto height on about half its paint layers (spec 45),
/// reaching across tile edges and the document's edge.
pub fn with_relief(mut doc: Document, seed: u64) -> Document {
    fn walk(nodes: &mut [Node], r: &mut Rng, w: u32, h: u32) {
        for n in nodes {
            match n {
                Node::Group(g) => walk(&mut g.children, r, w, h),
                Node::Layer(l) if l.adjustment.is_none() && r.chance(0.5) => {
                    let mut s = Surface::default();
                    for _ in 0..2 {
                        let a = rect(r, w, h);
                        noise(&mut s, r, a);
                    }
                    l.height = Some(s);
                }
                Node::Layer(_) => {}
            }
        }
    }
    let mut r = Rng::new(seed ^ 0x4e1);
    let (w, h) = (doc.width, doc.height);
    walk(&mut doc.root, &mut r, w, h);
    doc
}

/// `doc` with a spatial adjustment layer of a kind with a GPU pass dropped
/// into its top level (spec 52).
pub fn with_spatial(doc: Document, seed: u64) -> Document {
    with_spatial_n(doc, seed, 1)
}

/// [`with_spatial`] with `n` spatial layers (stacked ones from 2).
pub fn with_spatial_n(mut doc: Document, seed: u64, n: usize) -> Document {
    use serde_json::json;
    let mut r = Rng::new(seed ^ 0x5ba7);
    for i in 0..n {
        let kinds =
            ["gaussian-blur", "motion-blur", "sharpen", "emboss", "mosaic", "ripple", "halftone", "crystallize", "line-art", "bloom"];
        let kind = kinds[r.below(kinds.len() as u32) as usize];
        let params = match kind {
            "gaussian-blur" => json!({"radius": 1.0 + r.f() * 40.0}),
            "motion-blur" => json!({"angle": r.f() * 360.0, "distance": 1.0 + r.f() * 40.0}),
            "emboss" => json!({"angle": r.f() * 360.0, "distance": 0.5 + r.f() * 10.0, "amount": r.f(), "mono": r.chance(0.5)}),
            "mosaic" => json!({"cellSize": 2.0 + r.f() * 40.0}),
            "ripple" => json!({"amplitude": r.f() * 30.0, "wavelength": 10.0 + r.f() * 100.0, "angle": r.f() * 360.0}),
            "halftone" => json!({"cellSize": 2.0 + r.f() * 20.0, "angle": r.f() * 90.0, "mono": r.chance(0.5)}),
            "crystallize" => json!({"cellSize": 2.0 + r.f() * 60.0}),
            "line-art" => {
                let mode = if r.chance(0.5) { "sobel" } else { "xdog" };
                json!({"lineMode": mode, "radius": 1.0 + r.f() * 4.0, "threshold": r.f() * 0.3, "amount": r.f(), "mono": r.chance(0.5)})
            }
            "bloom" => json!({"threshold": r.f(), "amount": r.f(), "radius": 1.0 + r.f() * 40.0}),
            _ => json!({"amount": r.f()}),
        };
        let mut l = Layer::new(&format!("spatial{i}"));
        l.id = format!("spatial{i}");
        l.adjustment =
            Some(Box::new(onepaint_doc::Adjustment { kind: kind.into(), params: serde_json::from_value(params).unwrap_or_default() }));
        l.opacity = if r.chance(0.6) { 1.0 } else { 0.3 + r.f() * 0.7 };
        l.blend_mode = if r.chance(0.7) { BlendMode::Normal } else { mode(&mut r, false) };
        if r.chance(0.3) {
            let mut m = Surface::default();
            let a = rect(&mut r, doc.width, doc.height);
            noise(&mut m, &mut r, a);
            l.mask = Some(m);
        }
        let at = r.below(doc.root.len() as u32 + 1) as usize;
        doc.root.insert(at, Node::Layer(l));
    }
    doc
}

/// [`with_spatial`] for whole-canvas kinds (spec 53): one twirl, pinch,
/// polar, chromatic aberration, radial, spin, tilt-shift, iris or field
/// blur layer.
pub fn with_whole_spatial(mut doc: Document, seed: u64) -> Document {
    use serde_json::json;
    let mut r = Rng::new(seed ^ 0x3b01);
    let kinds = [
        "twirl",
        "pinch",
        "polar-coordinates",
        "chromatic-aberration",
        "radial-blur",
        "spin-blur",
        "tilt-shift",
        "iris-blur",
        "field-blur",
    ];
    let bokeh = if r.chance(0.5) { r.f() } else { 0.0 };
    let kind = kinds[r.below(kinds.len() as u32) as usize];
    let (cx, cy) = (0.2 + r.f() * 0.6, 0.2 + r.f() * 0.6);
    let params = match kind {
        "twirl" => json!({"centerX": cx, "centerY": cy, "angle": r.f() * 720.0 - 360.0, "extent": 0.2 + r.f() * 1.2}),
        "pinch" => json!({"centerX": cx, "centerY": cy, "amount": r.f() * 2.0 - 1.0, "extent": 0.2 + r.f() * 1.2}),
        "polar-coordinates" => {
            let mode = if r.chance(0.5) { "to-polar" } else { "to-rect" };
            json!({"centerX": cx, "centerY": cy, "polarMode": mode})
        }
        "radial-blur" => {
            let mode = if r.chance(0.5) { "zoom" } else { "spin" };
            json!({"centerX": cx, "centerY": cy, "radialMode": mode, "amount": r.f()})
        }
        "spin-blur" => json!({
            "centerX": cx, "centerY": cy, "angle": r.f() * 360.0, "extent": 0.2 + r.f() * 0.8,
            "aspect": 0.4 + r.f() * 1.2, "focus": r.f() * 0.9, "spin": r.f() * 90.0
        }),
        "tilt-shift" => json!({
            "centerX": cx, "centerY": cy, "angle": r.f() * 180.0, "focusSize": r.f() * 0.3,
            "falloffSize": r.f() * 0.4, "radius": 1.0 + r.f() * 30.0, "bokeh": bokeh
        }),
        "iris-blur" => json!({
            "centerX": cx, "centerY": cy, "angle": r.f() * 180.0, "extent": 0.2 + r.f() * 0.8,
            "aspect": 0.3 + r.f(), "focus": r.f() * 0.9, "radius": 1.0 + r.f() * 30.0, "bokeh": bokeh
        }),
        "field-blur" => {
            let pins: Vec<_> = (0..1 + r.below(4)).map(|_| json!({"x": r.f(), "y": r.f(), "blur": r.f() * 30.0})).collect();
            json!({"pins": pins, "bokeh": bokeh, "bokehRange": 0.5 + r.f() * 0.4})
        }
        _ => json!({"amount": r.f() * 2.0}),
    };
    let mut l = Layer::new("whole");
    l.id = "whole".into();
    l.adjustment =
        Some(Box::new(onepaint_doc::Adjustment { kind: kind.into(), params: serde_json::from_value(params).unwrap_or_default() }));
    l.opacity = if r.chance(0.6) { 1.0 } else { 0.3 + r.f() * 0.7 };
    l.blend_mode = if r.chance(0.7) { BlendMode::Normal } else { mode(&mut r, false) };
    if r.chance(0.3) {
        let mut m = Surface::default();
        let a = rect(&mut r, doc.width, doc.height);
        noise(&mut m, &mut r, a);
        l.mask = Some(m);
    }
    let at = r.below(doc.root.len() as u32 + 1) as usize;
    doc.root.insert(at, Node::Layer(l));
    doc
}
