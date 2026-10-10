#![allow(clippy::unwrap_used)]
//! The GPU compositor against the CPU oracle (spec 13); skipped without an adapter.
use onepaint_compose::program::{TileProgram, plan_tile, plan_tile_split};
use onepaint_compose::testdocs::{Rng, random_doc, random_stroke, with_adjustments};
use onepaint_compose::{StrokeOverlay, composite_tile};
use onepaint_doc::{Document, Layer, Node};
use onepaint_gpu::compose::GpuCompositor;
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
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The texture as row-major f32 RGBA.
fn read(device: &wgpu::Device, queue: &wgpu::Queue, tex: &wgpu::Texture) -> Vec<f32> {
    let (w, h) = (tex.width(), tex.height());
    let stride = (w * 8).div_ceil(256) * 256;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (stride * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
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

/// Largest error between the GPU texture and the CPU oracle, relative above 1.
fn max_error(doc: &Document, ov: Option<&StrokeOverlay>, gpu: &[f32]) -> (f32, (i32, i32, usize)) {
    let w = doc.width as i32;
    let mut worst = (0.0f32, (0, 0, 0));
    for (tx, ty) in doc.bounds().tiles() {
        // The oracle rounded to f16 like the display path.
        let cpu = Tile::from_f32(&composite_tile(doc, tx, ty, ov)).to_f32();
        for ly in 0..TILE {
            for lx in 0..TILE {
                let (x, y) = (tx * TILE + lx, ty * TILE + ly);
                if x >= w || y >= doc.height as i32 {
                    continue;
                }
                for c in 0..4 {
                    let a = cpu[((ly * TILE + lx) * 4) as usize + c];
                    let b = gpu[((y * w + x) * 4) as usize + c];
                    let e = (a - b).abs() / a.abs().max(1.0);
                    if e > worst.0 || e.is_nan() {
                        worst = (if e.is_nan() { f32::INFINITY } else { e }, (x, y, c));
                    }
                }
            }
        }
    }
    worst
}

/// Pixels where any channel differs from the oracle by more than `TOL`.
fn over_tolerance(doc: &Document, gpu: &[f32]) -> usize {
    let w = doc.width as i32;
    let mut over = 0;
    for (tx, ty) in doc.bounds().tiles() {
        let cpu = Tile::from_f32(&composite_tile(doc, tx, ty, None)).to_f32();
        for ly in 0..TILE {
            for lx in 0..TILE {
                let (x, y) = (tx * TILE + lx, ty * TILE + ly);
                if x >= w || y >= doc.height as i32 {
                    continue;
                }
                let (a, b) = (((ly * TILE + lx) * 4) as usize, ((y * w + x) * 4) as usize);
                if (0..4).any(|c| (cpu[a + c] - gpu[b + c]).abs() / cpu[a + c].abs().max(1.0) > TOL) {
                    over += 1;
                }
            }
        }
    }
    over
}

/// Split runs keep the layers below the hot unit as an f16 image (spec 38).
/// Hard steps above it (threshold, posterize, inside groups the planner
/// still splits) can flip at isolated pixels where that rounding crosses
/// the step, so such documents may miss on at most this share of pixels.
const SPLIT_ADJUST_OVER: f64 = 1e-4;

fn programs(doc: &Document, ov: Option<&StrokeOverlay>) -> Vec<TileProgram> {
    doc.bounds().tiles().map(|(tx, ty)| plan_tile(doc, tx, ty, ov).unwrap()).collect()
}

const TOL: f32 = 2e-3;

#[test]
fn random_documents_match_the_cpu_oracle() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..40 {
        let doc = random_doc(seed, w, h, true);
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "seed {seed}: error {e} at {at:?}");
    }
}

#[test]
fn strokes_in_progress_match_the_cpu_oracle() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 100..140 {
        let doc = random_doc(seed, w, h, false);
        let (buf, id, opacity, mode, mask) = random_stroke(seed, &doc).unwrap();
        let ov = StrokeOverlay { layer_id: &id, stroke: &buf, opacity, mode, mask };
        gc.run(&device, &queue, &programs(&doc, Some(&ov)), &view, [w, h]);
        let (e, at) = max_error(&doc, Some(&ov), &read(&device, &queue, &tex));
        assert!(e <= TOL, "seed {seed}: error {e} at {at:?}");
    }
}

/// A document whose tiles outnumber the atlas: it composites in several
/// batches, evicting between them, and a second run reuses what stayed.
#[test]
fn atlas_eviction_keeps_results_exact() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 1);
    let cap = gc.capacity();
    let (w, h) = (2048u32, 1024u32);
    let mut doc = Document::new(w, h, None);
    let mut rng = Rng::new(9);
    let layers = cap / 32 + 2;
    for i in 0..layers {
        let mut l = Layer::new(&format!("l{i}"));
        for (tx, ty) in doc.bounds().tiles() {
            let mut t = Tile::default();
            let c = [rng.f(), rng.f(), rng.f()];
            let a = 0.3 + 0.2 * rng.f();
            for y in 0..TILE as usize {
                for x in 0..TILE as usize {
                    t.set(x, y, [c[0] * a, c[1] * a, c[2] * a, a]);
                }
            }
            l.pixels.set_tile(tx, ty, Some(Arc::new(t)));
        }
        l.blend_mode = onepaint_doc::BlendMode::ALL[i % 19];
        doc.root.push(Node::Layer(l));
    }
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let progs = programs(&doc, None);
    gc.run(&device, &queue, &progs, &view, [w, h]);
    assert!(gc.uploads as usize > cap, "{} uploads, capacity {cap}", gc.uploads);
    let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
    assert!(e <= TOL, "error {e} at {at:?}");
    // Running one tile again needs no new uploads when its sources stayed resident.
    let last = progs.last().unwrap().clone();
    let before = gc.uploads;
    gc.run(&device, &queue, &[last], &view, [w, h]);
    assert_eq!(gc.uploads, before);
}

/// A changed tile (copy-on-write) is uploaded again; an unchanged one is not.
#[test]
fn changed_tiles_are_reuploaded() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let mut doc = random_doc(7, 256, 256, false);
    let tex = target(&device, 256, 256);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    gc.run(&device, &queue, &programs(&doc, None), &view, [256, 256]);
    let first = gc.uploads;
    gc.run(&device, &queue, &programs(&doc, None), &view, [256, 256]);
    assert_eq!(gc.uploads, first);
    // Paint one pixel on the first layer that has a tile here.
    let l = onepaint_compose::testdocs::layer_ids(&doc.root)[0].clone();
    let Some(Node::Layer(layer)) = doc.find_mut(&l) else { panic!("no layer") };
    layer.pixels.set(3, 3, [0.0, 0.5, 0.0, 0.5]);
    layer.visible = true;
    gc.run(&device, &queue, &programs(&doc, None), &view, [256, 256]);
    assert!(gc.uploads > first);
    let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
    assert!(e <= TOL, "error {e} at {at:?}");
}

/// Normal layers that cover only some tiles, with one multiply layer on a
/// single tile: one batch holds lean jobs and general jobs side by side.
#[test]
fn lean_and_general_jobs_share_a_batch() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (700, 600);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let mut doc = Document::new(w, h, Some(onepaint_color::Rgba::WHITE));
    let mut r = Rng::new(7);
    for i in 0..4 {
        let mut l = Layer::new(&format!("n{i}"));
        let x = i * 150;
        onepaint_compose::testdocs::noise(&mut l.pixels, &mut r, onepaint_raster::Rect::new(x, x / 2, 300, 260));
        l.opacity = 0.6 + 0.1 * i as f32;
        doc.root.push(Node::Layer(l));
    }
    let mut m = Layer::new("multiply");
    onepaint_compose::testdocs::noise(&mut m.pixels, &mut r, onepaint_raster::Rect::new(20, 20, 200, 200));
    m.blend_mode = onepaint_doc::BlendMode::Multiply;
    doc.root.push(Node::Layer(m));
    gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
    let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
    assert!(e <= TOL, "error {e} at {at:?}");
}

// ── Spec 38: split programs and the segment cache ──

fn split_programs(doc: &Document, ov: Option<&StrokeOverlay>, hot: &str) -> Vec<TileProgram> {
    doc.bounds().tiles().map(|(tx, ty)| plan_tile_split(doc, tx, ty, ov, Some(hot)).unwrap()).collect()
}

/// Every layer id, depth first.
fn layer_ids(doc: &Document) -> Vec<String> {
    doc.layer_ids()
}

#[test]
fn split_programs_match_the_oracle_cold_and_warm() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..24 {
        let doc = random_doc(seed, w, h, true);
        for hot in layer_ids(&doc).iter().step_by(2) {
            let progs = split_programs(&doc, None, hot);
            for pass in ["cold", "warm"] {
                gc.run(&device, &queue, &progs, &view, [w, h]);
                let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
                assert!(e <= TOL, "seed {seed} hot {hot} ({pass}): error {e} at {at:?}");
            }
        }
    }
    assert!(gc.fills > 0, "segments were cached");
}

#[test]
fn split_strokes_match_the_oracle() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 100..124 {
        let doc = random_doc(seed, w, h, false);
        let (buf, id, opacity, mode, mask) = random_stroke(seed, &doc).unwrap();
        let ov = StrokeOverlay { layer_id: &id, stroke: &buf, opacity, mode, mask };
        gc.run(&device, &queue, &split_programs(&doc, Some(&ov), &id), &view, [w, h]);
        let (e, at) = max_error(&doc, Some(&ov), &read(&device, &queue, &tex));
        assert!(e <= TOL, "seed {seed}: error {e} at {at:?}");
    }
}

/// A stack of plain layers, every one covering every tile, with a distinct
/// tile per layer and position.
fn deep_doc(layers: usize) -> Document {
    let mut d = Document::new(512, 512, None);
    let mut rng = Rng::new(7);
    for i in 0..layers {
        let mut l = Layer::new(&format!("l{i}"));
        for (tx, ty) in d.bounds().tiles() {
            let mut t = Tile::default();
            let a = 0.2 + 0.5 * rng.f();
            let c = [rng.f() * a, rng.f() * a, rng.f() * a, a];
            for y in 0..TILE as usize {
                for x in 0..TILE as usize {
                    t.set(x, y, c);
                }
            }
            l.pixels.set_tile(tx, ty, Some(Arc::new(t)));
        }
        d.root.push(Node::Layer(l));
    }
    d
}

fn set_opacity(d: &mut Document, id: &str, o: f32) {
    for n in &mut d.root {
        if let Node::Layer(l) = n
            && l.id == id
        {
            l.opacity = o;
        }
    }
}

#[test]
fn a_drag_fills_each_segment_once_then_reuses_it() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let mut d = deep_doc(8);
    let hot = d.layer_ids()[4].clone();
    let tex = target(&device, 512, 512);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let tiles = d.bounds().tiles().count() as u64;
    for frame in 0..10 {
        set_opacity(&mut d, &hot, 0.3 + 0.05 * frame as f32);
        gc.run(&device, &queue, &split_programs(&d, None, &hot), &view, [512, 512]);
        assert_eq!(gc.fills, 2 * tiles, "frame {frame}: a prefix and a suffix per tile, filled once");
    }
    let (e, at) = max_error(&d, None, &read(&device, &queue, &tex));
    assert!(e <= TOL, "error {e} at {at:?}");
    // Painting below the hot layer refills that tile's prefix only.
    let below = d.layer_ids()[1].clone();
    if let Some(Node::Layer(l)) = d.root.iter_mut().find(|n| n.id() == below) {
        let mut t = Tile::default();
        t.set(3, 3, [0.5, 0.0, 0.0, 0.5]);
        l.pixels.set_tile(0, 0, Some(Arc::new(t)));
    }
    gc.run(&device, &queue, &split_programs(&d, None, &hot), &view, [512, 512]);
    assert_eq!(gc.fills, 2 * tiles + 1, "one prefix refilled");
    let (e, at) = max_error(&d, None, &read(&device, &queue, &tex));
    assert!(e <= TOL, "error {e} at {at:?}");
}

#[test]
fn a_small_cache_still_composites_exactly() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    // Room for two segment images: every batch evicts the last one's.
    let tile_bytes = (TILE as u64) * (TILE as u64) * 8;
    let mut gc = GpuCompositor::with_cache(&device, 64 << 20, 2 * tile_bytes);
    let d = deep_doc(6);
    let hot = d.layer_ids()[2].clone();
    let tex = target(&device, 512, 512);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for _ in 0..2 {
        gc.run(&device, &queue, &split_programs(&d, None, &hot), &view, [512, 512]);
        let (e, at) = max_error(&d, None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "error {e} at {at:?}");
    }
    assert!(gc.cached_segments() <= 2);
}

#[test]
fn warm_up_fills_ahead_so_the_first_frame_fills_nothing() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let d = deep_doc(8);
    let hot = d.layer_ids()[3].clone();
    let progs = split_programs(&d, None, &hot);
    gc.warm(&device, &queue, &progs);
    let warmed = gc.fills;
    assert_eq!(warmed, 2 * progs.len() as u64);
    gc.warm(&device, &queue, &progs);
    assert_eq!(gc.fills, warmed, "a second warm-up has nothing to do");
    let tex = target(&device, 512, 512);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    gc.run(&device, &queue, &progs, &view, [512, 512]);
    assert_eq!(gc.fills, warmed, "the first frame only composites");
    let (e, at) = max_error(&d, None, &read(&device, &queue, &tex));
    assert!(e <= TOL, "error {e} at {at:?}");
}

#[test]
fn an_over_budget_frame_alternates_and_stays_exact() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    // An atlas of one array (≥ 256 tiles) and a document needing more.
    let tile_bytes = (TILE as u64) * (TILE as u64) * 8;
    let mut gc = GpuCompositor::new(&device, 64 * tile_bytes);
    let d = deep_doc(80);
    let tex = target(&device, 512, 512);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let progs = programs(&d, None);
    for _ in 0..3 {
        gc.run(&device, &queue, &progs, &view, [512, 512]);
        let (e, at) = max_error(&d, None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "error {e} at {at:?}");
    }
}

#[test]
fn colour_adjustment_layers_match_the_cpu_oracle() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..40 {
        let doc = with_adjustments(random_doc(seed, w, h, seed % 2 == 0), seed);
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "seed {seed}: error {e} at {at:?}");
        // Split at an adjustment layer and at a plain one (spec 38), cold and warm.
        let ids = layer_ids(&doc);
        let hots = [ids.iter().find(|id| id.starts_with("adj")), ids.get(seed as usize % ids.len())];
        for hot in hots.into_iter().flatten() {
            let progs = split_programs(&doc, None, hot);
            for pass in ["cold", "warm"] {
                gc.run(&device, &queue, &progs, &view, [w, h]);
                let over = over_tolerance(&doc, &read(&device, &queue, &tex));
                let limit = (SPLIT_ADJUST_OVER * (w * h) as f64).ceil() as usize;
                assert!(over <= limit, "seed {seed} hot {hot} ({pass}): {over} px over");
            }
        }
    }
}

#[test]
fn paper_shading_matches_the_cpu_oracle() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..18 {
        let mut doc = with_adjustments(random_doc(seed, w, h, false), seed);
        // Grains of different sizes, so the paper texture is replaced between documents.
        let grain = ["rough-tooth", "canvas", "linen"][seed as usize % 3];
        let scale = Some(0.5 + (seed % 4) as f64 * 0.4);
        doc.set_paper(Some(onepaint_doc::Paper { grain_id: grain.into(), scale, strength: 0.3 + (seed % 3) as f64 * 0.3 }));
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "seed {seed} ({grain}): error {e} at {at:?}");
        let ids = layer_ids(&doc);
        let hot = ids[seed as usize % ids.len()].clone();
        gc.run(&device, &queue, &split_programs(&doc, None, &hot), &view, [w, h]);
        let over = over_tolerance(&doc, &read(&device, &queue, &tex));
        let limit = (SPLIT_ADJUST_OVER * (w * h) as f64).ceil() as usize;
        assert!(over <= limit, "seed {seed} ({grain}) split at {hot}: {over} px over");
    }
}

#[test]
fn partial_panel_clips_match_the_cpu_oracle() {
    use onepaint_panel::geometry::{Pt, rect_outline};
    use onepaint_panel::{PanelSpec, set_panel};
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (600, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let cache = onepaint_compose::program::PlanCache::default();
    for seed in 0..12 {
        let mut art = random_doc(seed, w, h, seed % 2 == 0);
        let mut g = onepaint_doc::Group::new("Panel");
        let outline = vec![Pt::new(40.5, 30.0), Pt::new(560.0, 12.25), Pt::new(530.0, 280.0), Pt::new(60.0, 270.5)];
        set_panel(&mut g, Some(PanelSpec::new(outline, 12.0, 0.0)));
        let mut inner = onepaint_doc::Group::new("Inner");
        set_panel(&mut inner, Some(PanelSpec::new(rect_outline(Pt::new(300.0, 100.0), Pt::new(450.5, 230.0)), 6.0, 0.0)));
        inner.children = art.root.split_off(art.root.len() / 2);
        g.children = std::mem::take(&mut art.root);
        g.children.push(Node::Group(inner));
        art.root = vec![Node::Group(g)];
        let keys: Vec<_> = art.bounds().tiles().collect();
        for pass in ["cold", "cached"] {
            let (progs, cpu) = onepaint_compose::program::plan_tiles_cached(&art, &keys, None, None, Some(&cache));
            assert!(cpu.is_empty());
            gc.run(&device, &queue, &progs, &view, [w, h]);
            let (e, at) = max_error(&art, None, &read(&device, &queue, &tex));
            assert!(e <= TOL, "seed {seed} ({pass}): error {e} at {at:?}");
        }
    }
}

#[test]
fn relief_matches_the_cpu_oracle() {
    use onepaint_compose::testdocs::with_relief;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (600, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..16 {
        let doc = with_relief(random_doc(seed, w, h, false), seed);
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "seed {seed}: error {e} at {at:?}");
        // Split, so relief also runs in cached segment fills, cold and warm.
        let ids = layer_ids(&doc);
        let hot = ids[seed as usize % ids.len()].clone();
        for pass in ["cold", "warm"] {
            gc.run(&device, &queue, &split_programs(&doc, None, &hot), &view, [w, h]);
            let (e, at) = max_error(&doc, None, &read(&device, &queue, &tex));
            assert!(e <= TOL, "seed {seed} split at {hot} ({pass}): error {e} at {at:?}");
        }
    }
}
