//! GPU compositor (spec 13): runs `compose::program` tile programs in a
//! compute pass, writing straight into the canvas composite texture.
//!
//! Source tiles stay resident in `Rgba16Float` texture-array atlases. A slot
//! is keyed on the tile's `Arc` pointer and holds a `Weak`, which keeps the
//! allocation (so the address can't be reused) without pinning the pixels or
//! forcing copy-on-write clones. A tile changed through `Arc::make_mut` moves
//! to a new address and is uploaded again. Slots are evicted least recently
//! used, never from the batch being dispatched.
//!
//! Spec 38: a program split at the hot layer starts from a cached image of
//! the layers below and lays a cached image of the layers above over its
//! result. Those images live in a cache texture array keyed on the
//! segment's content key; a missing one is filled by a pass that runs just
//! before the composite.

use crate::sheets::Sheets;
use onepaint_compose::program::{Content, MaskSrc, Op, Reveal, Segment, StrokeSrc, TileProgram, op_sources, stroke_code};
use onepaint_doc::{BlendIf, BlendMode};
use onepaint_raster::{TILE, Tile};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};
use wgpu::util::DeviceExt;

pub const COMPOSE_WGSL: &str = include_str!("../shaders/compose.wgsl");
/// Blend modes and the sRGB transfer, shared with the layer-style pass.
pub const BLEND_WGSL: &str = include_str!("../shaders/blend.wgsl");

/// Atlas textures the shader binds.
const ATLASES: usize = 8;
/// Tiles per dispatch (the z workgroup count limit).
const MAX_JOBS: usize = 65_535;
/// A slot with its mip levels (a third more than level 0).
const TILE_BYTES: u64 = (TILE as u64) * (TILE as u64) * 8 * 4 / 3;
/// Default GPU memory for resident layer tiles (spec 13).
pub const DEFAULT_BUDGET: u64 = 1 << 30;
/// Default GPU memory for cached segment images (spec 38): two images (the
/// layers below and above the hot one) for each of a frame's 256 tiles, with
/// room to spare (576 slots). At 256 MiB (384 slots) a 205-tile drag
/// thrashed: 76 segments were recomposited every frame, tripling its time.
pub const DEFAULT_CACHE_BUDGET: u64 = 384 << 20;

/// Texel reads a whole-canvas refilter may take mid-stroke (spec 53): about
/// 8 ms on the integrated GPU it was measured on (radial blur 4096² is
/// 6.4 G reads in 173 ms, tilt-shift 2048² 1.7 G in 56 ms).
const INTERACTIVE_READS: u64 = 256 << 20;
/// Atlas formats: f16, and 8-bit (`free` and `grow` index by these).
const F16: usize = 0;
const U8: usize = 1;
/// "Not used" in a job's `init` and `dest`.
const NONE: u32 = u32::MAX;
const OP_OVER_CACHE: u32 = 5;
const OP_ADJUST: u32 = 6;
const OP_PAPER: u32 = 7;
const OP_COVERAGE: u32 = 8;
const OP_PUSH_COPY: u32 = 9;
const OP_MIX: u32 = 10;
const OP_KNOCKOUT: u32 = 11;
const OP_SNAPSHOT: u32 = 12;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    /// The target's size at this level.
    doc_size: [u32; 2],
    per_atlas: u32,
    /// Index of the first general job (the simple ones come first).
    general_base: u32,
    /// Level of detail (spec 39): sources are read from this mip.
    level: u32,
    /// Sheets across, and the sheet size at this level (spec 40).
    cols: u32,
    sheet: [u32; 2],
    /// 1 when pixel values are sRGB and blend as they are (spec 50).
    gamma: u32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Job {
    origin: [u32; 2],
    op_start: u32,
    op_count: u32,
    /// Cache slot to start from, or `NONE`.
    init: u32,
    /// Cache slot to write, or `NONE` for the canvas.
    dest: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuOp([u32; OP_WORDS]);

const F_INHERIT: u32 = 1;
const F_TINT: u32 = 2;
const F_BASE: u32 = 4;
const F_STROKE: u32 = 8;
const F_MASK: u32 = 16;
const F_MASK_ZERO: u32 = 32;
const F_MASK_STROKE: u32 = 64;
const F_RELIEF: u32 = 128;
const F_VMASK: u32 = 256;
/// The content's pixels are a filtered tile in the segment cache (spec 52).
const F_SPATIAL: u32 = 512;
/// The spatial content is a whole-canvas layer's filtered image (spec 53).
const F_WHOLE: u32 = 1024;
/// Five vec4<u32>s (compose.wgsl `Op`).
const OP_WORDS: usize = 20;

struct Slot {
    idx: u32,
    _tile: Weak<Tile>,
    used: u64,
    /// Its levels 1..=L_MAX are filled (spec 39; done when first needed).
    mipped: bool,
}

/// A cached segment image. The `Weak`s keep its sources' addresses from
/// being reused, so a stale key can never match new content.
struct CacheSlot {
    idx: u32,
    _sources: Vec<Weak<Tile>>,
    used: u64,
}

struct PassBuffers {
    params: wgpu::Buffer,
    jobs: wgpu::Buffer,
    ops: wgpu::Buffer,
    /// Colour adjustment params and LUTs (spec 42).
    adj: wgpu::Buffer,
}

impl PassBuffers {
    fn new(device: &wgpu::Device) -> Self {
        Self {
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("compose params"),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            jobs: storage(device, "compose jobs", 64 * std::mem::size_of::<Job>() as u64),
            ops: storage(device, "compose ops", 1024 * std::mem::size_of::<GpuOp>() as u64),
            adj: storage(device, "compose adjust", 4096),
        }
    }
}

/// One job's ops before encoding: what it runs, where it starts and ends.
struct Work<'a> {
    origin: [u32; 2],
    ops: Vec<&'a Op>,
    /// Lay this cache slot over the result after `ops[..over_at]`.
    over: Option<(usize, u32)>,
    init: u32,
    dest: u32,
}

/// Whether any blend in the ops is Pigment (the pipeline needs the KM code).
fn uses_pigment(ops: &[&Op]) -> bool {
    let pig = |m: BlendMode| m == BlendMode::Pigment;
    let stroke = |s: &Option<StrokeSrc>| s.as_ref().is_some_and(|s| pig(stroke_code(s.mode).1));
    ops.iter().any(|op| match op {
        Op::Layer { content, mode, .. } => pig(*mode) || stroke(&content.stroke) || stroke(&content.mask_stroke),
        Op::PushContent(c) | Op::Clip { content: c, .. } | Op::Knockout { content: c, .. } => stroke(&c.stroke) || stroke(&c.mask_stroke),
        Op::Pop { mode, .. } | Op::Adjust { mode, .. } => pig(*mode),
        Op::PushZero | Op::Paper { .. } | Op::Coverage(_) | Op::PushCopy | Op::Mix { .. } | Op::Snapshot => false,
    })
}

/// Whether every op is a plain normal layer, which the lean pipeline runs.
fn is_simple(ops: &[&Op]) -> bool {
    ops.iter().all(|op| {
        matches!(
            op,
            Op::Layer {
                content: Content { base: Some(_), stroke: None, mask: MaskSrc::None, mask_stroke: None, relief: None, vmask: None, .. },
                mode: BlendMode::Normal,
                inherit_alpha: false,
                blend_if: None,
                ..
            }
        )
    })
}

/// Lay cache slot `slot` over the register (`S + (1 − S.a) · x`).
fn over_cache(slot: u32) -> GpuOp {
    let mut o = [0u32; OP_WORDS];
    (o[0], o[1]) = (OP_OVER_CACHE, slot);
    GpuOp(o)
}

/// A batch's colour adjustment params and LUTs (spec 42), each distinct
/// adjustment stored once.
#[derive(Default)]
struct AdjTable {
    data: Vec<[f32; 4]>,
    seen: HashMap<u64, (u32, u32)>,
    /// By address: tiles planned with the session's cache share one.
    by_ptr: HashMap<usize, (u32, u32)>,
}

impl AdjTable {
    /// Where `a`'s params and LUT start (in vec4s); the LUT is `NONE` if absent.
    fn add(&mut self, a: &Arc<onepaint_compose::program::GpuAdjust>) -> (u32, u32) {
        let ptr = Arc::as_ptr(a) as usize;
        if let Some(&at) = self.by_ptr.get(&ptr) {
            return at;
        }
        let at = self.add_new(a);
        self.by_ptr.insert(ptr, at);
        at
    }

    fn add_new(&mut self, a: &onepaint_compose::program::GpuAdjust) -> (u32, u32) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (a.code, a.params.map(f32::to_bits), &a.lut).hash(&mut h);
        a.table.iter().flatten().for_each(|e| e.map(f32::to_bits).hash(&mut h));
        *self.seen.entry(h.finish()).or_insert_with(|| {
            let at = self.data.len() as u32;
            self.data.extend(a.params.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]));
            let lut = self.data.len() as u32;
            if let Some(l) = &a.lut {
                self.data.extend(l.iter().map(|e| e.map(|v| v as f32 / 255.0)));
            } else if let Some(r) = &a.table {
                self.data.extend_from_slice(r);
            } else {
                return (at, NONE);
            }
            (at, lut)
        })
    }

    /// Blend If's eight ranges (gray, red, green, blue; This Layer then
    /// Underlying Layer) as vec4s; the op's mode word carries `at + 1`.
    fn blend_if(&mut self, b: &BlendIf) -> u32 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (0xb1fu32, b).hash(&mut h);
        let data = &mut self.data;
        let (at, _) = *self.seen.entry(h.finish()).or_insert_with(|| {
            let at = data.len() as u32;
            for c in &b.0 {
                data.push(c.src.map(f32::from));
                data.push(c.dst.map(f32::from));
            }
            (at, NONE)
        });
        at + 1
    }
}

/// An op's mode word: the blend mode, and its Blend If's place in the table + 1 (0: none).
fn mode_word(mode: BlendMode, b: &Option<BlendIf>, adj: &mut AdjTable) -> u32 {
    mode as u32 | b.as_ref().map_or(0, |b| adj.blend_if(b)) << 8
}

fn key(t: &Arc<Tile>) -> usize {
    Arc::as_ptr(t) as usize
}

pub struct GpuCompositor {
    /// Plain normal layers only (keeps registers low, so occupancy high).
    simple: wgpu::ComputePipeline,
    /// The full interpreter, without and with the Kubelka–Munk (Pigment mode) code.
    pipelines: [wgpu::ComputePipeline; 2],
    layout: wgpu::BindGroupLayout,
    km: wgpu::Buffer,
    /// Params, jobs and ops buffers, one set per pass of a submit: each is
    /// written before the submit, so passes can't share one (spec 52's
    /// staged fills made that visible). `pass` counts the submit's passes.
    bufs: Vec<PassBuffers>,
    pass: usize,
    dummy: wgpu::TextureView,
    /// Atlases of either format: `Rgba16Float`, or `Rgba8Unorm` for 8-bit
    /// documents' tiles (half the memory; their bytes exactly).
    atlases: Vec<(wgpu::Texture, wgpu::TextureView)>,
    per_atlas: u32,
    /// GPU memory the atlases may take.
    budget: u64,
    slots: HashMap<usize, Slot>,
    /// Free slots in f16 atlases, then in 8-bit ones.
    free: [Vec<u32>; 2],
    clock: u64,
    /// Tiles uploaded since creation (for tests and stats).
    pub uploads: u64,
    /// Segment images: one texture array, read in the composite pass and
    /// written in the fill pass.
    /// The cache texture, its all-levels read view, and a write view per level.
    cache: Option<(wgpu::Texture, wgpu::TextureView, Vec<wgpu::TextureView>)>,
    cache_layers: u32,
    cache_slots: HashMap<u64, CacheSlot>,
    cache_free: Vec<u32>,
    /// Stand-in for the cache's storage view in the composite pass.
    dummy_store: wgpu::TextureView,
    /// Stand-in canvas target for a warm-up, which writes only the cache.
    dummy_target: wgpu::TextureView,
    /// Use split programs' cached segments (spec 38).
    pub split_cache: bool,
    /// Walk the next over-budget frame backwards.
    reverse: bool,
    mipper: crate::mip::Mipper,
    /// The level the current run composites at (spec 39).
    level: u32,
    /// The current run's programs hold sRGB values (spec 50).
    gamma: bool,
    /// The paper grain (spec 43), held so its identity stays unique, and a stand-in.
    paper: Option<(Arc<onepaint_brush::TipMask>, wgpu::TextureView)>,
    paper_dummy: wgpu::TextureView,
    /// Segment images computed since creation (for tests and stats).
    pub fills: u64,
    /// Spatial adjustment layers' filter (spec 52), and where this batch's
    /// filtered tiles are, by their source's key.
    spatial: crate::spatial::SpatialPass,
    /// Layer styles' pass (spec 56).
    styles: crate::styles::StylesPass,
    /// The live whole-canvas layer's filtered image (spec 53).
    whole: crate::whole::WholePass,
    /// A stroke is in progress (see [`GpuCompositor::set_interactive`]).
    interactive: bool,
    /// Tiles a run couldn't composite (their sources didn't fit): the
    /// caller composites them on the CPU ([`GpuCompositor::take_failed`]).
    failed: Vec<(i32, i32)>,
    spatial_slots: HashMap<u64, u32>,
    /// Filtered tiles: written by the spatial pass, read by fills and composites.
    filtered: Option<(wgpu::Texture, wgpu::TextureView, Vec<wgpu::TextureView>)>,
    filtered_layers: u32,
    filtered_slots: HashMap<u64, CacheSlot>,
    filtered_free: Vec<u32>,
    /// Filtered tiles computed since creation (for tests and stats).
    pub filters: u64,
}

fn storage(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl GpuCompositor {
    /// `budget` is the GPU memory for resident tiles, in bytes; segment
    /// images get [`DEFAULT_CACHE_BUDGET`] on top.
    pub fn new(device: &wgpu::Device, budget: u64) -> Self {
        Self::with_cache(device, budget, DEFAULT_CACHE_BUDGET)
    }

    /// Atlases of `layers` slots each, one of them: a working set that
    /// can't fit, for tests of what the CPU then takes.
    #[doc(hidden)]
    pub fn with_atlas_layers(device: &wgpu::Device, layers: u32) -> Self {
        let mut c = Self::new(device, 0);
        c.per_atlas = layers.max(1);
        c
    }

    /// As [`GpuCompositor::new`], with `cache_budget` bytes for segment images.
    pub fn with_cache(device: &wgpu::Device, budget: u64, cache_budget: u64) -> Self {
        let source = format!("{BLEND_WGSL}\n{COMPOSE_WGSL}\n{}", crate::wet::PIGMENT_WGSL);
        let module = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("compose"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let cs = wgpu::ShaderStages::COMPUTE;
        let buffer = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let mut entries = vec![
            buffer(0, wgpu::BufferBindingType::Uniform),
            buffer(1, wgpu::BufferBindingType::Uniform),
            buffer(2, wgpu::BufferBindingType::Storage { read_only: true }),
            buffer(3, wgpu::BufferBindingType::Storage { read_only: true }),
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: cs,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba16Float,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                },
                count: None,
            },
        ];
        for i in 0..ATLASES as u32 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 5 + i,
                visibility: cs,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 13,
            visibility: cs,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2Array,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 14,
            visibility: cs,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba16Float,
                view_dimension: wgpu::TextureViewDimension::D2Array,
            },
            count: None,
        });
        entries.push(buffer(15, wgpu::BufferBindingType::Storage { read_only: true }));
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 16,
            visibility: cs,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 17,
            visibility: cs,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2Array,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 18,
            visibility: cs,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("compose"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compose"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |general: bool, pigment: bool| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(match (general, pigment) {
                    (false, _) => "compose (simple)",
                    (true, false) => "compose",
                    (true, true) => "compose (pigment)",
                }),
                layout: Some(&pl),
                module: &module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("GENERAL", f64::from(u8::from(general))), ("PIGMENT", f64::from(u8::from(pigment)))],
                    ..Default::default()
                },
                cache: None,
            })
        };
        let simple = pipeline(false, false);
        let pipelines = [pipeline(true, false), pipeline(true, true)];
        let (b, t) = onepaint_color::pigment::basis_matrices();
        let mut km: Vec<f32> = b.iter().flat_map(|r| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0]).collect();
        km.extend((0..9).flat_map(|k| [t[0][k] as f32, t[1][k] as f32, t[2][k] as f32, 0.0]));
        let km = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("compose km"),
            contents: bytemuck::cast_slice(&km),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let dummy = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("compose atlas (unused)"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        // WebGPU guarantees 256 layers; a program never needs more than `MAX_SOURCES` (128).
        let per_atlas = device.limits().max_texture_array_layers.clamp(1, 256);
        let cache_layers = ((cache_budget / TILE_BYTES) as u32).min(device.limits().max_texture_array_layers);
        let dummy_store = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("compose cache (unused)"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let dummy_target = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("compose target (unused)"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        Self {
            simple,
            pipelines,
            layout,
            km,
            bufs: Vec::new(),
            pass: 0,
            dummy,
            atlases: Vec::new(),
            per_atlas,
            budget,
            slots: HashMap::new(),
            free: [Vec::new(), Vec::new()],
            clock: 0,
            uploads: 0,
            cache: None,
            cache_layers,
            cache_slots: HashMap::new(),
            cache_free: Vec::new(),
            dummy_store,
            dummy_target,
            split_cache: cache_layers >= 2,
            reverse: false,
            mipper: crate::mip::Mipper::new(device),
            level: 0,
            gamma: false,
            paper: None,
            paper_dummy: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("compose paper (unused)"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R32Float,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
            fills: 0,
            spatial: crate::spatial::SpatialPass::new(device),
            styles: crate::styles::StylesPass::new(device),
            whole: crate::whole::WholePass::new(device),
            interactive: false,
            failed: Vec::new(),
            spatial_slots: HashMap::new(),
            filtered: None,
            filtered_layers: (cache_layers / 2).clamp(16, device.limits().max_texture_array_layers),
            filtered_slots: HashMap::new(),
            filtered_free: Vec::new(),
            filters: 0,
        }
    }

    /// Slots the atlases can hold at most, in f16 atlases (8-bit tiles fit
    /// at least as many).
    /// Whether a stroke is in progress: a whole-canvas filter heavier than
    /// [`INTERACTIVE_READS`] then keeps its image until it ends (spec 53).
    pub fn set_interactive(&mut self, on: bool) {
        self.interactive = on;
    }

    /// The tiles runs since the last call couldn't composite, because what
    /// they read didn't fit the atlases or the cache. Their pixels on the
    /// target are left as they were; composite them on the CPU.
    pub fn take_failed(&mut self) -> Vec<(i32, i32)> {
        std::mem::take(&mut self.failed)
    }

    /// Drop the cache and filtered slots claimed this batch for `dests` and
    /// `outs`: their fills didn't run, so the images aren't there.
    fn forget(&mut self, dests: &HashSet<u32>, outs: &HashSet<u32>) {
        let clock = self.clock;
        let (mut cache_free, mut filtered_free) = (Vec::new(), Vec::new());
        self.cache_slots.retain(|_, c| {
            let gone = c.used == clock && dests.contains(&c.idx);
            if gone {
                cache_free.push(c.idx);
            }
            !gone
        });
        self.filtered_slots.retain(|_, c| {
            let gone = c.used == clock && outs.contains(&c.idx);
            if gone {
                filtered_free.push(c.idx);
            }
            !gone
        });
        self.cache_free.extend(cache_free);
        self.filtered_free.extend(filtered_free);
    }

    pub fn capacity(&self) -> usize {
        self.per_atlas as usize * ((self.budget / self.atlas_bytes(F16)) as usize).clamp(1, ATLASES)
    }

    /// How many f16 and 8-bit atlases exist (for tests and stats).
    pub fn atlas_counts(&self) -> [usize; 2] {
        let u8s = (0..self.atlases.len()).filter(|&i| self.format_of(i) == U8).count();
        [self.atlases.len() - u8s, u8s]
    }

    fn atlas_bytes(&self, format: usize) -> u64 {
        self.per_atlas as u64 * TILE_BYTES / if format == U8 { 2 } else { 1 }
    }

    fn format_of(&self, atlas: usize) -> usize {
        usize::from(self.atlases[atlas].0.format() == wgpu::TextureFormat::Rgba8Unorm)
    }

    /// Add an atlas of `format` while the budget allows; the first of each
    /// format always fits.
    fn grow(&mut self, device: &wgpu::Device, format: usize) -> bool {
        if self.atlases.len() >= ATLASES {
            return false;
        }
        let used: u64 = (0..self.atlases.len()).map(|i| self.atlas_bytes(self.format_of(i))).sum();
        let first = !(0..self.atlases.len()).any(|i| self.format_of(i) == format);
        if !first && used + self.atlas_bytes(format) > self.budget {
            return false;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("compose atlas"),
            size: wgpu::Extent3d { width: TILE as u32, height: TILE as u32, depth_or_array_layers: self.per_atlas },
            mip_level_count: crate::mip::LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if format == U8 { wgpu::TextureFormat::Rgba8Unorm } else { wgpu::TextureFormat::Rgba16Float },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let view =
            tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let base = self.atlases.len() as u32 * self.per_atlas;
        self.atlases.push((tex, view));
        self.free[format].extend((base..base + self.per_atlas).rev());
        true
    }

    /// The least recently used slot of `format` outside the current batch.
    fn evict(&mut self, format: usize) -> Option<u32> {
        let clock = self.clock;
        let per = self.per_atlas;
        let (&k, s) = self
            .slots
            .iter()
            .filter(|(_, s)| s.used != clock && self.format_of((s.idx / per) as usize) == format)
            .min_by_key(|(_, s)| s.used)?;
        let idx = s.idx;
        self.slots.remove(&k);
        Some(idx)
    }

    /// Make a tile resident for the current batch; returns its slot.
    fn resident(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &Arc<Tile>) -> Option<u32> {
        let k = key(t);
        if let Some(s) = self.slots.get_mut(&k) {
            s.used = self.clock;
            return Some(s.idx);
        }
        // An 8-bit tile prefers an 8-bit atlas and fits an f16 one exactly;
        // an f16 tile only fits an f16 atlas.
        let mut slot = |f: usize| match self.free[f].pop() {
            Some(i) => Some(i),
            None if self.grow(device, f) => self.free[f].pop(),
            None => self.evict(f),
        };
        let idx = match t.unorm8_data() {
            Some(_) => slot(U8).or_else(|| slot(F16))?,
            None => slot(F16)?,
        };
        let atlas_i = (idx / self.per_atlas) as usize;
        let (atlas, _) = &self.atlases[atlas_i];
        let origin = wgpu::TexelCopyTextureInfo {
            texture: atlas,
            mip_level: 0,
            origin: wgpu::Origin3d { x: 0, y: 0, z: idx % self.per_atlas },
            aspect: wgpu::TextureAspect::All,
        };
        let size = wgpu::Extent3d { width: TILE as u32, height: TILE as u32, depth_or_array_layers: 1 };
        match t.unorm8_data().filter(|_| self.format_of(atlas_i) == U8) {
            Some(bytes) => queue.write_texture(
                origin,
                bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(TILE as u32 * 4), rows_per_image: Some(TILE as u32) },
                size,
            ),
            None => queue.write_texture(
                origin,
                bytemuck::cast_slice(&t.f16_data()),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(TILE as u32 * 8), rows_per_image: Some(TILE as u32) },
                size,
            ),
        }
        self.uploads += 1;
        self.slots.insert(k, Slot { idx, _tile: Arc::downgrade(t), used: self.clock, mipped: false });
        Some(idx)
    }

    fn slot(&self, t: &Arc<Tile>) -> u32 {
        // Every source was made resident before encoding (see `flush`).
        self.slots.get(&key(t)).map_or(0, |s| s.idx)
    }

    fn encode_content(&self, o: &mut [u32; OP_WORDS], c: &Content) {
        let stroke = |s: &StrokeSrc| {
            let (kind, mode) = stroke_code(s.mode);
            (self.slot(&s.tile), kind | (mode.effective() as u32) << 8, s.opacity.to_bits())
        };
        if let Some(b) = &c.base {
            o[3] |= F_BASE;
            o[4] = self.slot(b);
        }
        if let Some(s) = &c.spatial {
            o[3] |= F_SPATIAL;
            if s.whole.is_some() {
                o[3] |= F_WHOLE;
            }
            o[4] = self.spatial_slots.get(&s.key).copied().unwrap_or(0);
        }
        if let Some(s) = &c.stroke {
            o[3] |= F_STROKE;
            (o[5], o[6], o[7]) = stroke(s);
        }
        // Spec 45: five height slots, two to a word (0xffff: no tile).
        if let Some(r) = &c.relief {
            o[3] |= F_RELIEF;
            let [cc, l, rt, u, d] = r.tiles.each_ref().map(|t| t.as_ref().map_or(0xffff, |t| self.slot(t) & 0xffff));
            (o[12], o[13], o[14]) = (cc | l << 16, rt | u << 16, d);
        }
        self.encode_vmask(o, &c.vmask);
        if !self.encode_mask(o, &c.mask, c.density) {
            return;
        }
        if let Some(s) = &c.mask_stroke {
            o[3] |= F_MASK_STROKE;
            (o[9], o[10], o[11]) = stroke(s);
        }
    }

    /// The vector mask's coverage slot in `e.x`.
    fn encode_vmask(&self, o: &mut [u32; OP_WORDS], v: &Option<Arc<Tile>>) {
        if let Some(t) = v {
            o[3] |= F_VMASK;
            o[16] = self.slot(t);
        }
    }

    /// The mask's slot and flags, and its density in `d.w`; false without one.
    fn encode_mask(&self, o: &mut [u32; OP_WORDS], m: &MaskSrc, density: f32) -> bool {
        match m {
            MaskSrc::None => return false,
            MaskSrc::Tile(t) => {
                o[3] |= F_MASK;
                o[8] = self.slot(t);
            }
            MaskSrc::Zero => o[3] |= F_MASK | F_MASK_ZERO,
        }
        o[15] = density.to_bits();
        true
    }

    fn encode(&self, op: &Op, adj: &mut AdjTable) -> GpuOp {
        let mut o = [0u32; OP_WORDS];
        match op {
            Op::Coverage(c) => (o[0], o[8]) = (OP_COVERAGE, self.slot(&c.tile)),
            Op::Paper { shade, .. } => {
                o[0] = OP_PAPER;
                (o[4], o[5]) = ((shade.tex.tile_px() as f32).to_bits(), shade.amt.to_bits());
                // A procedural paper's look (spec 57): mottle, a flag, the fleck colour.
                if let Some(l) = shade.tex.look() {
                    (o[6], o[7]) = (l.mottle.to_bits(), 1);
                    (o[8], o[9], o[10]) = (l.fleck[0].to_bits(), l.fleck[1].to_bits(), l.fleck[2].to_bits());
                }
            }
            Op::Adjust { adjust, mask, density, vmask, mode, opacity, blend_if } => {
                self.encode_vmask(&mut o, vmask);
                (o[0], o[1], o[2]) = (OP_ADJUST, mode_word(*mode, blend_if, adj), opacity.to_bits());
                (o[4], o[5]) = adj.add(&adjust.gpu);
                o[6] = adjust.gpu.code;
                self.encode_mask(&mut o, mask, *density);
            }
            Op::Layer { content, mode, opacity, inherit_alpha, blend_if } => {
                (o[0], o[1], o[2]) = (0, mode_word(*mode, blend_if, adj), opacity.to_bits());
                if *inherit_alpha {
                    o[3] |= F_INHERIT;
                }
                self.encode_content(&mut o, content);
            }
            Op::PushZero => o[0] = 1,
            Op::PushCopy => o[0] = OP_PUSH_COPY,
            Op::Mix { opacity } => (o[0], o[2]) = (OP_MIX, opacity.to_bits()),
            Op::Snapshot => o[0] = OP_SNAPSHOT,
            Op::Knockout { content, opacity, reveal } => {
                let r = match reveal {
                    Reveal::Clear => 0,
                    Reveal::Below => 1,
                    Reveal::Background => 2,
                };
                (o[0], o[1], o[2]) = (OP_KNOCKOUT, r, opacity.to_bits());
                self.encode_content(&mut o, content);
            }
            Op::PushContent(c) => {
                o[0] = 2;
                self.encode_content(&mut o, c);
            }
            Op::Clip { content, opacity } => {
                (o[0], o[2]) = (3, opacity.to_bits());
                self.encode_content(&mut o, content);
            }
            Op::Pop { mode, opacity, clip, tint, blend_if } => {
                (o[0], o[1], o[2]) = (4, mode_word(*mode, blend_if, adj), opacity.to_bits());
                if *clip {
                    o[3] |= F_INHERIT;
                }
                if let Some(t) = tint {
                    o[3] |= F_TINT;
                    (o[12], o[13], o[14]) = (t[0].to_bits(), t[1].to_bits(), t[2].to_bits());
                }
            }
        }
        GpuOp(o)
    }

    /// Composite `programs` into `target`, a `D2Array` view of one
    /// `Rgba16Float` storage layer of the document size. Edge tiles are
    /// clipped to the document.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        programs: &[TileProgram],
        target: &wgpu::TextureView,
        doc_size: [u32; 2],
    ) {
        self.run_level(device, queue, programs, target, &Sheets::single(doc_size), 0);
    }

    /// [`GpuCompositor::run`] into composite sheets laid out as `sheets`
    /// (spec 40), at level of detail `level` (spec 39): `target` is that
    /// level's view; tiles are `256 >> level` pixels and read their sources'
    /// mip `level`.
    pub fn run_level(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        programs: &[TileProgram],
        target: &wgpu::TextureView,
        sheets: &Sheets,
        level: u32,
    ) {
        self.level = level.min(crate::mip::L_MAX);
        self.gamma = programs.first().is_some_and(|p| p.space == onepaint_color::Space::Gamma);
        // A whole-canvas layer's image comes first: every tile reads it (spec 53).
        let whole = programs
            .iter()
            .flat_map(|p| onepaint_compose::program::op_contents(&p.ops))
            .find_map(|c| c.spatial.as_ref().and_then(|s| s.whole.clone()));
        let mut kept: Vec<&TileProgram> = programs.iter().collect();
        if let Some(w) = whole
            && !self.prepare_whole(device, queue, &w, sheets)
        {
            // Every tile reading the layer's image fails with it.
            let reads = |p: &TileProgram| {
                onepaint_compose::program::op_contents(&p.ops).any(|c| c.spatial.as_ref().is_some_and(|s| s.whole.is_some()))
            };
            self.failed.extend(programs.iter().filter(|p| reads(p)).map(|p| (p.tx, p.ty)));
            kept.retain(|p| !reads(p));
        }
        let cap = self.capacity();
        // Each program may need two cache slots; keep a batch within the cache.
        let cache_cap = (self.cache_layers as usize / 2).max(1);
        let mut order = kept;
        // A frame bigger than the atlas runs in several batches. Alternate the
        // direction each time, so the LRU keeps what the next frame starts on
        // instead of evicting exactly that (photocraft gpu/src/lib.rs:886-930).
        let total: HashSet<usize> = programs.iter().flat_map(|p| self.batch_sources(p)).map(key).collect();
        if total.len() > cap {
            if self.reverse {
                order.reverse();
            }
            self.reverse = !self.reverse;
        }
        let mut start = 0;
        let mut need: HashSet<usize> = HashSet::new();
        // Slots this batch's spatial layers take (spec 52): their backdrops in
        // the cache, stacked ones' too, and their results in the filtered tiles.
        fn spatial_need(ops: &[Op]) -> (usize, usize) {
            // A whole-canvas layer reads its own image (spec 53).
            onepaint_compose::program::op_contents(ops).filter_map(|c| c.spatial.as_ref()).filter(|s| s.whole.is_none()).fold(
                (0, 0),
                |(c, f), s| {
                    let (bc, bf) = s.backdrop.iter().map(|(_, p)| spatial_need(&p.ops)).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
                    // A styled source's backdrop takes a slot too (spec 56).
                    let (uc, uf) = s.styled.as_ref().map_or((0, 0), |st| {
                        let (c, f) = spatial_need(&st.under.ops);
                        (c + 1, f)
                    });
                    (c + s.backdrop.len() + bc + uc, f + 1 + bf + uf)
                },
            )
        }
        let (mut cache_used, mut filtered_used) = (0, 0);
        for (i, p) in order.iter().enumerate() {
            let mut fresh: Vec<usize> = self.batch_sources(p).map(key).filter(|k| !need.contains(k)).collect();
            fresh.sort_unstable();
            fresh.dedup();
            let (more, more_f) = spatial_need(&p.ops);
            let full = need.len() + fresh.len() > cap
                || i - start >= MAX_JOBS
                || (self.split_cache && i - start >= cache_cap)
                || (more > 0 && cache_used + more > self.cache_layers as usize)
                || (more_f > 0 && filtered_used + more_f > self.filtered_layers as usize);
            if i > start && full {
                self.flush(device, queue, &order[start..i], target, sheets);
                start = i;
                need.clear();
                (cache_used, filtered_used) = (0, 0);
            }
            need.extend(fresh);
            cache_used += more;
            filtered_used += more_f;
        }
        if start < order.len() {
            self.flush(device, queue, &order[start..], target, sheets);
        }
    }

    /// The tiles a program may read in a batch: with a split, the hot and
    /// tail ops plus any segment that isn't cached yet.
    fn batch_sources<'a>(&self, p: &'a TileProgram) -> Box<dyn Iterator<Item = &'a Arc<Tile>> + 'a> {
        match (&p.split, self.split_cache) {
            (Some(s), true) => {
                let missing = |seg: &'a Option<Segment>| {
                    seg.as_ref()
                        .filter(|g| !self.cache_slots.contains_key(&self.level_key(g.key)))
                        .into_iter()
                        .flat_map(|g| op_sources(p.at(&g.range)))
                };
                Box::new(op_sources(p.at(&s.hot)).chain(op_sources(p.at(&s.tail))).chain(missing(&s.prefix)).chain(missing(&s.suffix)))
            }
            _ => Box::new(p.sources()),
        }
    }

    fn ensure_cache(&mut self, device: &wgpu::Device) {
        if self.cache.is_some() {
            return;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("compose segment cache"),
            size: wgpu::Extent3d { width: TILE as u32, height: TILE as u32, depth_or_array_layers: self.cache_layers },
            mip_level_count: crate::mip::LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let read =
            tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let write = (0..crate::mip::LEVELS).map(|l| crate::mip::level_view(&tex, l)).collect();
        self.cache = Some((tex, read, write));
        self.cache_free = (0..self.cache_layers).rev().collect();
    }

    /// The cache slot for a segment this batch: `(slot, needs a fill)`.
    fn cache_slot(&mut self, seg: &Segment, ops: &[Op]) -> Option<(u32, bool)> {
        self.cache_slot_key(seg.key, ops)
    }

    /// The cache slot for an image keyed `key` that `ops` read: `(slot, new)`.
    fn cache_slot_key(&mut self, key: u64, ops: &[Op]) -> Option<(u32, bool)> {
        let clock = self.clock;
        let key = self.level_key(key);
        if let Some(c) = self.cache_slots.get_mut(&key) {
            c.used = clock;
            return Some((c.idx, false));
        }
        let idx = match self.cache_free.pop() {
            Some(i) => i,
            None => {
                let (&k, c) = self.cache_slots.iter().filter(|(_, c)| c.used != clock).min_by_key(|(_, c)| c.used)?;
                let idx = c.idx;
                self.cache_slots.remove(&k);
                idx
            }
        };
        let sources = op_sources(ops).map(Arc::downgrade).collect();
        self.cache_slots.insert(key, CacheSlot { idx, _sources: sources, used: clock });
        Some((idx, true))
    }

    /// A segment's image at the current level (spec 39) has its own key.
    fn level_key(&self, key: u64) -> u64 {
        key ^ u64::from(self.level).wrapping_mul(0x9E37_79B9_7F4A_7C15)
    }

    /// Plan a batch's jobs: segment fills first, then the composites.
    fn plan_batch<'a>(&mut self, device: &wgpu::Device, batch: &[&'a TileProgram]) -> (Vec<Work<'a>>, Vec<Work<'a>>) {
        let mut fills: Vec<Work<'a>> = Vec::new();
        let mut main: Vec<Work<'a>> = Vec::new();
        for &p in batch {
            let edge = (TILE as u32) >> self.level;
            let origin = [p.tx as u32 * edge, p.ty as u32 * edge];
            let split = p.split.as_ref().filter(|_| self.split_cache);
            let Some(s) = split else {
                main.push(Work { origin, ops: p.ops.iter().collect(), over: None, init: NONE, dest: NONE });
                continue;
            };
            self.ensure_cache(device);
            let mut slot = |seg: &'a Option<Segment>, fills: &mut Vec<Work<'a>>| -> Option<Option<u32>> {
                let Some(seg) = seg else { return Some(None) };
                let ops = p.at(&seg.range);
                let (idx, fresh) = self.cache_slot(seg, ops)?;
                if fresh {
                    // The origin only places relief taps; the image goes to cache slot `idx`.
                    fills.push(Work { origin, ops: ops.iter().collect(), over: None, init: NONE, dest: idx });
                }
                Some(Some(idx))
            };
            let (Some(prefix), Some(suffix)) = (slot(&s.prefix, &mut fills), slot(&s.suffix, &mut fills)) else {
                // The cache is full of this batch's own segments: run unsplit.
                main.push(Work { origin, ops: p.ops.iter().collect(), over: None, init: NONE, dest: NONE });
                continue;
            };
            let mut ops: Vec<&Op> = p.at(&s.hot).iter().collect();
            let over = suffix.map(|idx| (ops.len(), idx));
            ops.extend(p.at(&s.tail));
            main.push(Work { origin, ops, over, init: prefix.unwrap_or(NONE), dest: NONE });
        }
        (fills, main)
    }

    /// The filter jobs for the spatial layers `batch` draws (spec 52), in
    /// stages: a stage's backdrop fills, then its filters. A layer stacked on
    /// another comes a stage later, as its backdrop reads the lower one's
    /// filtered tiles.
    fn plan_spatial<'a>(&mut self, device: &wgpu::Device, batch: &[&'a TileProgram]) -> Vec<(Vec<Work<'a>>, Vec<crate::spatial::Job<'a>>)> {
        use onepaint_compose::program::op_contents;
        use onepaint_compose::spatial::SpatialSrc;
        self.spatial_slots.clear();
        // Every source the batch reads, with its stage (1 + its backdrops' deepest).
        fn walk<'a>(ops: &'a [Op], depth: &mut HashMap<u64, usize>, out: &mut Vec<&'a SpatialSrc>) -> usize {
            let mut deepest = 0;
            for c in op_contents(ops) {
                let Some(s) = c.spatial.as_deref().filter(|s| s.whole.is_none()) else { continue };
                let d = match depth.get(&s.key) {
                    Some(&d) => d,
                    None => {
                        // A styled source also waits for what's under it (spec 56).
                        let under = s.styled.as_ref().map_or(0, |st| walk(&st.under.ops, depth, out));
                        let d = 1 + s.backdrop.iter().map(|(_, p)| walk(&p.ops, depth, out)).max().unwrap_or(0).max(under);
                        depth.insert(s.key, d);
                        out.push(s);
                        d
                    }
                };
                deepest = deepest.max(d);
            }
            deepest
        }
        let mut depth = HashMap::new();
        let mut sources = Vec::new();
        for &p in batch {
            walk(&p.ops, &mut depth, &mut sources);
        }
        let stages = sources.iter().map(|s| depth[&s.key]).max().unwrap_or(0);
        let mut out: Vec<(Vec<Work<'a>>, Vec<crate::spatial::Job<'a>>)> = (0..stages).map(|_| (Vec::new(), Vec::new())).collect();
        let edge = (TILE as u32) >> self.level;
        for s in sources {
            self.ensure_cache(device);
            self.ensure_filtered(device);
            let sources: Vec<Weak<Tile>> = s.backdrop.iter().flat_map(|(_, p)| op_sources(&p.ops)).map(Arc::downgrade).collect();
            let Some((idx, fresh)) = self.filtered_slot(s.key, sources) else {
                log::error!("compose filtered images full; a spatial layer at {},{} skipped", s.tx, s.ty);
                continue;
            };
            self.spatial_slots.insert(s.key, idx);
            if !fresh {
                continue;
            }
            let (fills, jobs) = &mut out[depth[&s.key] - 1];
            let gx = s.backdrop.iter().map(|b| b.0.0).min().unwrap_or(s.tx);
            let gy = s.backdrop.iter().map(|b| b.0.1).min().unwrap_or(s.ty);
            let cols = (s.backdrop.iter().map(|b| b.0.0).max().unwrap_or(s.tx) - gx + 1) as usize;
            let rows = (s.backdrop.iter().map(|b| b.0.1).max().unwrap_or(s.ty) - gy + 1) as usize;
            let mut grid = vec![crate::spatial::NONE; cols * rows];
            for ((x, y), bp) in &s.backdrop {
                let key = onepaint_compose::program::segment_key(&bp.ops, bp.origin());
                let Some((slot, fresh)) = self.cache_slot_key(key, &bp.ops) else { continue };
                if fresh {
                    fills.push(Work {
                        origin: [*x as u32 * edge, *y as u32 * edge],
                        ops: bp.ops.iter().collect(),
                        over: None,
                        init: NONE,
                        dest: slot,
                    });
                }
                grid[(*y - gy) as usize * cols + (*x - gx) as usize] = slot;
            }
            // A styled source's backdrop: the layers under it on this tile.
            let mut under = crate::spatial::NONE;
            if let Some(st) = &s.styled
                && !st.under.ops.is_empty()
            {
                let key = onepaint_compose::program::segment_key(&st.under.ops, st.under.origin());
                if let Some((slot, fresh)) = self.cache_slot_key(key, &st.under.ops) {
                    if fresh {
                        fills.push(Work {
                            origin: [s.tx as u32 * edge, s.ty as u32 * edge],
                            ops: st.under.ops.iter().collect(),
                            over: None,
                            init: NONE,
                            dest: slot,
                        });
                    }
                    under = slot;
                }
            }
            jobs.push(crate::spatial::Job { src: s, out: idx, grid, under });
        }
        out
    }

    /// Filter a whole-canvas layer's backdrop into its image (spec 53), if
    /// the image doesn't already hold it: fill the backdrop tiles whose
    /// segment changed (in chunks the cache holds), copy each into the
    /// backdrop image, then one pass over the document.
    ///
    /// False when the backdrop didn't fit: the image is then unusable.
    fn prepare_whole(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, w: &onepaint_compose::spatial::Whole, sheets: &Sheets) -> bool {
        let wkey = self.level_key(w.key);
        let doc = [w.frame.doc_w, w.frame.doc_h];
        self.whole.back(device, doc, self.level);
        if self.whole.key == Some(wkey) && self.whole.view().is_some() {
            return true;
        }
        // Mid-stroke, a filter too heavy for the frame keeps this layer's
        // image until the stroke ends, as the CPU path does.
        let size = crate::whole::WholePass::level_size(doc, self.level);
        let reads = u64::from(size[0]) * u64::from(size[1]) * crate::whole::reads_per_px(w.kind, &w.params);
        if self.interactive
            && reads > INTERACTIVE_READS
            && self.whole.layer.as_deref() == Some(w.id.as_str())
            && self.whole.view().is_some()
        {
            return true;
        }
        self.ensure_cache(device);
        let edge = (TILE as u32) >> self.level;
        let changed: Vec<_> = w
            .backdrop
            .iter()
            .map(|(at, bp)| (*at, bp, self.level_key(onepaint_compose::program::segment_key(&bp.ops, bp.origin()))))
            .filter(|(at, _, k)| self.whole.back_tiles.get(at) != Some(k))
            .collect();
        // Chunks the cache and the atlases hold: a chunk's fills and their
        // sources stay resident until its copies run.
        let most = (self.cache_layers as usize / 2).clamp(1, MAX_JOBS);
        let cap = self.capacity();
        let mut parts: Vec<&[_]> = Vec::new();
        let (mut start, mut seen) = (0, HashSet::new());
        for (i, (_, bp, _)) in changed.iter().enumerate() {
            let fresh = op_sources(&bp.ops).map(key).filter(|k| !seen.contains(k)).count();
            if i > start && (i - start >= most || seen.len() + fresh > cap) {
                parts.push(&changed[start..i]);
                start = i;
                seen.clear();
            }
            seen.extend(op_sources(&bp.ops).map(key));
        }
        if start < changed.len() {
            parts.push(&changed[start..]);
        }
        for part in parts {
            self.clock += 1;
            let mut copies = Vec::new();
            let mut fills = Vec::new();
            for ((x, y), bp, k) in part {
                let seg = onepaint_compose::program::segment_key(&bp.ops, bp.origin());
                let Some((slot, fresh)) = self.cache_slot_key(seg, &bp.ops) else { continue };
                if fresh {
                    fills.push(Work {
                        origin: [*x as u32 * edge, *y as u32 * edge],
                        ops: bp.ops.iter().collect(),
                        over: None,
                        init: NONE,
                        dest: slot,
                    });
                }
                copies.push(((*x, *y), slot, *k));
            }
            self.fills += fills.len() as u64;
            let mut fit = copies.len() == part.len();
            for wk in &fills {
                for t in wk.ops.iter().flat_map(|op| op_sources(std::slice::from_ref(*op))) {
                    fit &= self.resident(device, queue, t).is_some();
                }
            }
            if !fit {
                log::warn!("compose atlas full; a whole-canvas layer goes to the CPU");
                let dests = fills.iter().map(|w| w.dest).collect();
                self.forget(&dests, &HashSet::new());
                self.whole.key = None;
                return false;
            }
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("compose whole backdrop") });
            self.mip_sources(device, &mut enc, fills.iter());
            self.dispatch(device, queue, &mut enc, &fills, sheets, None, true);
            let back = self.whole.back(device, doc, self.level);
            if let Some((cache, _, _)) = &self.cache {
                for ((x, y), slot, _) in &copies {
                    let (ox, oy) = (*x as u32 * edge, *y as u32 * edge);
                    let (cw, ch) = (edge.min(size[0].saturating_sub(ox)), edge.min(size[1].saturating_sub(oy)));
                    if cw == 0 || ch == 0 {
                        continue;
                    }
                    enc.copy_texture_to_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: cache,
                            mip_level: self.level,
                            origin: wgpu::Origin3d { x: 0, y: 0, z: *slot },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: back,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: ox, y: oy, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
                    );
                }
            }
            queue.submit([enc.finish()]);
            self.pass = 0;
            for (at, _, k) in copies {
                self.whole.back_tiles.insert(at, k);
            }
        }
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("compose whole") });
        self.whole.record(device, &mut enc, w, self.level);
        self.filters += 1;
        queue.submit([enc.finish()]);
        self.whole.key = Some(wkey);
        self.whole.layer = Some(w.id.clone());
        true
    }

    fn ensure_filtered(&mut self, device: &wgpu::Device) {
        if self.filtered.is_some() {
            return;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("compose filtered tiles"),
            size: wgpu::Extent3d { width: TILE as u32, height: TILE as u32, depth_or_array_layers: self.filtered_layers },
            mip_level_count: crate::mip::LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let read =
            tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let write = (0..crate::mip::LEVELS).map(|l| crate::mip::level_view(&tex, l)).collect();
        self.filtered = Some((tex, read, write));
        self.filtered_free = (0..self.filtered_layers).rev().collect();
    }

    /// The slot for a filtered tile keyed `key` (at this level): `(slot, new)`.
    fn filtered_slot(&mut self, key: u64, sources: Vec<Weak<Tile>>) -> Option<(u32, bool)> {
        let clock = self.clock;
        let key = self.level_key(key);
        if let Some(c) = self.filtered_slots.get_mut(&key) {
            c.used = clock;
            return Some((c.idx, false));
        }
        let idx = match self.filtered_free.pop() {
            Some(i) => i,
            None => {
                let (&k, c) = self.filtered_slots.iter().filter(|(_, c)| c.used != clock).min_by_key(|(_, c)| c.used)?;
                let idx = c.idx;
                self.filtered_slots.remove(&k);
                idx
            }
        };
        self.filtered_slots.insert(key, CacheSlot { idx, _sources: sources, used: clock });
        Some((idx, true))
    }

    fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, batch: &[&TileProgram], target: &wgpu::TextureView, sheets: &Sheets) {
        self.clock += 1;
        let (fills, main) = self.plan_batch(device, batch);
        let stages = self.plan_spatial(device, batch);
        self.fills += fills.len() as u64;
        let stage_fills = stages.iter().flat_map(|(f, _)| f.iter());
        let mut fit = true;
        for w in fills.iter().chain(&main).chain(stage_fills) {
            for t in w.ops.iter().flat_map(|op| op_sources(std::slice::from_ref(*op))) {
                fit &= self.resident(device, queue, t).is_some();
            }
        }
        if !fit {
            // What the batch reads doesn't fit: leave its tiles as they are
            // for the CPU, and forget the cache images it claimed.
            log::warn!("compose atlas full; {} tiles go to the CPU", batch.len());
            let dests = fills.iter().chain(stages.iter().flat_map(|(f, _)| f.iter())).map(|w| w.dest).collect();
            let outs = stages.iter().flat_map(|(_, j)| j.iter().map(|j| j.out)).collect();
            self.forget(&dests, &outs);
            self.failed.extend(batch.iter().map(|p| (p.tx, p.ty)));
            return;
        }
        self.ensure_paper(device, queue, main.iter().flat_map(|w| w.ops.iter()));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("compose") });
        self.mip_sources(device, &mut enc, fills.iter().chain(&main).chain(stages.iter().flat_map(|(f, _)| f.iter())));
        // Fills and composites can't share a dispatch: one writes the cache, the other reads it.
        self.dispatch(device, queue, &mut enc, &fills, sheets, Some(target), true);
        // Spatial layers stage by stage (spec 52): their backdrops (which may
        // read a lower layer's filtered tiles), then their filters.
        for (fills, jobs) in &stages {
            self.dispatch(device, queue, &mut enc, fills, sheets, Some(target), true);
            if let (Some((_, read, _)), Some((_, _, write))) = (&self.cache, &self.filtered) {
                for job in jobs {
                    match job.src.styled.as_deref() {
                        Some(styled) => {
                            let j = crate::styles::Job { src: job.src, styled, out: job.out, grid: job.grid.clone(), under: job.under };
                            self.styles.record(device, &mut enc, &j, self.level, read, &write[self.level as usize]);
                        }
                        None => self.spatial.record(device, &mut enc, job, self.level, read, &write[self.level as usize]),
                    }
                }
                self.filters += jobs.len() as u64;
            }
        }
        self.dispatch(device, queue, &mut enc, &main, sheets, Some(target), false);
        // Submit per batch: the next batch's uploads may evict slots this one reads.
        queue.submit([enc.finish()]);
        self.pass = 0;
    }

    /// Upload the paper grain the batch's paper op reads, if it changed (spec 43).
    fn ensure_paper<'a>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, mut ops: impl Iterator<Item = &'a &'a Op>) {
        let Some(Op::Paper { shade, .. }) = ops.find(|op| matches!(op, Op::Paper { .. })) else { return };
        let arc = shade.tex.mask_arc();
        if self.paper.as_ref().is_some_and(|(k, _)| Arc::ptr_eq(k, arc)) {
            return;
        }
        let mask = arc.as_ref();
        let n = mask.size as u32;
        // Height, and a procedural paper's mottle and fleck (0.5 = none).
        let look = shade.tex.look();
        let texels: Vec<[f32; 4]> = (0..mask.data.len())
            .map(|i| match look {
                Some(l) => [mask.data[i], l.mottle_tile.data[i], l.fleck_tile.data[i], 0.0],
                None => [mask.data[i], 0.5, 0.5, 0.0],
            })
            .collect();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("compose paper"),
            size: wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            tex.as_image_copy(),
            bytemuck::cast_slice(&texels),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(n * 16), rows_per_image: Some(n) },
            wgpu::Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        );
        self.paper = Some((arc.clone(), tex.create_view(&Default::default())));
    }

    /// Above level 0, fill the mips of the slots `works` read that lack them.
    fn mip_sources<'a>(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, works: impl Iterator<Item = &'a Work<'a>>) {
        if self.level == 0 {
            return;
        }
        let mut per_atlas: Vec<Vec<u32>> = vec![Vec::new(); self.atlases.len()];
        for w in works {
            for t in w.ops.iter().flat_map(|op| op_sources(std::slice::from_ref(*op))) {
                if let Some(s) = self.slots.get_mut(&key(t))
                    && !s.mipped
                {
                    s.mipped = true;
                    per_atlas[(s.idx / self.per_atlas) as usize].push(s.idx % self.per_atlas);
                }
            }
        }
        for (i, layers) in per_atlas.iter().enumerate() {
            for chunk in layers.chunks(MAX_JOBS) {
                self.mipper.slots(device, enc, &self.atlases[i].0, chunk);
            }
        }
    }

    /// Record one pass over `works`: the fill pass writes cache slots, the
    /// composite pass writes `target`.
    #[allow(clippy::too_many_arguments)]
    fn dispatch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        works: &[Work],
        sheets: &Sheets,
        target: Option<&wgpu::TextureView>,
        fill: bool,
    ) {
        if works.is_empty() {
            return;
        }
        let (jobs, ops, simple, adj) = self.encode_jobs(works);
        let general = works.len() - simple;
        let pigment = works.iter().filter(|w| !is_simple(&w.ops)).any(|w| uses_pigment(&w.ops));
        let params = Params {
            doc_size: sheets.doc_at(self.level),
            per_atlas: self.per_atlas,
            general_base: simple as u32,
            level: self.level,
            sheet: sheets.sheet_at(self.level),
            cols: sheets.grid[0],
            gamma: u32::from(self.gamma),
            _pad: [0; 3],
        };
        let bind = self.bind(device, queue, (&jobs, &ops, &adj), params, target, fill);
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(if fill { "compose fill" } else { "compose" }),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &bind, &[]);
        let n = ((TILE as u32) >> self.level).div_ceil(8);
        if simple > 0 {
            pass.set_pipeline(&self.simple);
            pass.dispatch_workgroups(n, n, simple as u32);
        }
        if general > 0 {
            pass.set_pipeline(&self.pipelines[usize::from(pigment)]);
            pass.dispatch_workgroups(n, n, general as u32);
        }
    }

    /// Jobs and ops for `works`, the lean ones first; returns how many are lean.
    fn encode_jobs(&self, works: &[Work]) -> (Vec<Job>, Vec<GpuOp>, usize, Vec<[f32; 4]>) {
        let (simple, general): (Vec<&Work>, Vec<&Work>) = works.iter().partition(|w| is_simple(&w.ops));
        let mut jobs = Vec::with_capacity(works.len());
        let mut ops = Vec::new();
        let mut adj = AdjTable::default();
        for w in simple.iter().chain(&general) {
            let op_start = ops.len() as u32;
            for (i, op) in w.ops.iter().enumerate() {
                if let Some((at, slot)) = w.over
                    && at == i
                {
                    ops.push(over_cache(slot));
                }
                ops.push(self.encode(op, &mut adj));
            }
            if let Some((at, slot)) = w.over
                && at == w.ops.len()
            {
                ops.push(over_cache(slot));
            }
            jobs.push(Job { origin: w.origin, op_start, op_count: ops.len() as u32 - op_start, init: w.init, dest: w.dest });
        }
        if ops.is_empty() {
            ops.push(GpuOp::default());
        }
        if adj.data.is_empty() {
            adj.data.push([0.0; 4]);
        }
        (jobs, ops, simple.len(), adj.data)
    }

    #[allow(clippy::too_many_arguments)]
    fn bind(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        (jobs, ops, adj): (&[Job], &[GpuOp], &[[f32; 4]]),
        params: Params,
        target: Option<&wgpu::TextureView>,
        fill: bool,
    ) -> wgpu::BindGroup {
        // Each pass has its own buffers: all are written before the submit.
        let n = self.pass;
        self.pass += 1;
        if self.bufs.len() <= n {
            self.bufs.push(PassBuffers::new(device));
        }
        let b = &mut self.bufs[n];
        let jobs_bytes: &[u8] = bytemuck::cast_slice(jobs);
        let ops_bytes: &[u8] = bytemuck::cast_slice(ops);
        if b.jobs.size() < jobs_bytes.len() as u64 {
            b.jobs = storage(device, "compose jobs", (jobs_bytes.len() as u64).next_power_of_two());
        }
        if b.ops.size() < ops_bytes.len() as u64 {
            b.ops = storage(device, "compose ops", (ops_bytes.len() as u64).next_power_of_two());
        }
        let adj_bytes: &[u8] = bytemuck::cast_slice(adj);
        if b.adj.size() < adj_bytes.len() as u64 {
            b.adj = storage(device, "compose adjust", (adj_bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&b.jobs, 0, jobs_bytes);
        queue.write_buffer(&b.ops, 0, ops_bytes);
        queue.write_buffer(&b.adj, 0, adj_bytes);
        queue.write_buffer(&b.params, 0, bytemuck::bytes_of(&params));
        let b = &self.bufs[n];
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: b.params.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: self.km.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: b.jobs.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: b.ops.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(target.unwrap_or(&self.dummy_target)) },
        ];
        for i in 0..ATLASES {
            let view = self.atlases.get(i).map_or(&self.dummy, |(_, v)| v);
            entries.push(wgpu::BindGroupEntry { binding: 5 + i as u32, resource: wgpu::BindingResource::TextureView(view) });
        }
        let (read, write) = match (&self.cache, fill) {
            (Some((_, _, w)), true) => (&self.dummy, &w[self.level as usize]),
            (Some((_, r, _)), false) => (r, &self.dummy_store),
            (None, _) => (&self.dummy, &self.dummy_store),
        };
        entries.push(wgpu::BindGroupEntry { binding: 13, resource: wgpu::BindingResource::TextureView(read) });
        entries.push(wgpu::BindGroupEntry { binding: 14, resource: wgpu::BindingResource::TextureView(write) });
        entries.push(wgpu::BindGroupEntry { binding: 15, resource: b.adj.as_entire_binding() });
        let paper = self.paper.as_ref().map_or(&self.paper_dummy, |(_, v)| v);
        entries.push(wgpu::BindGroupEntry { binding: 16, resource: wgpu::BindingResource::TextureView(paper) });
        let filtered = self.filtered.as_ref().map_or(&self.dummy, |(_, r, _)| r);
        entries.push(wgpu::BindGroupEntry { binding: 17, resource: wgpu::BindingResource::TextureView(filtered) });
        let whole = self.whole.view().unwrap_or(&self.paper_dummy);
        entries.push(wgpu::BindGroupEntry { binding: 18, resource: wgpu::BindingResource::TextureView(whole) });
        device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("compose"), layout: &self.layout, entries: &entries })
    }

    /// Fill the cache images `programs` will need, without compositing
    /// (spec 38 warm-up, run on idle frames after the active layer changes).
    /// `doc_size` places relief taps at the document's edge.
    pub fn warm(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, programs: &[TileProgram], doc_size: [u32; 2]) {
        self.warm_level(device, queue, programs, &Sheets::single(doc_size), 0);
    }

    /// [`GpuCompositor::warm`] for the cache images at `level` (spec 39).
    pub fn warm_level(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, programs: &[TileProgram], sheets: &Sheets, level: u32) {
        self.level = level.min(crate::mip::L_MAX);
        self.gamma = programs.first().is_some_and(|p| p.space == onepaint_color::Space::Gamma);
        if !self.split_cache {
            return;
        }
        let cache_cap = (self.cache_layers as usize / 2).max(1);
        let all: Vec<&TileProgram> = programs.iter().collect();
        for chunk in all.chunks(cache_cap) {
            self.clock += 1;
            let (fills, _) = self.plan_batch(device, chunk);
            if fills.is_empty() {
                continue;
            }
            self.fills += fills.len() as u64;
            for w in &fills {
                for t in w.ops.iter().flat_map(|op| op_sources(std::slice::from_ref(*op))) {
                    if self.resident(device, queue, t).is_none() {
                        log::error!("compose atlas full; a warm-up fill skipped");
                    }
                }
            }
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("compose warm") });
            self.mip_sources(device, &mut enc, fills.iter());
            // Fills write cache slots only, but relief clamps its taps to the document.
            self.dispatch(device, queue, &mut enc, &fills, sheets, None, true);
            queue.submit([enc.finish()]);
            self.pass = 0;
        }
    }

    /// Cached segment images held.
    pub fn cached_segments(&self) -> usize {
        self.cache_slots.len()
    }

    /// Tiles currently resident.
    pub fn resident_tiles(&self) -> usize {
        self.slots.len()
    }
}
