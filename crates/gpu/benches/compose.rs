//! GPU compositor benches (spec 13 targets), with the CPU oracle for scale.
//! Each GPU sample waits for the queue, so it measures the work, not the submit.
#![allow(clippy::unwrap_used)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use onepaint_compose::composite_tiles;
use onepaint_compose::program::{TileProgram, plan_tile};
use onepaint_doc::{BlendMode, Document, Layer, Node};
use onepaint_gpu::compose::{DEFAULT_BUDGET, GpuCompositor};
use onepaint_raster::{TILE, Tile};
use std::sync::Arc;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// `layers` layers; each covers every `every`-th tile.
fn doc(size: u32, layers: usize, every: usize) -> Document {
    let mut d = Document::new(size, size, None);
    let keys: Vec<_> = d.bounds().tiles().collect();
    for i in 0..layers {
        let mut l = Layer::new(&format!("l{i}"));
        let a = 0.4 + 0.02 * i as f32;
        let mut t = Tile::default();
        for y in 0..TILE as usize {
            for x in 0..TILE as usize {
                t.set(x, y, [0.3 * a, (x as f32 / 256.0) * a, (y as f32 / 256.0) * a, a]);
            }
        }
        let t = Arc::new(t);
        for (k, &(tx, ty)) in keys.iter().enumerate() {
            if (k + i) % every == 0 {
                l.pixels.set_tile(tx, ty, Some(t.clone()));
            }
        }
        l.blend_mode = BlendMode::Normal;
        d.root.push(Node::Layer(l));
    }
    d
}

fn target(device: &wgpu::Device, size: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn bench(c: &mut Criterion) {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut g = c.benchmark_group("compose");
    g.sample_size(10);
    let (name, size) = ("4096² × 20 layers, dense", 4096);
    let d = doc(size, 20, 1);
    let keys: Vec<_> = d.bounds().tiles().collect();
    let progs: Vec<TileProgram> = keys.iter().map(|&(tx, ty)| plan_tile(&d, tx, ty, None).unwrap()).collect();
    let view = target(&device, size);
    let mut gc = GpuCompositor::new(&device, DEFAULT_BUDGET);
    let mut run = |progs: &[TileProgram]| {
        gc.run(&device, &queue, progs, &view, [size, size]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    };
    run(&progs);
    g.bench_function(format!("gpu full recomposite, {name}"), |b| b.iter(|| run(black_box(&progs))));
    g.bench_function(format!("plan, {name}"), |b| b.iter(|| keys.iter().map(|&(tx, ty)| plan_tile(&d, tx, ty, None)).collect::<Vec<_>>()));
    g.bench_function(format!("cpu full recomposite, {name}"), |b| b.iter(|| composite_tiles(&d, black_box(&keys), None)));
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
