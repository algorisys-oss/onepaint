#![allow(clippy::unwrap_used)]
//! The GPU wet step against the CPU oracle (AGENTS.md: float passes match
//! within 1e-3). Skipped when no GPU adapter exists.
use onepaint_brush::fluid::{FluidSim, WetDab};
use onepaint_gpu::wet::WetGpu;
use onepaint_raster::{Rect, Surface};

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

const DOC: Rect = Rect::new(0, 0, 320, 200);

/// Blue wash, then yellow across it (KM), with a brush shove.
fn scene(oil: bool, tilt: [f32; 2], paper: Option<onepaint_doc::Paper>) -> FluidSim {
    let mut s = FluidSim::new(DOC, Rect::new(60, 60, 200, 80), oil);
    s.set_tilt(tilt[0], tilt[1]);
    s.set_paper(paper.as_ref(), 300.0);
    let dab = |x: f64, y: f64, c: [f32; 3], vx: f64| WetDab {
        x,
        y,
        radius: 18.0,
        water: 2.0,
        pigment: 0.4,
        color: c,
        vx,
        vy: 0.0,
        rewet: 0.0,
        dry: 0.0,
        grain_thr: 0.0,
        halo: 1.0,
        rot: 0.0,
    };
    s.deposit(&(0..50).map(|i| dab(80.0 + i as f64 * 3.0, 100.0, [0.02, 0.1, 0.7], 6.0)).collect::<Vec<_>>(), None, None);
    s.deposit(&(0..20).map(|i| dab(160.0, 70.0 + i as f64 * 3.0, [0.9, 0.75, 0.02], 0.0)).collect::<Vec<_>>(), None, None);
    s
}

fn max_diff(a: &Surface, b: &Surface, r: Rect) -> f32 {
    a.read_rect(r).iter().zip(b.read_rect(r)).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

/// A wash's wall down x = 170, through the scene (spec 58 C).
fn wall(s: &mut FluidSim) {
    use onepaint_brush::fluid::wash::{WashOpts, wash_field};
    let (w, h) = (DOC.w as usize, DOC.h as usize);
    let mask: Vec<bool> = (0..w * h).map(|i| i % w < 170).collect();
    s.add_wall(&wash_field(&mask, w, h, 100, 100, &WashOpts::default()).unwrap());
}

fn compare(oil: bool, tilt: [f32; 2], paper: Option<onepaint_doc::Paper>, steps: usize) {
    compare_with(oil, tilt, paper, steps, false);
}

fn compare_with(oil: bool, tilt: [f32; 2], paper: Option<onepaint_doc::Paper>, steps: usize, walled: bool) {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let mut gpu = WetGpu::new(&device, &queue);
    let (mut cpu_sim, mut gpu_sim) = (scene(oil, tilt, paper.clone()), scene(oil, tilt, paper));
    if walled {
        wall(&mut cpu_sim);
        wall(&mut gpu_sim);
    }
    let (mut cpu_layer, mut gpu_layer) = (Surface::new(), Surface::new());
    for _ in 0..steps {
        let r = cpu_sim.rect();
        cpu_sim.step(&mut cpu_layer, r, false);
    }
    let r = gpu_sim.rect();
    assert!(gpu_sim.step_with(&mut gpu, &mut gpu_layer, r, steps).is_some(), "the GPU took the job");
    let (dl, dp) = (max_diff(&cpu_layer, &gpu_layer, DOC), max_diff(&cpu_sim.preview(), &gpu_sim.preview(), DOC));
    let mass = (cpu_sim.suspended_mass(), gpu_sim.suspended_mass());
    eprintln!("layer Δ {dl:.2e}, wet Δ {dp:.2e}, mass {mass:?}");
    assert!(dl < 1e-3 && dp < 1e-3, "layer Δ {dl}, wet Δ {dp}");
    assert!((mass.0 - mass.1).abs() / mass.0.max(1.0) < 1e-3);
}

#[test]
fn a_wash_matches_the_cpu() {
    compare(false, [0.0, 0.0], None, 24);
}

#[test]
fn tilt_and_paper_match_the_cpu() {
    let rough = onepaint_doc::PAPER_PRESETS.iter().find(|p| p.id == "rough").and_then(|p| p.paper());
    compare(false, [0.3, 0.8], rough, 24);
}

#[test]
fn wash_walls_match_the_cpu() {
    compare_with(false, [0.6, 0.2], None, 24, true);
}

#[test]
fn oils_match_the_cpu() {
    compare(true, [0.0, 0.0], None, 12);
}

#[test]
fn a_session_dries_the_same_on_the_gpu() {
    use onepaint_engine::{Command, Session};
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let run = |gpu: bool| {
        let mut s = Session::new(240, 160, None);
        if gpu {
            s.set_wet_backend(Some(Box::new(WetGpu::new(&device, &queue))));
        }
        let mut b = onepaint_brush::preset("watercolor-live").unwrap();
        b.extra.get_mut("fluid").unwrap()["water"] = serde_json::json!(0.05);
        s.execute(Command::SetBrushPreset { preset: Box::new(b) }).unwrap();
        s.execute(Command::SetBrushSize { size: 30.0 }).unwrap();
        let pt = |x: f64| onepaint_stroke::StrokePoint { x, y: 80.0, pressure: 0.7, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None };
        s.stroke_begin(pt(40.0));
        s.stroke_extend(&(0..=70).map(|i| pt(40.0 + i as f64 * 2.0)).collect::<Vec<_>>());
        s.stroke_end();
        let mut now = 0.0;
        for _ in 0..2000 {
            if !s.wet_active() {
                break;
            }
            now += 64.0; // four steps per tick, the most a tick takes
            s.wet_tick(now);
        }
        assert!(!s.wet_active(), "dried on its own");
        assert!(s.can_undo());
        s.doc().layer(s.active()).unwrap().pixels.read_rect(s.doc().bounds())
    };
    let (cpu, gpu) = (run(false), run(true));
    let d = cpu.iter().zip(&gpu).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    let painted = gpu.chunks_exact(4).filter(|p| p[3] > 0.05).count();
    let n = cpu.len() as f32;
    let mean = cpu.iter().zip(&gpu).map(|(a, b)| (a - b).abs()).sum::<f32>() / n;
    let (mc, mg) = (cpu.iter().sum::<f32>() / n, gpu.iter().sum::<f32>() / n);
    eprintln!("session Δ max {d:.2e} mean {mean:.2e}, means {mc:.5} vs {mg:.5}, {painted} px painted");
    // The step-level tests hold 1e-3. Over a whole session the CPU writes each
    // step's settled paint into f16 tiles, where an increment under half an
    // f16 step is lost; the GPU sums in f32 and writes once a tick, so it keeps
    // a little more paint. Hence a mean bound, and a looser max.
    assert!(painted > 1000 && mean < 1e-4 && d < 5e-3, "Δ max {d}, mean {mean}");
}

/// A wash fill dries the same on the GPU: it steps one at a time while
/// the wash spreads, with the CPU laying each band (spec 58 C).
#[test]
fn a_wash_fill_dries_the_same_on_the_gpu() {
    use onepaint_engine::{Command, Session, WashFillOptions};
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let run = |gpu: bool| {
        let mut s = Session::new(240, 160, None);
        if gpu {
            s.set_wet_backend(Some(Box::new(WetGpu::new(&device, &queue))));
        }
        let left = vec![[0.0, 0.0], [100.0, 0.0], [100.0, 160.0], [0.0, 160.0]];
        s.execute(Command::SelectShape { contours: vec![left], mode: onepaint_engine::SelectMode::Replace, antialias: false }).unwrap();
        s.execute(Command::SetColor { rgba: onepaint_color::Rgba::new(0.0, 0.0, 0.0, 1.0) }).unwrap();
        s.execute(Command::FillSelection).unwrap();
        s.execute(Command::Deselect).unwrap();
        s.execute(Command::SetColor { rgba: onepaint_color::Rgba::new(0.2, 0.4, 0.9, 1.0) }).unwrap();
        let o =
            WashFillOptions { tolerance: 0.1, grow: 0, sample_all: false, water: 0.4, pigment: 0.7, grade: 0.3, soft: 4.8, speed: 10.0 };
        s.execute(Command::WashFill { x: 180, y: 80, options: o }).unwrap();
        let mut now = 0.0;
        for _ in 0..4000 {
            if !s.wet_active() {
                break;
            }
            now += 64.0;
            s.wet_tick(now);
        }
        assert!(!s.wet_active(), "dried on its own");
        s.doc().layer(s.active()).unwrap().pixels.read_rect(s.doc().bounds())
    };
    let (cpu, gpu) = (run(false), run(true));
    let n = cpu.len() as f32;
    let mean = cpu.iter().zip(&gpu).map(|(a, b)| (a - b).abs()).sum::<f32>() / n;
    let d = cpu.iter().zip(&gpu).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
    let painted = gpu.chunks_exact(4).filter(|p| p[3] > 0.05).count();
    eprintln!("wash Δ max {d:.2e} mean {mean:.2e}, {painted} px painted");
    // The step-level tests hold 1e-3. A wash keeps half the canvas wet for
    // about a thousand steps, and the GPU's and CPU's float orders drift
    // apart over them (the same with f32 tiles, so not the f16 loss above).
    assert!(painted > 8000 && mean < 5e-4 && d < 5e-3, "Δ max {d}, mean {mean}");
}
