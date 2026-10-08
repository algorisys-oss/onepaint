//! GPU compositor (spec 13): runs `compose::program` tile programs in a
//! compute pass, writing straight into the canvas composite texture.
//!
//! Source tiles stay resident in `Rgba16Float` texture-array atlases. A slot
//! is keyed on the tile's `Arc` pointer and holds a `Weak`, which keeps the
//! allocation (so the address can't be reused) without pinning the pixels or
//! forcing copy-on-write clones. A tile changed through `Arc::make_mut` moves
//! to a new address and is uploaded again. Slots are evicted least recently
//! used, never from the batch being dispatched.

use onepaint_compose::program::{Content, MaskSrc, Op, StrokeSrc, TileProgram, stroke_code};
use onepaint_doc::BlendMode;
use onepaint_raster::{TILE, Tile};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};
use wgpu::util::DeviceExt;

pub const COMPOSE_WGSL: &str = include_str!("../shaders/compose.wgsl");

/// Atlas textures the shader binds.
const ATLASES: usize = 8;
/// Tiles per dispatch (the z workgroup count limit).
const MAX_JOBS: usize = 65_535;
const TILE_BYTES: u64 = (TILE as u64) * (TILE as u64) * 8;
/// Default GPU memory for resident layer tiles (spec 13).
pub const DEFAULT_BUDGET: u64 = 1 << 30;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    doc_size: [u32; 2],
    per_atlas: u32,
    /// Index of the first general job (the simple ones come first).
    general_base: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Job {
    origin: [u32; 2],
    op_start: u32,
    op_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuOp([u32; 16]);

const F_INHERIT: u32 = 1;
const F_TINT: u32 = 2;
const F_BASE: u32 = 4;
const F_STROKE: u32 = 8;
const F_MASK: u32 = 16;
const F_MASK_ZERO: u32 = 32;
const F_MASK_STROKE: u32 = 64;

struct Slot {
    idx: u32,
    _tile: Weak<Tile>,
    used: u64,
}

/// Whether any blend in the program is Pigment (the pipeline needs the KM code).
fn uses_pigment(p: &TileProgram) -> bool {
    let pig = |m: BlendMode| m == BlendMode::Pigment;
    let stroke = |s: &Option<StrokeSrc>| s.as_ref().is_some_and(|s| pig(stroke_code(s.mode).1));
    p.ops.iter().any(|op| match op {
        Op::Layer { content, mode, .. } => pig(*mode) || stroke(&content.stroke) || stroke(&content.mask_stroke),
        Op::PushContent(c) | Op::Clip { content: c, .. } => stroke(&c.stroke) || stroke(&c.mask_stroke),
        Op::Pop { mode, .. } => pig(*mode),
        Op::PushZero => false,
    })
}

/// Whether every op is a plain normal layer, which the lean pipeline runs.
fn is_simple(p: &TileProgram) -> bool {
    p.ops.iter().all(|op| {
        matches!(
            op,
            Op::Layer {
                content: Content { base: Some(_), stroke: None, mask: MaskSrc::None, mask_stroke: None },
                mode: BlendMode::Normal,
                inherit_alpha: false,
                ..
            }
        )
    })
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
    params: wgpu::Buffer,
    km: wgpu::Buffer,
    jobs: wgpu::Buffer,
    ops: wgpu::Buffer,
    dummy: wgpu::TextureView,
    atlases: Vec<(wgpu::Texture, wgpu::TextureView)>,
    per_atlas: u32,
    max_atlases: usize,
    slots: HashMap<usize, Slot>,
    free: Vec<u32>,
    clock: u64,
    /// Tiles uploaded since creation (for tests and stats).
    pub uploads: u64,
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
    /// `budget` is the GPU memory for resident tiles, in bytes.
    pub fn new(device: &wgpu::Device, budget: u64) -> Self {
        let source = format!("{COMPOSE_WGSL}\n{}", crate::wet::PIGMENT_WGSL);
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
                    view_dimension: wgpu::TextureViewDimension::D2,
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
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compose params"),
            size: std::mem::size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
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
        let max_atlases = ((budget / TILE_BYTES) as usize).div_ceil(per_atlas as usize).clamp(1, ATLASES);
        Self {
            simple,
            pipelines,
            layout,
            params,
            km,
            jobs: storage(device, "compose jobs", 64 * std::mem::size_of::<Job>() as u64),
            ops: storage(device, "compose ops", 1024 * std::mem::size_of::<GpuOp>() as u64),
            dummy,
            atlases: Vec::new(),
            per_atlas,
            max_atlases,
            slots: HashMap::new(),
            free: Vec::new(),
            clock: 0,
            uploads: 0,
        }
    }

    /// Slots the atlases can hold at most.
    pub fn capacity(&self) -> usize {
        self.per_atlas as usize * self.max_atlases
    }

    fn grow(&mut self, device: &wgpu::Device) -> bool {
        if self.atlases.len() >= self.max_atlases {
            return false;
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("compose atlas"),
            size: wgpu::Extent3d { width: TILE as u32, height: TILE as u32, depth_or_array_layers: self.per_atlas },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view =
            tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let base = self.atlases.len() as u32 * self.per_atlas;
        self.atlases.push((tex, view));
        self.free.extend((base..base + self.per_atlas).rev());
        true
    }

    /// The least recently used slot outside the current batch.
    fn evict(&mut self) -> Option<u32> {
        let clock = self.clock;
        let (&k, s) = self.slots.iter().filter(|(_, s)| s.used != clock).min_by_key(|(_, s)| s.used)?;
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
        let idx = match self.free.pop() {
            Some(i) => i,
            None if self.grow(device) => self.free.pop()?,
            None => self.evict()?,
        };
        let (atlas, _) = &self.atlases[(idx / self.per_atlas) as usize];
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: atlas,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: idx % self.per_atlas },
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&t.data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(TILE as u32 * 8), rows_per_image: Some(TILE as u32) },
            wgpu::Extent3d { width: TILE as u32, height: TILE as u32, depth_or_array_layers: 1 },
        );
        self.uploads += 1;
        self.slots.insert(k, Slot { idx, _tile: Arc::downgrade(t), used: self.clock });
        Some(idx)
    }

    fn slot(&self, t: &Arc<Tile>) -> u32 {
        // Every source was made resident before encoding (see `flush`).
        self.slots.get(&key(t)).map_or(0, |s| s.idx)
    }

    fn encode_content(&self, o: &mut [u32; 16], c: &Content) {
        let stroke = |s: &StrokeSrc| {
            let (kind, mode) = stroke_code(s.mode);
            (self.slot(&s.tile), kind | (mode as u32) << 8, s.opacity.to_bits())
        };
        if let Some(b) = &c.base {
            o[3] |= F_BASE;
            o[4] = self.slot(b);
        }
        if let Some(s) = &c.stroke {
            o[3] |= F_STROKE;
            (o[5], o[6], o[7]) = stroke(s);
        }
        match &c.mask {
            MaskSrc::None => return,
            MaskSrc::Tile(t) => {
                o[3] |= F_MASK;
                o[8] = self.slot(t);
            }
            MaskSrc::Zero => o[3] |= F_MASK | F_MASK_ZERO,
        }
        if let Some(s) = &c.mask_stroke {
            o[3] |= F_MASK_STROKE;
            (o[9], o[10], o[11]) = stroke(s);
        }
    }

    fn encode(&self, op: &Op) -> GpuOp {
        let mut o = [0u32; 16];
        match op {
            Op::Layer { content, mode, opacity, inherit_alpha } => {
                (o[0], o[1], o[2]) = (0, *mode as u32, opacity.to_bits());
                if *inherit_alpha {
                    o[3] |= F_INHERIT;
                }
                self.encode_content(&mut o, content);
            }
            Op::PushZero => o[0] = 1,
            Op::PushContent(c) => {
                o[0] = 2;
                self.encode_content(&mut o, c);
            }
            Op::Clip { content, opacity } => {
                (o[0], o[2]) = (3, opacity.to_bits());
                self.encode_content(&mut o, content);
            }
            Op::Pop { mode, opacity, clip, tint } => {
                (o[0], o[1], o[2]) = (4, *mode as u32, opacity.to_bits());
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

    /// Composite `programs` into `target` (an `Rgba16Float` storage texture of
    /// the document size). Edge tiles are clipped to the document.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        programs: &[TileProgram],
        target: &wgpu::TextureView,
        doc_size: [u32; 2],
    ) {
        let cap = self.capacity();
        let mut start = 0;
        let mut need: HashSet<usize> = HashSet::new();
        for (i, p) in programs.iter().enumerate() {
            let mut fresh: Vec<usize> = p.sources().map(key).filter(|k| !need.contains(k)).collect();
            fresh.sort_unstable();
            fresh.dedup();
            if i > start && (need.len() + fresh.len() > cap || i - start >= MAX_JOBS) {
                self.flush(device, queue, &programs[start..i], target, doc_size);
                start = i;
                need.clear();
            }
            need.extend(fresh);
        }
        if start < programs.len() {
            self.flush(device, queue, &programs[start..], target, doc_size);
        }
    }

    fn flush(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, batch: &[TileProgram], target: &wgpu::TextureView, doc_size: [u32; 2]) {
        self.clock += 1;
        for p in batch {
            for t in p.sources() {
                if self.resident(device, queue, t).is_none() {
                    log::error!("compose atlas full; tile ({}, {}) skipped", p.tx, p.ty);
                }
            }
        }
        let mut jobs = Vec::with_capacity(batch.len());
        let mut ops = Vec::new();
        let (simple, general): (Vec<&TileProgram>, Vec<&TileProgram>) = batch.iter().partition(|p| is_simple(p));
        for p in simple.iter().chain(&general) {
            let op_start = ops.len() as u32;
            ops.extend(p.ops.iter().map(|op| self.encode(op)));
            jobs.push(Job { origin: [(p.tx * TILE) as u32, (p.ty * TILE) as u32], op_start, op_count: ops.len() as u32 - op_start });
        }
        if ops.is_empty() {
            ops.push(GpuOp::default());
        }
        let jobs_bytes: &[u8] = bytemuck::cast_slice(&jobs);
        let ops_bytes: &[u8] = bytemuck::cast_slice(&ops);
        if self.jobs.size() < jobs_bytes.len() as u64 {
            self.jobs = storage(device, "compose jobs", (jobs_bytes.len() as u64).next_power_of_two());
        }
        if self.ops.size() < ops_bytes.len() as u64 {
            self.ops = storage(device, "compose ops", (ops_bytes.len() as u64).next_power_of_two());
        }
        queue.write_buffer(&self.jobs, 0, jobs_bytes);
        queue.write_buffer(&self.ops, 0, ops_bytes);
        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params { doc_size, per_atlas: self.per_atlas, general_base: simple.len() as u32 }),
        );
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: self.params.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: self.km.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: self.jobs.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: self.ops.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(target) },
        ];
        for i in 0..ATLASES {
            let view = self.atlases.get(i).map_or(&self.dummy, |(_, v)| v);
            entries.push(wgpu::BindGroupEntry { binding: 5 + i as u32, resource: wgpu::BindingResource::TextureView(view) });
        }
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("compose"), layout: &self.layout, entries: &entries });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("compose") });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("compose"), timestamp_writes: None });
            pass.set_bind_group(0, &bind, &[]);
            let n = (TILE as u32) / 8;
            if !simple.is_empty() {
                pass.set_pipeline(&self.simple);
                pass.dispatch_workgroups(n, n, simple.len() as u32);
            }
            if !general.is_empty() {
                pass.set_pipeline(&self.pipelines[general.iter().any(|p| uses_pigment(p)) as usize]);
                pass.dispatch_workgroups(n, n, general.len() as u32);
            }
        }
        // Submit per batch: the next batch's uploads may evict slots this one reads.
        queue.submit([enc.finish()]);
    }

    /// Tiles currently resident.
    pub fn resident_tiles(&self) -> usize {
        self.slots.len()
    }
}
