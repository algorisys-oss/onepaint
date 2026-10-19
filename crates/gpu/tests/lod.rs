#![allow(clippy::unwrap_used)]
//! Level-of-detail compositing (spec 39) against the box-downsampled CPU
//! oracle, on one composite texture and split into sheets (spec 40); skipped
//! without an adapter.
use onepaint_compose::StrokeOverlay;
use onepaint_compose::composite_tile;
use onepaint_compose::program::{TileProgram, plan_tile, plan_tile_split};
use onepaint_compose::testdocs::{Rng, random_doc, random_stroke};
use onepaint_doc::{BlendMode, Document, Layer, Node};
use onepaint_gpu::Sheets;
use onepaint_gpu::compose::GpuCompositor;
use onepaint_gpu::mip::{LEVELS, Mipper, level_view};
use onepaint_raster::{TILE, Tile};
use std::sync::Arc;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// The composite as one texture, and split into 256² sheets.
fn layouts(doc: [u32; 2]) -> [Sheets; 2] {
    let split = Sheets::plan(doc, 256, 256).unwrap();
    assert!(split.count() > 1);
    [Sheets::single(doc), split]
}

fn target(device: &wgpu::Device, sheets: &Sheets) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: sheets.sheet[0], height: sheets.sheet[1], depth_or_array_layers: sheets.count() },
        mip_level_count: LEVELS,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Level `l` of the document, gathered from its sheets, as row-major f32 RGBA.
fn read(device: &wgpu::Device, queue: &wgpu::Queue, tex: &wgpu::Texture, sheets: &Sheets, l: u32) -> Vec<f32> {
    let [w, h] = sheets.doc_at(l);
    let [sw, sh] = sheets.sheet_at(l);
    let mut out = vec![0.0f32; (w * h * 4) as usize];
    for layer in 0..sheets.count() {
        let [x0, y0, _, _] = sheets.rect(layer);
        let (x0, y0) = (x0 >> l, y0 >> l);
        let (cw, ch) = (sw.min(w - x0), sh.min(h - y0));
        let stride = (cw * 8).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (stride * ch) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: tex,
                mip_level: l,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(stride), rows_per_image: Some(ch) },
            },
            wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buf.slice(..).get_mapped_range().unwrap();
        for y in 0..ch as usize {
            let row: &[half::f16] = bytemuck::cast_slice(&data[y * stride as usize..y * stride as usize + cw as usize * 8]);
            let d = (((y0 as usize + y) * w as usize) + x0 as usize) * 4;
            for (o, v) in out[d..d + row.len()].iter_mut().zip(row) {
                *o = v.to_f32();
            }
        }
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
    sheets: &Sheets,
    progs: &[TileProgram],
    l: u32,
) -> Vec<f32> {
    let view = level_view(tex, l);
    gc.run_level(device, queue, progs, &view, sheets, l);
    read(device, queue, tex, sheets, l)
}

#[test]
fn downsampling_a_level_0_composite_matches_the_box_oracle() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let doc = random_doc(11, 600, 520, true);
    for sheets in layouts([600, 520]) {
        let tex = target(&device, &sheets);
        let mut gc = GpuCompositor::new(&device, 64 << 20);
        run_at(&mut gc, &device, &queue, &tex, &sheets, &programs(&doc, None, None), 0);
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let mut enc = device.create_command_encoder(&Default::default());
        Mipper::new(&device).tiles(&device, &mut enc, &tex, &sheets, &keys, 0);
        queue.submit([enc.finish()]);
        for l in 1..LEVELS {
            let (want, _) = oracle(&doc, None, l);
            let e = max_error(&want, &read(&device, &queue, &tex, &sheets, l));
            assert!(e <= 3e-3, "{:?} sheets, level {l}: error {e}", sheets.grid);
        }
    }
}

#[test]
fn uniform_tiles_composite_exactly_at_every_level() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let doc = uniform_doc(10);
    let hot = doc.layer_ids()[5].clone();
    for sheets in layouts([600, 520]) {
        let tex = target(&device, &sheets);
        let mut gc = GpuCompositor::new(&device, 64 << 20);
        for l in 1..LEVELS {
            let (want, _) = oracle(&doc, None, l);
            for (name, progs) in [("unsplit", programs(&doc, None, None)), ("split", programs(&doc, None, Some(&hot)))] {
                for pass in ["cold", "warm"] {
                    let e = max_error(&want, &run_at(&mut gc, &device, &queue, &tex, &sheets, &progs, l));
                    assert!(e <= 2e-3, "{:?} sheets, level {l} {name} {pass}: error {e}", sheets.grid);
                }
            }
        }
    }
}

/// Hard Mix and Darker/Lighter Color threshold each pixel, so a preview
/// blends the averaged inputs into a coarser on/off pattern rather than the
/// fine one's average. A preview draws Dissolve as Normal, the dither's
/// average, but a nonlinear blend above the dither doesn't average the same
/// way. No bulk bound holds for these on noise, so they become Normal here
/// (level 0 is exact for them, and idle frames refine to it).
fn unthreshold(nodes: &mut [Node]) {
    for n in nodes {
        if matches!(n.blend_mode(), BlendMode::HardMix | BlendMode::DarkerColor | BlendMode::LighterColor | BlendMode::Dissolve) {
            n.set_blend_mode(BlendMode::Normal);
        }
        match n {
            Node::Group(g) => unthreshold(&mut g.children),
            // Blend If is a tone step too.
            Node::Layer(l) => l.set_blend_if(None),
        }
    }
}

#[test]
fn random_documents_preview_within_the_bound() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let sheets = Sheets::single([300, 300]);
    let tex = target(&device, &sheets);
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    for seed in 0..16 {
        let mut doc = random_doc(seed, 300, 300, seed % 2 == 0);
        unthreshold(&mut doc.root);
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
            let got = run_at(&mut gc, &device, &queue, &tex, &sheets, &programs(&doc, ov.as_ref(), None), l);
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

#[test]
fn sheets_composite_the_same_as_one_texture() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let doc = random_doc(5, 600, 520, true);
    let progs = programs(&doc, None, None);
    let [one, split] = layouts([600, 520]);
    let (t1, t2) = (target(&device, &one), target(&device, &split));
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    for l in 0..=3 {
        let a = run_at(&mut gc, &device, &queue, &t1, &one, &progs, l);
        let b = run_at(&mut gc, &device, &queue, &t2, &split, &progs, l);
        assert_eq!(a, b, "level {l}");
    }
}

/// Mip levels of 8-bit atlases (spec 39 on `Rgba8Unorm`) preview like the
/// f16 ones: each level rounds to bytes once more, nothing else changes.
#[test]
fn eight_bit_atlases_preview_like_f16_ones() {
    use onepaint_raster::Store;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let sheets = Sheets::single([300, 300]);
    let tex = target(&device, &sheets);
    let doc = random_doc(5, 300, 300, false);
    let mut q = doc.clone();
    fn walk(nodes: &mut [Node]) {
        for n in nodes {
            match n {
                Node::Layer(l) => {
                    for (tx, ty) in l.pixels.tile_keys() {
                        let t = l.pixels.tile(tx, ty).map(|t| std::sync::Arc::new(t.to_store(Store::Unorm8)));
                        l.pixels.set_tile(tx, ty, t);
                    }
                }
                Node::Group(g) => walk(&mut g.children),
            }
        }
    }
    walk(&mut q.root);
    // The f16 reference holds the same (quantized) values.
    let mut r = q.clone();
    fn widen(nodes: &mut [Node]) {
        for n in nodes {
            match n {
                Node::Layer(l) => {
                    for (tx, ty) in l.pixels.tile_keys() {
                        let t = l.pixels.tile(tx, ty).map(|t| std::sync::Arc::new(t.to_store(Store::F16)));
                        l.pixels.set_tile(tx, ty, t);
                    }
                }
                Node::Group(g) => widen(&mut g.children),
            }
        }
    }
    widen(&mut r.root);
    let _ = doc;
    for l in 1..=3 {
        let mut a = GpuCompositor::new(&device, 64 << 20);
        let mut b = GpuCompositor::new(&device, 64 << 20);
        let got = run_at(&mut a, &device, &queue, &tex, &sheets, &programs(&q, None, None), l);
        let want = run_at(&mut b, &device, &queue, &tex, &sheets, &programs(&r, None, None), l);
        assert!(a.atlas_counts()[1] >= 1 && b.atlas_counts()[1] == 0);
        let worst = got.iter().zip(&want).map(|(x, y)| (x - y).abs()).fold(0.0f32, f32::max);
        // Each level rounds to 1/255 once: within a few steps at level 3.
        assert!(worst <= 4.0 / 255.0, "level {l}: {worst}");
    }
}

/// Spec 53 above level 0: a whole-canvas layer filters the level's image,
/// with positions as fractions of the document, so it previews like the
/// filtered document box-averaged. The backdrop is smooth: over hard edges
/// a map that minifies (polar's radius) aliases at reduced size, which is
/// the preview's nature, not its maths.
#[test]
fn whole_canvas_layers_preview_within_the_bound() {
    use onepaint_compose::spatial::rendered;
    use serde_json::json;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let sheets = Sheets::single([300, 300]);
    let tex = target(&device, &sheets);
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    let cases = [
        ("twirl", json!({"centerX": 0.4, "centerY": 0.55, "angle": 140.0, "extent": 0.9})),
        ("pinch", json!({"centerX": 0.5, "centerY": 0.5, "amount": 0.6, "extent": 0.8})),
        ("polar-coordinates", json!({"centerX": 0.5, "centerY": 0.5, "polarMode": "to-polar"})),
        ("chromatic-aberration", json!({"amount": 1.5})),
        ("radial-blur", json!({"centerX": 0.4, "centerY": 0.5, "radialMode": "zoom", "amount": 0.6})),
        ("spin-blur", json!({"centerX": 0.5, "centerY": 0.5, "extent": 0.7, "aspect": 0.6, "focus": 0.3, "spin": 60.0})),
        ("light-rays", json!({"centerX": 0.3, "centerY": 0.2, "extent": 0.7, "threshold": 0.4, "amount": 0.8})),
        ("tilt-shift", json!({"centerX": 0.5, "centerY": 0.45, "angle": 20.0, "focusSize": 0.1, "falloffSize": 0.2, "radius": 18.0})),
        ("iris-blur", json!({"centerX": 0.4, "centerY": 0.5, "extent": 0.6, "aspect": 0.7, "focus": 0.4, "radius": 20.0, "bokeh": 0.6})),
        ("field-blur", json!({"pins": [{"x": 0.2, "y": 0.8, "blur": 0.0}, {"x": 0.7, "y": 0.2, "blur": 25.0}]})),
    ];
    for (kind, params) in cases {
        let mut doc = Document::new(300, 300, None);
        let mut paint = Layer::new("paint");
        for y in 0..300 {
            for x in 0..300 {
                let (u, v) = (x as f32 / 300.0, y as f32 / 300.0);
                let wave = 0.5 + 0.4 * (u * 9.0).sin() * (v * 7.0).cos();
                paint.pixels.set(x, y, [wave, v, 0.5, 1.0]);
            }
        }
        let mut l = Layer::new(kind);
        l.adjustment = Some(Box::new(onepaint_doc::Adjustment { kind: kind.into(), params: serde_json::from_value(params).unwrap() }));
        doc.root = vec![Node::Layer(paint), Node::Layer(l)];
        let keys: Vec<_> = doc.bounds().tiles().collect();
        let (progs, cpu) = onepaint_compose::program::plan_tiles_cached(&doc, &keys, None, None, None);
        assert!(cpu.is_empty());
        let want_doc = rendered(&doc);
        for l in 1..=3 {
            let (want, _) = oracle(&want_doc, None, l);
            let got = run_at(&mut gc, &device, &queue, &tex, &sheets, &progs, l);
            let mut errs: Vec<f32> = want.iter().zip(&got).map(|(a, b)| (a - b).abs()).collect();
            errs.sort_by(f32::total_cmp);
            let mean = errs.iter().sum::<f32>() / errs.len() as f32;
            let p99 = errs[(errs.len() - 1) * 99 / 100];
            assert!(mean <= 0.015 && p99 <= 0.15, "{kind} level {l}: mean {mean}, p99 {p99}");
        }
    }
}

/// Spec 55 above level 0: a ghost group through its peg previews like the
/// level-0 composite box-averaged.
#[test]
fn pegged_ghosts_preview_within_the_bound() {
    use onepaint_compose::GHOST_PEG_KEY;
    use onepaint_doc::Group;
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let sheets = Sheets::single([600, 400]);
    let tex = target(&device, &sheets);
    let mut gc = GpuCompositor::new(&device, 64 << 20);
    for (seed, peg) in [(1u64, [30.0f32, -20.0, 0.2, 1.1]), (2, [-15.0, 40.0, -0.3, 0.9])] {
        let mut doc = random_doc(seed, 600, 400, false);
        unthreshold(&mut doc.root);
        let mut g = Group::new("ghost");
        g.children = std::mem::take(&mut doc.root);
        g.extra.insert(GHOST_PEG_KEY.into(), serde_json::json!(peg));
        doc.root = vec![Node::Group(g)];
        let progs = programs(&doc, None, None);
        for l in 1..=3 {
            let (want, _) = oracle(&doc, None, l);
            let got = run_at(&mut gc, &device, &queue, &tex, &sheets, &progs, l);
            let mut errs: Vec<f32> = want.iter().zip(&got).map(|(a, b)| (a - b).abs()).collect();
            errs.sort_by(f32::total_cmp);
            let mean = errs.iter().sum::<f32>() / errs.len() as f32;
            let p99 = errs[(errs.len() - 1) * 99 / 100];
            assert!(mean <= 0.015 && p99 <= 0.15, "seed {seed} level {l}: mean {mean}, p99 {p99}");
        }
    }
}
