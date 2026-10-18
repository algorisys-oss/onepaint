//! Whole-canvas filter layers on the GPU (spec 53): one pass over the
//! document at the current level reads every backdrop tile from its segment
//! cache slot and writes the filtered image, which every tile's composite
//! reads at its offset. `whole.wgsl` ports the CPU kinds tap for tap.

use onepaint_compose::spatial::Whole;
use onepaint_filters::{Kind, is_identity};
use onepaint_raster::TILE;
use wgpu::util::DeviceExt;

pub const WHOLE_WGSL: &str = include_str!("../shaders/whole.wgsl");

/// "No backdrop tile" in the grid.
pub const NONE: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Prm {
    size: [i32; 2],
    edge: i32,
    cols: i32,
    level: i32,
    kind: u32,
    gamma: u32,
    _p0: u32,
    doc: [f32; 2],
    _p1: [f32; 2],
    k0: [f32; 4],
    k1: [f32; 4],
}

/// A kind's code in `whole.wgsl` and its params; 0 copies the backdrop.
fn kind_params(kind: Kind, p: &onepaint_filters::Params) -> (u32, [f32; 4]) {
    let c = [p.center_x.unwrap_or(0.5), p.center_y.unwrap_or(0.5)];
    if is_identity(kind, p) {
        return (0, [0.0; 4]);
    }
    match kind {
        Kind::Twirl => (1, [c[0], c[1], p.angle.unwrap_or(0.0).to_radians(), p.extent.unwrap_or(0.8)]),
        Kind::Pinch => (2, [c[0], c[1], p.amount.unwrap_or(0.0).clamp(-1.0, 1.0), p.extent.unwrap_or(0.8)]),
        Kind::PolarCoordinates => {
            let to_rect = if p.polar_mode.as_deref() == Some("to-rect") { 1.0 } else { 0.0 };
            (3, [c[0], c[1], to_rect, 0.0])
        }
        Kind::ChromaticAberration => (4, [p.amount.unwrap_or(0.0), 0.0, 0.0, 0.0]),
        _ => (0, [0.0; 4]),
    }
}

pub struct WholePass {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// The filtered image at the current level, and its read view.
    image: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2])>,
    /// The key (level and layer) the image holds.
    pub key: Option<u64>,
}

impl WholePass {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whole"),
            source: wgpu::ShaderSource::Wgsl(WHOLE_WGSL.into()),
        });
        let cs = wgpu::ShaderStages::COMPUTE;
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
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: cs,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: cs,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba16Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
        ];
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("whole"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whole"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("whole filter"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("whole_filter"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline, layout, image: None, key: None }
    }

    /// The filtered image's read view, if there is one.
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.image.as_ref().map(|(_, v, _)| v)
    }

    /// Record the filter of `whole` at `level`: `grid` holds each document
    /// tile's cache slot, row by row; `cache` is the cache's read view.
    pub fn record(
        &mut self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        whole: &Whole,
        level: u32,
        grid: &[u32],
        cache: &wgpu::TextureView,
    ) {
        let f = whole.frame;
        let k = (1u32 << level) as f32;
        let size = [f.doc_w.div_ceil(1 << level).max(1), f.doc_h.div_ceil(1 << level).max(1)];
        if self.image.as_ref().is_none_or(|(_, _, s)| *s != size) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("whole filtered image"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.image = Some((tex, view, size));
        }
        let Some((tex, _, _)) = &self.image else { return };
        let out = tex.create_view(&Default::default());
        let (kind, k0) = kind_params(whole.kind, &whole.params);
        let cols = f.doc_w.div_ceil(TILE as u32) as i32;
        let prm = Prm {
            size: [size[0] as i32, size[1] as i32],
            edge: TILE >> level,
            cols,
            level: level as i32,
            kind,
            gamma: u32::from(f.space == onepaint_color::Space::Gamma),
            _p0: 0,
            doc: [f.doc_w as f32 / k, f.doc_h as f32 / k],
            _p1: [0.0; 2],
            k0,
            k1: [0.0; 4],
        };
        let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whole params"),
            contents: bytemuck::bytes_of(&prm),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let slots = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whole grid"),
            contents: bytemuck::cast_slice(grid),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("whole"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: slots.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(cache) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&out) },
            ],
        });
        let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("whole filter"), timestamp_writes: None });
        cp.set_pipeline(&self.pipeline);
        cp.set_bind_group(0, &bind, &[]);
        cp.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
    }
}
