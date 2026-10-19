//! Layer styles on the GPU (spec 56): `styles.wgsl` runs compose
//! styles.rs's pass per tile. The node's content over its region and what's
//! under it come from cache slots; the exact distance field is two compute
//! passes; the effects go up as an ordered list of steps.

use onepaint_compose::spatial::{SpatialSrc, Styled};
use onepaint_doc::styles::{Contour, Effect, EffectKind};
use onepaint_raster::TILE;
use wgpu::util::DeviceExt;

pub const STYLES_WGSL: &str = include_str!("../shaders/styles.wgsl");

/// "No slot" in a job.
pub const NONE: u32 = u32::MAX;
/// The scratch region's side: a tile and a tile's margin each way.
const SCRATCH: u32 = 3 * TILE as u32;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Prm {
    region: [i32; 4],
    tile: [i32; 2],
    edge: i32,
    level: i32,
    grid: [i32; 3],
    out_slot: u32,
    doc: [f32; 2],
    cap: f32,
    fill: f32,
    flags: u32,
    steps: u32,
    under_slot: u32,
    bevel_on: u32,
    bv0: [f32; 4],
    bv1: [f32; 4],
    light: [f32; 4],
    tex: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Step {
    kind: u32,
    mode: u32,
    extra: u32,
    _p: u32,
    color: [f32; 4],
    a: [f32; 4],
    b: [f32; 4],
}

/// One styled tile's job: the source, where the result goes, the cache slot
/// of each content tile (row by row over the region's tiles) and of what's
/// under the node on this tile.
pub struct Job<'a> {
    pub src: &'a SpatialSrc,
    pub styled: &'a Styled,
    pub out: u32,
    pub grid: Vec<u32>,
    pub under: u32,
}

fn pattern_index(id: &str) -> u32 {
    onepaint_filters::pattern::PATTERNS.iter().position(|(p, _)| *p == id).unwrap_or(0) as u32
}

fn contour_index(c: Contour) -> f32 {
    Contour::ALL.iter().position(|x| *x == c).unwrap_or(0) as f32
}

pub struct StylesPass {
    assemble: wgpu::ComputePipeline,
    cols: wgpu::ComputePipeline,
    rows: wgpu::ComputePipeline,
    style: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// The content region: written by `assemble`, read by the rest.
    scratch: (wgpu::TextureView, wgpu::TextureView),
    sq: wgpu::Buffer,
    field: wgpu::Buffer,
    patterns: wgpu::Buffer,
    dummy_tex: wgpu::TextureView,
    dummy_store: wgpu::TextureView,
    dummy_out: wgpu::TextureView,
}

impl StylesPass {
    pub fn new(device: &wgpu::Device) -> Self {
        let source = format!("{}\n{STYLES_WGSL}", crate::compose::BLEND_WGSL);
        let module = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("styles"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let cs = wgpu::ShaderStages::COMPUTE;
        let buffer = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
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
        let store = |binding, format, dim| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: cs,
            ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format, view_dimension: dim },
            count: None,
        };
        let ro = wgpu::BufferBindingType::Storage { read_only: true };
        let rw = wgpu::BufferBindingType::Storage { read_only: false };
        let entries = [
            buffer(0, wgpu::BufferBindingType::Uniform),
            buffer(1, ro),
            tex(2, wgpu::TextureViewDimension::D2Array),
            tex(3, wgpu::TextureViewDimension::D2),
            store(4, wgpu::TextureFormat::Rgba32Float, wgpu::TextureViewDimension::D2),
            store(5, wgpu::TextureFormat::Rgba16Float, wgpu::TextureViewDimension::D2Array),
            buffer(6, rw),
            buffer(7, rw),
            buffer(8, ro),
            buffer(9, ro),
            buffer(10, ro),
        ];
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("styles"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("styles"),
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
        let image = |label: &str, size: u32, format: wgpu::TextureFormat, usage: wgpu::TextureUsages, array: bool| {
            let t = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            });
            let dim = if array { wgpu::TextureViewDimension::D2Array } else { wgpu::TextureViewDimension::D2 };
            let v = || t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(dim), ..Default::default() });
            (v(), v())
        };
        let both = wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING;
        let scratch = image("styles region", SCRATCH, wgpu::TextureFormat::Rgba32Float, both, false);
        let n = (SCRATCH * SCRATCH) as u64 * 4;
        let storage = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            })
        };
        // The built-in pattern tiles, 32 × 32 grey each, in `PATTERNS` order.
        let grey: Vec<f32> = onepaint_filters::pattern::PATTERNS
            .iter()
            .flat_map(|(id, _)| {
                let t = onepaint_filters::pattern::pattern_pixels(id, [1.0; 3], [0.0; 3]);
                t.data.chunks_exact(4).map(|p| p[0] as f32 / 255.0).collect::<Vec<_>>()
            })
            .collect();
        let patterns = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("styles patterns"),
            contents: bytemuck::cast_slice(&grey),
            usage: wgpu::BufferUsages::STORAGE,
        });
        Self {
            assemble: pipeline("assemble"),
            cols: pipeline("edt_cols"),
            rows: pipeline("edt_rows"),
            style: pipeline("style"),
            layout,
            scratch,
            sq: storage("styles squared distances", 2 * n),
            field: storage("styles field", n),
            patterns,
            dummy_tex: image("styles (unused)", 1, wgpu::TextureFormat::Rgba32Float, wgpu::TextureUsages::TEXTURE_BINDING, false).0,
            dummy_store: image("styles (unused)", 1, wgpu::TextureFormat::Rgba32Float, wgpu::TextureUsages::STORAGE_BINDING, false).0,
            dummy_out: image("styles (unused)", 1, wgpu::TextureFormat::Rgba16Float, wgpu::TextureUsages::STORAGE_BINDING, true).0,
        }
    }

    /// Record `job` at `level`: `cache` is the cache's all-levels read view,
    /// `out` the filtered tiles' write view at `level`.
    pub fn record(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        job: &Job,
        level: u32,
        cache: &wgpu::TextureView,
        out: &wgpu::TextureView,
    ) {
        let s = job.src;
        let st = &job.styled.styles;
        let k = (1i32 << level) as f32;
        let ki = 1i32 << level;
        let edge = TILE >> level;
        let (x0, y0) = (s.region.x.div_euclid(ki), s.region.y.div_euclid(ki));
        let (x1, y1) = ((s.region.right() + ki - 1).div_euclid(ki), (s.region.bottom() + ki - 1).div_euclid(ki));
        let region = [x0, y0, x1 - x0, y1 - y0];
        let tiles = s.region.tiles().collect::<Vec<_>>();
        let gx = tiles.iter().map(|t| t.0).min().unwrap_or(s.tx);
        let gy = tiles.iter().map(|t| t.1).min().unwrap_or(s.ty);
        let cols = tiles.iter().map(|t| t.0).max().unwrap_or(s.tx) - gx + 1;
        let f = s.frame;
        let linear = f.space == onepaint_color::Space::Linear;
        let rgb = |c: [f32; 3]| if linear { c.map(onepaint_color::srgb_to_linear) } else { c };
        let (steps, ramps) = steps_of(st, f, k, &rgb);
        let bv = st.effects.iter().find(|e| e.active() && e.kind == EffectKind::Bevel);
        let (bv0, bv1, light, tex) = match bv {
            Some(e) => {
                let b = &e.bevel;
                let style = match b.style.as_str() {
                    "outer" => 1.0,
                    "emboss" => 2.0,
                    "pillow" => 3.0,
                    _ => 0.0,
                };
                let contour = contour_index(if b.contour_on { b.contour } else { Contour::HalfRound });
                let size = e.size / k;
                let (az, alt) = (e.angle.to_radians(), b.altitude.to_radians());
                (
                    [e.opacity, style, contour, size],
                    [size * b.depth * if b.up { 1.0 } else { -1.0 }, (1.0 + b.soften) / k, 32.0 * b.texture_scale / k, b.texture_depth],
                    [alt.cos() * az.cos(), -alt.cos() * az.sin(), alt.sin(), 0.0],
                    [
                        if b.texture_on { 1.0 } else { 0.0 },
                        if b.texture_invert { -1.0 } else { 1.0 },
                        pattern_index(&b.texture_pattern) as f32,
                        0.0,
                    ],
                )
            }
            None => ([0.0; 4], [1.0, 1.0, 32.0, 0.0], [0.0; 4], [0.0; 4]),
        };
        let reach = st.reach();
        let flat = reach == 0;
        let prm = Prm {
            region,
            tile: [s.tx * edge, s.ty * edge],
            edge,
            level: level as i32,
            grid: [gx, gy, cols],
            out_slot: job.out,
            doc: [f.doc_w as f32 / k, f.doc_h as f32 / k],
            cap: (reach + 4) as f32 / k,
            fill: st.fill_opacity,
            flags: u32::from(linear) | if job.under != NONE { 2 } else { 0 } | if flat { 4 } else { 0 },
            steps: steps.len() as u32,
            under_slot: job.under,
            bevel_on: u32::from(bv.is_some()),
            bv0,
            bv1,
            light,
            tex,
        };
        let init = |label: &str, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
        };
        let ubuf = init("styles params", bytemuck::bytes_of(&prm), wgpu::BufferUsages::UNIFORM);
        let slots = init("styles slots", bytemuck::cast_slice(&job.grid), wgpu::BufferUsages::STORAGE);
        let steps = if steps.is_empty() { vec![Step::default()] } else { steps };
        let sbuf = init("styles steps", bytemuck::cast_slice(&steps), wgpu::BufferUsages::STORAGE);
        let ramps = if ramps.is_empty() { vec![[0.0f32; 4]] } else { ramps };
        let rbuf = init("styles ramps", bytemuck::cast_slice(&ramps), wgpu::BufferUsages::STORAGE);
        let groups = |n: i32, w: u32| (n.max(1) as u32).div_ceil(w);
        let mut pass = |pipeline: &wgpu::ComputePipeline,
                        read: &wgpu::TextureView,
                        write: &wgpu::TextureView,
                        out_v: &wgpu::TextureView,
                        n: [u32; 2]| {
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("styles"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: slots.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(cache) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(read) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(write) },
                    wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(out_v) },
                    wgpu::BindGroupEntry { binding: 6, resource: self.sq.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: self.field.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 8, resource: sbuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 9, resource: rbuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 10, resource: self.patterns.as_entire_binding() },
                ],
            });
            let mut cp = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("styles"), timestamp_writes: None });
            cp.set_pipeline(pipeline);
            cp.set_bind_group(0, &bind, &[]);
            cp.dispatch_workgroups(n[0], n[1], 1);
        };
        let (read, write) = (&self.scratch.1, &self.scratch.0);
        pass(&self.assemble, &self.dummy_tex, write, &self.dummy_out, [groups(region[2], 8), groups(region[3], 8)]);
        if !flat {
            pass(&self.cols, read, &self.dummy_store, &self.dummy_out, [groups(region[2], 64), 1]);
            pass(&self.rows, read, &self.dummy_store, &self.dummy_out, [groups(region[3], 64), 1]);
        }
        pass(&self.style, read, &self.dummy_store, out, [groups(edge, 8), groups(edge, 8)]);
    }
}

/// The effect stack as steps in compose styles.rs's order, sizes in level
/// px (doc px over `k`), and the gradients' ramps, one row each.
fn steps_of(
    st: &onepaint_doc::styles::LayerStyles,
    f: onepaint_filters::Frame,
    k: f32,
    rgb: &dyn Fn([f32; 3]) -> [f32; 3],
) -> (Vec<Step>, Vec<[f32; 4]>) {
    let on: Vec<&Effect> = st.effects.iter().filter(|e| e.active()).collect();
    let list = |kd: EffectKind| on.iter().copied().filter(|e| e.kind == kd).take(4).collect::<Vec<_>>();
    let first = |kd: EffectKind| on.iter().copied().find(|e| e.kind == kd);
    let step = |kind: u32, e: &Effect, color: [f32; 3], opacity: f32| Step {
        kind,
        mode: e.blend_mode as u32,
        color: [color[0], color[1], color[2], opacity],
        ..Default::default()
    };
    let fall = |e: &Effect| [e.size * e.spread / k, e.size * (1.0 - e.spread) / k];
    let off = |e: &Effect| onepaint_doc::styles::shadow_offset(e.angle, e.distance).map(|v| v / k);
    let strokes = list(EffectKind::Stroke);
    let mut out = Vec::new();
    let mut ramps = Vec::new();
    for e in list(EffectKind::DropShadow).iter().rev() {
        let (o, fl) = (off(e), fall(e));
        out.push(Step { a: [o[0], o[1], fl[0], fl[1]], ..step(1, e, rgb(e.color), e.opacity) });
    }
    if let Some(e) = first(EffectKind::OuterGlow) {
        let fl = fall(e);
        out.push(Step { a: [fl[0], fl[1], 0.0, 0.0], ..step(2, e, rgb(e.color), e.opacity) });
    }
    for e in strokes.iter().rev().filter(|e| e.position == "outside") {
        out.push(Step { a: [e.size / k, 0.0, 0.0, 0.0], ..step(3, e, rgb(e.color), e.opacity) });
    }
    if let Some(e) = first(EffectKind::PatternOverlay) {
        let b = rgb(e.color_b);
        out.push(Step {
            extra: pattern_index(&e.pattern),
            a: [32.0 * e.scale / k, 0.0, 0.0, 0.0],
            b: [b[0], b[1], b[2], 0.0],
            ..step(4, e, rgb(e.color), e.opacity)
        });
    }
    let gos = list(EffectKind::GradientOverlay);
    for e in gos.iter().rev() {
        let (ga, gb) = onepaint_compose::styles::overlay_ends(&e.shape, e.angle, e.scale, f.doc_w as f32, f.doc_h as f32);
        let shape = match e.shape.as_str() {
            "radial" => 1,
            "angle" => 2,
            "reflected" => 3,
            "diamond" => 4,
            _ => 0,
        };
        let row = (ramps.len() / onepaint_filters::gradient::RAMP_SIZE) as u32;
        ramps.extend_from_slice(onepaint_compose::styles::ramp_of(e, f.space).entries());
        out.push(Step {
            extra: shape | u32::from(e.reverse) << 8 | row << 16,
            a: [ga[0] / k, ga[1] / k, gb[0] / k, gb[1] / k],
            ..step(5, e, [0.0; 3], e.opacity)
        });
    }
    for e in list(EffectKind::ColorOverlay).iter().rev() {
        out.push(step(6, e, rgb(e.color), e.opacity));
    }
    if let Some(e) = first(EffectKind::Satin) {
        let o = off(e);
        out.push(Step {
            extra: u32::from(e.invert),
            a: [o[0], o[1], e.size / k, contour_index(e.contour)],
            ..step(7, e, rgb(e.color), e.opacity)
        });
    }
    if let Some(e) = first(EffectKind::InnerGlow) {
        let fl = fall(e);
        out.push(Step { a: [fl[0], fl[1], 0.0, 0.0], ..step(8, e, rgb(e.color), e.opacity) });
    }
    for e in list(EffectKind::InnerShadow).iter().rev() {
        let (o, fl) = (off(e), fall(e));
        out.push(Step { a: [o[0], o[1], fl[0], fl[1]], ..step(9, e, rgb(e.color), e.opacity) });
    }
    for e in strokes.iter().rev().filter(|e| e.position == "inside") {
        out.push(Step { a: [e.size / k, 0.0, 0.0, 0.0], ..step(10, e, rgb(e.color), e.opacity) });
    }
    out.push(Step::default());
    if let Some(e) = first(EffectKind::Bevel) {
        let b = &e.bevel;
        out.push(Step { mode: b.shadow_mode as u32, ..step(11, e, rgb(b.shadow_color), b.shadow_opacity) });
        out.push(Step { mode: b.highlight_mode as u32, ..step(12, e, rgb(b.highlight_color), b.highlight_opacity) });
    }
    for e in strokes.iter().rev().filter(|e| e.position == "center") {
        out.push(Step { a: [e.size / k, 0.0, 0.0, 0.0], ..step(13, e, rgb(e.color), e.opacity) });
    }
    (out, ramps)
}
