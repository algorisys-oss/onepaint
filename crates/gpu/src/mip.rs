//! Mip levels for the canvas composite and the tile atlases (spec 39): each
//! level is the 2 × 2 box average of the one below, one dispatch per level.

use onepaint_raster::TILE;
use wgpu::util::DeviceExt;

pub const MIP_WGSL: &str = include_str!("../shaders/mip.wgsl");

/// Coarsest level of detail: a 256² tile is 8 × 8 pixels there.
pub const L_MAX: u32 = 5;
/// Mip levels per texture (0 ..= `L_MAX`).
pub const LEVELS: u32 = L_MAX + 1;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    dst_size: [u32; 2],
    tile: u32,
    _pad: u32,
}

pub struct Mipper {
    flat: wgpu::ComputePipeline,
    array: wgpu::ComputePipeline,
    flat_layout: wgpu::BindGroupLayout,
    array_layout: wgpu::BindGroupLayout,
}

/// A single-level view: `D2` for the composite, `D2Array` for atlases.
pub fn level_view(tex: &wgpu::Texture, level: u32, array: bool) -> wgpu::TextureView {
    tex.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(if array { wgpu::TextureViewDimension::D2Array } else { wgpu::TextureViewDimension::D2 }),
        base_mip_level: level,
        mip_level_count: Some(1),
        ..Default::default()
    })
}

impl Mipper {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("mip"), source: wgpu::ShaderSource::Wgsl(MIP_WGSL.into()) });
        let cs = wgpu::ShaderStages::COMPUTE;
        let common = [
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
        ];
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
        let store = |binding, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba16Float,
                view_dimension: dim,
            },
            count: None,
        };
        let layout = |entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("mip"), entries })
        };
        let mut flat_entries = common.to_vec();
        flat_entries.extend([tex(2, wgpu::TextureViewDimension::D2), store(3, wgpu::TextureViewDimension::D2)]);
        let mut array_entries = common.to_vec();
        array_entries.extend([tex(4, wgpu::TextureViewDimension::D2Array), store(5, wgpu::TextureViewDimension::D2Array)]);
        let (flat_layout, array_layout) = (layout(&flat_entries), layout(&array_entries));
        let pipeline = |l: &wgpu::BindGroupLayout, entry: &str| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("mip"),
                bind_group_layouts: &[Some(l)],
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self { flat: pipeline(&flat_layout, "mip_2d"), array: pipeline(&array_layout, "mip_array"), flat_layout, array_layout }
    }

    /// Downsample tiles `keys` of the 2D `tex` from level `from` up to `L_MAX`.
    pub fn tiles(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, tex: &wgpu::Texture, keys: &[(i32, i32)], from: u32) {
        let keys: Vec<[u32; 2]> = keys.iter().filter(|(x, y)| *x >= 0 && *y >= 0).map(|&(x, y)| [x as u32, y as u32]).collect();
        let top = (tex.mip_level_count() - 1).min(L_MAX);
        if keys.is_empty() || from >= top {
            return;
        }
        let jobs = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mip jobs"),
            contents: bytemuck::cast_slice(&keys),
            usage: wgpu::BufferUsages::STORAGE,
        });
        for l in from + 1..=top {
            let tile = (TILE as u32 >> l).max(1);
            let size = [(tex.width() >> l).max(1), (tex.height() >> l).max(1)];
            let (src, dst) = (level_view(tex, l - 1, false), level_view(tex, l, false));
            self.pass(
                device,
                enc,
                &self.flat,
                &self.flat_layout,
                &jobs,
                keys.len(),
                Params { dst_size: size, tile, _pad: 0 },
                (2, &src),
                (3, &dst),
            );
        }
    }

    /// Downsample atlas `slots` of the array `tex` from level 0 up to `L_MAX`.
    pub fn slots(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, tex: &wgpu::Texture, slots: &[u32]) {
        if slots.is_empty() {
            return;
        }
        let keys: Vec<[u32; 2]> = slots.iter().map(|&s| [s, 0]).collect();
        let jobs = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mip jobs"),
            contents: bytemuck::cast_slice(&keys),
            usage: wgpu::BufferUsages::STORAGE,
        });
        for l in 1..tex.mip_level_count().min(LEVELS) {
            let tile = (TILE as u32 >> l).max(1);
            let (src, dst) = (level_view(tex, l - 1, true), level_view(tex, l, true));
            self.pass(
                device,
                enc,
                &self.array,
                &self.array_layout,
                &jobs,
                keys.len(),
                Params { dst_size: [tile, tile], tile, _pad: 0 },
                (4, &src),
                (5, &dst),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        layout: &wgpu::BindGroupLayout,
        jobs: &wgpu::Buffer,
        n: usize,
        params: Params,
        src: (u32, &wgpu::TextureView),
        dst: (u32, &wgpu::TextureView),
    ) {
        let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mip params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mip"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: jobs.as_entire_binding() },
                wgpu::BindGroupEntry { binding: src.0, resource: wgpu::BindingResource::TextureView(src.1) },
                wgpu::BindGroupEntry { binding: dst.0, resource: wgpu::BindingResource::TextureView(dst.1) },
            ],
        });
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("mip"), timestamp_writes: None });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind, &[]);
        let groups = params.tile.div_ceil(8);
        // The z workgroup count is limited; jobs come in chunks the caller keeps small.
        pass.dispatch_workgroups(groups, groups, n as u32);
    }
}
