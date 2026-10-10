//! The layer tree flattened into one instruction list per output tile, for
//! the GPU compositor (spec 13). Sources are the `Arc<Tile>`s themselves, so
//! the GPU side can key its atlas on pointer identity.
//!
//! [`plan_tile`] mirrors `composite_nodes` step for step. A tile whose tree
//! uses something the GPU path does not handle yet returns `None` and stays
//! on the CPU. [`run`] interprets a program on the CPU, which tests the
//! flattening against [`crate::composite_tile`].

use crate::blend::{blend_px, clip_over};
use crate::{GHOST_PEG_KEY, GHOST_TINT_KEY, StrokeMode, StrokeOverlay, apply_stroke_px, panel_clip, riders};
use onepaint_doc::{BlendMode, Document, Layer, Node};
pub use onepaint_filters::gpu::GpuAdjust;
use onepaint_panel::coverage::TileCoverage;
use onepaint_raster::{Px, TILE, TILE_PX, Tile};
use std::ops::Range;
use std::sync::Arc;

/// Registers per pixel: the tile itself plus nested groups and clip families.
pub const MAX_STACK: usize = 8;

/// Most distinct source tiles one program may read. The GPU atlas always holds
/// more than this, so a program never has to be split.
pub const MAX_SOURCES: usize = 128;

/// A stroke buffer tile applied to a layer (or its mask) before it blends.
#[derive(Clone, Debug)]
pub struct StrokeSrc {
    pub tile: Arc<Tile>,
    pub opacity: f32,
    pub mode: StrokeMode,
}

/// The layer mask on this tile.
#[derive(Clone, Debug)]
pub enum MaskSrc {
    None,
    Tile(Arc<Tile>),
    /// The mask has no tile here but a mask stroke does: start from zero.
    Zero,
}

/// What `layer_content` produces: pixels, then stroke, then relief, then mask.
#[derive(Clone, Debug)]
pub struct Content {
    pub base: Option<Arc<Tile>>,
    pub stroke: Option<StrokeSrc>,
    pub mask: MaskSrc,
    pub mask_stroke: Option<StrokeSrc>,
    /// Impasto relief lighting (spec 45).
    pub relief: Option<Arc<Relief>>,
}

/// The height tiles relief reads for tile `(tx, ty)` (spec 45): its own and
/// the four sharing an edge, since a pixel's taps are one pixel left, right,
/// up and down. Taps clamp to the document, as `relief::shade` does.
#[derive(Debug)]
pub struct Relief {
    /// Centre, left, right, up, down.
    pub tiles: [Option<Arc<Tile>>; 5],
    pub tx: i32,
    pub ty: i32,
    /// Document size in pixels.
    pub doc: [i32; 2],
}

impl Relief {
    /// Height at doc pixel (x, y), clamped to the document; (x, y) is on the
    /// tile or one pixel off an edge.
    fn height(&self, x: i32, y: i32) -> f32 {
        let (x, y) = (x.clamp(0, self.doc[0] - 1), y.clamp(0, self.doc[1] - 1));
        let (dx, dy) = (x.div_euclid(TILE) - self.tx, y.div_euclid(TILE) - self.ty);
        let k = match (dx, dy) {
            (0, 0) => 0,
            (-1, 0) => 1,
            (1, 0) => 2,
            (0, -1) => 3,
            (0, 1) => 4,
            _ => return 0.0,
        };
        let i = (y.rem_euclid(TILE) * TILE + x.rem_euclid(TILE)) as usize;
        self.tiles[k].as_ref().map_or(0.0, |t| load(t, i)[0])
    }
}

/// A colour adjustment layer (spec 42): prepared for the CPU interpreter
/// and packed for the GPU.
#[derive(Clone)]
pub struct Adjust {
    pub prepared: onepaint_filters::Prepared,
    pub gpu: onepaint_filters::gpu::GpuAdjust,
}

impl std::fmt::Debug for Adjust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Adjust({:?})", self.prepared.kind)
    }
}

/// A panel frame's partial clip over one tile (spec 44): exact for the CPU
/// interpreter, as an f16 tile (coverage in every channel) for the GPU.
#[derive(Debug)]
pub struct Coverage {
    pub values: Vec<f32>,
    pub tile: Arc<Tile>,
}

impl Coverage {
    pub fn new(values: Vec<f32>) -> Self {
        let rgba: Vec<f32> = values.iter().flat_map(|&c| [c; 4]).collect();
        Self { tile: Arc::new(Tile::from_f32(&rgba)), values }
    }
}

/// A panel clip as the planner caches it.
#[derive(Clone)]
enum Clip {
    Full,
    Empty,
    Partial(Arc<Coverage>),
}

/// State the planner keeps between frames (spec 44): panel coverage by panel
/// spec and tile, so a panel border isn't rasterized, nor its coverage tile
/// uploaded, on every frame. Owned by the session; plans without one
/// recompute everything.
#[derive(Default)]
pub struct PlanCache {
    coverage: std::sync::Mutex<std::collections::HashMap<(u64, i32, i32), Clip>>,
}

/// Cached coverage entries kept at most; past this the cache starts over.
const MAX_CACHED_CLIPS: usize = 4096;

impl PlanCache {
    fn get(&self, key: &(u64, i32, i32)) -> Option<Clip> {
        self.coverage.lock().ok()?.get(key).cloned()
    }

    fn put(&self, key: (u64, i32, i32), c: Clip) {
        if let Ok(mut m) = self.coverage.lock() {
            if m.len() >= MAX_CACHED_CLIPS {
                m.clear();
            }
            m.insert(key, c);
        }
    }

    /// Coverage entries held (tests).
    pub fn len(&self) -> usize {
        self.coverage.lock().map_or(0, |m| m.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The document paper's visible tooth (spec 43).
#[derive(Clone)]
pub struct PaperShade {
    pub tex: onepaint_brush::paper::PaperTexture,
    /// `visible_amount(strength)`, above 0.
    pub amt: f32,
}

impl std::fmt::Debug for PaperShade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PaperShade({}, {})", self.tex.tile_px(), self.amt)
    }
}

#[derive(Clone, Debug)]
pub enum Op {
    /// Blend content onto the top register (`draw_layer`).
    Layer { content: Content, mode: BlendMode, opacity: f32, inherit_alpha: bool },
    /// Push a transparent register (a group starts).
    PushZero,
    /// Push content as a new register (a clip family's base).
    PushContent(Content),
    /// Clip content onto the top register (a clipped layer).
    Clip { content: Content, opacity: f32 },
    /// Pop the top register, tint it, and blend (or clip) it onto the next.
    Pop { mode: BlendMode, opacity: f32, clip: bool, tint: Option<[f32; 3]> },
    /// Adjust the top register, mask the result and blend it back
    /// (`draw_adjustment`, spec 42).
    Adjust { adjust: Arc<Adjust>, mask: Option<Arc<Tile>>, mode: BlendMode, opacity: f32 },
    /// Shade the finished tile `(tx, ty)` with the paper's tooth (spec 43).
    Paper { shade: Arc<PaperShade>, tx: i32, ty: i32 },
    /// Multiply the top register by a panel frame's coverage (spec 44).
    Coverage(Arc<Coverage>),
}

#[derive(Clone, Debug)]
pub struct TileProgram {
    pub tx: i32,
    pub ty: i32,
    pub ops: Vec<Op>,
    /// The same ops cut at the hot layer (spec 38), when one was given.
    pub split: Option<Split>,
}

/// A run of the program's ops whose result is cached as one image (spec 38).
#[derive(Clone, Debug)]
pub struct Segment {
    /// Changes whenever anything the segment reads changes.
    pub key: u64,
    pub range: Range<usize>,
}

/// A program cut at the hot unit, as ranges into its ops: the prefix and
/// suffix images are cached, only `hot` and `tail` run every frame.
#[derive(Clone, Debug)]
pub struct Split {
    /// Everything below the hot unit; the frame starts from its image.
    pub prefix: Option<Segment>,
    pub hot: Range<usize>,
    /// Normal, backdrop-independent units above: applied as one image `S`
    /// over the result (`S + (1 − S.a) · x`).
    pub suffix: Option<Segment>,
    /// Units above that can't fold; they run every frame.
    pub tail: Range<usize>,
}

/// Every source tile `ops` read, in op order (with repeats).
pub fn op_sources(ops: &[Op]) -> impl Iterator<Item = &Arc<Tile>> {
    ops.iter().flat_map(|op| {
        let (c, adjust_mask) = match op {
            Op::Layer { content, .. } | Op::PushContent(content) | Op::Clip { content, .. } => (Some(content), None),
            Op::Adjust { mask, .. } => (None, mask.as_ref()),
            Op::Coverage(c) => (None, Some(&c.tile)),
            _ => (None, None),
        };
        let content = c.into_iter().flat_map(|c| {
            let mask = match &c.mask {
                MaskSrc::Tile(t) => Some(t),
                _ => None,
            };
            let relief = c.relief.iter().flat_map(|r| r.tiles.iter().flatten());
            c.base.iter().chain(c.stroke.iter().map(|s| &s.tile)).chain(mask).chain(c.mask_stroke.iter().map(|s| &s.tile)).chain(relief)
        });
        content.chain(adjust_mask)
    })
}

impl TileProgram {
    /// Every source tile the program reads, in op order (with repeats).
    pub fn sources(&self) -> impl Iterator<Item = &Arc<Tile>> {
        op_sources(&self.ops)
    }
}

impl TileProgram {
    /// The ops in `r` (a split's range).
    pub fn at(&self, r: &Range<usize>) -> &[Op] {
        &self.ops[r.clone()]
    }
}

/// A content key for `ops`: their kinds and parameters, and the address of
/// every source tile (copy-on-write moves an edited tile to a new address).
pub fn segment_key(ops: &[Op]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let ptr = |t: &Arc<Tile>| Arc::as_ptr(t) as usize;
    let content = |c: &Content, h: &mut std::collections::hash_map::DefaultHasher| {
        c.base.as_ref().map(ptr).hash(h);
        for s in [&c.stroke, &c.mask_stroke] {
            if let Some(s) = s {
                ptr(&s.tile).hash(h);
                s.opacity.to_bits().hash(h);
                let (code, mode) = stroke_code(s.mode);
                (code, mode as u8).hash(h);
            } else {
                0usize.hash(h);
            }
        }
        match &c.mask {
            MaskSrc::None => 0usize.hash(h),
            MaskSrc::Zero => 1usize.hash(h),
            MaskSrc::Tile(t) => (2usize, ptr(t)).hash(h),
        }
        // Relief depends on where the tile is (taps clamp at the document's edge).
        if let Some(r) = &c.relief {
            (r.tx, r.ty, r.doc).hash(h);
            r.tiles.iter().map(|t| t.as_ref().map(ptr)).for_each(|p| p.hash(h));
        }
    };
    for op in ops {
        match op {
            Op::Layer { content: c, mode, opacity, inherit_alpha } => {
                (0u8, *mode as u8, opacity.to_bits(), *inherit_alpha).hash(&mut h);
                content(c, &mut h);
            }
            Op::PushZero => 1u8.hash(&mut h),
            Op::PushContent(c) => {
                2u8.hash(&mut h);
                content(c, &mut h);
            }
            Op::Clip { content: c, opacity } => {
                (3u8, opacity.to_bits()).hash(&mut h);
                content(c, &mut h);
            }
            Op::Pop { mode, opacity, clip, tint } => {
                (4u8, *mode as u8, opacity.to_bits(), *clip).hash(&mut h);
                tint.map(|t| t.map(f32::to_bits)).hash(&mut h);
            }
            Op::Adjust { adjust, mask, mode, opacity } => {
                (5u8, *mode as u8, opacity.to_bits(), adjust.gpu.code).hash(&mut h);
                adjust.gpu.params.map(f32::to_bits).hash(&mut h);
                adjust.gpu.lut.hash(&mut h);
                mask.as_ref().map(ptr).hash(&mut h);
            }
            Op::Paper { shade, tx, ty } => {
                (6u8, *tx, *ty, shade.amt.to_bits(), shade.tex.tile_px().to_bits()).hash(&mut h);
                (shade.tex.mask().data.as_ptr() as usize).hash(&mut h);
            }
            Op::Coverage(c) => (7u8, ptr(&c.tile)).hash(&mut h),
        }
    }
    h.finish()
}

/// Why a tile cannot run on the GPU; any of these keeps it on the CPU.
struct Unsupported;

struct Planner<'a> {
    tx: i32,
    ty: i32,
    ov: Option<&'a StrokeOverlay<'a>>,
    ops: Vec<Op>,
    depth: usize,
    /// Top-level units: (op range start, end, holds the hot layer, folds).
    units: Vec<Unit>,
    hot: Option<&'a str>,
    cache: Option<&'a PlanCache>,
    /// Document size, for relief taps at its edge.
    doc: [u32; 2],
}

/// A top-level node (with its clip family) and the ops it produced.
struct Unit {
    start: usize,
    end: usize,
    hot: bool,
    /// Its result does not depend on what lies below (spec 38).
    folds: bool,
}

/// Whether `id` is `n` or anything inside it.
fn contains(n: &Node, id: &str) -> bool {
    match n {
        Node::Layer(l) => l.id == id,
        Node::Group(g) => g.id == id || g.children.iter().any(|c| contains(c, id)),
    }
}

/// Plan one output tile, or `None` when it needs the CPU compositor.
pub fn plan_tile(doc: &Document, tx: i32, ty: i32, ov: Option<&StrokeOverlay>) -> Option<TileProgram> {
    plan_tile_split(doc, tx, ty, ov, None)
}

/// Plan many tiles in parallel, in order: the programs, and the keys the
/// GPU path can't handle (they stay on the CPU compositor).
pub fn plan_tiles(
    doc: &Document,
    keys: &[(i32, i32)],
    ov: Option<&StrokeOverlay>,
    hot: Option<&str>,
) -> (Vec<TileProgram>, Vec<(i32, i32)>) {
    plan_tiles_cached(doc, keys, ov, hot, None)
}

/// [`plan_tiles`], reusing what `cache` kept from earlier frames.
pub fn plan_tiles_cached(
    doc: &Document,
    keys: &[(i32, i32)],
    ov: Option<&StrokeOverlay>,
    hot: Option<&str>,
    cache: Option<&PlanCache>,
) -> (Vec<TileProgram>, Vec<(i32, i32)>) {
    use rayon::prelude::*;
    let planned: Vec<Result<TileProgram, (i32, i32)>> =
        keys.par_iter().map(|&(tx, ty)| plan(doc, tx, ty, ov, hot, cache).ok_or((tx, ty))).collect();
    let mut progs = Vec::with_capacity(planned.len());
    let mut cpu = Vec::new();
    for p in planned {
        match p {
            Ok(p) => progs.push(p),
            Err(k) => cpu.push(k),
        }
    }
    (progs, cpu)
}

/// Segments reading fewer sources than this aren't worth a cache image.
const MIN_SEGMENT_SOURCES: usize = 2;

/// [`plan_tile`], plus the ops cut at `hot` (a layer id) for spec 38.
pub fn plan_tile_split(doc: &Document, tx: i32, ty: i32, ov: Option<&StrokeOverlay>, hot: Option<&str>) -> Option<TileProgram> {
    plan(doc, tx, ty, ov, hot, None)
}

fn plan(doc: &Document, tx: i32, ty: i32, ov: Option<&StrokeOverlay>, hot: Option<&str>, cache: Option<&PlanCache>) -> Option<TileProgram> {
    let doc_size = [doc.width, doc.height];
    let mut p = Planner { tx, ty, ov, ops: Vec::new(), depth: 1, units: Vec::new(), hot, cache, doc: doc_size };
    p.nodes(&doc.root).ok()?;
    // The paper shades the finished composite (spec 43), after every unit.
    if let Some(paper) = doc.paper() {
        let amt = onepaint_brush::paper::visible_amount(paper.strength());
        if let Some(tex) = onepaint_brush::paper::PaperTexture::new(&paper).filter(|_| amt > 0.0) {
            p.ops.push(Op::Paper { shade: Arc::new(PaperShade { tex, amt }), tx, ty });
        }
    }
    let split = hot.and_then(|_| split(&p.ops, &p.units));
    let prog = TileProgram { tx, ty, ops: p.ops, split };
    let mut seen: Vec<*const Tile> = prog.sources().map(Arc::as_ptr).collect();
    seen.sort_unstable();
    seen.dedup();
    (seen.len() <= MAX_SOURCES).then_some(prog)
}

impl Planner<'_> {
    fn content(&self, l: &Layer) -> Result<Option<Content>, Unsupported> {
        let (tx, ty) = (self.tx, self.ty);
        let on_layer = self.ov.filter(|o| o.layer_id == l.id);
        let src = |o: &StrokeOverlay| o.stroke.tile(tx, ty).map(|t| StrokeSrc { tile: t.clone(), opacity: o.opacity, mode: o.mode });
        let stroke = on_layer.filter(|o| !o.mask).and_then(src);
        let mask_stroke = on_layer.filter(|o| o.mask).and_then(src);
        let base = l.pixels.tile(tx, ty).cloned();
        if base.is_none() && stroke.is_none() {
            return Ok(None);
        }
        let relief = l.height.as_ref().and_then(|h| {
            let tiles = [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)].map(|(dx, dy)| h.tile(tx + dx, ty + dy).cloned());
            let doc = [self.doc[0] as i32, self.doc[1] as i32];
            tiles.iter().any(Option::is_some).then(|| Arc::new(Relief { tiles, tx, ty, doc }))
        });
        let mut c = Content { base, stroke, mask: MaskSrc::None, mask_stroke: None, relief };
        if l.mask_enabled
            && let Some(mask) = &l.mask
        {
            c.mask = match (mask.tile(tx, ty), &mask_stroke) {
                (Some(t), _) => MaskSrc::Tile(t.clone()),
                (None, Some(_)) => MaskSrc::Zero,
                (None, None) => return Ok(None),
            };
            c.mask_stroke = mask_stroke;
        }
        Ok(Some(c))
    }

    /// A panel group's clip over this tile (`panel_clip`), through the cache.
    fn panel_clip(&self, g: &onepaint_doc::Group) -> Option<Clip> {
        let spec = g.extra.get("panel")?;
        let (tx, ty) = (self.tx, self.ty);
        let make = || match panel_clip(g, tx, ty)? {
            TileCoverage::Full => Some(Clip::Full),
            TileCoverage::Empty => Some(Clip::Empty),
            TileCoverage::Partial(v) => Some(Clip::Partial(Arc::new(Coverage::new(v)))),
        };
        let Some(cache) = self.cache else { return make() };
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        spec.to_string().hash(&mut h);
        let key = (h.finish(), tx, ty);
        if let Some(c) = cache.get(&key) {
            return Some(c);
        }
        // `panel_clip` is `None` for a spec it can't read: never cached, never clipped.
        let c = make()?;
        cache.put(key, c.clone());
        Some(c)
    }

    /// An adjustment layer (`draw_adjustment`): pixel-only colour kinds run
    /// on the GPU (spec 42), other colour kinds keep the tile on the CPU, and
    /// anything else draws nothing.
    fn adjust(&mut self, l: &Layer) -> Result<(), Unsupported> {
        let Some(adj) = &l.adjustment else { return Ok(()) };
        if !l.visible || l.opacity <= 0.0 {
            return Ok(());
        }
        let Some(kind) = onepaint_filters::Kind::parse(&adj.kind).filter(|k| k.is_color()) else { return Ok(()) };
        let gpu = onepaint_filters::gpu::gpu_adjust(kind, &adj.params).ok_or(Unsupported)?;
        let mask = match (&l.mask, l.mask_enabled) {
            (Some(m), true) => match m.tile(self.tx, self.ty) {
                Some(t) => Some(t.clone()),
                None => return Ok(()), // fully masked out on this tile
            },
            _ => None,
        };
        let prepared = onepaint_filters::prepare(kind, &adj.params);
        let adjust = Arc::new(Adjust { prepared, gpu });
        self.ops.push(Op::Adjust { adjust, mask, mode: l.blend_mode, opacity: l.opacity });
        Ok(())
    }

    fn push(&mut self, op: Op) -> Result<(), Unsupported> {
        self.depth += 1;
        if self.depth > MAX_STACK {
            return Err(Unsupported);
        }
        self.ops.push(op);
        Ok(())
    }

    fn pop(&mut self, mode: BlendMode, opacity: f32, clip: bool, tint: Option<[f32; 3]>) {
        self.depth -= 1;
        self.ops.push(Op::Pop { mode, opacity, clip, tint });
    }

    fn clipped(&mut self, riders: &[Node]) -> Result<(), Unsupported> {
        for r in riders {
            let Node::Layer(c) = r else { continue };
            if !c.visible || c.opacity <= 0.0 {
                continue;
            }
            if let Some(content) = self.content(c)? {
                self.ops.push(Op::Clip { content, opacity: c.opacity });
            }
        }
        Ok(())
    }

    fn nodes(&mut self, nodes: &[Node]) -> Result<(), Unsupported> {
        let mut i = 0;
        while i < nodes.len() {
            // An adjustment layer is never a clip base (as in `composite_nodes`).
            if let Node::Layer(l) = &nodes[i]
                && l.adjustment.is_some()
            {
                let start = self.ops.len();
                self.adjust(l)?;
                if self.depth == 1 {
                    let hot = self.hot.is_some_and(|id| id == l.id);
                    // It reads the backdrop, so it never folds into a suffix image.
                    self.units.push(Unit { start, end: self.ops.len(), hot, folds: false });
                }
                i += 1;
                continue;
            }
            let j = riders(nodes, i);
            let top = self.depth == 1;
            let start = self.ops.len();
            match &nodes[i] {
                Node::Group(g) => {
                    let clip = self.panel_clip(g);
                    if g.visible && g.opacity > 0.0 && !g.children.is_empty() && !matches!(clip, Some(Clip::Empty)) {
                        if g.extra.contains_key(GHOST_PEG_KEY) {
                            return Err(Unsupported);
                        }
                        self.push(Op::PushZero)?;
                        self.nodes(&g.children)?;
                        // A panel's frame clips the group before its riders go on.
                        if let Some(Clip::Partial(c)) = clip {
                            self.ops.push(Op::Coverage(c));
                        }
                        self.clipped(&nodes[i + 1..j])?;
                        let tint = g.extra.get(GHOST_TINT_KEY).and_then(|v| serde_json::from_value::<[f32; 3]>(v.clone()).ok());
                        self.pop(g.blend_mode, g.opacity, false, tint);
                    }
                }
                Node::Layer(l) => {
                    if l.visible && l.opacity > 0.0 {
                        let family = &nodes[i + 1..j];
                        let content = self.content(l)?;
                        if family.is_empty() {
                            if let Some(content) = content {
                                self.ops.push(Op::Layer {
                                    content,
                                    mode: l.blend_mode,
                                    opacity: l.opacity,
                                    inherit_alpha: l.inherit_alpha,
                                });
                            }
                        } else {
                            match content {
                                Some(c) => self.push(Op::PushContent(c))?,
                                None => self.push(Op::PushZero)?,
                            }
                            self.clipped(family)?;
                            self.pop(l.blend_mode, l.opacity, l.inherit_alpha, None);
                        }
                    }
                }
            }
            if top {
                let hot = self.hot.is_some_and(|id| nodes[i..j].iter().any(|n| contains(n, id)));
                let folds = match &nodes[i] {
                    Node::Group(g) => g.blend_mode == BlendMode::Normal,
                    Node::Layer(l) => l.blend_mode == BlendMode::Normal && !l.inherit_alpha,
                };
                self.units.push(Unit { start, end: self.ops.len(), hot, folds });
            }
            i = j;
        }
        Ok(())
    }
}

/// Cut `ops` at the hot unit (spec 38): `None` when no unit holds it.
fn split(ops: &[Op], units: &[Unit]) -> Option<Split> {
    let h = units.iter().position(|u| u.hot)?;
    let hot_unit = &units[h];
    let after = &units[h + 1..];
    // Units are contiguous: every top-level op belongs to one.
    let folds = after.iter().take_while(|u| u.folds).count();
    let fold_end = if folds == 0 { hot_unit.end } else { after[folds - 1].end };
    let worth = |r: &Range<usize>| op_sources(&ops[r.clone()]).nth(MIN_SEGMENT_SOURCES - 1).is_some();
    let segment = |r: Range<usize>| Segment { key: segment_key(&ops[r.clone()]), range: r };
    // A segment not worth caching runs with its neighbour instead.
    let (prefix, hot_start) = match 0..hot_unit.start {
        r if worth(&r) => (Some(segment(r)), hot_unit.start),
        _ => (None, 0),
    };
    let (suffix, tail_start) = match hot_unit.end..fold_end {
        r if worth(&r) => (Some(segment(r)), fold_end),
        _ => (None, hot_unit.end),
    };
    Some(Split { prefix, hot: hot_start..hot_unit.end, suffix, tail: tail_start..ops.len() })
}

// ── CPU interpreter (the encoding's own oracle) ──

fn load(t: &Tile, i: usize) -> Px {
    let d = &t.data[i * 4..i * 4 + 4];
    [d[0].to_f32(), d[1].to_f32(), d[2].to_f32(), d[3].to_f32()]
}

fn eval(c: &Content, i: usize) -> Px {
    let mut p = c.base.as_ref().map_or([0.0; 4], |t| load(t, i));
    if let Some(s) = &c.stroke {
        p = apply_stroke_px(p, load(&s.tile, i), s.opacity, s.mode);
    }
    if let Some(r) = &c.relief {
        let (x, y) = (r.tx * TILE + (i % TILE as usize) as i32, r.ty * TILE + (i / TILE as usize) as i32);
        if x < r.doc[0] && y < r.doc[1] {
            crate::relief::light_px(&mut p, [r.height(x - 1, y), r.height(x + 1, y), r.height(x, y - 1), r.height(x, y + 1)]);
        }
    }
    let mut m = match &c.mask {
        MaskSrc::None => return p,
        MaskSrc::Tile(t) => load(t, i),
        MaskSrc::Zero => [0.0; 4],
    };
    if let Some(s) = &c.mask_stroke {
        m = apply_stroke_px(m, load(&s.tile, i), s.opacity, s.mode);
    }
    p.map(|v| v * m[0])
}

/// Run `ops` on one pixel, starting from the register `init`.
fn exec(ops: &[Op], init: Px, i: usize, stack: &mut Vec<Px>) -> Px {
    stack.clear();
    stack.push(init);
    for op in ops {
        // The stack is never empty: the planner pairs every push with a pop.
        let Some(top) = stack.last_mut() else { break };
        match op {
            Op::Layer { content, mode, opacity, inherit_alpha } => {
                let mut c = eval(content, i);
                if *inherit_alpha {
                    let a = top[3];
                    c = c.map(|v| v * a);
                }
                *top = blend_px(*mode, *top, c, *opacity);
            }
            Op::PushZero => stack.push([0.0; 4]),
            Op::PushContent(c) => {
                let v = eval(c, i);
                stack.push(v);
            }
            Op::Clip { content, opacity } => *top = clip_over(*top, eval(content, i), *opacity),
            Op::Adjust { adjust, mask, mode, opacity } => {
                // Pixel-only kinds: the position doesn't matter.
                let mut o = adjust.prepared.color_px(*top, 0, 0);
                if let Some(m) = mask {
                    let k = load(m, i)[0];
                    o = o.map(|v| v * k);
                }
                *top = blend_px(*mode, *top, o, *opacity);
            }
            Op::Coverage(c) => {
                let k = c.values[i];
                top.iter_mut().for_each(|v| *v *= k);
            }
            Op::Paper { shade, tx, ty } => {
                if top[..3].iter().any(|v| *v != 0.0) {
                    let (lx, ly) = ((i % TILE as usize) as i32, (i / TILE as usize) as i32);
                    let (x, y) = ((tx * TILE + lx) as f64 + 0.5, (ty * TILE + ly) as f64 + 0.5);
                    let k = onepaint_brush::paper::shade_at(&shade.tex, shade.amt, x, y);
                    top[..3].iter_mut().for_each(|v| *v *= k);
                }
            }
            Op::Pop { mode, opacity, clip, tint } => {
                let Some(mut v) = stack.pop() else { break };
                if let Some(t) = tint {
                    for c in 0..3 {
                        v[c] += (t[c] * v[3] - v[c]) * 0.65;
                    }
                }
                let Some(dst) = stack.last_mut() else { break };
                *dst = if *clip { clip_over(*dst, v, *opacity) } else { blend_px(*mode, *dst, v, *opacity) };
            }
        }
    }
    stack[0]
}

/// Run a program on the CPU: premultiplied linear f32, `TILE × TILE`.
pub fn run(prog: &TileProgram) -> Vec<f32> {
    let mut out = vec![0.0; TILE_PX * 4];
    let mut stack: Vec<Px> = Vec::with_capacity(MAX_STACK);
    for i in 0..TILE_PX {
        let v = exec(&prog.ops, [0.0; 4], i, &mut stack);
        out[i * 4..i * 4 + 4].copy_from_slice(&v);
    }
    out
}

/// Run a program through its split (spec 38): the prefix image, the hot
/// ops, the suffix image over that, then the tail. Without a split, [`run`].
pub fn run_split(prog: &TileProgram) -> Vec<f32> {
    let Some(s) = &prog.split else { return run(prog) };
    let mut out = vec![0.0; TILE_PX * 4];
    let mut stack: Vec<Px> = Vec::with_capacity(MAX_STACK);
    for i in 0..TILE_PX {
        let base = s.prefix.as_ref().map_or([0.0; 4], |p| exec(prog.at(&p.range), [0.0; 4], i, &mut stack));
        let mut v = exec(prog.at(&s.hot), base, i, &mut stack);
        if let Some(sfx) = &s.suffix {
            let img = exec(prog.at(&sfx.range), [0.0; 4], i, &mut stack);
            v = blend_px(BlendMode::Normal, v, img, 1.0);
        }
        v = exec(prog.at(&s.tail), v, i, &mut stack);
        out[i * 4..i * 4 + 4].copy_from_slice(&v);
    }
    out
}

/// Strokes keep their mode on the GPU as a small code.
pub fn stroke_code(m: StrokeMode) -> (u32, BlendMode) {
    match m {
        StrokeMode::Paint(b) => (0, b),
        StrokeMode::Erase => (1, BlendMode::Normal),
        StrokeMode::AlphaLock(b) => (2, b),
    }
}
