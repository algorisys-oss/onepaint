//! Whole-canvas filter layers on the GPU (spec 53): the backdrop tiles,
//! filled through the segment cache, are copied into a backdrop image the
//! size of the document at the current level; one pass over it writes the
//! filtered image, which every tile's composite
//! reads at its offset. The Blur Gallery adds half-resolution blur levels
//! and a blend. `whole.wgsl` ports the CPU kinds tap for tap.

use onepaint_compose::spatial::Whole;
use onepaint_filters::{Kind, is_identity};
use onepaint_raster::TILE;
use wgpu::util::DeviceExt;

pub const WHOLE_WGSL: &str = include_str!("../shaders/whole.wgsl");

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

/// The Blur Gallery's pass params (`Gallery` in `whole.wgsl`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Gallery {
    half: [i32; 2],
    kind: u32,
    pins: u32,
    dir: [f32; 2],
    radius: f32,
    r_max: f32,
    per: [f32; 2],
    bokeh: f32,
    range: f32,
    c: [f32; 2],
    focus: f32,
    falloff: f32,
    u: [f32; 2],
    n: [f32; 2],
    rx: f32,
    ry: f32,
    inner: f32,
    _p: f32,
    pin: [[f32; 4]; 8],
}

/// The Blur Gallery's geometry at a level's `size` (doc px / 2^level), or
/// `None` for other kinds (filters `blur.rs` `gallery`).
fn gallery(kind: Kind, p: &onepaint_filters::Params, size: [f32; 2], scale: f32) -> Option<Gallery> {
    let code = match kind {
        Kind::TiltShift => 0,
        Kind::IrisBlur => 1,
        Kind::FieldBlur => 2,
        _ => return None,
    };
    let r_max = if kind == Kind::FieldBlur { onepaint_filters::field_blur_max(p) } else { p.radius.unwrap_or(0.0).max(0.0) } * scale;
    let a = p.angle.unwrap_or(0.0).to_radians();
    let min_side = size[0].min(size[1]);
    let rx = (p.extent.unwrap_or(0.6) * min_side * 0.5).max(1.0);
    let mut pin = [[0.0; 4]; 8];
    let pins = p.pins.as_deref().unwrap_or(&[]);
    for (o, q) in pin.iter_mut().zip(pins.iter().take(onepaint_filters::MAX_BLUR_PINS)) {
        *o = [q.x * size[0], q.y * size[1], q.blur.max(0.0) * scale, 0.0];
    }
    Some(Gallery {
        half: [0; 2],
        kind: code,
        pins: pins.len().min(onepaint_filters::MAX_BLUR_PINS) as u32,
        dir: [0.0; 2],
        radius: 0.0,
        r_max,
        per: [1.0; 2],
        bokeh: p.bokeh.unwrap_or(0.0).clamp(0.0, 1.0),
        range: p.bokeh_range.unwrap_or(0.75).clamp(0.0, 0.99),
        c: [p.center_x.unwrap_or(0.5) * size[0], p.center_y.unwrap_or(0.5) * size[1]],
        focus: p.focus_size.unwrap_or(0.1).max(0.0) * min_side,
        falloff: p.falloff_size.unwrap_or(0.2).max(0.0) * min_side,
        u: [a.cos(), -a.sin()],
        n: [a.sin(), a.cos()],
        rx,
        ry: (rx * p.aspect.unwrap_or(0.7)).max(1.0),
        inner: p.focus.unwrap_or(0.5).clamp(0.0, 0.95),
        _p: 0.0,
        pin,
    })
}

/// Texel reads per pixel of a kind's filter, roughly: what a refilter costs.
pub fn reads_per_px(kind: Kind, p: &onepaint_filters::Params) -> u64 {
    match kind {
        Kind::Twirl | Kind::Pinch | Kind::PolarCoordinates => 4,
        Kind::ChromaticAberration => 9,
        Kind::RadialBlur => 96 * 4,
        Kind::SpinBlur => 64 * 4,
        // Three levels of two 65-tap passes at a quarter of the pixels, the
        // bokeh disc's 64 taps if on, and the blend's 13.
        Kind::TiltShift | Kind::IrisBlur | Kind::FieldBlur => {
            let bokeh = if p.bokeh.unwrap_or(0.0) > 0.0 { 64 * 4 } else { 0 };
            3 * (2 * 65 * 4 + bokeh) / 4 + 13
        }
        _ => 4,
    }
}

/// A kind's code in `whole.wgsl` and its params; 0 copies the backdrop.
fn kind_params(kind: Kind, p: &onepaint_filters::Params) -> (u32, [f32; 4], [f32; 4]) {
    let c = [p.center_x.unwrap_or(0.5), p.center_y.unwrap_or(0.5)];
    if is_identity(kind, p) {
        return (0, [0.0; 4], [0.0; 4]);
    }
    if kind == Kind::SpinBlur {
        let k0 = [c[0], c[1], p.angle.unwrap_or(0.0).to_radians(), p.extent.unwrap_or(0.4)];
        let k1 = [p.aspect.unwrap_or(1.0), p.focus.unwrap_or(0.8).clamp(0.0, 0.95), p.spin.unwrap_or(0.0).to_radians(), 0.0];
        return (6, k0, k1);
    }
    let (code, k0) = match kind {
        Kind::Twirl => (1, [c[0], c[1], p.angle.unwrap_or(0.0).to_radians(), p.extent.unwrap_or(0.8)]),
        Kind::Pinch => (2, [c[0], c[1], p.amount.unwrap_or(0.0).clamp(-1.0, 1.0), p.extent.unwrap_or(0.8)]),
        Kind::PolarCoordinates => {
            let to_rect = if p.polar_mode.as_deref() == Some("to-rect") { 1.0 } else { 0.0 };
            (3, [c[0], c[1], to_rect, 0.0])
        }
        Kind::ChromaticAberration => (4, [p.amount.unwrap_or(0.0), 0.0, 0.0, 0.0]),
        Kind::RadialBlur => {
            let zoom = if p.radial_mode.as_deref() != Some("spin") { 1.0 } else { 0.0 };
            (5, [c[0], c[1], p.amount.unwrap_or(0.0).clamp(0.0, 1.0), zoom])
        }
        _ => (0, [0.0; 4]),
    };
    (code, k0, [0.0; 4])
}

pub struct WholePass {
    filter: wgpu::ComputePipeline,
    blur_src: wgpu::ComputePipeline,
    blur: wgpu::ComputePipeline,
    bokeh: wgpu::ComputePipeline,
    blend: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// The filtered image at the current level, and its read view.
    image: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2])>,
    /// The Blur Gallery's half-resolution images (two scratch, three
    /// levels), at their size.
    half: Option<([wgpu::TextureView; 5], [u32; 2])>,
    dummy_in: wgpu::TextureView,
    dummy_out: wgpu::TextureView,
    /// The backdrop image at the current level, and the segment each of its
    /// tiles holds (by the segment's key), so only changed tiles refill.
    back: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2], u32)>,
    pub back_tiles: std::collections::HashMap<(i32, i32), u64>,
    /// The key (level and layer) the image holds, and the layer's id.
    pub key: Option<u64>,
    pub layer: Option<String>,
}

fn half_image(device: &wgpu::Device, size: [u32; 2], usage: wgpu::TextureUsages) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("whole gallery image"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl WholePass {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("whole"),
            source: wgpu::ShaderSource::Wgsl(WHOLE_WGSL.into()),
        });
        let cs = wgpu::ShaderStages::COMPUTE;
        let uniform = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
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
        let store = |binding, format| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let d2 = wgpu::TextureViewDimension::D2;
        let entries = [
            uniform(0),
            tex(2, d2),
            store(3, wgpu::TextureFormat::Rgba16Float),
            tex(4, d2),
            store(5, wgpu::TextureFormat::Rgba32Float),
            tex(6, d2),
            tex(7, d2),
            tex(8, d2),
            uniform(9),
        ];
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("whole"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("whole"),
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
            filter: pipeline("whole_filter"),
            blur_src: pipeline("gallery_blur_src"),
            blur: pipeline("gallery_blur"),
            bokeh: pipeline("gallery_bokeh"),
            blend: pipeline("gallery_final"),
            layout,
            image: None,
            half: None,
            back: None,
            back_tiles: Default::default(),
            layer: None,
            dummy_in: half_image(device, [1, 1], sampled),
            dummy_out: half_image(device, [1, 1], storage),
            key: None,
        }
    }

    /// The level's size: rounded down, as the mips and the target are (a
    /// partial edge texel is half off the document, and clamped taps must
    /// not read it).
    pub fn level_size(doc: [u32; 2], level: u32) -> [u32; 2] {
        [(doc[0] >> level).max(1), (doc[1] >> level).max(1)]
    }

    /// The backdrop image for a document of `doc` px at `level`, made anew
    /// (and its tiles forgotten) when the size or level changed.
    pub fn back(&mut self, device: &wgpu::Device, doc: [u32; 2], level: u32) -> &wgpu::Texture {
        let size = Self::level_size(doc, level);
        if self.back.as_ref().is_none_or(|b| b.2 != size || b.3 != level) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("whole backdrop image"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.back = Some((tex, view, size, level));
            self.back_tiles.clear();
            self.key = None;
            self.layer = None;
        }
        &self.back.as_ref().expect("made above").0
    }

    /// The filtered image's read view, if there is one.
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.image.as_ref().map(|(_, v, _)| v)
    }

    /// Record the filter of `whole` at `level` over the backdrop image
    /// ([`Self::back`], filled for this level).
    pub fn record(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, whole: &Whole, level: u32) {
        let f = whole.frame;
        let k = (1u32 << level) as f32;
        let size = Self::level_size([f.doc_w, f.doc_h], level);
        let Some((_, back, _, _)) = &self.back else { return };
        let back = back.clone();
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
        let (kind, k0, k1) = kind_params(whole.kind, &whole.params);
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
            k1,
        };
        let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("whole params"),
            contents: bytemuck::bytes_of(&prm),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let geometry = gallery(whole.kind, &whole.params, prm.doc, 1.0 / k).filter(|_| !is_identity(whole.kind, &whole.params));
        // One pass: `pipeline` over `n` pixels, reading `g_in`/levels, writing `g_out`.
        let pass =
            |enc: &mut wgpu::CommandEncoder, pipeline: &wgpu::ComputePipeline, gal: &Gallery, g: [&wgpu::TextureView; 5], n: [u32; 2]| {
                let gbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("whole gallery params"),
                    contents: bytemuck::bytes_of(gal),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("whole"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&back) },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&out) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(g[0]) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(g[1]) },
                        wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(g[2]) },
                        wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(g[3]) },
                        wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(g[4]) },
                        wgpu::BindGroupEntry { binding: 9, resource: gbuf.as_entire_binding() },
                    ],
                });
                let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("whole filter"), timestamp_writes: None });
                cp.set_pipeline(pipeline);
                cp.set_bind_group(0, &bind, &[]);
                cp.dispatch_workgroups(n[0].div_ceil(8), n[1].div_ceil(8), 1);
            };
        let (di, dout) = (&self.dummy_in, &self.dummy_out);
        let Some(geo) = geometry else {
            let gal = gallery(Kind::TiltShift, &Default::default(), [1.0; 2], 1.0).expect("tilt-shift is a gallery kind");
            pass(enc, &self.filter, &gal, [di, dout, di, di, di], size);
            return;
        };
        // Three half-resolution blur levels at R/4, R/2 and R, then the blend.
        let hs = [size[0].div_ceil(2).max(1), size[1].div_ceil(2).max(1)];
        if self.half.as_ref().is_none_or(|(_, s)| *s != hs) {
            let usage = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING;
            let v = || half_image(device, hs, usage);
            self.half = Some(([v(), v(), v(), v(), v()], hs));
        }
        let Some(([ta, tb, l1, l2, l3], _)) = &self.half else { return };
        let half_per = [hs[0] as f32 / size[0] as f32, hs[1] as f32 / size[1] as f32];
        let mut gal = geo;
        gal.half = [hs[0] as i32, hs[1] as i32];
        for (r, level_out) in [geo.r_max / 4.0, geo.r_max / 2.0, geo.r_max].into_iter().zip([l1, l2, l3]) {
            if geo.bokeh > 0.0 {
                let g = Gallery { dir: [1.0, 0.0], radius: r / 8.0, per: [1.0, 1.0], ..gal };
                pass(enc, &self.blur_src, &g, [di, ta, di, di, di], hs);
                let g = Gallery { dir: [0.0, 1.0], radius: r / 8.0, per: half_per, ..gal };
                pass(enc, &self.blur, &g, [ta, tb, di, di, di], hs);
                let g = Gallery { radius: r, ..gal };
                pass(enc, &self.bokeh, &g, [tb, level_out, di, di, di], hs);
            } else {
                let g = Gallery { dir: [1.0, 0.0], radius: r, per: [1.0, 1.0], ..gal };
                pass(enc, &self.blur_src, &g, [di, ta, di, di, di], hs);
                let g = Gallery { dir: [0.0, 1.0], radius: r, per: half_per, ..gal };
                pass(enc, &self.blur, &g, [ta, level_out, di, di, di], hs);
            }
        }
        pass(enc, &self.blend, &gal, [di, dout, l1, l2, l3], size);
    }
}
