//! The live wet simulation's steps as compute passes (spec 35 B): a
//! [`StepBackend`] for `brush::fluid`. One upload and one readback per
//! batch of steps; the CPU step is the oracle (`tests/wet.rs`).

use onepaint_brush::fluid::{StepBackend, StepJob};
use wgpu::util::DeviceExt;

pub const WET_WGSL: &str = include_str!("../shaders/wet.wgsl");
/// Kubelka–Munk mixing, appended to the shaders that use it.
pub const PIGMENT_WGSL: &str = include_str!("../shaders/pigment.wgsl");

/// Activity block size (px), matching the shader.
const BLOCK: u32 = 16;

/// Blocks that may hold state during the batch: those holding any now,
/// widened by the reach of `steps` steps (2 px diffusion + 2 px flow each).
fn active_blocks(job: &StepJob<'_>, block: u32) -> Vec<u32> {
    let (w, h) = (job.rect.w as usize, job.rect.h as usize);
    let b = block as usize;
    let (bw, bh) = (w.div_ceil(b), h.div_ceil(b));
    let mut has = vec![false; bw * bh];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let (s, c) = (job.sim[i], job.col[i]);
            if s[0] != 0.0 || s[2] != 0.0 || s[3] != 0.0 || c[3] != 0.0 {
                has[(y / b) * bw + x / b] = true;
            }
        }
    }
    let reach = (4 * job.steps).div_ceil(b) as i64;
    let mut out = vec![0u32; bw * bh];
    for by in 0..bh as i64 {
        for bx in 0..bw as i64 {
            if !has[by as usize * bw + bx as usize] {
                continue;
            }
            for ny in (by - reach).max(0)..=(by + reach).min(bh as i64 - 1) {
                for nx in (bx - reach).max(0)..=(bx + reach).min(bw as i64 - 1) {
                    out[ny as usize * bw + nx as usize] = 1;
                }
            }
        }
    }
    out
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    size: [u32; 2],
    origin: [f32; 2],
    tilt: [f32; 2],
    oil: f32,
    paper_strength: f32,
    paper_tile_px: f32,
    paper_size: u32,
    blocks_w: u32,
    /// 1 when colours are sRGB values (8-bit documents, spec 50).
    gamma: f32,
}

/// Buffers for one allocation size.
struct Buffers {
    len: u64,
    sim: [wgpu::Buffer; 2],
    col: [wgpu::Buffer; 2],
    acc: wgpu::Buffer,
    finger: wgpu::Buffer,
    readback: wgpu::Buffer,
}

pub struct WetGpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    settle: wgpu::ComputePipeline,
    update: wgpu::ComputePipeline,
    km: wgpu::Buffer,
    params: wgpu::Buffer,
    buffers: Option<Buffers>,
    /// The paper tile on the GPU and the grain it came from, held so a
    /// freed grain's address can't stand for a new one.
    paper: Option<(std::sync::Arc<onepaint_brush::TipMask>, wgpu::Buffer)>,
    blank_paper: wgpu::Buffer,
    /// The state generation the buffers hold, and which ping-pong half.
    resident: Option<(u64, usize)>,
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

impl WetGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("wet"),
            source: wgpu::ShaderSource::Wgsl(format!("{WET_WGSL}\n{PIGMENT_WGSL}").into()),
        });
        let mut entries = vec![wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        }];
        for (b, ro) in [(1, true), (2, true), (3, false), (4, false), (5, false), (6, true), (7, true), (9, true)] {
            entries.push(storage(b, ro));
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("wet"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("wet"),
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
        let (b, t) = onepaint_color::pigment::basis_matrices();
        let mut km: Vec<f32> = b.iter().flat_map(|r| [r[0] as f32, r[1] as f32, r[2] as f32, 0.0]).collect();
        km.extend((0..9).flat_map(|k| [t[0][k] as f32, t[1][k] as f32, t[2][k] as f32, 0.0]));
        let init = |label: &str, contents: &[u8], usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
        };
        Self {
            settle: pipeline("settle"),
            update: pipeline("update"),
            km: init("km basis", bytemuck::cast_slice(&km), wgpu::BufferUsages::UNIFORM),
            params: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("wet params"),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            blank_paper: init("paper", bytemuck::cast_slice(&[0.5f32]), wgpu::BufferUsages::STORAGE),
            layout,
            buffers: None,
            paper: None,
            resident: None,
            device: device.clone(),
            queue: queue.clone(),
        }
    }

    fn buffers(&mut self, px: u64) -> &Buffers {
        if self.buffers.as_ref().is_none_or(|b| b.len != px) {
            self.resident = None;
            let d = &self.device;
            let rw = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
            let mk = |label: &str, size: u64, usage| {
                d.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false })
            };
            self.buffers = Some(Buffers {
                len: px,
                sim: [mk("sim a", px * 16, rw), mk("sim b", px * 16, rw)],
                col: [mk("col a", px * 16, rw), mk("col b", px * 16, rw)],
                acc: mk("acc", px * 16, rw),
                finger: mk("finger", px * 4, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST),
                readback: mk("readback", px * 48, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
            });
        }
        self.buffers.as_ref().unwrap_or_else(|| unreachable!())
    }
}

impl StepBackend for WetGpu {
    fn run(&mut self, job: StepJob<'_>) -> bool {
        let (w, h) = (job.rect.w as u64, job.rect.h as u64);
        let px = w * h;
        if px == 0 || px * 16 > self.device.limits().max_storage_buffer_binding_size {
            return false;
        }
        // Paper tile (cached while the same tile is in use).
        let paper_buf = match job.paper {
            Some((m, _, _)) => {
                if self.paper.as_ref().is_none_or(|p| !std::sync::Arc::ptr_eq(&p.0, m)) {
                    let buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("paper"),
                        contents: bytemuck::cast_slice(&m.data),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                    self.paper = Some((m.clone(), buf));
                }
                self.paper.as_ref().map(|p| p.1.clone())
            }
            None => None,
        }
        .unwrap_or_else(|| self.blank_paper.clone());
        let params = Params {
            size: [w as u32, h as u32],
            origin: [job.rect.x as f32, job.rect.y as f32],
            tilt: job.tilt,
            oil: if job.oil { 1.0 } else { 0.0 },
            paper_strength: job.paper.map_or(0.0, |p| p.2),
            paper_tile_px: job.paper.map_or(1.0, |p| p.1 as f32),
            paper_size: job.paper.map_or(1, |p| p.0.size as u32),
            blocks_w: (w as u32).div_ceil(BLOCK),
            gamma: if job.space == onepaint_color::Space::Gamma { 1.0 } else { 0.0 },
        };
        let active = active_blocks(&job, BLOCK);
        let active_buf = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("active blocks"),
            contents: bytemuck::cast_slice(&active),
            usage: wgpu::BufferUsages::STORAGE,
        });
        self.queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        self.buffers(px);
        let (layout, params_buf, km) = (&self.layout, &self.params, &self.km);
        // The state is already on the GPU when nothing changed it since our
        // last run (the drying phase); else upload it.
        let first = match self.resident.take() {
            Some((g, half)) if g == job.generation => half,
            _ => usize::MAX,
        };
        let Some(b) = self.buffers.as_ref() else { return false };
        let first = if first == usize::MAX {
            self.queue.write_buffer(&b.sim[0], 0, bytemuck::cast_slice(job.sim));
            self.queue.write_buffer(&b.col[0], 0, bytemuck::cast_slice(job.col));
            self.queue.write_buffer(&b.finger, 0, bytemuck::cast_slice(job.finger));
            0
        } else {
            first
        };
        let group = |from: usize| {
            let to = 1 - from;
            fn e(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
                wgpu::BindGroupEntry { binding, resource: buf.as_entire_binding() }
            }
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("wet"),
                layout,
                entries: &[
                    e(0, params_buf),
                    e(1, &b.sim[from]),
                    e(2, &b.col[from]),
                    e(3, &b.sim[to]),
                    e(4, &b.col[to]),
                    e(5, &b.acc),
                    e(6, &b.finger),
                    e(7, &paper_buf),
                    e(8, km),
                    e(9, &active_buf),
                ],
            })
        };
        let groups = [group(0), group(1)];
        // Only rows touching live blocks can hold anything: clear and read those.
        let bw = (w as u32).div_ceil(BLOCK) as usize;
        let rows = active.chunks(bw).enumerate().filter(|(_, r)| r.iter().any(|v| *v != 0)).map(|(i, _)| i as u64);
        let (Some(r0), Some(r1)) = (rows.clone().min(), rows.max()) else {
            // Nothing live: nothing moves or settles.
            self.resident = None;
            return true;
        };
        let (y0, y1) = (r0 * BLOCK as u64, ((r1 + 1) * BLOCK as u64).min(h));
        let (off, len) = (y0 * w * 16, (y1 - y0) * w * 16);
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("wet") });
        enc.clear_buffer(&b.acc, off, Some(len));
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("wet steps"), timestamp_writes: None });
            let (gx, gy) = (w.div_ceil(8) as u32, h.div_ceil(8) as u32);
            for step in 0..job.steps {
                pass.set_bind_group(0, &groups[(first + step) % 2], &[]);
                pass.set_pipeline(&self.settle);
                pass.dispatch_workgroups(gx, gy, 1);
                pass.set_pipeline(&self.update);
                pass.dispatch_workgroups(gx, gy, 1);
            }
        }
        let last = (first + job.steps) % 2;
        enc.copy_buffer_to_buffer(&b.sim[last], off, &b.readback, 0, len);
        enc.copy_buffer_to_buffer(&b.col[last], off, &b.readback, len, len);
        enc.copy_buffer_to_buffer(&b.acc, off, &b.readback, 2 * len, len);
        self.queue.submit([enc.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        b.readback.map_async(wgpu::MapMode::Read, ..3 * len, move |r| {
            let _ = tx.send(r);
        });
        if self.device.poll(wgpu::PollType::wait_indefinitely()).is_err() || !matches!(rx.recv(), Ok(Ok(()))) {
            return false;
        }
        {
            let Ok(view) = b.readback.get_mapped_range(..3 * len) else { return false };
            let all: &[[f32; 4]] = bytemuck::cast_slice(&view);
            let (o, n) = ((y0 * w) as usize, ((y1 - y0) * w) as usize);
            job.sim[o..o + n].copy_from_slice(&all[..n]);
            job.col[o..o + n].copy_from_slice(&all[n..2 * n]);
            job.settled[o..o + n].copy_from_slice(&all[2 * n..3 * n]);
        }
        b.readback.unmap();
        self.resident = Some((job.next_generation, last));
        true
    }
}

/// One [`WetGpu`] shared by every open document's session.
#[derive(Clone)]
pub struct SharedWetGpu(std::sync::Arc<std::sync::Mutex<WetGpu>>);

impl SharedWetGpu {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(WetGpu::new(device, queue))))
    }
}

impl StepBackend for SharedWetGpu {
    fn run(&mut self, job: StepJob<'_>) -> bool {
        self.0.lock().map(|mut g| g.run(job)).unwrap_or(false)
    }
}
