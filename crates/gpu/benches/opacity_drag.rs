//! The 8k × 100-layer opacity drag (AGENTS.md performance target: 60 fps
//! on 8k×8k documents with 100+ layers). Each frame of the drag changes one
//! layer's opacity and recomposites that layer's tiles on the GPU, at most
//! the canvas's 256 tiles a frame, visible first. Each sample waits for the
//! queue, so it measures the GPU work, not the submit.
//!
//! Two documents: one whose distinct tiles fit the 1 GiB tile budget, and one
//! that overflows it, so the atlas has to evict and re-upload.
//! `ONEPAINT_BENCH_FULL=1` also times a full 1024-tile recomposite.
#![allow(clippy::unwrap_used)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use onepaint_compose::program::{TileProgram, plan_tile, plan_tiles};
use onepaint_doc::{Document, Layer, Node};
use onepaint_gpu::compose::{DEFAULT_BUDGET, GpuCompositor};
use onepaint_raster::{TILE, Tile};
use std::sync::Arc;

const SIZE: u32 = 8192;
const LAYERS: usize = 100;
/// Each layer covers about this share of the page, in one clustered blob.
const COVER: f64 = 0.2;
/// Tiles the canvas composites per frame (`GPU_TILES_PER_FRAME`).
const PER_FRAME: usize = 256;
/// The layer being dragged.
const DRAGGED: usize = 50;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let limits = adapter.limits();
    let desc = wgpu::DeviceDescriptor { required_limits: limits, ..Default::default() };
    pollster::block_on(adapter.request_device(&desc)).ok()
}

/// A translucent tile with a gradient, so no two layers composite alike.
fn tile(seed: usize) -> Arc<Tile> {
    let a = 0.25 + 0.5 * ((seed * 37) % 100) as f32 / 100.0;
    let mut t = Tile::default();
    for y in 0..TILE as usize {
        for x in 0..TILE as usize {
            let (u, v) = (x as f32 / 256.0, y as f32 / 256.0);
            t.set(x, y, [((seed % 7) as f32 / 7.0) * a, u * a, v * a, a]);
        }
    }
    Arc::new(t)
}

/// 100 layers, each a blob of ~20% of the tiles around its own centre.
/// `distinct` tiles per layer are cycled through its blob (real art has one
/// per tile; fewer keeps the bench inside RAM).
fn document(distinct: usize) -> Document {
    let mut d = Document::new(SIZE, SIZE, None);
    let n = (SIZE / TILE as u32) as i32;
    let r = ((COVER * (n * n) as f64) / std::f64::consts::PI).sqrt();
    for i in 0..LAYERS {
        let mut l = Layer::new(&format!("l{i}"));
        let tiles: Vec<Arc<Tile>> = (0..distinct).map(|k| tile(i * 1000 + k)).collect();
        // Centres spread over the page on a golden-angle spiral.
        let f = i as f64 / LAYERS as f64;
        let (ang, rad) = (i as f64 * 2.399_963, f.sqrt() * n as f64 * 0.35);
        let (cx, cy) = (n as f64 / 2.0 + rad * ang.cos(), n as f64 / 2.0 + rad * ang.sin());
        let mut k = 0;
        for ty in 0..n {
            for tx in 0..n {
                if (tx as f64 + 0.5 - cx).hypot(ty as f64 + 0.5 - cy) <= r {
                    l.pixels.set_tile(tx, ty, Some(tiles[k % distinct].clone()));
                    k += 1;
                }
            }
        }
        d.root.push(Node::Layer(l));
    }
    d
}

fn target(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        mip_level_count: onepaint_gpu::mip::LEVELS,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    })
}

fn set_opacity(d: &mut Document, i: usize, o: f32) {
    if let Node::Layer(l) = &mut d.root[i] {
        l.opacity = o;
    }
}

fn bench(c: &mut Criterion) {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    if device.limits().max_texture_dimension_2d < SIZE {
        eprintln!("skipping: the GPU's textures stop short of {SIZE} px");
        return;
    }
    let tex = target(&device);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    // Level 3 (1/8 scale): about what an 8k page shows at fit-to-window (spec 39).
    let view3 = onepaint_gpu::mip::level_view(&tex, 3);
    let mut g = c.benchmark_group("opacity drag 8192² × 100 layers");
    g.sample_size(10);
    let full = std::env::var("ONEPAINT_BENCH_FULL").is_ok();
    // 8 distinct tiles a layer: 800 × 512 KiB = 400 MiB, inside the budget.
    // 32: 3200 tiles = 1.6 GiB, past it.
    for (name, distinct) in [("fits the tile budget", 8usize), ("past the tile budget", 32)] {
        let mut d = document(distinct);
        let all: Vec<_> = d.bounds().tiles().collect();
        let dirty: Vec<(i32, i32)> = match &d.root[DRAGGED] {
            Node::Layer(l) => l.pixels.tile_keys(),
            _ => unreachable!(),
        };
        let frame_keys: Vec<_> = dirty.iter().copied().take(PER_FRAME).collect();
        let depth: usize = frame_keys.iter().map(|&(tx, ty)| plan_tile(&d, tx, ty, None).map_or(0, |p| p.sources().count())).sum();
        eprintln!(
            "{name}: dragged layer covers {} tiles; a frame composites {} tiles, {:.1} layers deep on average",
            dirty.len(),
            frame_keys.len(),
            depth as f64 / frame_keys.len().max(1) as f64
        );
        let mut gc = GpuCompositor::new(&device, DEFAULT_BUDGET);
        let run = |gc: &mut GpuCompositor, progs: &[TileProgram]| {
            gc.run(&device, &queue, progs, &view, [SIZE, SIZE]);
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        };
        // Warm: the whole page once, as when the document opens.
        let warm: Vec<TileProgram> = all.iter().filter_map(|&(tx, ty)| plan_tile(&d, tx, ty, None)).collect();
        run(&mut gc, &warm);
        let hot = d.root[DRAGGED].id().to_string();
        let mut o = 0.5f32;
        // Spec 38: plan with the dragged layer hot; the first frame fills the cache.
        let split = |d: &Document| -> Vec<TileProgram> { plan_tiles(d, &frame_keys, None, Some(&hot)).0 };
        run(&mut gc, &split(&d));
        if std::env::var("ONEPAINT_BENCH_TRACE").is_ok() {
            // Alternate unsplit and split frames, so both see the same machine.
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for f in 0..30 {
                set_opacity(&mut d, DRAGGED, 0.2 + 0.02 * f as f32);
                let t0 = std::time::Instant::now();
                let progs: Vec<TileProgram> = frame_keys.iter().filter_map(|&(tx, ty)| plan_tile(&d, tx, ty, None)).collect();
                run(&mut gc, &progs);
                a.push(t0.elapsed().as_secs_f64() * 1e3);
                let t0 = std::time::Instant::now();
                let progs = split(&d);
                run(&mut gc, &progs);
                b.push(t0.elapsed().as_secs_f64() * 1e3);
            }
            let stats = |v: &mut Vec<f64>| {
                v.sort_by(f64::total_cmp);
                (v[0], v[v.len() / 2])
            };
            let ((amin, amed), (bmin, bmed)) = (stats(&mut a), stats(&mut b));
            eprintln!("{name}: unsplit min {amin:.1} / median {amed:.1} ms; split min {bmin:.1} / median {bmed:.1} ms");
            // Where a split frame's time goes.
            let time = |f: &mut dyn FnMut()| {
                let mut v: Vec<f64> = (0..15)
                    .map(|_| {
                        let t0 = std::time::Instant::now();
                        f();
                        t0.elapsed().as_secs_f64() * 1e3
                    })
                    .collect();
                stats(&mut v).1
            };
            let plan = time(&mut || drop(black_box(split(&d))));
            let progs = split(&d);
            let mut gpu = |n: usize| time(&mut || run(&mut gc, &progs[..n.min(progs.len())]));
            let (g1, g50, gall) = (gpu(1), gpu(50), gpu(progs.len()));
            eprintln!("{name}: split plan {plan:.2} ms; GPU 1 tile {g1:.2} ms, 50 tiles {g50:.2} ms, {} tiles {gall:.2} ms", progs.len());
            // The same drag frame at level 3.
            gc.warm_level(&device, &queue, &progs, 3);
            let l3 = time(&mut || {
                set_opacity(&mut d, DRAGGED, 0.5);
                let progs = split(&d);
                gc.run_level(&device, &queue, &progs, &view3, &onepaint_gpu::Sheets::single([SIZE, SIZE]), 3);
                device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            });
            eprintln!("{name}: split drag frame at level 3 (plan + GPU) {l3:.2} ms");
        }
        g.bench_function(format!("drag frame, split (plan + GPU), {name}"), |b| {
            b.iter(|| {
                o = if o > 0.9 { 0.2 } else { o + 0.05 };
                set_opacity(&mut d, DRAGGED, o);
                run(&mut gc, black_box(&split(&d)));
            })
        });
        let split_progs = split(&d);
        g.bench_function(format!("drag frame, split, GPU only, {name}"), |b| b.iter(|| run(&mut gc, black_box(&split_progs))));
        g.bench_function(format!("drag frame (plan + GPU), {name}"), |b| {
            b.iter(|| {
                o = if o > 0.9 { 0.2 } else { o + 0.05 };
                set_opacity(&mut d, DRAGGED, o);
                let progs: Vec<TileProgram> = frame_keys.iter().filter_map(|&(tx, ty)| plan_tile(&d, tx, ty, None)).collect();
                run(&mut gc, black_box(&progs));
            })
        });
        let progs: Vec<TileProgram> = frame_keys.iter().filter_map(|&(tx, ty)| plan_tile(&d, tx, ty, None)).collect();
        g.bench_function(format!("drag frame, plan only, {name}"), |b| {
            b.iter(|| frame_keys.iter().filter_map(|&(tx, ty)| plan_tile(&d, tx, ty, None)).collect::<Vec<_>>())
        });
        g.bench_function(format!("drag frame, GPU only, {name}"), |b| b.iter(|| run(&mut gc, black_box(&progs))));
        if full {
            g.bench_function(format!("full recomposite (1024 tiles), {name}"), |b| b.iter(|| run(&mut gc, black_box(&warm))));
        }
    }
    g.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
