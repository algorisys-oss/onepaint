//! GPU display of the document composite.
//!
//! The composite lives in an Rgba16Float texture array (premultiplied,
//! linear): one layer of the document size, or past the GPU texture limit
//! several equal sheets (`sheets`, spec 40). Dirty 256² tiles are either
//! composited into it on the GPU (`compose`) or uploaded as raw f16 from the
//! CPU compositor, and a present pass draws it through the view transform
//! over a checkerboard. See `docs/specs/07-canvas-ui.md` and
//! `docs/specs/13-gpu-compositor.md`.

pub mod compose;
pub mod mip;
pub mod sheets;
pub mod wet;

pub use mip::L_MAX;
pub use sheets::Sheets;

use onepaint_color::cmyk::{Press, ViewModes};
use onepaint_raster::{TILE, Tile};
use std::sync::Arc;

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
    /// Sheet width and height at level 0, sheets across and down (spec 40).
    sheets: [f32; 4],
    /// Present-only view modes (spec 48): grayscale, proof, gamut-warning
    /// threshold in LUT units (0 = off), any on.
    view: [f32; 4],
    /// The proof's black and white points, linear (media-relative unless
    /// simulated).
    lo: [f32; 4],
    hi: [f32; 4],
}

struct DocTextures {
    texture: wgpu::Texture,
    /// The composite as a compute-pass storage target, one view per level.
    storage: Vec<wgpu::TextureView>,
    /// Selection coverage, in the same sheets.
    selection: wgpu::Texture,
    bind: wgpu::BindGroup,
    sheets: Sheets,
}

pub struct CanvasGpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    texture: Option<DocTextures>,
    has_selection: bool,
    srgb_target: bool,
    /// The largest sheet side, and the most sheets.
    max_dim: u32,
    max_layers: u32,
    compositor: Option<compose::GpuCompositor>,
    mipper: mip::Mipper,
    /// The level the present pass samples (spec 39).
    lod: u32,
    /// View modes for the present pass (spec 48), and the proof LUT with the
    /// press it came from (`None`: the built-in press), held so it stays unique.
    view: [[f32; 4]; 3],
    proof: Option<(Option<Arc<Press>>, wgpu::TextureView)>,
    proof_dummy: wgpu::TextureView,
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
                        view_dimension: wgpu::TextureViewDimension::D2Array,
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
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D3,
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
            max_layers: device.limits().max_texture_array_layers,
            compositor: None,
            mipper: mip::Mipper::new(device),
            lod: 0,
            view: [[0.0; 4]; 3],
            proof: None,
            proof_dummy: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("onepaint_proof (unused)"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D3,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
        }
    }

    /// Present-only view modes (spec 48): grayscale, soft proof on `press`
    /// and its gamut warning, applied per screen pixel like happypaint's
    /// present shader (`cmyk::apply_view_with`); `None` is the built-in press.
    /// Takes effect at the next `set_view`; a new press rebuilds the bind group.
    pub fn set_view_modes(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, v: &ViewModes, owner: Option<&Arc<Press>>) {
        let press: &Press = match owner {
            Some(p) => p,
            None => onepaint_color::cmyk::builtin_press(),
        };
        use onepaint_color::srgb_to_linear as s2l;
        let warn = if v.gamut_warn > 0.0 { (v.gamut_warn as f64 * onepaint_color::cmyk::DELTA_E_SCALE) as f32 } else { 0.0 };
        let flag = |b: bool| if b { 1.0 } else { 0.0 };
        let lo = if v.black_ink { [0.0; 3] } else { press.black.map(|c| s2l(c as f32)) };
        let hi = if v.paper_white { [1.0; 3] } else { press.white.map(|c| s2l(c as f32)) };
        self.view = [[flag(v.grayscale), flag(v.proof), warn, flag(v.any())], [lo[0], lo[1], lo[2], 0.0], [hi[0], hi[1], hi[2], 0.0]];
        let same = |k: &Option<Arc<Press>>| match (k, owner) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        };
        if !(v.proof || v.gamut_warn > 0.0) || self.proof.as_ref().is_some_and(|(k, _)| same(k)) {
            return;
        }
        let n = onepaint_color::cmyk::GRID as u32;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("onepaint_proof"),
            size: wgpu::Extent3d { width: n, height: n, depth_or_array_layers: n },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            tex.as_image_copy(),
            &press.proof_lut,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(n * 4), rows_per_image: Some(n) },
            wgpu::Extent3d { width: n, height: n, depth_or_array_layers: n },
        );
        self.proof = Some((owner.cloned(), tex.create_view(&Default::default())));
        // The bind group holds the old LUT: rebuild it with the composite.
        if let Some(t) = &mut self.texture {
            t.bind = Self::bind_group(
                device,
                &self.layout,
                &self.uniforms,
                &self.sampler,
                &t.texture,
                &t.selection,
                self.proof.as_ref().map_or(&self.proof_dummy, |(_, v)| v),
            );
        }
    }

    fn bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        sampler: &wgpu::Sampler,
        composite: &wgpu::Texture,
        selection: &wgpu::Texture,
        proof: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        let array = wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() };
        let (view, sel_view) = (composite.create_view(&array), selection.create_view(&array));
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("onepaint_canvas"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&sel_view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(proof) },
            ],
        })
    }

    /// Composite on the GPU from now on, keeping up to `budget` bytes of layer tiles resident.
    pub fn enable_compositor(&mut self, device: &wgpu::Device, budget: u64) {
        self.compositor = Some(compose::GpuCompositor::new(device, budget));
    }

    pub fn has_compositor(&self) -> bool {
        self.compositor.is_some()
    }

    /// Cap the sheet side below the GPU limit, to exercise sheets on small
    /// documents (tests and the `ONEPAINT_MAX_SHEET` debug hook).
    pub fn set_max_sheet(&mut self, max: u32) {
        self.max_dim = max.min(self.max_dim);
        self.texture = None;
    }

    /// The composite's current sheet layout.
    pub fn sheets(&self) -> Option<Sheets> {
        self.texture.as_ref().map(|t| t.sheets)
    }

    /// Run tile programs straight into the composite texture at `level`
    /// (spec 39), then downsample those tiles into the coarser levels.
    pub fn composite(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        programs: &[onepaint_compose::program::TileProgram],
        level: u32,
    ) {
        let (Some(c), Some(t)) = (&mut self.compositor, &self.texture) else {
            return;
        };
        if programs.is_empty() {
            return;
        }
        let level = level.min(t.storage.len() as u32 - 1);
        c.run_level(device, queue, programs, &t.storage[level as usize], &t.sheets, level);
        let keys: Vec<(i32, i32)> = programs.iter().map(|p| (p.tx, p.ty)).collect();
        self.mip_tiles(device, queue, &keys, level);
    }

    /// Downsample tiles from `from` into every coarser level.
    pub fn mip_tiles(&self, device: &wgpu::Device, queue: &wgpu::Queue, keys: &[(i32, i32)], from: u32) {
        let Some(t) = &self.texture else { return };
        if keys.is_empty() {
            return;
        }
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("composite mips") });
        for chunk in keys.chunks(65_535) {
            self.mipper.tiles(device, &mut enc, &t.texture, &t.sheets, chunk, from);
        }
        queue.submit([enc.finish()]);
    }

    /// The level the present pass samples.
    pub fn set_lod(&mut self, level: u32) {
        let top = self.texture.as_ref().map_or(0, |t| t.storage.len() as u32 - 1);
        self.lod = level.min(top);
    }

    /// Fill the cache images `programs` will need (spec 38 warm-up).
    pub fn warm(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, programs: &[onepaint_compose::program::TileProgram], level: u32) {
        if let (Some(c), Some(t)) = (&mut self.compositor, &self.texture)
            && !programs.is_empty()
        {
            c.warm_level(device, queue, programs, &t.sheets, level);
        }
    }

    /// (Re)create the composite texture when the document size changes.
    /// Returns false if it can't fit, even in sheets.
    pub fn ensure_size(&mut self, device: &wgpu::Device, w: u32, h: u32) -> bool {
        if self.texture.as_ref().is_some_and(|t| t.sheets.doc == [w, h]) {
            return true;
        }
        let Some(sheets) = Sheets::plan([w, h], self.max_dim, self.max_layers) else {
            log::error!("document {w}x{h} doesn't fit in {} sheets of {} px", self.max_layers, self.max_dim);
            self.texture = None;
            return false;
        };
        if sheets.count() > 1 {
            log::info!("document {w}x{h} composites in {:?} sheets of {:?}", sheets.grid, sheets.sheet);
        }
        let [sw, sh] = sheets.sheet;
        let extent = wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: sheets.count() };
        // Levels of detail (spec 39), as many as the size allows.
        let levels = mip::LEVELS.min(32 - sw.max(sh).leading_zeros());
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("onepaint_composite"),
            size: extent,
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let storage = (0..levels).map(|l| mip::level_view(&texture, l)).collect();
        let selection = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("onepaint_selection"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let proof = self.proof.as_ref().map_or(&self.proof_dummy, |(_, v)| v);
        let bind = Self::bind_group(device, &self.layout, &self.uniforms, &self.sampler, &texture, &selection, proof);
        self.texture = Some(DocTextures { texture, storage, selection, bind, sheets });
        self.has_selection = false;
        true
    }

    /// Upload composited tiles; edge tiles are clipped to the document.
    pub fn upload(&self, queue: &wgpu::Queue, tiles: &[((i32, i32), Tile)]) {
        let Some(DocTextures { texture, sheets, .. }) = &self.texture else {
            return;
        };
        let [w, h] = sheets.doc;
        for ((tx, ty), tile) in tiles {
            let (x, y) = (tx * TILE, ty * TILE);
            if *tx < 0 || *ty < 0 || x as u32 >= w || y as u32 >= h {
                continue;
            }
            let cw = (TILE as u32).min(w - x as u32);
            let ch = (TILE as u32).min(h - y as u32);
            // Sheets are tile-aligned, so a tile lies in one.
            let (layer, [lx, ly]) = sheets.locate(x as u32, y as u32);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: lx, y: ly, z: layer },
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
        let Some(DocTextures { selection, sheets, .. }) = &self.texture else {
            return;
        };
        let w = sheets.doc[0] as usize;
        let Some(data) = mask.filter(|m| m.len() == w * sheets.doc[1] as usize) else {
            self.has_selection = false;
            return;
        };
        for layer in 0..sheets.count() {
            let [x, y, sw, sh] = sheets.rect(layer);
            // Rows must be 256-byte aligned for texture copies.
            let stride = sw.next_multiple_of(256) as usize;
            let mut padded = vec![0u8; stride * sh as usize];
            for r in 0..sh as usize {
                let s = (y as usize + r) * w + x as usize;
                padded[r * stride..r * stride + sw as usize].copy_from_slice(&data[s..s + sw as usize]);
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: selection,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                &padded,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride as u32), rows_per_image: Some(sh) },
                wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: 1 },
            );
        }
        self.has_selection = true;
    }

    pub fn set_view(&self, queue: &wgpu::Queue, v: &ViewParams) {
        let sheets = self.texture.as_ref().map_or(Sheets::single([1, 1]), |t| t.sheets);
        let doc = sheets.doc;
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
            sel: [if self.has_selection { 1.0 } else { 0.0 }, v.time, self.lod as f32, 0.0],
            sheets: [sheets.sheet[0] as f32, sheets.sheet[1] as f32, sheets.grid[0] as f32, sheets.grid[1] as f32],
            view: self.view[0],
            lo: self.view[1],
            hi: self.view[2],
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
