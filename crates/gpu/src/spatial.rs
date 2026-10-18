//! Spatial adjustment layers on the GPU (spec 52): one filter job per
//! (layer, tile) reads the backdrop tiles' segment cache slots and writes
//! the filtered tile to a cache slot the composite reads. `spatial.wgsl`
//! runs filters `blur_pass` tap for tap.

use onepaint_compose::spatial::SpatialSrc;
use onepaint_filters::{Kind, is_identity};
use onepaint_raster::TILE;
use wgpu::util::DeviceExt;

pub const SPATIAL_WGSL: &str = include_str!("../shaders/spatial.wgsl");

/// "No backdrop tile" in a job's grid.
pub const NONE: u32 = u32::MAX;
/// The scratch images' side: a tile and a tile's reach on each side.
const SCRATCH: u32 = 3 * TILE as u32;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Prm {
    region: [i32; 4],
    tile: [i32; 2],
    edge: i32,
    level: i32,
    grid: [i32; 3],
    mode: u32,
    dir: [f32; 2],
    radius: f32,
    amount: f32,
    out_slot: u32,
    kind: u32,
    gamma: u32,
    _p0: u32,
    doc: [f32; 2],
    scale: f32,
    _p1: f32,
    k0: [f32; 4],
    k1: [f32; 4],
}

/// A per-pixel kind's code in `spatial.wgsl` (`kind_px`) and its params.
fn kind_params(kind: Kind, p: &onepaint_filters::Params) -> Option<(u32, [f32; 4])> {
    Some(match kind {
        Kind::Emboss => {
            let a = p.angle.unwrap_or(135.0).to_radians();
            let h = p.distance.unwrap_or(3.0).max(0.5);
            let mono = if p.mono.unwrap_or(true) { 1.0 } else { 0.0 };
            (1, [a.cos() * h, -a.sin() * h, p.amount.unwrap_or(0.5).max(0.0), mono])
        }
        Kind::Mosaic => (2, [p.cell_size.unwrap_or(16.0).max(2.0), 0.0, 0.0, 0.0]),
        Kind::Ripple => {
            let a = p.angle.unwrap_or(0.0).to_radians();
            let len = p.wavelength.unwrap_or(60.0).max(2.0);
            (3, [a.cos(), -a.sin(), std::f32::consts::TAU / len.max(1.0), p.amplitude.unwrap_or(0.0).max(0.0)])
        }
        Kind::Halftone => {
            let a = p.angle.unwrap_or(15.0).to_radians();
            let keep = if p.mono.unwrap_or(false) { 0.0 } else { 1.0 };
            (4, [a.cos(), a.sin(), p.cell_size.unwrap_or(6.0).max(2.0), keep])
        }
        Kind::Crystallize => (5, [p.cell_size.unwrap_or(16.0).max(2.0), 0.0, 0.0, 0.0]),
        Kind::LineArt if p.line_mode.as_deref() != Some("xdog") => {
            let line = if p.mono.unwrap_or(false) { 1.0 } else { 0.0 };
            let strength = 1.0 + p.amount.unwrap_or(0.5) * 4.0;
            (7, [p.radius.unwrap_or(1.0).max(1.0), p.threshold.unwrap_or(0.12), strength, line])
        }
        _ => return None,
    })
}

/// One filter job: the source, where its result goes, and the cache slot of
/// each backdrop tile (row by row over the region's tiles; `NONE` = empty).
pub struct Job<'a> {
    pub src: &'a SpatialSrc,
    pub out: u32,
    pub grid: Vec<u32>,
}

pub struct SpatialPass {
    assemble: wgpu::ComputePipeline,
    bright: wgpu::ComputePipeline,
    lum: wgpu::ComputePipeline,
    blur_region: wgpu::ComputePipeline,
    blur_out: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// Scratch images: the assembled region, and a blur pass's output.
    a: (wgpu::TextureView, wgpu::TextureView),
    b: (wgpu::TextureView, wgpu::TextureView),
    c: (wgpu::TextureView, wgpu::TextureView),
    d: (wgpu::TextureView, wgpu::TextureView),
    dummy_store: wgpu::TextureView,
    dummy_out: wgpu::TextureView,
    dummy_array: wgpu::TextureView,
}

fn scratch(device: &wgpu::Device, label: &str) -> (wgpu::TextureView, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: SCRATCH, height: SCRATCH, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    (tex.create_view(&Default::default()), tex.create_view(&Default::default()))
}

fn tiny(device: &wgpu::Device, format: wgpu::TextureFormat, usage: wgpu::TextureUsages, array: bool) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("spatial (unused)"),
        size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let dim = if array { wgpu::TextureViewDimension::D2Array } else { wgpu::TextureViewDimension::D2 };
    tex.create_view(&wgpu::TextureViewDescriptor { dimension: Some(dim), ..Default::default() })
}

impl SpatialPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spatial"),
            source: wgpu::ShaderSource::Wgsl(SPATIAL_WGSL.into()),
        });
        let cs = wgpu::ShaderStages::COMPUTE;
        let tex = |binding, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: dim,
            },
            count: None,
        };
        let store = |binding, format, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format, view_dimension: dim },
            count: None,
        };
        let entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: cs,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: cs,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            tex(2, wgpu::TextureViewDimension::D2Array),
            tex(3, wgpu::TextureViewDimension::D2),
            store(4, wgpu::TextureFormat::Rgba32Float, wgpu::TextureViewDimension::D2),
            store(5, wgpu::TextureFormat::Rgba16Float, wgpu::TextureViewDimension::D2Array),
            tex(6, wgpu::TextureViewDimension::D2),
        ];
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("spatial"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spatial"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let sampled = wgpu::TextureUsages::TEXTURE_BINDING;
        let storage = wgpu::TextureUsages::STORAGE_BINDING;
        Self {
            assemble: pipeline("assemble"),
            bright: pipeline("bright"),
            lum: pipeline("lum"),
            blur_region: pipeline("blur_region"),
            blur_out: pipeline("blur_out"),
            layout,
            a: scratch(device, "spatial scratch a"),
            b: scratch(device, "spatial scratch b"),
            c: scratch(device, "spatial scratch c"),
            d: scratch(device, "spatial scratch d"),
            dummy_store: tiny(device, wgpu::TextureFormat::Rgba32Float, storage, false),
            dummy_out: tiny(device, wgpu::TextureFormat::Rgba16Float, storage, true),
            dummy_array: tiny(device, wgpu::TextureFormat::Rgba16Float, sampled, true),
        }
    }

    /// Record `job` at `level`: `cache` is the cache's all-levels read view,
    /// `cache_out` its write view at `level`.
    pub fn record(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        job: &Job,
        level: u32,
        cache: &wgpu::TextureView,
        cache_out: &wgpu::TextureView,
    ) {
        let s = job.src;
        let k = 1i32 << level;
        let edge = TILE >> level;
        // The region in level pixels: its start rounded down, its end up.
        let (x0, y0) = (s.region.x.div_euclid(k), s.region.y.div_euclid(k));
        let (x1, y1) = ((s.region.right() + k - 1).div_euclid(k), (s.region.bottom() + k - 1).div_euclid(k));
        let region = [x0, y0, x1 - x0, y1 - y0];
        let tiles = s.region.tiles().collect::<Vec<_>>();
        let gx = tiles.iter().map(|t| t.0).min().unwrap_or(s.tx);
        let gy = tiles.iter().map(|t| t.1).min().unwrap_or(s.ty);
        let cols = tiles.iter().map(|t| t.0).max().unwrap_or(s.tx) - gx + 1;
        let scale = 1.0 / k as f32;
        let p = &s.params;
        let identity = is_identity(s.kind, p);
        let (kind, k0) = kind_params(s.kind, p).unwrap_or((0, [0.0; 4]));
        let gamma = u32::from(s.frame.space == onepaint_color::Space::Gamma);
        // (dir, radius) of the region pass and of the output pass, and the output mode.
        let (first, second, mode, amount) = match s.kind {
            Kind::GaussianBlur => {
                let r = p.radius.unwrap_or(0.0) * scale;
                (Some(([1.0, 0.0], r)), ([0.0, 1.0], r), 0, 0.0)
            }
            Kind::MotionBlur => {
                let t = p.angle.unwrap_or(0.0).to_radians();
                (None, ([t.cos(), t.sin()], p.distance.unwrap_or(0.0) * scale), 0, 0.0)
            }
            Kind::Bloom => {
                let r = p.radius.unwrap_or(15.0) * scale;
                (Some(([1.0, 0.0], r)), ([0.0, 1.0], r), 4, p.amount.unwrap_or(0.5))
            }
            _ => (Some(([1.0, 0.0], 2.0 * scale)), ([0.0, 1.0], 2.0 * scale), 1, p.amount.unwrap_or(0.0) * 3.0),
        };
        let k0 = if s.kind == Kind::Bloom { [p.threshold.unwrap_or(0.6), 0.0, 0.0, 0.0] } else { k0 };
        let prm = |dir: [f32; 2], radius: f32, mode: u32| Prm {
            region,
            tile: [s.tx * edge, s.ty * edge],
            edge,
            level: level as i32,
            grid: [gx, gy, cols],
            mode,
            dir,
            radius,
            amount,
            out_slot: job.out,
            kind,
            gamma,
            _p0: 0,
            doc: [s.frame.doc_w as f32, s.frame.doc_h as f32],
            scale: k as f32,
            _p1: 0.0,
            k0,
            k1: [0.0; 4],
        };
        let slots = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("spatial slots"),
            contents: bytemuck::cast_slice(&job.grid),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let groups = |n: i32| (n.max(1) as u32).div_ceil(8);
        let mut pass = |pipeline: &wgpu::ComputePipeline, prm: Prm, views: [&wgpu::TextureView; 5], size: [i32; 2]| {
            let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("spatial params"),
                contents: bytemuck::bytes_of(&prm),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let [cache_in, src, dst, out, orig] = views;
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("spatial"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: slots.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(cache_in) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(src) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(dst) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(out) },
                    wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(orig) },
                ],
            });
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("spatial"), timestamp_writes: None });
            cp.set_pipeline(pipeline);
            cp.set_bind_group(0, &bind, &[]);
            cp.dispatch_workgroups(groups(size[0]), groups(size[1]), 1);
        };
        let size = [region[2], region[3]];
        // The region from the backdrop slots into A.
        pass(&self.assemble, prm([0.0; 2], 0.0, 0), [cache, &self.b.1, &self.a.0, &self.dummy_out, &self.b.1], size);
        if !identity && s.kind == Kind::LineArt && kind == 0 {
            // XDoG: the luminance map (B), blurred across at σ (C) and 1.6σ
            // (D), then down both and thresholded.
            let sigma = p.radius.unwrap_or(2.0).max(0.8) * scale;
            let line = if p.mono.unwrap_or(false) { 1.0 } else { 0.0 };
            let eps = (p.threshold.unwrap_or(0.5) - 0.5) * 0.2;
            let phi = 5.0 + p.amount.unwrap_or(0.5) * 60.0;
            let xdog = |dir: [f32; 2], r: f32, mode: u32| Prm { k0: [sigma * 1.6, eps, phi, line], ..prm(dir, r, mode) };
            pass(&self.lum, prm([0.0; 2], 0.0, 0), [&self.dummy_array, &self.a.1, &self.b.0, &self.dummy_out, &self.a.1], size);
            pass(&self.blur_region, prm([1.0, 0.0], sigma, 0), [&self.dummy_array, &self.b.1, &self.c.0, &self.dummy_out, &self.b.1], size);
            pass(
                &self.blur_region,
                prm([1.0, 0.0], sigma * 1.6, 0),
                [&self.dummy_array, &self.b.1, &self.d.0, &self.dummy_out, &self.b.1],
                size,
            );
            pass(
                &self.blur_out,
                xdog([0.0, 1.0], sigma, 5),
                [&self.dummy_array, &self.c.1, &self.dummy_store, cache_out, &self.d.1],
                [edge; 2],
            );
            return;
        }
        if identity || kind != 0 {
            // A copy, or a per-pixel kind reading the region.
            let mode = if identity { 2 } else { 3 };
            pass(
                &self.blur_out,
                prm([0.0; 2], 0.0, mode),
                [&self.dummy_array, &self.a.1, &self.dummy_store, cache_out, &self.a.1],
                [edge; 2],
            );
            return;
        }
        // Bloom blurs the light map (B, from A), not the source.
        let blur_src = if s.kind == Kind::Bloom {
            pass(&self.bright, prm([0.0; 2], 0.0, 0), [&self.dummy_array, &self.a.1, &self.b.0, &self.dummy_out, &self.a.1], size);
            &self.b.1
        } else {
            &self.a.1
        };
        let src = match first {
            Some((dir, r)) => {
                pass(&self.blur_region, prm(dir, r, 0), [&self.dummy_array, blur_src, &self.c.0, &self.dummy_out, &self.a.1], size);
                &self.c.1
            }
            None => blur_src,
        };
        let (dir, r) = second;
        pass(&self.blur_out, prm(dir, r, mode), [&self.dummy_array, src, &self.dummy_store, cache_out, &self.a.1], [edge; 2]);
    }
}
