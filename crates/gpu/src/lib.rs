//! GPU display of the document composite.
//!
//! The composite lives in one Rgba16Float texture (premultiplied, linear);
//! dirty 256² tiles are uploaded as raw f16, and a present pass draws it through
//! the view transform over a checkerboard. See `docs/specs/07-canvas-ui.md`.

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
}

pub struct CanvasGpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    texture: Option<(wgpu::Texture, wgpu::BindGroup, [u32; 2])>,
    srgb_target: bool,
    max_dim: u32,
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
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
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
            srgb_target: target.is_srgb(),
            max_dim: device.limits().max_texture_dimension_2d,
        }
    }

    /// (Re)create the composite texture when the document size changes. Returns false if too large.
    pub fn ensure_size(&mut self, device: &wgpu::Device, w: u32, h: u32) -> bool {
        if self.texture.as_ref().is_some_and(|t| t.2 == [w, h]) {
            return true;
        }
        if w > self.max_dim || h > self.max_dim || w == 0 || h == 0 {
            log::error!(
                "document {w}x{h} exceeds the GPU texture limit {}",
                self.max_dim
            );
            self.texture = None;
            return false;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("onepaint_composite"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("onepaint_canvas"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.texture = Some((texture, bind, [w, h]));
        true
    }

    /// Upload composited tiles; edge tiles are clipped to the document.
    pub fn upload(&self, queue: &wgpu::Queue, tiles: &[((i32, i32), Tile)]) {
        let Some((texture, _, [w, h])) = &self.texture else {
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
                    origin: wgpu::Origin3d {
                        x: x as u32,
                        y: y as u32,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&tile.data),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(TILE as u32 * 8),
                    rows_per_image: Some(TILE as u32),
                },
                wgpu::Extent3d {
                    width: cw,
                    height: ch,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub fn set_view(&self, queue: &wgpu::Queue, v: &ViewParams) {
        let doc = self.texture.as_ref().map_or([1, 1], |t| t.2);
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
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some((_, bind, _)) = &self.texture else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
