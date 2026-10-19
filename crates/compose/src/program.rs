//! The layer tree flattened into one instruction list per output tile, for
//! the GPU compositor (spec 13). Sources are the `Arc<Tile>`s themselves, so
//! the GPU side can key its atlas on pointer identity.
//!
//! [`plan_tile`] mirrors `composite_nodes` step for step. A tile whose tree
//! uses something the GPU path does not handle yet returns `None` and stays
//! on the CPU. [`run`] interprets a program on the CPU, which tests the
//! flattening against [`crate::composite_tile`].

use crate::blend::{blend_px_at, blend_px_if, blend_px_in, clip_over};
use crate::{
    GHOST_PEG_KEY, GHOST_TINT_KEY, StrokeMode, StrokeOverlay, apply_stroke_px, apply_stroke_px_at, panel_clip, passes_through, riders,
};
use onepaint_color::Space;
use onepaint_doc::{BlendIf, BlendMode, Document, Layer, Node, mask_k};
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
    /// The mask's density: it multiplies by `1 − density · (1 − m)`.
    pub density: f32,
    /// The vector mask's coverage here (`None`: no vector mask, or it covers the tile).
    pub vmask: Option<Arc<Tile>>,
    /// A spatial adjustment layer's pixels, in place of `base` (spec 52).
    pub spatial: Option<Arc<crate::spatial::SpatialSrc>>,
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
    /// Shared by every tile of the layer when planned with a cache.
    pub prepared: Arc<onepaint_filters::Prepared>,
    pub gpu: Arc<onepaint_filters::gpu::GpuAdjust>,
    /// The tile's origin in doc px (noise and fills depend on position).
    pub at: [i32; 2],
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
    /// Prepared adjustments by kind, params and document frame, so LUTs,
    /// ramps and pattern tiles are built once, not per tile and frame.
    adjust: std::sync::Mutex<std::collections::HashMap<u64, PreparedAdjust>>,
    /// Whether a tile is fully opaque, by its address (the `Weak` keeps the
    /// address from being reused while the entry lives).
    opaque: std::sync::Mutex<std::collections::HashMap<usize, (std::sync::Weak<Tile>, bool)>>,
    /// Feathered mask tiles by the mask tiles they read (held weakly, so a
    /// freed address can't match), the feather, the tile and the document size.
    feather: std::sync::Mutex<std::collections::HashMap<u64, Feathered>>,
    /// Flattened vector masks by their JSON, and their coverage by mask and tile.
    vflat: std::sync::Mutex<std::collections::HashMap<String, Arc<crate::vmask::Flat>>>,
    vcover: std::sync::Mutex<std::collections::HashMap<(u64, i32, i32), VCover>>,
}

/// A vector mask's coverage of one tile, as the planner keeps it.
#[derive(Clone, Debug)]
pub enum VCover {
    Empty,
    Full,
    Partial(Arc<Tile>),
}

/// Cached vector mask coverage tiles kept at most; past this the cache starts over.
const MAX_CACHED_VCOVER: usize = 4096;

type Feathered = (Vec<Option<std::sync::Weak<Tile>>>, Option<Arc<Tile>>);

/// Cached feathered mask tiles kept at most; past this the cache starts over.
const MAX_CACHED_FEATHER: usize = 1024;

type PreparedAdjust = (Arc<onepaint_filters::Prepared>, Arc<onepaint_filters::gpu::GpuAdjust>);

/// Cached adjustments kept at most; past this the cache starts over.
const MAX_CACHED_ADJUST: usize = 256;

/// Cached coverage entries kept at most; past this the cache starts over.
const MAX_CACHED_CLIPS: usize = 4096;
/// Cached opacity flags kept at most (dead tiles are dropped first).
const MAX_CACHED_OPAQUE: usize = 1 << 16;

/// Every pixel's alpha is exactly 1 (f16 0x3c00).
fn is_opaque(t: &Tile) -> bool {
    t.is_opaque()
}

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

    fn adjust(&self, key: u64, make: impl FnOnce() -> Option<PreparedAdjust>) -> Option<PreparedAdjust> {
        if let Some(hit) = self.adjust.lock().ok().and_then(|m| m.get(&key).cloned()) {
            return Some(hit);
        }
        let v = make()?;
        let Ok(mut m) = self.adjust.lock() else { return Some(v) };
        if m.len() >= MAX_CACHED_ADJUST {
            m.clear();
        }
        // Another thread may have prepared it meanwhile: share theirs.
        Some(m.entry(key).or_insert(v).clone())
    }

    /// The feathered mask tile `make` builds from the `window` tiles, unless
    /// one was built from the same tiles before.
    fn feathered(&self, key: u64, window: &[Option<Arc<Tile>>], make: impl FnOnce() -> Option<Arc<Tile>>) -> Option<Arc<Tile>> {
        let same = |w: &[Option<std::sync::Weak<Tile>>]| {
            w.len() == window.len()
                && w.iter().zip(window).all(|(a, b)| match (a, b) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.strong_count() > 0 && std::ptr::eq(a.as_ptr(), Arc::as_ptr(b)),
                    _ => false,
                })
        };
        if let Some(hit) = self.feather.lock().ok().and_then(|m| m.get(&key).filter(|e| same(&e.0)).map(|e| e.1.clone())) {
            return hit;
        }
        let v = make();
        if let Ok(mut m) = self.feather.lock() {
            if m.len() >= MAX_CACHED_FEATHER {
                m.clear();
            }
            m.insert(key, (window.iter().map(|t| t.as_ref().map(Arc::downgrade)).collect(), v.clone()));
        }
        v
    }

    /// `v`'s coverage of tile `(tx, ty)`, flattened once per mask.
    fn vector_cover(&self, v: &onepaint_doc::VectorMask, tx: i32, ty: i32) -> VCover {
        let json = v.key();
        let flat = {
            let hit = self.vflat.lock().ok().and_then(|m| m.get(&json).cloned());
            hit.unwrap_or_else(|| {
                let f = Arc::new(crate::vmask::flatten(v));
                if let Ok(mut m) = self.vflat.lock() {
                    if m.len() >= MAX_CACHED_ADJUST {
                        m.clear();
                    }
                    m.insert(json.clone(), f.clone());
                }
                f
            })
        };
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        json.hash(&mut h);
        let key = (h.finish(), tx, ty);
        if let Some(hit) = self.vcover.lock().ok().and_then(|m| m.get(&key).cloned()) {
            return hit;
        }
        let c = match crate::vmask::cover(&flat, tx, ty) {
            crate::vmask::Cover::Empty => VCover::Empty,
            crate::vmask::Cover::Full => VCover::Full,
            crate::vmask::Cover::Partial(t) => VCover::Partial(Arc::new(t)),
        };
        if let Ok(mut m) = self.vcover.lock() {
            if m.len() >= MAX_CACHED_VCOVER {
                m.clear();
            }
            m.insert(key, c.clone());
        }
        c
    }

    /// Whether every pixel of `t` has alpha exactly 1, cached by tile.
    fn opaque(&self, t: &Arc<Tile>) -> bool {
        let key = Arc::as_ptr(t) as usize;
        if let Some(hit) = self.opaque.lock().ok().and_then(|m| m.get(&key).filter(|(w, _)| w.strong_count() > 0).map(|e| e.1)) {
            return hit;
        }
        let v = is_opaque(t);
        if let Ok(mut m) = self.opaque.lock() {
            if m.len() >= MAX_CACHED_OPAQUE {
                m.retain(|_, (w, _)| w.strong_count() > 0);
                if m.len() >= MAX_CACHED_OPAQUE {
                    m.clear();
                }
            }
            m.insert(key, (Arc::downgrade(t), v));
        }
        v
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
    Layer { content: Content, mode: BlendMode, opacity: f32, inherit_alpha: bool, blend_if: Option<BlendIf> },
    /// Push a transparent register (a group starts).
    PushZero,
    /// Push content as a new register (a clip family's base).
    PushContent(Content),
    /// Clip content onto the top register (a clipped layer).
    Clip { content: Content, opacity: f32 },
    /// Pop the top register, tint it, and blend (or clip) it onto the next.
    Pop { mode: BlendMode, opacity: f32, clip: bool, tint: Option<[f32; 3]>, blend_if: Option<BlendIf> },
    /// Adjust the top register, mask the result and blend it back
    /// (`draw_adjustment`, spec 42).
    /// `mask` is never [`MaskSrc::Zero`] unless `density` < 1.
    Adjust {
        adjust: Arc<Adjust>,
        mask: MaskSrc,
        density: f32,
        vmask: Option<Arc<Tile>>,
        mode: BlendMode,
        opacity: f32,
        blend_if: Option<BlendIf>,
    },
    /// Shade the finished tile `(tx, ty)` with the paper's tooth (spec 43).
    Paper { shade: Arc<PaperShade>, tx: i32, ty: i32 },
    /// Multiply the top register by a panel frame's coverage (spec 44).
    Coverage(Arc<Coverage>),
    /// Push a copy of the top register (a Pass Through group starts).
    PushCopy,
    /// Pop the top register `v` and fade it with the next one `d`:
    /// `d·(1 − o) + v·o` (a Pass Through group ends).
    Mix { opacity: f32 },
    /// Punch a knocked-out layer's shape through the top register:
    /// `mix(top, reveal, content.a · opacity)`.
    Knockout { content: Content, opacity: f32, reveal: Reveal },
    /// Keep the top register as the Background top-level knockouts reveal.
    Snapshot,
}

/// What a knockout's hole shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reveal {
    /// Transparency: in an isolated group, or at the top level without a
    /// Background.
    Clear,
    /// The register under the top: a Pass Through group's backdrop.
    Below,
    /// The [`Op::Snapshot`] image.
    Background,
}

#[derive(Clone, Debug)]
pub struct TileProgram {
    pub tx: i32,
    pub ty: i32,
    pub ops: Vec<Op>,
    /// The same ops cut at the hot layer (spec 38), when one was given.
    pub split: Option<Split>,
    /// What the pixel values mean (spec 50): every blend happens in it.
    pub space: Space,
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
pub fn op_sources(ops: &[Op]) -> Box<dyn Iterator<Item = &Arc<Tile>> + '_> {
    Box::new(ops.iter().flat_map(|op| {
        let (c, extra): (Option<&Content>, [Option<&Arc<Tile>>; 2]) = match op {
            Op::Layer { content, .. } | Op::PushContent(content) | Op::Clip { content, .. } | Op::Knockout { content, .. } => {
                (Some(content), [None, None])
            }
            Op::Adjust { mask, vmask, .. } => (None, [if let MaskSrc::Tile(m) = mask { Some(m) } else { None }, vmask.as_ref()]),
            Op::Coverage(c) => (None, [Some(&c.tile), None]),
            _ => (None, [None, None]),
        };
        let content = c.into_iter().flat_map(|c| {
            let mask = match &c.mask {
                MaskSrc::Tile(t) => Some(t),
                _ => None,
            };
            let relief = c.relief.iter().flat_map(|r| r.tiles.iter().flatten());
            c.base
                .iter()
                .chain(c.stroke.iter().map(|s| &s.tile))
                .chain(mask)
                .chain(c.mask_stroke.iter().map(|s| &s.tile))
                .chain(relief)
                .chain(c.vmask.iter())
                // A spatial layer reads its backdrop programs' tiles (spec 52).
                .chain(c.spatial.iter().flat_map(|s| s.backdrop.iter().flat_map(|(_, p)| op_sources(&p.ops))))
        });
        content.chain(extra.into_iter().flatten())
    }))
}

/// Every content the ops draw.
pub fn op_contents(ops: &[Op]) -> impl Iterator<Item = &Content> {
    ops.iter().filter_map(|op| match op {
        Op::Layer { content, .. } | Op::PushContent(content) | Op::Clip { content, .. } | Op::Knockout { content, .. } => Some(content),
        _ => None,
    })
}

impl TileProgram {
    /// Every source tile the program reads, in op order (with repeats).
    pub fn sources(&self) -> impl Iterator<Item = &Arc<Tile>> {
        op_sources(&self.ops)
    }
}

impl TileProgram {
    /// The tile's top-left corner in doc px.
    pub fn origin(&self) -> [i32; 2] {
        [self.tx * TILE, self.ty * TILE]
    }

    /// The ops in `r` (a split's range).
    pub fn at(&self, r: &Range<usize>) -> &[Op] {
        &self.ops[r.clone()]
    }
}

/// A content key for `ops` on the tile at `at` (doc px): their kinds and
/// parameters, and the address of every source tile (copy-on-write moves an
/// edited tile to a new address). The position counts too: Dissolve's
/// dither depends on it.
pub fn segment_key(ops: &[Op], at: [i32; 2]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    at.hash(&mut h);
    let ptr = |t: &Arc<Tile>| Arc::as_ptr(t) as usize;
    let mask_key = |m: &MaskSrc, h: &mut std::collections::hash_map::DefaultHasher| match m {
        MaskSrc::None => 0usize.hash(h),
        MaskSrc::Zero => 1usize.hash(h),
        MaskSrc::Tile(t) => (2usize, ptr(t)).hash(h),
    };
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
        mask_key(&c.mask, h);
        c.density.to_bits().hash(h);
        c.spatial.as_ref().map(|s| s.key).hash(h);
        c.vmask.as_ref().map(ptr).hash(h);
        // Relief depends on where the tile is (taps clamp at the document's edge).
        if let Some(r) = &c.relief {
            (r.tx, r.ty, r.doc).hash(h);
            r.tiles.iter().map(|t| t.as_ref().map(ptr)).for_each(|p| p.hash(h));
        }
    };
    for op in ops {
        match op {
            Op::Layer { content: c, mode, opacity, inherit_alpha, blend_if } => {
                (0u8, *mode as u8, opacity.to_bits(), *inherit_alpha, blend_if).hash(&mut h);
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
            Op::Pop { mode, opacity, clip, tint, blend_if } => {
                (4u8, *mode as u8, opacity.to_bits(), *clip, blend_if).hash(&mut h);
                tint.map(|t| t.map(f32::to_bits)).hash(&mut h);
            }
            Op::Adjust { adjust, mask, density, vmask, mode, opacity, blend_if } => {
                (5u8, *mode as u8, opacity.to_bits(), adjust.gpu.code, density.to_bits(), blend_if).hash(&mut h);
                vmask.as_ref().map(ptr).hash(&mut h);
                adjust.gpu.params.map(f32::to_bits).hash(&mut h);
                adjust.gpu.lut.hash(&mut h);
                adjust.gpu.table.iter().flatten().for_each(|e| e.map(f32::to_bits).hash(&mut h));
                adjust.at.hash(&mut h);
                mask_key(mask, &mut h);
            }
            Op::Paper { shade, tx, ty } => {
                (6u8, *tx, *ty, shade.amt.to_bits(), shade.tex.tile_px().to_bits()).hash(&mut h);
                (shade.tex.mask().data.as_ptr() as usize).hash(&mut h);
            }
            Op::Coverage(c) => (7u8, ptr(&c.tile)).hash(&mut h),
            Op::PushCopy => 8u8.hash(&mut h),
            Op::Mix { opacity } => (9u8, opacity.to_bits()).hash(&mut h),
            Op::Knockout { content: c, opacity, reveal } => {
                (10u8, opacity.to_bits(), *reveal).hash(&mut h);
                content(c, &mut h);
            }
            Op::Snapshot => 11u8.hash(&mut h),
        }
    }
    h.finish()
}

/// Why a tile cannot run on the GPU; any of these keeps it on the CPU.
struct Unsupported;

struct Planner<'a> {
    doc: &'a Document,
    tx: i32,
    ty: i32,
    ov: Option<&'a StrokeOverlay<'a>>,
    ops: Vec<Op>,
    depth: usize,
    /// Top-level units: (op range start, end, holds the hot layer, folds).
    units: Vec<Unit>,
    hot: Option<&'a str>,
    cache: Option<&'a PlanCache>,
    /// The document as a filter frame: its size bounds relief taps, and
    /// noise and fills are laid out in it.
    frame: onepaint_filters::Frame,
    /// Where each open register's ops begin (the top level's at 0).
    regs: Vec<usize>,
    /// The lowest point top-level ops were cut back to by an opaque layer.
    cut: Option<usize>,
    /// What a knockout in the nodes being planned reveals.
    reveal: Reveal,
    /// The top level's knockouts reveal the Background (`background_reveals`).
    background: bool,
    /// The Background snapshot was taken: top-level ops can't be cut, and
    /// the program can't split (a cached prefix would lose it).
    snapshot: bool,
    /// A spatial layer was planned: no split (the GPU fills segments in a
    /// pass that can't read the filtered images; spec 52).
    spatial: std::cell::Cell<bool>,
    /// This call's whole-canvas layers (spec 53).
    wholes: &'a Wholes,
}

/// A top-level node (with its clip family) and the ops it produced.
struct Unit {
    start: usize,
    end: usize,
    hot: bool,
    /// Its result does not depend on what lies below (spec 38).
    folds: bool,
    /// An adjustment layer or a Blend If: it reads the backdrop and may
    /// amplify rounding (Blend If's tone steps flip on it).
    adjust: bool,
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
    let wholes = plan_wholes(doc, ov, cache);
    let planned: Vec<Result<TileProgram, (i32, i32)>> =
        keys.par_iter().map(|&(tx, ty)| plan(doc, tx, ty, ov, hot, cache, &wholes).ok_or((tx, ty))).collect();
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
    // A whole-canvas layer is planned once for many tiles (`plan_tiles_cached`);
    // alone, a tile leaves it to the CPU.
    plan(doc, tx, ty, ov, hot, None, &Wholes::default())
}

/// Whole-canvas spatial layers (spec 53) planned for one call, by layer id:
/// `None` where one can't run on the GPU.
#[derive(Default)]
pub(crate) struct Wholes(std::collections::HashMap<String, Option<Arc<crate::spatial::Whole>>>);

/// Plan every whole-canvas layer's backdrop once, before the tiles, so
/// every tile shares it (planning it per tile would be quadratic). Their
/// backdrops can't hold spatial layers of their own (no stacking yet).
fn plan_wholes(doc: &Document, ov: Option<&StrokeOverlay>, cache: Option<&PlanCache>) -> Wholes {
    use rayon::prelude::*;
    fn walk<'a>(nodes: &'a [Node], out: &mut Vec<(&'a Layer, &'a [Node])>) {
        for (i, n) in nodes.iter().enumerate() {
            match n {
                Node::Group(g) => walk(&g.children, out),
                Node::Layer(l) => {
                    if let Some(kind) = crate::spatial::spatial_kind(l)
                        && onepaint_filters::reach(kind, &Default::default()) >= onepaint_filters::GLOBAL_REACH
                    {
                        out.push((l, &nodes[..i]));
                    }
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(&doc.root, &mut found);
    let mut out = Wholes::default();
    for (l, below) in found {
        let kind = crate::spatial::spatial_kind(l).expect("walk keeps spatial layers");
        let params = l.adjustment.as_ref().map(|a| a.params.clone()).unwrap_or_default();
        let whole = crate::spatial::whole_supported(kind, &params).then(|| {
            let mut sub = doc.clone_shallow();
            sub.root = below.to_vec();
            sub.source = None;
            sub.set_paper(None);
            let keys: Vec<_> = doc.bounds().tiles().collect();
            let none = Wholes::default();
            let backdrop = keys
                .par_iter()
                .map(|&(x, y)| plan(&sub, x, y, ov, None, cache, &none).map(|p| ((x, y), Arc::new(p))))
                .collect::<Option<Vec<_>>>()?;
            let nested = backdrop.iter().any(|(_, p)| op_contents(&p.ops).any(|c| c.spatial.is_some()));
            (!nested).then(|| Arc::new(crate::spatial::Whole::new(l.id.clone(), kind, params, crate::doc_frame(doc), backdrop)))
        });
        out.0.insert(l.id.clone(), whole.flatten());
    }
    out
}

fn plan(
    doc: &Document,
    tx: i32,
    ty: i32,
    ov: Option<&StrokeOverlay>,
    hot: Option<&str>,
    cache: Option<&PlanCache>,
    wholes: &Wholes,
) -> Option<TileProgram> {
    let frame = crate::doc_frame(doc);
    let mut p = Planner {
        doc,
        tx,
        ty,
        ov,
        ops: Vec::new(),
        depth: 1,
        units: Vec::new(),
        hot,
        cache,
        frame,
        regs: vec![0],
        cut: None,
        reveal: Reveal::Clear,
        background: crate::background_reveals(&doc.root),
        snapshot: false,
        spatial: Default::default(),
        wholes,
    };
    p.nodes(&doc.root).ok()?;
    // The paper shades the finished composite (spec 43), after every unit.
    if let Some(paper) = doc.paper() {
        let amt = onepaint_brush::paper::visible_amount(paper.strength());
        if let Some(tex) = onepaint_brush::paper::PaperTexture::new(&paper).filter(|_| amt > 0.0) {
            p.ops.push(Op::Paper { shade: Arc::new(PaperShade { tex, amt }), tx, ty });
        }
    }
    let split = hot.filter(|_| !p.snapshot && !p.spatial.get()).and_then(|_| split(&p.ops, &p.units, [tx * TILE, ty * TILE]));
    let prog = TileProgram { tx, ty, ops: p.ops, split, space: doc.space() };
    let mut seen: Vec<*const Tile> = prog.sources().map(Arc::as_ptr).collect();
    seen.sort_unstable();
    seen.dedup();
    (seen.len() <= MAX_SOURCES).then_some(prog)
}

impl Planner<'_> {
    fn content(&self, l: &Layer) -> Result<Option<Content>, Unsupported> {
        self.content_with(l, None)
    }

    /// A spatial adjustment layer's content (spec 52): its filter over the
    /// layers `below` it in its container, planned on every tile within its
    /// reach. At the top level or in an isolated group that is its whole
    /// backdrop; in a Pass Through group the backdrop reaches outside, so
    /// the tile stays on the CPU.
    fn spatial_content(&self, l: &Layer, kind: onepaint_filters::Kind, below: &[Node]) -> Result<Option<Content>, Unsupported> {
        let params = l.adjustment.as_ref().map(|a| a.params.clone()).unwrap_or_default();
        let r = onepaint_filters::reach(kind, &params);
        if r >= onepaint_filters::GLOBAL_REACH && self.reveal != Reveal::Below {
            // A whole-canvas layer (spec 53): its backdrop was planned before the tiles.
            let whole = self.wholes.0.get(&l.id).cloned().flatten().ok_or(Unsupported)?;
            self.spatial.set(true);
            return self.content_with(l, Some(Arc::new(crate::spatial::SpatialSrc::of_whole(whole, (self.tx, self.ty)))));
        }
        if self.reveal == Reveal::Below || !crate::spatial::gpu_supported(kind, &params) || r > TILE {
            return Err(Unsupported);
        }
        let (tx, ty) = (self.tx, self.ty);
        let bounds = self.doc.bounds();
        let t = onepaint_raster::Rect::from_tile(tx, ty);
        let region = onepaint_raster::Rect::new(t.x - r, t.y - r, t.w + 2 * r, t.h + 2 * r).intersect(&bounds).ok_or(Unsupported)?;
        let mut sub = self.doc.clone_shallow();
        sub.root = below.to_vec();
        sub.source = None;
        sub.set_paper(None);
        let backdrop = region
            .tiles()
            .map(|(x, y)| plan(&sub, x, y, self.ov, None, self.cache, &Wholes::default()).map(|p| ((x, y), Arc::new(p))).ok_or(Unsupported))
            .collect::<Result<Vec<_>, _>>()?;
        self.spatial.set(true);
        let src = crate::spatial::SpatialSrc::new(kind, params, (tx, ty), region, self.frame, backdrop);
        self.content_with(l, Some(Arc::new(src)))
    }

    /// A ghost group's content through its peg (spec 55): the group's
    /// children planned on the tiles the inverse transform reaches, which
    /// the filter resamples. The GPU assembles them in a 3 × 3-tile image,
    /// so a peg that shrinks past that stays on the CPU.
    /// A styled node's content (spec 56): `alone` (the node by itself)
    /// planned on the tiles within the styles' margin, the layers `below`
    /// it on this tile as what its effects blend against, and the styles.
    /// The GPU assembles 3 × 3 tiles, so wider margins stay on the CPU.
    fn styled_content(&self, styles: onepaint_doc::styles::LayerStyles, alone: Vec<Node>, below: &[Node]) -> Result<Content, Unsupported> {
        if self.reveal == Reveal::Below {
            return Err(Unsupported);
        }
        let m = crate::styles::margin(&styles);
        if m > TILE {
            return Err(Unsupported);
        }
        let t = onepaint_raster::Rect::from_tile(self.tx, self.ty);
        let region = onepaint_raster::Rect::new(t.x - m, t.y - m, t.w + 2 * m, t.h + 2 * m).intersect(&self.doc.bounds()).unwrap_or(t);
        let sub = |root: Vec<Node>| {
            let mut d = self.doc.clone_shallow();
            d.root = root;
            d.source = None;
            d.set_paper(None);
            d
        };
        let (own, under) = (sub(alone), sub(below.to_vec()));
        let plan_on =
            |d: &Document, x: i32, y: i32| plan(d, x, y, self.ov, None, self.cache, &Wholes::default()).map(Arc::new).ok_or(Unsupported);
        let content = region.tiles().map(|(x, y)| Ok(((x, y), plan_on(&own, x, y)?))).collect::<Result<Vec<_>, Unsupported>>()?;
        let under = plan_on(&under, self.tx, self.ty)?;
        self.spatial.set(true);
        let styled = crate::spatial::Styled { styles, under };
        let src = crate::spatial::SpatialSrc::of_styled(styled, (self.tx, self.ty), region, self.frame, content);
        Ok(Content {
            base: None,
            stroke: None,
            mask: MaskSrc::None,
            mask_stroke: None,
            relief: None,
            density: 1.0,
            vmask: None,
            spatial: Some(Arc::new(src)),
        })
    }

    /// `None`: nothing over the document reaches the tile.
    fn pegged_content(&self, g: &onepaint_doc::Group) -> Result<Option<Content>, Unsupported> {
        let peg = crate::ghost_peg(g).ok_or(Unsupported)?;
        let (xs, ys) = crate::peg_span(self.frame, peg, self.tx, self.ty);
        if xs.is_empty() || ys.is_empty() {
            return Ok(None);
        }
        let (cols, rows) = (xs.end() - xs.start() + 1, ys.end() - ys.start() + 1);
        if cols > 3 || rows > 3 {
            return Err(Unsupported);
        }
        let mut sub = self.doc.clone_shallow();
        sub.root = g.children.clone();
        sub.source = None;
        sub.set_paper(None);
        let backdrop = ys
            .clone()
            .flat_map(|y| xs.clone().map(move |x| (x, y)))
            .map(|(x, y)| plan(&sub, x, y, self.ov, None, self.cache, &Wholes::default()).map(|p| ((x, y), Arc::new(p))).ok_or(Unsupported))
            .collect::<Result<Vec<_>, _>>()?;
        // The content tiles whole, beyond the document's edge too, as the oracle reads them.
        let region = onepaint_raster::Rect::new(xs.start() * TILE, ys.start() * TILE, cols * TILE, rows * TILE);
        self.spatial.set(true);
        let spatial = Some(Arc::new(crate::spatial::SpatialSrc::of_peg(peg, (self.tx, self.ty), region, self.frame, backdrop)));
        Ok(Some(Content {
            base: None,
            stroke: None,
            mask: MaskSrc::None,
            mask_stroke: None,
            relief: None,
            density: 1.0,
            vmask: None,
            spatial,
        }))
    }

    fn content_with(&self, l: &Layer, spatial: Option<Arc<crate::spatial::SpatialSrc>>) -> Result<Option<Content>, Unsupported> {
        let (tx, ty) = (self.tx, self.ty);
        let on_layer = self.ov.filter(|o| o.layer_id == l.id);
        let src = |o: &StrokeOverlay| o.stroke.tile(tx, ty).map(|t| StrokeSrc { tile: t.clone(), opacity: o.opacity, mode: o.mode });
        let stroke = on_layer.filter(|o| !o.mask).and_then(src);
        let mask_stroke = on_layer.filter(|o| o.mask).and_then(src);
        let base = if spatial.is_some() { None } else { l.pixels.tile(tx, ty).cloned() };
        if base.is_none() && stroke.is_none() && spatial.is_none() {
            return Ok(None);
        }
        let relief = l.height.as_ref().and_then(|h| {
            let tiles = [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)].map(|(dx, dy)| h.tile(tx + dx, ty + dy).cloned());
            let doc = [self.frame.doc_w as i32, self.frame.doc_h as i32];
            tiles.iter().any(Option::is_some).then(|| Arc::new(Relief { tiles, tx, ty, doc }))
        });
        let vmask = match self.vector_cover(l) {
            Some(VCover::Empty) => return Ok(None),
            Some(VCover::Partial(t)) => Some(t),
            Some(VCover::Full) | None => None,
        };
        let mut c = Content { base, stroke, mask: MaskSrc::None, mask_stroke: None, relief, density: 1.0, vmask, spatial };
        if l.mask_enabled
            && let Some(mask) = &l.mask
        {
            c.density = l.mask_density();
            // Feathered, except while the mask is being painted.
            let tile = if mask_stroke.is_none() { self.mask_tile(l, mask) } else { mask.tile(tx, ty).cloned() };
            c.mask = match (tile, &mask_stroke) {
                (Some(t), _) => MaskSrc::Tile(t),
                (None, Some(_)) => MaskSrc::Zero,
                (None, None) if c.density >= 1.0 => return Ok(None),
                (None, None) => MaskSrc::Zero,
            };
            c.mask_stroke = mask_stroke;
        }
        Ok(Some(c))
    }

    /// `l`'s active vector mask over this tile, through the cache.
    fn vector_cover(&self, l: &Layer) -> Option<VCover> {
        let v = l.vector_mask().filter(|v| v.active())?;
        let (tx, ty) = (self.tx, self.ty);
        let Some(cache) = self.cache else {
            return Some(match crate::vmask::cover(&crate::vmask::flatten(&v), tx, ty) {
                crate::vmask::Cover::Empty => VCover::Empty,
                crate::vmask::Cover::Full => VCover::Full,
                crate::vmask::Cover::Partial(t) => VCover::Partial(Arc::new(t)),
            });
        };
        Some(cache.vector_cover(&v, tx, ty))
    }

    /// `l`'s mask tile here, blurred by its feather (`feather.rs`) through
    /// the cache; `None` where it covers nothing (no tile, or nothing in reach).
    fn mask_tile(&self, l: &Layer, mask: &onepaint_raster::Surface) -> Option<Arc<Tile>> {
        let (tx, ty) = (self.tx, self.ty);
        let f = l.mask_feather();
        if f <= 0.0 {
            return mask.tile(tx, ty).cloned();
        }
        let doc = [self.frame.doc_w, self.frame.doc_h];
        let make = || crate::feather::feathered_mask_tile(mask, f, tx, ty, doc).map(Arc::new);
        let Some(cache) = self.cache else { return make() };
        let window = crate::feather::window(mask, f, tx, ty).tiles;
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        window.iter().map(|t| t.as_ref().map(|t| Arc::as_ptr(t) as usize)).for_each(|p| p.hash(&mut h));
        (f.to_bits(), tx, ty, doc).hash(&mut h);
        cache.feathered(h.finish(), &window, make)
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

    /// An adjustment layer (`draw_adjustment`): colour kinds with a GPU form
    /// run there (specs 42, 46), other colour kinds keep the tile on the
    /// CPU, and anything else draws nothing.
    fn adjust(&mut self, l: &Layer) -> Result<(), Unsupported> {
        let Some(adj) = &l.adjustment else { return Ok(()) };
        if !l.visible || l.opacity <= 0.0 {
            return Ok(());
        }
        let Some(kind) = onepaint_filters::Kind::parse(&adj.kind).filter(|k| k.is_color()) else { return Ok(()) };
        let frame = self.frame;
        let make = || {
            let gpu = onepaint_filters::gpu::gpu_adjust(kind, &adj.params, frame)?;
            Some((Arc::new(onepaint_filters::prepare_in(kind, &adj.params, frame)), Arc::new(gpu)))
        };
        let (prepared, gpu) = match self.cache {
            Some(cache) => {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                (&adj.kind, serde_json::to_string(&adj.params).unwrap_or_default()).hash(&mut h);
                (frame.doc_w, frame.doc_h, frame.dpi.to_bits()).hash(&mut h);
                cache.adjust(h.finish(), make)
            }
            None => make(),
        }
        .ok_or(Unsupported)?;
        let density = l.mask_density();
        let mask = match (&l.mask, l.mask_enabled) {
            (Some(m), true) => match self.mask_tile(l, m) {
                Some(t) => MaskSrc::Tile(t),
                None if density >= 1.0 => return Ok(()), // fully masked out on this tile
                None => MaskSrc::Zero,
            },
            _ => MaskSrc::None,
        };
        let vmask = match self.vector_cover(l) {
            Some(VCover::Empty) => return Ok(()),
            Some(VCover::Partial(t)) => Some(t),
            Some(VCover::Full) | None => None,
        };
        let adjust = Arc::new(Adjust { prepared, gpu, at: [self.tx * TILE, self.ty * TILE] });
        let blend_if = if l.clipped { None } else { l.blend_if() };
        self.ops.push(Op::Adjust { adjust, mask, density, vmask, mode: l.blend_mode.effective(), opacity: l.opacity, blend_if });
        Ok(())
    }

    fn push(&mut self, op: Op) -> Result<(), Unsupported> {
        self.depth += 1;
        if self.depth > MAX_STACK {
            return Err(Unsupported);
        }
        self.ops.push(op);
        self.regs.push(self.ops.len());
        Ok(())
    }

    fn pop(&mut self, mode: BlendMode, opacity: f32, clip: bool, tint: Option<[f32; 3]>, blend_if: Option<BlendIf>) {
        self.depth -= 1;
        self.regs.pop();
        self.ops.push(Op::Pop { mode: mode.effective(), opacity, clip, tint, blend_if });
    }

    /// A normal, fully opaque layer replaces its register (`c + top · 0`):
    /// drop what the register held so far. Needs the session's cache, so a
    /// tile's opacity is checked once.
    fn occlude(&mut self, l: &Layer, c: &Content) {
        let Some(cache) = self.cache else { return };
        if self.snapshot && self.depth == 1 {
            return;
        }
        let plain =
            matches!(c.mask, MaskSrc::None) && c.stroke.is_none() && c.mask_stroke.is_none() && c.relief.is_none() && c.vmask.is_none();
        if l.blend_mode != BlendMode::Normal || l.opacity != 1.0 || l.inherit_alpha || !plain || l.blend_if().is_some() {
            return;
        }
        let Some(base) = &c.base else { return };
        if !cache.opaque(base) {
            return;
        }
        let at = self.regs.last().copied().unwrap_or(0);
        self.ops.truncate(at);
        if self.depth == 1 {
            self.cut = Some(self.cut.map_or(at, |c| c.min(at)));
        }
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
                && crate::spatial::spatial_kind(l).is_none()
            {
                let start = self.ops.len();
                self.adjust(l)?;
                if self.depth == 1 {
                    let hot = self.hot.is_some_and(|id| id == l.id);
                    // It reads the backdrop, so it never folds into a suffix image.
                    self.units.push(Unit { start, end: self.ops.len(), hot, folds: false, adjust: true });
                }
                i += 1;
                continue;
            }
            let j = riders(nodes, i);
            let top = self.depth == 1;
            let mut start = self.ops.len();
            match &nodes[i] {
                Node::Group(g) if passes_through(g, &nodes[i + 1..j]) => {
                    if g.visible && g.opacity > 0.0 && !g.children.is_empty() {
                        self.push(Op::PushCopy)?;
                        let outer = std::mem::replace(&mut self.reveal, Reveal::Below);
                        self.nodes(&g.children)?;
                        self.reveal = outer;
                        self.depth -= 1;
                        self.regs.pop();
                        self.ops.push(Op::Mix { opacity: g.opacity });
                    }
                }
                Node::Group(g) => {
                    let clip = self.panel_clip(g);
                    if g.visible && g.opacity > 0.0 && !g.children.is_empty() && !matches!(clip, Some(Clip::Empty)) {
                        let styled = g.styles().filter(|s| s.is_active());
                        if let Some(st) = styled {
                            // Layer styles (spec 56): the register starts as the styled
                            // group; riders and pegs stay on the CPU.
                            let riders = nodes[i + 1..j].iter().any(|r| r.visible() && r.opacity() > 0.0);
                            if riders || g.extra.contains_key(GHOST_PEG_KEY) {
                                return Err(Unsupported);
                            }
                            let content = self.styled_content(st, g.children.clone(), &nodes[..i])?;
                            self.push(Op::PushContent(content))?;
                        } else if g.extra.contains_key(GHOST_PEG_KEY) {
                            // Spec 55: the group's content through its peg, as a register.
                            match self.pegged_content(g)? {
                                Some(content) => self.push(Op::PushContent(content))?,
                                None => self.push(Op::PushZero)?,
                            }
                        } else {
                            self.push(Op::PushZero)?;
                            let outer = std::mem::replace(&mut self.reveal, Reveal::Clear);
                            self.nodes(&g.children)?;
                            self.reveal = outer;
                        }
                        // A panel's frame clips the group before its riders go on.
                        if let Some(Clip::Partial(c)) = clip {
                            self.ops.push(Op::Coverage(c));
                        }
                        self.clipped(&nodes[i + 1..j])?;
                        let tint = g.extra.get(GHOST_TINT_KEY).and_then(|v| serde_json::from_value::<[f32; 3]>(v.clone()).ok());
                        self.pop(g.blend_mode, g.opacity, false, tint, None);
                    }
                }
                Node::Layer(l) => {
                    if l.visible && l.opacity > 0.0 {
                        let family = &nodes[i + 1..j];
                        let styled = l.styles().filter(|s| s.is_active()).filter(|_| l.adjustment.is_none());
                        let content = match (styled, crate::spatial::spatial_kind(l)) {
                            (Some(st), _) => {
                                // Layer styles (spec 56); families, inherit alpha and
                                // knockouts stay on the CPU.
                                if !family.is_empty() || l.inherit_alpha || l.knockout().is_some() {
                                    return Err(Unsupported);
                                }
                                let mut alone = l.clone();
                                (alone.opacity, alone.blend_mode, alone.clipped, alone.visible) = (1.0, BlendMode::Normal, false, true);
                                alone.set_styles(None);
                                alone.set_blend_if(None);
                                alone.set_knockout(None);
                                Some(self.styled_content(st, vec![Node::Layer(alone)], &nodes[..i])?)
                            }
                            (None, Some(kind)) => self.spatial_content(l, kind, &nodes[..i])?,
                            (None, None) => self.content(l)?,
                        };
                        let punch = |c: &Content| Op::Knockout { content: c.clone(), opacity: l.opacity, reveal: self.reveal };
                        let punch = content.as_ref().filter(|_| l.knockout().is_some()).map(punch);
                        if family.is_empty() {
                            if let Some(content) = content {
                                self.occlude(l, &content);
                                self.ops.extend(punch);
                                self.ops.push(Op::Layer {
                                    content,
                                    mode: l.blend_mode.effective(),
                                    opacity: l.opacity,
                                    inherit_alpha: l.inherit_alpha,
                                    // Inherit alpha has no Blend If.
                                    blend_if: l.blend_if().filter(|_| !l.inherit_alpha),
                                });
                            }
                        } else {
                            self.ops.extend(punch);
                            match content {
                                Some(c) => self.push(Op::PushContent(c))?,
                                None => self.push(Op::PushZero)?,
                            }
                            self.clipped(family)?;
                            // The base's Blend If applies to the whole family.
                            self.pop(l.blend_mode, l.opacity, l.inherit_alpha, None, l.blend_if().filter(|_| !l.inherit_alpha));
                        }
                    }
                }
            }
            // The Background is drawn: keep it for the knockouts above.
            if top && i == 0 && self.background {
                self.ops.push(Op::Snapshot);
                self.snapshot = true;
                self.reveal = Reveal::Background;
            }
            if top {
                // An opaque layer cut the ops below it: their units are gone.
                if let Some(c) = self.cut.take() {
                    self.units.retain(|u| u.end <= c);
                    start = start.min(c);
                }
                let hot = self.hot.is_some_and(|id| nodes[i..j].iter().any(|n| contains(n, id)));
                // A Pass Through group reads the backdrop, like its children.
                let through = matches!(&nodes[i], Node::Group(g) if passes_through(g, &nodes[i + 1..j]));
                // So does a knockout: it punches to what is under it.
                let knocks = matches!(&nodes[i], Node::Layer(l) if l.knockout().is_some());
                // A spatial layer reads the backdrop around it (spec 52).
                let spatial = matches!(&nodes[i], Node::Layer(l) if crate::spatial::spatial_kind(l).is_some());
                let folds = !through
                    && !knocks
                    && !spatial
                    && match &nodes[i] {
                        Node::Group(g) => g.blend_mode.effective() == BlendMode::Normal,
                        // Blend If reads the backdrop.
                        Node::Layer(l) => l.blend_mode.effective() == BlendMode::Normal && !l.inherit_alpha && l.blend_if().is_none(),
                    };
                let blend_if = |o: &Op| matches!(o, Op::Layer { blend_if: Some(_), .. } | Op::Pop { blend_if: Some(_), .. });
                let adjust = (through && self.ops[start..].iter().any(|o| matches!(o, Op::Adjust { .. })))
                    || self.ops[start..].iter().any(blend_if)
                    || spatial;
                self.units.push(Unit { start, end: self.ops.len(), hot, folds, adjust });
            }
            i = j;
        }
        Ok(())
    }
}

/// Cut `ops` at the hot unit (spec 38): `None` when no unit holds it.
fn split(ops: &[Op], units: &[Unit], at: [i32; 2]) -> Option<Split> {
    let h = units.iter().position(|u| u.hot)?;
    // An adjustment layer (or a Blend If) at or above the hot one would read
    // the cached f16 image below it, and the stronger ones (hue near grey,
    // threshold, levels, Blend If's tone steps) turn that rounding into
    // visible differences: run unsplit.
    if units[h..].iter().any(|u| u.adjust && !ops[u.start..u.end].is_empty()) {
        return None;
    }
    let hot_unit = &units[h];
    let after = &units[h + 1..];
    // Units are contiguous: every top-level op belongs to one.
    let folds = after.iter().take_while(|u| u.folds).count();
    let fold_end = if folds == 0 { hot_unit.end } else { after[folds - 1].end };
    let worth = |r: &Range<usize>| op_sources(&ops[r.clone()]).nth(MIN_SEGMENT_SOURCES - 1).is_some();
    let segment = |r: Range<usize>| Segment { key: segment_key(&ops[r.clone()], at), range: r };
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
    t.get(i % TILE as usize, i / TILE as usize)
}

/// Doc pixel `i` of the tile whose origin is `at`.
#[inline]
fn pos(at: [i32; 2], i: usize) -> (i32, i32) {
    (at[0] + (i % TILE as usize) as i32, at[1] + (i / TILE as usize) as i32)
}

fn eval(c: &Content, i: usize, space: Space, at: [i32; 2]) -> Px {
    let mut p = match &c.spatial {
        Some(s) => {
            let f = s.filtered();
            [f[i * 4], f[i * 4 + 1], f[i * 4 + 2], f[i * 4 + 3]]
        }
        None => c.base.as_ref().map_or([0.0; 4], |t| load(t, i)),
    };
    if let Some(s) = &c.stroke {
        let (x, y) = pos(at, i);
        p = apply_stroke_px_at(space, p, load(&s.tile, i), s.opacity, s.mode, x, y);
    }
    if let Some(r) = &c.relief {
        let (x, y) = (r.tx * TILE + (i % TILE as usize) as i32, r.ty * TILE + (i / TILE as usize) as i32);
        if x < r.doc[0] && y < r.doc[1] {
            crate::relief::light_px(&mut p, [r.height(x - 1, y), r.height(x + 1, y), r.height(x, y - 1), r.height(x, y + 1)]);
        }
    }
    let m = match &c.mask {
        MaskSrc::None => None,
        MaskSrc::Tile(t) => Some(load(t, i)),
        MaskSrc::Zero => Some([0.0; 4]),
    };
    if let Some(mut m) = m {
        if let Some(s) = &c.mask_stroke {
            m = apply_stroke_px(m, load(&s.tile, i), s.opacity, s.mode);
        }
        let k = mask_k(c.density, m[0]);
        p = p.map(|v| v * k);
    }
    // The vector mask after the pixel mask, as `layer_content` does.
    if let Some(v) = &c.vmask {
        let k = load(v, i)[0];
        p = p.map(|x| x * k);
    }
    p
}

/// Run `ops` on one pixel, starting from the register `init`; `at` is the
/// tile's origin in doc px.
fn exec(ops: &[Op], init: Px, i: usize, stack: &mut Vec<Px>, space: Space, at: [i32; 2]) -> Px {
    stack.clear();
    stack.push(init);
    let (x, y) = pos(at, i);
    let mut background = [0.0; 4];
    for op in ops {
        if let Op::Knockout { content, opacity, reveal } = op {
            let k = eval(content, i, space, at)[3] * opacity;
            let n = stack.len();
            let bg = match reveal {
                Reveal::Clear => [0.0; 4],
                Reveal::Below if n >= 2 => stack[n - 2],
                Reveal::Below => [0.0; 4],
                Reveal::Background => background,
            };
            let Some(top) = stack.last_mut() else { break };
            *top = std::array::from_fn(|c| top[c] + (bg[c] - top[c]) * k);
            continue;
        }
        // The stack is never empty: the planner pairs every push with a pop.
        let Some(top) = stack.last_mut() else { break };
        match op {
            Op::Layer { content, mode, opacity, inherit_alpha, blend_if } => {
                let mut c = eval(content, i, space, at);
                if *inherit_alpha {
                    let a = top[3];
                    c = c.map(|v| v * a);
                }
                *top = blend_at(space, *mode, *top, c, *opacity, x, y, blend_if.as_ref());
            }
            Op::PushZero => stack.push([0.0; 4]),
            Op::PushContent(c) => {
                let v = eval(c, i, space, at);
                stack.push(v);
            }
            Op::PushCopy => {
                let v = *top;
                stack.push(v);
            }
            Op::Snapshot => background = *top,
            Op::Knockout { .. } => {}
            Op::Mix { opacity } => {
                let Some(v) = stack.pop() else { break };
                let Some(dst) = stack.last_mut() else { break };
                *dst = std::array::from_fn(|c| dst[c] * (1.0 - opacity) + v[c] * opacity);
            }
            Op::Clip { content, opacity } => *top = clip_over(*top, eval(content, i, space, at), *opacity),
            Op::Adjust { adjust, mask, density, vmask, mode, opacity, blend_if } => {
                let (x, y) = (adjust.at[0] + (i % TILE as usize) as i32, adjust.at[1] + (i / TILE as usize) as i32);
                let mut o = adjust.prepared.color_px(*top, x, y);
                let m = match mask {
                    MaskSrc::None => None,
                    MaskSrc::Tile(m) => Some(load(m, i)[0]),
                    MaskSrc::Zero => Some(0.0),
                };
                if let Some(m) = m {
                    let k = mask_k(*density, m);
                    o = o.map(|v| v * k);
                }
                if let Some(v) = vmask {
                    let k = load(v, i)[0];
                    o = o.map(|c| c * k);
                }
                *top = blend_at(space, *mode, *top, o, *opacity, x, y, blend_if.as_ref());
            }
            Op::Coverage(c) => {
                let k = c.values[i];
                top.iter_mut().for_each(|v| *v *= k);
            }
            Op::Paper { shade, tx, ty } => {
                if top[..3].iter().any(|v| *v != 0.0) {
                    let (lx, ly) = ((i % TILE as usize) as i32, (i / TILE as usize) as i32);
                    let (x, y) = ((tx * TILE + lx) as f64 + 0.5, (ty * TILE + ly) as f64 + 0.5);
                    let k = onepaint_brush::paper::shade_at(&shade.tex, shade.amt, x, y, space);
                    top[..3].iter_mut().for_each(|v| *v *= k);
                }
            }
            Op::Pop { mode, opacity, clip, tint, blend_if } => {
                let Some(mut v) = stack.pop() else { break };
                if let Some(t) = tint {
                    for c in 0..3 {
                        v[c] += (t[c] * v[3] - v[c]) * 0.65;
                    }
                }
                let Some(dst) = stack.last_mut() else { break };
                *dst =
                    if *clip { clip_over(*dst, v, *opacity) } else { blend_at(space, *mode, *dst, v, *opacity, x, y, blend_if.as_ref()) };
            }
        }
    }
    stack[0]
}

#[allow(clippy::too_many_arguments)]
#[inline]
fn blend_at(space: Space, mode: BlendMode, bp: Px, src: Px, opacity: f32, x: i32, y: i32, b: Option<&BlendIf>) -> Px {
    match b {
        Some(b) => blend_px_if(space, mode, bp, src, opacity, x, y, b),
        None => blend_px_at(space, mode, bp, src, opacity, x, y),
    }
}

/// Run a program on the CPU: premultiplied linear f32, `TILE × TILE`.
pub fn run(prog: &TileProgram) -> Vec<f32> {
    let mut out = vec![0.0; TILE_PX * 4];
    let mut stack: Vec<Px> = Vec::with_capacity(MAX_STACK);
    for i in 0..TILE_PX {
        let v = exec(&prog.ops, [0.0; 4], i, &mut stack, prog.space, prog.origin());
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
        let at = prog.origin();
        let base = s.prefix.as_ref().map_or([0.0; 4], |p| exec(prog.at(&p.range), [0.0; 4], i, &mut stack, prog.space, at));
        let mut v = exec(prog.at(&s.hot), base, i, &mut stack, prog.space, at);
        if let Some(sfx) = &s.suffix {
            let img = exec(prog.at(&sfx.range), [0.0; 4], i, &mut stack, prog.space, at);
            v = blend_px_in(prog.space, BlendMode::Normal, v, img, 1.0);
        }
        v = exec(prog.at(&s.tail), v, i, &mut stack, prog.space, at);
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
