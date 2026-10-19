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

/// Pixels where any channel differs from the oracle by more than `tol`.
fn over_tolerance(doc: &Document, ov: Option<&StrokeOverlay>, gpu: &[f32], tol: f32) -> usize {
    let w = doc.width as i32;
    let mut over = 0;
    for (tx, ty) in doc.bounds().tiles() {
        let cpu = Tile::from_f32(&composite_tile(doc, tx, ty, ov)).to_f32();
        for ly in 0..TILE {
            for lx in 0..TILE {
                let (x, y) = (tx * TILE + lx, ty * TILE + ly);
                if x >= w || y >= doc.height as i32 {
                    continue;
                }
                let (a, b) = (((ly * TILE + lx) * 4) as usize, ((y * w + x) * 4) as usize);
                if (0..4).any(|c| (cpu[a + c] - gpu[b + c]).abs() / cpu[a + c].abs().max(1.0) > tol) {
                    over += 1;
                }
            }
        }
    }
    over
}

/// Hard steps flip at isolated pixels that sit on the step: threshold and
/// posterize adjustments, and the Hard Mix, Darker/Lighter Color and
/// Dissolve modes (Vivid Light near 0.5 is nearly as steep), where a split's
/// f16 image (spec 38) rounds across it; screentone at cell edges where the
/// GPU fuses a multiply-add. Such documents may miss on at most this share
/// of pixels.
const STEP_OVER: f64 = 1e-4;

/// A split run (spec 38) rounds its segment images to f16 once more than
/// the oracle, which Divide and the dodges amplify a few ulps.
const SPLIT_TOL: f32 = 4e-3;

fn assert_stepped(doc: &Document, ov: Option<&StrokeOverlay>, gpu: &[f32], tol: f32, what: &str) {
    let over = over_tolerance(doc, ov, gpu, tol);
    let limit = (STEP_OVER * (doc.width * doc.height) as f64).ceil() as usize;
    assert!(over <= limit, "{what}: {over} px over");
}

fn assert_adjusted(doc: &Document, gpu: &[f32], what: &str) {
    assert_stepped(doc, None, gpu, TOL, what);
}

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
                assert_stepped(&doc, None, &read(&device, &queue, &tex), SPLIT_TOL, &format!("seed {seed} hot {hot} ({pass})"));
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
        assert_stepped(&doc, Some(&ov), &read(&device, &queue, &tex), SPLIT_TOL, &format!("seed {seed}"));
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
    gc.warm(&device, &queue, &progs, [512, 512]);
    let warmed = gc.fills;
    assert_eq!(warmed, 2 * progs.len() as u64);
    gc.warm(&device, &queue, &progs, [512, 512]);
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
        assert_adjusted(&doc, &read(&device, &queue, &tex), &format!("seed {seed}"));
        // Split at an adjustment layer and at a plain one (spec 38), cold and warm.
        let ids = layer_ids(&doc);
        let hots = [ids.iter().find(|id| id.starts_with("adj")), ids.get(seed as usize % ids.len())];
        for hot in hots.into_iter().flatten() {
            let progs = split_programs(&doc, None, hot);
            for pass in ["cold", "warm"] {
                gc.run(&device, &queue, &progs, &view, [w, h]);
                assert_adjusted(&doc, &read(&device, &queue, &tex), &format!("seed {seed} hot {hot} ({pass})"));
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
        let grain = ["rough-tooth", "canvas", "linen", "paper-khadi", "paper-kraft", "paper-washi"][seed as usize % 6];
        let scale = Some(0.5 + (seed % 4) as f64 * 0.4);
        doc.set_paper(Some(onepaint_doc::Paper { grain_id: grain.into(), scale, strength: 0.3 + (seed % 3) as f64 * 0.3 }));
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        assert_adjusted(&doc, &read(&device, &queue, &tex), &format!("seed {seed} ({grain})"));
        let ids = layer_ids(&doc);
        let hot = ids[seed as usize % ids.len()].clone();
        gc.run(&device, &queue, &split_programs(&doc, None, &hot), &view, [w, h]);
        assert_adjusted(&doc, &read(&device, &queue, &tex), &format!("seed {seed} ({grain}) split at {hot}"));
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
            assert_stepped(&doc, None, &read(&device, &queue, &tex), SPLIT_TOL, &format!("seed {seed} split at {hot} ({pass})"));
        }
    }
}

#[test]
fn warmed_segments_keep_their_relief() {
    use onepaint_compose::testdocs::with_relief;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (w, h) = (600, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..8 {
        let doc = with_relief(random_doc(seed, w, h, false), seed);
        let ids = layer_ids(&doc);
        let hot = ids[ids.len() - 1].clone();
        let progs = split_programs(&doc, None, &hot);
        let mut gc = GpuCompositor::new(&device, 64 << 20);
        // Idle warm-up fills the segments; the frame then only reads them.
        gc.warm(&device, &queue, &progs, [w, h]);
        let filled = gc.fills;
        gc.run(&device, &queue, &progs, &view, [w, h]);
        assert_eq!(gc.fills, filled, "seed {seed}: the frame reused the warmed segments");
        assert_stepped(&doc, None, &read(&device, &queue, &tex), SPLIT_TOL, &format!("seed {seed}"));
    }
}

#[test]
fn eight_bit_documents_blend_in_gamma_space_like_the_cpu() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..24 {
        let mut doc = with_adjustments(random_doc(seed, w, h, seed % 2 == 0), seed);
        doc.depth = onepaint_doc::Depth::Rgba8;
        if seed % 3 == 0 {
            doc.set_paper(Some(onepaint_doc::Paper { grain_id: "linen".into(), scale: Some(0.8), strength: 0.7 }));
        }
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        assert_adjusted(&doc, &read(&device, &queue, &tex), &format!("seed {seed}"));
        let ids = layer_ids(&doc);
        let hot = ids[seed as usize % ids.len()].clone();
        for pass in ["cold", "warm"] {
            gc.run(&device, &queue, &split_programs(&doc, None, &hot), &view, [w, h]);
            assert_adjusted(&doc, &read(&device, &queue, &tex), &format!("seed {seed} split at {hot} ({pass})"));
        }
    }
}

/// Every layer's pixel and mask tiles stored 8-bit, as an 8-bit document keeps them (spec 49).
/// Hide threshold and posterize layers. In an 8-bit document whole flat
/// regions share one byte value, so where it sits on a step the region flips
/// together, past what `STEP_OVER` allows for isolated pixels.
fn unposterize(nodes: &mut [Node]) {
    for n in nodes {
        match n {
            Node::Group(g) => unposterize(&mut g.children),
            Node::Layer(l) => {
                if l.adjustment.as_ref().is_some_and(|a| a.kind == "posterize" || a.kind == "threshold") {
                    l.visible = false;
                }
            }
        }
    }
}

fn quantized(mut doc: Document) -> Document {
    use onepaint_raster::Store;
    fn walk(nodes: &mut [Node]) {
        for n in nodes {
            match n {
                Node::Layer(l) => {
                    for s in std::iter::once(&mut l.pixels).chain(l.mask.as_mut()) {
                        for (tx, ty) in s.tile_keys() {
                            let t = s.tile(tx, ty).map(|t| t.to_store(Store::Unorm8));
                            s.set_tile(tx, ty, t.map(Arc::new));
                        }
                    }
                }
                Node::Group(g) => walk(&mut g.children),
            }
        }
    }
    doc.depth = onepaint_doc::Depth::Rgba8;
    walk(&mut doc.root);
    doc
}

/// 8-bit tiles live in `Rgba8Unorm` atlases (half the memory) and read back
/// their exact bytes, so the result matches the CPU oracle as before.
#[test]
fn eight_bit_tiles_use_8_bit_atlases() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..8 {
        let mut doc = quantized(with_adjustments(random_doc(seed, w, h, false), seed));
        unposterize(&mut doc.root);
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        assert_adjusted(&doc, &read(&device, &queue, &tex), &format!("seed {seed}"));
    }
    let [f16, u8s] = gc.atlas_counts();
    assert!(u8s >= 1, "8-bit tiles went to an 8-bit atlas ({f16} f16, {u8s} 8-bit)");
    // A 16-bit document never makes one.
    let mut plain = GpuCompositor::new(&device, 64 << 20);
    plain.run(&device, &queue, &programs(&random_doc(3, w, h, false), None), &view, [w, h]);
    assert_eq!(plain.atlas_counts()[1], 0);
}

/// Spatial adjustment layers (spec 52): blur, motion blur and sharpen run
/// as GPU filter passes over the backdrop and match the rendered document.
#[test]
fn spatial_layers_match_the_rendered_document() {
    use onepaint_compose::spatial::rendered;
    use onepaint_compose::testdocs::with_spatial;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let mut ran = 0;
    for seed in 0..24 {
        let doc = with_spatial(random_doc(seed, w, h, seed % 2 == 0), seed);
        let progs: Vec<TileProgram> = doc.bounds().tiles().filter_map(|(tx, ty)| plan_tile(&doc, tx, ty, None)).collect();
        // Stacked spatial layers stay on the CPU (spec 52, phase 1).
        if progs.len() < doc.bounds().tiles().count() {
            continue;
        }
        ran += 1;
        let before = gc.filters;
        gc.run(&device, &queue, &progs, &view, [w, h]);
        assert!(gc.filters > before, "seed {seed}: filters ran");
        let kind = doc
            .root
            .iter()
            .filter_map(|n| n.as_layer())
            .find_map(|l| l.adjustment.as_ref().filter(|_| l.id == "spatial0"))
            .map(|a| a.kind.clone());
        // Mosaic, halftone and crystallize step at cell edges, where the GPU's
        // division (2.5 ulp in WGSL) may put a pixel in the next cell.
        // Crystallize's site hash chains multiply-adds the GPU may fuse, so
        // its facet edges move by a hair: allow it ten times the share.
        let gpu = read(&device, &queue, &tex);
        let over = over_tolerance(&rendered(&doc), None, &gpu, TOL);
        let share = if kind.as_deref() == Some("crystallize") { 1e-3 } else { STEP_OVER };
        assert!(over as f64 <= (share * (w * h) as f64).ceil(), "seed {seed} ({kind:?}): {over} px over");
        // A second frame reuses the filtered tiles.
        let again = gc.filters;
        gc.run(&device, &queue, &progs, &view, [w, h]);
        assert_eq!(gc.filters, again, "seed {seed}: cached");
    }
    assert!(ran >= 16, "{ran} documents ran on the GPU");
}

/// Bloom and Sobel line art on content they act on strongly (the random
/// documents rarely do): bright shapes, a low threshold, full intensity.
#[test]
fn bloom_and_line_art_layers_match_the_rendered_document() {
    use onepaint_compose::spatial::rendered;
    use onepaint_doc::Adjustment;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let mut base = Document::new(w, h, None);
    let mut l = Layer::new("shapes");
    for (x0, y0, c) in [(40, 40, [1.0, 0.9, 0.2, 1.0]), (150, 120, [0.2, 0.6, 1.0, 1.0]), (230, 230, [0.9, 0.9, 0.9, 1.0])] {
        for y in y0..y0 + 50 {
            for x in x0..x0 + 50 {
                if x < w as i32 && y < h as i32 {
                    l.pixels.set(x, y, c);
                }
            }
        }
    }
    base.root = vec![Node::Layer(l)];
    for (kind, params) in [
        ("bloom", serde_json::json!({"threshold": 0.3, "amount": 1.0, "radius": 12.0})),
        ("line-art", serde_json::json!({"lineMode": "sobel", "radius": 1.5, "threshold": 0.05, "amount": 0.8, "mono": true})),
        ("line-art", serde_json::json!({"lineMode": "xdog", "radius": 2.5, "threshold": 0.5, "amount": 0.4, "mono": false})),
    ] {
        let mut doc = base.clone();
        if kind == "line-art" && params["lineMode"] == "sobel" {
            // A gentle ramp: the edge strength sits mid-way up the smoothstep.
            let mut ramp = Layer::new("ramp");
            for y in 0..h as i32 {
                for x in 0..w as i32 {
                    let v = 0.2 + 0.6 * x as f32 / w as f32 + 0.1 * (y as f32 / 40.0).sin();
                    ramp.pixels.set(x, y, [v, v, v, 1.0]);
                }
            }
            doc.root = vec![Node::Layer(ramp)];
        }
        let mut s = Layer::new(kind);
        s.adjustment = Some(Box::new(Adjustment { kind: kind.into(), params: serde_json::from_value(params.clone()).unwrap() }));
        doc.root.push(Node::Layer(s));
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        let (e, at) = max_error(&rendered(&doc), None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "{kind}: error {e} at {at:?}");
    }
}

/// A spatial layer in an isolated group filters the group's own content.
#[test]
fn a_spatial_layer_in_a_group_matches_the_rendered_document() {
    use onepaint_compose::spatial::rendered;
    use onepaint_doc::{Adjustment, Group};
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let mut doc = random_doc(3, w, h, false);
    let mut paint = Layer::new("paint");
    for y in 60..200 {
        for x in 40..220 {
            paint.pixels.set(x, y, [0.2, 0.5, 0.9, 1.0]);
        }
    }
    let mut blur = Layer::new("blur");
    blur.adjustment = Some(Box::new(Adjustment {
        kind: "gaussian-blur".into(),
        params: serde_json::from_value(serde_json::json!({"radius": 9.0})).unwrap(),
    }));
    let mut g = Group::new("g");
    g.opacity = 0.8;
    g.children = vec![Node::Layer(paint), Node::Layer(blur)];
    doc.root.push(Node::Group(g));
    gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
    let (e, at) = max_error(&rendered(&doc), None, &read(&device, &queue, &tex));
    assert!(e <= TOL, "error {e} at {at:?}");
}

/// Stacked spatial layers: the upper one's backdrop reads the lower one's
/// filtered tiles, a stage later (spec 52 phase 3b).
#[test]
fn stacked_spatial_layers_match_the_rendered_document() {
    use onepaint_compose::spatial::rendered;
    use onepaint_compose::testdocs::with_spatial_n;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let mut ran = 0;
    for seed in 0..8 {
        let doc = with_spatial_n(random_doc(seed, w, h, false), seed, 2);
        let Some(progs) = doc.bounds().tiles().map(|(tx, ty)| plan_tile(&doc, tx, ty, None)).collect::<Option<Vec<_>>>() else { continue };
        ran += 1;
        gc.run(&device, &queue, &progs, &view, [w, h]);
        let gpu = read(&device, &queue, &tex);
        let over = over_tolerance(&rendered(&doc), None, &gpu, 4e-3);
        assert!(over as f64 <= (1e-3 * (w * h) as f64).ceil(), "seed {seed}: {over} px over");
    }
    assert!(ran >= 6, "{ran}");
}

/// Two stacked blurs over half-transparent paint: the upper one's backdrop
/// fill reads the lower one's filtered tile. Fills of different stages had
/// shared one buffer set, so the last stage's jobs ran for every fill and the
/// upper layer came out empty (0.75 where 0.9375 is right).
#[test]
fn stacked_blurs_over_half_transparent_paint() {
    use onepaint_compose::spatial::rendered;
    use onepaint_doc::Adjustment;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for (bg, alpha, r0, r1) in [(false, 0.5, 0.0, 0.0), (false, 0.5, 6.0, 6.0), (true, 1.0, 6.0, 6.0)] {
        let mut doc = Document::new(w, h, bg.then_some(onepaint_color::Rgba::WHITE));
        let mut paint = Layer::new("paint");
        for y in 60..200 {
            for x in 40..280 {
                paint.pixels.set(x, y, [0.2 * alpha, 0.5 * alpha, 0.9 * alpha, alpha]);
            }
        }
        doc.root.push(Node::Layer(paint));
        for (i, r) in [r0, r1].into_iter().enumerate() {
            let mut b = Layer::new(&format!("blur{i}"));
            b.adjustment = Some(Box::new(Adjustment {
                kind: "gaussian-blur".into(),
                params: serde_json::from_value(serde_json::json!({"radius": r})).unwrap(),
            }));
            doc.root.push(Node::Layer(b));
        }
        let mut gc = GpuCompositor::new(&device, 64 << 20);
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        let (e, at) = max_error(&rendered(&doc), None, &read(&device, &queue, &tex));
        assert!(e <= TOL, "bg {bg} alpha {alpha} radii {r0}/{r1}: error {e} at {at:?}");
    }
}

/// Spec 53: a whole-canvas layer (twirl, pinch, polar, chromatic
/// aberration, radial and spin blur) filtered over the whole backdrop matches the rendered document. GPU trig
/// moves a twirl's taps by a hundredth of a pixel, which shows at hard
/// edges, and pinch is singular at its centre: the stepped count, as for
/// spec 52's cell kinds.
#[test]
fn whole_canvas_layers_match_the_rendered_document() {
    use onepaint_compose::program::plan_tiles_cached;
    use onepaint_compose::spatial::rendered;
    use onepaint_compose::testdocs::with_whole_spatial;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let mut ran = 0;
    for seed in 0..12 {
        let doc = with_whole_spatial(random_doc(seed, w, h, false), seed);
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let (progs, cpu) = plan_tiles_cached(&doc, &keys, None, None, None);
        if !cpu.is_empty() {
            continue;
        }
        ran += 1;
        gc.run(&device, &queue, &progs, &view, [w, h]);
        let over = over_tolerance(&rendered(&doc), None, &read(&device, &queue, &tex), TOL);
        assert!(over as f64 <= (1.5e-3 * (w * h) as f64).ceil(), "seed {seed}: {over} px over");
    }
    assert!(ran >= 10, "{ran}");
}

/// Spec 53, each kind and mode over stripes and a ramp, so every map shows.
#[test]
fn whole_canvas_kinds_over_stripes_match_the_oracle() {
    use onepaint_compose::spatial::rendered;
    use onepaint_doc::Adjustment;
    use serde_json::json;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let cases = [
        ("twirl", json!({"centerX": 0.4, "centerY": 0.55, "angle": 140.0, "extent": 0.9})),
        ("pinch", json!({"centerX": 0.5, "centerY": 0.5, "amount": 0.6, "extent": 0.8})),
        ("pinch", json!({"centerX": 0.6, "centerY": 0.4, "amount": -0.7, "extent": 1.1})),
        ("polar-coordinates", json!({"centerX": 0.5, "centerY": 0.5, "polarMode": "to-polar"})),
        ("polar-coordinates", json!({"centerX": 0.45, "centerY": 0.6, "polarMode": "to-rect"})),
        ("chromatic-aberration", json!({"amount": 1.5})),
        ("radial-blur", json!({"centerX": 0.4, "centerY": 0.5, "radialMode": "zoom", "amount": 0.6})),
        ("radial-blur", json!({"centerX": 0.6, "centerY": 0.45, "radialMode": "spin", "amount": 0.8})),
        ("spin-blur", json!({"centerX": 0.5, "centerY": 0.5, "angle": 30.0, "extent": 0.7, "aspect": 0.6, "focus": 0.3, "spin": 60.0})),
        ("tilt-shift", json!({"centerX": 0.5, "centerY": 0.45, "angle": 20.0, "focusSize": 0.1, "falloffSize": 0.2, "radius": 18.0})),
        ("tilt-shift", json!({"centerX": 0.5, "centerY": 0.5, "angle": 0.0, "focusSize": 0.05, "radius": 24.0, "bokeh": 0.8})),
        ("iris-blur", json!({"centerX": 0.4, "centerY": 0.5, "extent": 0.6, "aspect": 0.7, "angle": 30.0, "focus": 0.4, "radius": 20.0})),
        (
            "field-blur",
            json!({"pins": [{"x": 0.2, "y": 0.8, "blur": 0.0}, {"x": 0.7, "y": 0.2, "blur": 25.0}, {"x": 0.9, "y": 0.9, "blur": 10.0}]}),
        ),
    ];
    for (kind, params) in cases {
        let mut doc = Document::new(w, h, None);
        let mut paint = Layer::new("paint");
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let stripe = if (x / 12 + y / 20) % 2 == 0 { 0.9 } else { 0.15 };
                let a = 0.4 + 0.6 * x as f32 / w as f32;
                paint.pixels.set(x, y, [stripe * a, (y as f32 / h as f32) * a, 0.5 * a, a]);
            }
        }
        let mut l = Layer::new(kind);
        l.adjustment = Some(Box::new(Adjustment { kind: kind.into(), params: serde_json::from_value(params.clone()).unwrap() }));
        doc.root = vec![Node::Layer(paint), Node::Layer(l)];
        let mut gc = GpuCompositor::new(&device, 64 << 20);
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let (progs, cpu) = onepaint_compose::program::plan_tiles_cached(&doc, &keys, None, None, None);
        assert!(cpu.is_empty(), "{kind} plans on the GPU");
        gc.run(&device, &queue, &progs, &view, [w, h]);
        let over = over_tolerance(&rendered(&doc), None, &read(&device, &queue, &tex), TOL);
        assert!(over as f64 <= (1.5e-3 * (w * h) as f64).ceil(), "{kind} {params}: {over} px over");
    }
}

/// Spec 53: mid-stroke, a whole-canvas filter too heavy for a frame keeps its
/// image (as the CPU path keeps its stale render); light ones refilter.
#[test]
fn heavy_whole_canvas_filters_wait_for_the_stroke_to_end() {
    use onepaint_compose::program::plan_tiles_cached;
    use onepaint_compose::spatial::rendered;
    use onepaint_doc::Adjustment;
    use serde_json::json;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let n = 1024u32;
    let tex = target(&device, n, n);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for (kind, params, heavy) in [("radial-blur", json!({"amount": 0.4}), true), ("twirl", json!({"angle": 90.0}), false)] {
        let mut gc = GpuCompositor::new(&device, 256 << 20);
        let mut doc = Document::new(n, n, Some(onepaint_color::Rgba::WHITE));
        let mut paint = Layer::new("paint");
        paint.pixels.set(300, 300, [0.0, 0.0, 0.0, 1.0]);
        let mut l = Layer::new("f");
        l.adjustment = Some(Box::new(Adjustment { kind: kind.into(), params: serde_json::from_value(params).unwrap() }));
        doc.root.extend([Node::Layer(paint), Node::Layer(l)]);
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let run = |gc: &mut GpuCompositor, doc: &Document| {
            let (progs, cpu) = plan_tiles_cached(doc, &keys, None, None, None);
            assert!(cpu.is_empty());
            gc.run(&device, &queue, &progs, &view, [n, n]);
            read(&device, &queue, &tex)
        };
        run(&mut gc, &doc);
        let before = gc.filters;
        // A stroke paints a block under the filter.
        let Node::Layer(p) = &mut doc.root[1] else { unreachable!() };
        for y in 400..600 {
            for x in 400..600 {
                p.pixels.set(x, y, [0.1, 0.6, 0.2, 1.0]);
            }
        }
        gc.set_interactive(true);
        run(&mut gc, &doc);
        assert_eq!(gc.filters - before, u64::from(!heavy), "{kind} mid-stroke");
        gc.set_interactive(false);
        let gpu = run(&mut gc, &doc);
        let over = over_tolerance(&rendered(&doc), None, &gpu, TOL);
        assert!(over as f64 <= (1.5e-3 * (n * n) as f64).ceil(), "{kind} after the stroke: {over} px over");
    }
}

/// A tile whose sources can't all be resident isn't composited: the GPU
/// reports it for the CPU and leaves its pixels as they were.
#[test]
fn tiles_that_dont_fit_are_left_for_the_cpu() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::with_atlas_layers(&device, 2);
    let (w, h) = (512, 256);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    let mut doc = Document::new(w, h, None);
    for i in 0..3 {
        let mut l = Layer::new(&format!("l{i}"));
        l.pixels.set(10 + i, 10, [0.5, 0.2, 0.1, 0.5]);
        if i == 0 {
            l.pixels.set(300, 10, [0.1, 0.2, 0.9, 1.0]);
        }
        doc.root.push(Node::Layer(l));
    }
    // Tile (1, 0) reads one tile and fits; (0, 0) reads three and doesn't.
    let progs: Vec<_> = [(1, 0), (0, 0)].iter().map(|&(tx, ty)| plan_tile(&doc, tx, ty, None).unwrap()).collect();
    gc.run(&device, &queue, &progs, &view, [w, h]);
    assert_eq!(gc.take_failed(), vec![(0, 0)]);
    let px = read(&device, &queue, &tex);
    let at = |x: u32, y: u32| px[((y * w + x) * 4) as usize..][..4].to_vec();
    assert!(at(300, 10)[3] > 0.99, "the tile that fits composites");
    assert_eq!(at(10, 10), vec![0.0; 4], "the one that doesn't is left alone");
    assert!(gc.take_failed().is_empty(), "taken once");
}

/// Spec 55: ghost groups through their pegs, resampled on the GPU from their
/// content's cache images, match the tree compositor.
#[test]
fn pegged_ghosts_match_the_tree_compositor() {
    use onepaint_compose::testdocs::Rng;
    use onepaint_compose::{GHOST_PEG_KEY, GHOST_TINT_KEY};
    use onepaint_doc::Group;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let (w, h) = (600, 400);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for seed in 0..8u64 {
        let mut doc = random_doc(seed, w, h, seed % 2 == 0);
        let mut r = Rng::new(seed ^ 0x9e9);
        let mut g = Group::new("ghost");
        g.children = std::mem::take(&mut doc.root);
        let peg = [r.f() * 80.0 - 40.0, r.f() * 80.0 - 40.0, r.f() * 0.6 - 0.3, 0.8 + r.f() * 0.5];
        g.extra.insert(GHOST_PEG_KEY.into(), serde_json::json!(peg));
        if r.chance(0.5) {
            g.extra.insert(GHOST_TINT_KEY.into(), serde_json::json!([r.f(), r.f(), r.f()]));
        }
        g.opacity = 0.4 + r.f() * 0.6;
        doc.root = vec![Node::Group(g)];
        gc.run(&device, &queue, &programs(&doc, None), &view, [w, h]);
        let gpu = read(&device, &queue, &tex);
        let over = over_tolerance(&doc, None, &gpu, TOL);
        let (e, at) = max_error(&doc, None, &gpu);
        assert!(over as f64 <= (1e-3 * (w * h) as f64).ceil(), "seed {seed}: {over} px over, worst {e} at {at:?}");
    }
}

/// Spec 56: styled layers and groups run their style pass on the GPU (the
/// exact distance field and every effect) and match the tree compositor.
#[test]
fn layer_styles_match_the_tree_compositor() {
    use onepaint_compose::testdocs::styled_docs;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (w, h) = (300, 300);
    let tex = target(&device, w, h);
    let view = onepaint_gpu::mip::level_view(&tex, 0);
    for (n, doc) in styled_docs().iter().enumerate() {
        let mut gc = GpuCompositor::new(&device, 64 << 20);
        gc.run(&device, &queue, &programs(doc, None), &view, [w, h]);
        let gpu = read(&device, &queue, &tex);
        let over = over_tolerance(doc, None, &gpu, 4e-3);
        let (e, at) = max_error(doc, None, &gpu);
        assert!(over as f64 <= (1e-3 * (w * h) as f64).ceil(), "doc {n}: {over} px over, worst {e} at {at:?}");
    }
}
