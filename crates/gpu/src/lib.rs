//! GPU display of the document composite.
//!
//! The composite lives in one Rgba16Float texture (premultiplied, linear);
//! dirty 256² tiles are either composited into it on the GPU (`compose`) or
//! uploaded as raw f16 from the CPU compositor, and a present pass draws it
//! through the view transform over a checkerboard. See
//! `docs/specs/07-canvas-ui.md` and `docs/specs/13-gpu-compositor.md`.

pub mod compose;
pub mod wet;

use onepaint_raster::{TILE, Tile};

pub const CANVAS_WGSL: &str = include_str!("../shaders/canvas.wgsl");

/// View parameters, all in framebuffer (physical) pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewParams {
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub offset: [f32; 2],
    pub scale: f32,
    pub rotation: f32,
    pub checker: f32,
    pub stage: [f32; 4],
    pub checker_a: [f32; 4],
    pub checker_b: [f32; 4],
    /// Seconds, for the marching-ants animation.
    pub time: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    origin: [f32; 2],
    size: [f32; 2],
    offset: [f32; 2],
    doc_size: [f32; 2],
    scale: f32,
    rotation: f32,
    checker: f32,
    srgb_target: f32,
    stage: [f32; 4],
    checker_a: [f32; 4],
    checker_b: [f32; 4],
    sel: [f32; 4],
}

struct DocTextures {
    texture: wgpu::Texture,
    /// The composite as a compute-pass storage target.
    storage: wgpu::TextureView,
    selection: wgpu::Texture,
    bind: wgpu::BindGroup,
    size: [u32; 2],
}

pub struct CanvasGpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    texture: Option<DocTextures>,
    has_selection: bool,
    srgb_target: bool,
    max_dim: u32,
    compositor: Option<compose::GpuCompositor>,
}

impl CanvasGpu {
    pub fn new(device: &wgpu::Device, target: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("onepaint_canvas"),
            source: wgpu::ShaderSource::Wgsl(CANVAS_WGSL.into()),
        });
        let vis = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("onepaint_canvas"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: vis,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("onepaint_canvas"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("onepaint_canvas"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: target, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("onepaint_canvas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("onepaint_canvas"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            layout,
            sampler,
            uniforms,
            texture: None,
            has_selection: false,
            srgb_target: target.is_srgb(),
            max_dim: device.limits().max_texture_dimension_2d,
            compositor: None,
        }
    }

    /// Composite on the GPU from now on, keeping up to `budget` bytes of layer tiles resident.
    pub fn enable_compositor(&mut self, device: &wgpu::Device, budget: u64) {
        self.compositor = Some(compose::GpuCompositor::new(device, budget));
    }

    pub fn has_compositor(&self) -> bool {
        self.compositor.is_some()
    }

    /// Run tile programs straight into the composite texture.
    pub fn composite(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, programs: &[onepaint_compose::program::TileProgram]) {
        let (Some(c), Some(t)) = (&mut self.compositor, &self.texture) else {
            return;
        };
        if !programs.is_empty() {
            c.run(device, queue, programs, &t.storage, t.size);
        }
    }

    /// (Re)create the composite texture when the document size changes. Returns false if too large.
    pub fn ensure_size(&mut self, device: &wgpu::Device, w: u32, h: u32) -> bool {
        if self.texture.as_ref().is_some_and(|t| t.size == [w, h]) {
            return true;
        }
        if w > self.max_dim || h > self.max_dim || w == 0 || h == 0 {
            log::error!("document {w}x{h} exceeds the GPU texture limit {}", self.max_dim);
            self.texture = None;
            return false;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("onepaint_composite"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let storage = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let selection = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("onepaint_selection"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sel_view = selection.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("onepaint_canvas"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&sel_view) },
            ],
        });
        self.texture = Some(DocTextures { texture, storage, selection, bind, size: [w, h] });
        self.has_selection = false;
        true
    }

    /// Upload composited tiles; edge tiles are clipped to the document.
    pub fn upload(&self, queue: &wgpu::Queue, tiles: &[((i32, i32), Tile)]) {
        let Some(DocTextures { texture, size: [w, h], .. }) = &self.texture else {
            return;
        };
        for ((tx, ty), tile) in tiles {
            let (x, y) = (tx * TILE, ty * TILE);
            if *tx < 0 || *ty < 0 || x as u32 >= *w || y as u32 >= *h {
                continue;
            }
            let cw = (TILE as u32).min(w - x as u32);
            let ch = (TILE as u32).min(h - y as u32);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: x as u32, y: y as u32, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&tile.data),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(TILE as u32 * 8), rows_per_image: Some(TILE as u32) },
                wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
            );
        }
    }

    /// Upload the selection coverage (one byte per doc pixel, row-major), or clear it.
    pub fn set_selection(&mut self, queue: &wgpu::Queue, mask: Option<&[u8]>) {
        let Some(DocTextures { selection, size: [w, h], .. }) = &self.texture else {
            return;
        };
        let Some(data) = mask.filter(|m| m.len() == (*w * *h) as usize) else {
            self.has_selection = false;
            return;
        };
        // Rows must be 256-byte aligned for texture copies.
        let stride = (*w).div_ceil(256) * 256;
        let mut padded = vec![0u8; (stride * *h) as usize];
        for y in 0..*h as usize {
            let (s, d) = (y * *w as usize, y * stride as usize);
            padded[d..d + *w as usize].copy_from_slice(&data[s..s + *w as usize]);
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: selection, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &padded,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(*h) },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
        self.has_selection = true;
    }

    pub fn set_view(&self, queue: &wgpu::Queue, v: &ViewParams) {
        let doc = self.texture.as_ref().map_or([1, 1], |t| t.size);
        let u = Uniforms {
            origin: v.origin,
            size: v.size,
            offset: v.offset,
            doc_size: [doc[0] as f32, doc[1] as f32],
            scale: v.scale,
            rotation: v.rotation,
            checker: v.checker,
            srgb_target: if self.srgb_target { 1.0 } else { 0.0 },
            stage: v.stage,
            checker_a: v.checker_a,
            checker_b: v.checker_b,
            sel: [if self.has_selection { 1.0 } else { 0.0 }, v.time, 0.0, 0.0],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(DocTextures { bind, .. }) = &self.texture else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
