#![allow(clippy::unwrap_used)]
//! Level-of-detail compositing (spec 39) against the box-downsampled CPU
//! oracle; skipped without an adapter.
use onepaint_compose::StrokeOverlay;
use onepaint_compose::composite_tile;
use onepaint_compose::program::{TileProgram, plan_tile, plan_tile_split};
use onepaint_compose::testdocs::{Rng, random_doc, random_stroke};
use onepaint_doc::{BlendMode, Document, Layer, Node};
use onepaint_gpu::compose::GpuCompositor;
use onepaint_gpu::mip::{LEVELS, Mipper, level_view};
use onepaint_raster::{TILE, Tile};
use std::sync::Arc;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

fn target(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: LEVELS,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn level_size(tex: &wgpu::Texture, l: u32) -> [u32; 2] {
    [(tex.width() >> l).max(1), (tex.height() >> l).max(1)]
}

/// Level `l` of the texture as row-major f32 RGBA.
fn read(device: &wgpu::Device, queue: &wgpu::Queue, tex: &wgpu::Texture, l: u32) -> Vec<f32> {
    let [w, h] = level_size(tex, l);
    let stride = (w * 8).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (stride * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: tex, mip_level: l, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = buf.slice(..).get_mapped_range().unwrap();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let row: &[half::f16] = bytemuck::cast_slice(&data[y * stride as usize..y * stride as usize + w as usize * 8]);
        out.extend(row.iter().map(|v| v.to_f32()));
    }
    out
}

/// The CPU oracle for the whole document at level 0, then box-averaged to
/// level `l` (row-major RGBA, edges clamped like the downsample pass).
fn oracle(doc: &Document, ov: Option<&StrokeOverlay>, l: u32) -> (Vec<f32>, [u32; 2]) {
    let (w, h) = (doc.width, doc.height);
    let mut img = vec![0.0f32; (w * h * 4) as usize];
    for (tx, ty) in doc.bounds().tiles() {
        let t = Tile::from_f32(&composite_tile(doc, tx, ty, ov)).to_f32();
        for ly in 0..TILE {
            for lx in 0..TILE {
                let (x, y) = (tx * TILE + lx, ty * TILE + ly);
                if x < w as i32 && y < h as i32 {
                    let (s, d) = (((ly * TILE + lx) * 4) as usize, ((y as u32 * w + x as u32) * 4) as usize);
                    img[d..d + 4].copy_from_slice(&t[s..s + 4]);
                }
            }
        }
    }
    let mut size = [w, h];
    for _ in 0..l {
        let n = [(size[0] >> 1).max(1), (size[1] >> 1).max(1)];
        let mut next = vec![0.0f32; (n[0] * n[1] * 4) as usize];
        for y in 0..n[1] {
            for x in 0..n[0] {
                for c in 0..4 {
                    let at = |xx: u32, yy: u32| img[((yy.min(size[1] - 1) * size[0] + xx.min(size[0] - 1)) * 4 + c) as usize];
                    let v = at(2 * x, 2 * y) + at(2 * x + 1, 2 * y) + at(2 * x, 2 * y + 1) + at(2 * x + 1, 2 * y + 1);
                    next[((y * n[0] + x) * 4 + c) as usize] = half::f16::from_f32(v * 0.25).to_f32();
                }
            }
        }
        (img, size) = (next, n);
    }
    (img, size)
}

fn max_error(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs() / x.abs().max(1.0)).fold(0.0, f32::max)
}

/// A document of uniform-colour tiles, so every box average is exact.
fn uniform_doc(layers: usize) -> Document {
    let mut d = Document::new(600, 520, None);
    let mut rng = Rng::new(3);
    for i in 0..layers {
        let mut l = Layer::new(&format!("l{i}"));
        for (tx, ty) in d.bounds().tiles() {
            let a = 0.2 + 0.6 * rng.f();
            let c = [rng.f() * a, rng.f() * a, rng.f() * a, a];
            let mut t = Tile::default();
            for y in 0..TILE as usize {
                for x in 0..TILE as usize {
                    t.set(x, y, c);
                }
            }
            l.pixels.set_tile(tx, ty, Some(Arc::new(t)));
        }
        l.blend_mode = BlendMode::ALL[i % 19];
        d.root.push(Node::Layer(l));
    }
    d
}

fn programs(doc: &Document, ov: Option<&StrokeOverlay>, hot: Option<&str>) -> Vec<TileProgram> {
    doc.bounds()
        .tiles()
        .map(|(tx, ty)| match hot {
            Some(h) => plan_tile_split(doc, tx, ty, ov, Some(h)).unwrap(),
            None => plan_tile(doc, tx, ty, ov).unwrap(),
        })
        .collect()
}

fn run_at(
    gc: &mut GpuCompositor,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    tex: &wgpu::Texture,
    progs: &[TileProgram],
    l: u32,
) -> Vec<f32> {
    let view = level_view(tex, l, false);
    gc.run_level(device, queue, progs, &view, level_size(tex, l), l);
    read(device, queue, tex, l)
}

#[test]
fn downsampling_a_level_0_composite_matches_the_box_oracle() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let doc = random_doc(11, 600, 520, true);
    let tex = target(&device, 600, 520);
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    run_at(&mut gc, &device, &queue, &tex, &programs(&doc, None, None), 0);
    let keys: Vec<_> = doc.bounds().tiles().collect();
    let mut enc = device.create_command_encoder(&Default::default());
    Mipper::new(&device).tiles(&device, &mut enc, &tex, &keys, 0);
    queue.submit([enc.finish()]);
    for l in 1..LEVELS {
        let (want, _) = oracle(&doc, None, l);
        let e = max_error(&want, &read(&device, &queue, &tex, l));
        assert!(e <= 3e-3, "level {l}: error {e}");
    }
}

#[test]
fn uniform_tiles_composite_exactly_at_every_level() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let doc = uniform_doc(10);
    let tex = target(&device, 600, 520);
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let hot = doc.layer_ids()[5].clone();
    for l in 1..LEVELS {
        let (want, _) = oracle(&doc, None, l);
        for (name, progs) in [("unsplit", programs(&doc, None, None)), ("split", programs(&doc, None, Some(&hot)))] {
            for pass in ["cold", "warm"] {
                let e = max_error(&want, &run_at(&mut gc, &device, &queue, &tex, &progs, l));
                assert!(e <= 2e-3, "level {l} {name} {pass}: error {e}");
            }
        }
    }
}

#[test]
fn random_documents_preview_within_the_bound() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let tex = target(&device, 300, 300);
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    for seed in 0..16 {
        let doc = random_doc(seed, 300, 300, seed % 2 == 0);
        let stroke = random_stroke(seed, &doc);
        let ov = stroke.as_ref().map(|(buf, id, opacity, mode, mask)| StrokeOverlay {
            layer_id: id,
            stroke: buf,
            opacity: *opacity,
            mode: *mode,
            mask: *mask,
        });
        for l in 1..=3 {
            let (want, _) = oracle(&doc, ov.as_ref(), l);
            let got = run_at(&mut gc, &device, &queue, &tex, &programs(&doc, ov.as_ref(), None), l);
            // A preview: blending at reduced size differs at hard edges, so
            // bound the bulk, not the worst pixel (idle frames refine to exact).
            let mut errs: Vec<f32> = want.iter().zip(&got).map(|(a, b)| (a - b).abs()).collect();
            errs.sort_by(f32::total_cmp);
            let mean = errs.iter().sum::<f32>() / errs.len() as f32;
            let p99 = errs[(errs.len() - 1) * 99 / 100];
            assert!(mean <= 0.015 && p99 <= 0.15, "seed {seed} level {l}: mean {mean}, p99 {p99}");
        }
    }
}
