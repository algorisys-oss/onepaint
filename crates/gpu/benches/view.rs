//! Pan, zoom and rotate on an 8k × 100-layer document (AGENTS.md target:
//! 60 fps). A view change re-presents the resident composite: one present
//! draw into a 1920 × 1080 target. Zooming in from fit-to-window first
//! composites the newly visible level-0 tiles (about 40 at 100%).
//! Each sample waits for the queue, so it measures GPU work.
#![allow(clippy::unwrap_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use onepaint_compose::program::{TileProgram, plan_tile};
use onepaint_doc::{Document, Layer, Node};
use onepaint_gpu::{CanvasGpu, ViewParams, compose::DEFAULT_BUDGET};
use onepaint_raster::{TILE, Tile};
use std::sync::Arc;

const SIZE: u32 = 8192;
const LAYERS: usize = 100;
const VIEW: [u32; 2] = [1920, 1080];

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let limits = adapter.limits();
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: limits, ..Default::default() })).ok()
}

/// 100 layers, each a translucent blob of ~20% of the page (as the
/// opacity-drag bench), 8 distinct tiles a layer.
fn document() -> Document {
    let mut d = Document::new(SIZE, SIZE, None);
    let n = (SIZE / TILE as u32) as i32;
    let r = ((0.2 * f64::from(n * n)) / std::f64::consts::PI).sqrt();
    for i in 0..LAYERS {
        let mut l = Layer::new(&format!("l{i}"));
        let tiles: Vec<Arc<Tile>> = (0..8)
            .map(|k| {
                let a = 0.25 + 0.5 * (((i * 8 + k) * 37) % 100) as f32 / 100.0;
                Arc::new(Tile::filled([((i % 7) as f32 / 7.0) * a, (k as f32 / 8.0) * a, 0.5 * a, a]))
            })
            .collect();
        let f = i as f64 / LAYERS as f64;
        let (ang, rad) = (i as f64 * 2.399_963, f.sqrt() * f64::from(n) * 0.35);
        let (cx, cy) = (f64::from(n) / 2.0 + rad * ang.cos(), f64::from(n) / 2.0 + rad * ang.sin());
        let mut k = 0;
        for ty in 0..n {
            for tx in 0..n {
                if (f64::from(tx) + 0.5 - cx).hypot(f64::from(ty) + 0.5 - cy) <= r {
                    l.pixels.set_tile(tx, ty, Some(tiles[k % 8].clone()));
                    k += 1;
                }
            }
        }
        d.root.push(Node::Layer(l));
    }
    d
}

fn view(scale: f32, offset: [f32; 2], rotation: f32) -> ViewParams {
    ViewParams {
        origin: [0.0, 0.0],
        size: [VIEW[0] as f32, VIEW[1] as f32],
        offset,
        scale,
        rotation,
        checker: 8.0,
        stage: [0.1, 0.1, 0.1, 1.0],
        checker_a: [1.0; 4],
        checker_b: [0.9, 0.9, 0.9, 1.0],
        time: 0.0,
    }
}

fn bench(c: &mut Criterion) {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: VIEW[0], height: VIEW[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let tv = target.create_view(&Default::default());
    let mut canvas = CanvasGpu::new(&device, format);
    canvas.enable_compositor(&device, DEFAULT_BUDGET);
    if !canvas.ensure_size(&device, SIZE, SIZE) {
        eprintln!("skipping: the composite doesn't fit");
        return;
    }
    let doc = document();
    let all: Vec<TileProgram> = doc.bounds().tiles().filter_map(|(tx, ty)| plan_tile(&doc, tx, ty, None)).collect();
    canvas.composite(&device, &queue, &all, 0);
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let present = |canvas: &CanvasGpu, v: &ViewParams| {
        canvas.set_view(&queue, v);
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &tv,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            canvas.draw(&mut pass);
        }
        queue.submit([enc.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    };
    let mut g = c.benchmark_group("view 8192² × 100 layers");
    g.sample_size(20);
    let fit = VIEW[1] as f32 / SIZE as f32;
    let mut x = 0.0f32;
    canvas.set_lod(3);
    g.bench_function("pan at fit (level 3)", |b| {
        b.iter(|| {
            x = (x + 7.0) % 200.0;
            present(&canvas, &view(fit, [x, 0.0], 0.0));
        })
    });
    let mut r = 0.0f32;
    g.bench_function("rotate at fit (level 3)", |b| {
        b.iter(|| {
            r += 0.02;
            present(&canvas, &view(fit, [0.0, 0.0], r));
        })
    });
    canvas.set_lod(0);
    g.bench_function("pan at 100% (level 0)", |b| {
        b.iter(|| {
            x = (x + 7.0) % 200.0;
            present(&canvas, &view(1.0, [x, 0.0], 0.0));
        })
    });
    // Zooming in from fit: the visible level-0 tiles composite afresh
    // (about 8 × 5 at 100% in a 1920 × 1080 view, centred).
    let n = (SIZE / TILE as u32) as i32;
    let visible: Vec<TileProgram> = all.iter().filter(|p| (p.tx - n / 2).abs() <= 4 && (p.ty - n / 2).abs() <= 3).cloned().collect();
    eprintln!("zoom-in composites {} tiles", visible.len());
    g.bench_function("zoom in: composite the visible level-0 tiles", |b| {
        b.iter(|| {
            canvas.composite(&device, &queue, &visible, 0);
            present(&canvas, &view(1.0, [0.0, 0.0], 0.0));
        })
    });
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
