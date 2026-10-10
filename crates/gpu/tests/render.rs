#![allow(clippy::unwrap_used)]
//! Headless render of the present pass; skipped when no GPU adapter exists.
use onepaint_gpu::{CanvasGpu, ViewParams};
use onepaint_raster::Tile;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

#[test]
fn present_pass_draws_document_checker_and_stage() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (w, h) = (64u32, 64u32);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut gpu = CanvasGpu::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    assert!(gpu.ensure_size(&device, 32, 32));
    // Left half opaque red, right half transparent (shows the checker).
    let mut t = Tile::default();
    for y in 0..32 {
        for x in 0..16 {
            t.set(x, y, [1.0, 0.0, 0.0, 1.0]);
        }
    }
    gpu.upload(&queue, &[((0, 0), t)]);
    // Select the doc rect 4..28 (doc px) so its edge gets marching ants.
    let mut sel = vec![0u8; 32 * 32];
    for y in 4..28 {
        for x in 4..28 {
            sel[y * 32 + x] = 255;
        }
    }
    gpu.set_selection(&queue, Some(&sel));
    gpu.set_view(
        &queue,
        &ViewParams {
            origin: [0.0, 0.0],
            size: [64.0, 64.0],
            offset: [0.0, 0.0],
            scale: 1.0,
            rotation: 0.0,
            checker: 4.0,
            stage: [0.0, 0.0, 1.0, 1.0],
            checker_a: [0.85, 0.85, 0.85, 1.0],
            checker_b: [0.64, 0.64, 0.64, 1.0],
            time: 0.0,
        },
    );
    let view = target.create_view(&Default::default());
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        gpu.draw(&mut pass);
    }
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (256 * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(h) },
        },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = buf.slice(..).get_mapped_range().unwrap().to_vec();
    let px = |x: u32, y: u32| {
        let i = (y * 256 + x * 4) as usize;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    };
    // Doc (32×32) is centred: it covers 16..48 on both axes.
    assert_eq!(px(18, 32), [255, 0, 0, 255], "red document pixel");
    let chk = px(40, 32);
    assert!(chk[0] == chk[1] && chk[0] > 150, "checker {chk:?}");
    assert_eq!(px(2, 2), [0, 0, 255, 255], "stage");
    // Ants on the selection's inner edge (doc x=4 → screen x=20), black or white.
    let edge = px(20, 32);
    assert!(edge == [0, 0, 0, 255] || edge == [255, 255, 255, 255], "ants {edge:?}");
    // Away from the edge the content shows through.
    assert_eq!(px(22, 32), [255, 0, 0, 255]);
}

/// Draw `gpu` into a `w`×`h` Rgba8 target and read it back, tightly packed.
fn render(gpu: &CanvasGpu, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32) -> Vec<u8> {
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let mut enc = device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        gpu.draw(&mut pass);
    }
    let stride = (w * 4).next_multiple_of(256);
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (stride * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        target.as_image_copy(),
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
    (0..h as usize).flat_map(|y| data[y * stride as usize..y * stride as usize + w as usize * 4].to_vec()).collect()
}

#[test]
fn sheets_present_like_one_texture() {
    let Some((device, queue)) = device() else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (dw, dh) = (600u32, 520u32);
    // A smooth field with sharp bands across the sheet seams at 256 and 512.
    let tiles: Vec<((i32, i32), Tile)> = (0..3)
        .flat_map(|ty| (0..3).map(move |tx| (tx, ty)))
        .map(|(tx, ty)| {
            let mut t = Tile::default();
            for y in 0..256 {
                for x in 0..256 {
                    let (gx, gy) = ((tx * 256 + x) as f32, (ty * 256 + y) as f32);
                    let band = if (gx as i32 / 7 + gy as i32 / 11) % 2 == 0 { 1.0 } else { 0.4 };
                    let a = 0.5 + 0.5 * (gx * 0.05).sin().abs();
                    t.set(x as usize, y as usize, [a * band * gx / dw as f32, a * gy / dh as f32, a * 0.5, a]);
                }
            }
            ((tx, ty), t)
        })
        .collect();
    let keys: Vec<(i32, i32)> = tiles.iter().map(|(k, _)| *k).collect();
    let mut sel = vec![0u8; (dw * dh) as usize];
    for y in 200..300 {
        for x in 230..540 {
            sel[y * dw as usize + x] = 255;
        }
    }
    let mut one = CanvasGpu::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    let mut split = CanvasGpu::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    split.set_max_sheet(256);
    for gpu in [&mut one, &mut split] {
        assert!(gpu.ensure_size(&device, dw, dh));
        gpu.upload(&queue, &tiles);
        gpu.mip_tiles(&device, &queue, &keys, 0);
        gpu.set_selection(&queue, Some(&sel));
    }
    assert_eq!(split.sheets().unwrap().grid, [3, 3]);
    // Magnified over the seam at (512, 256), at 1:1, and zoomed out at level 1.
    for (scale, offset, lod) in [(3.0, [-636.0, 21.0], 0), (1.0, [0.0, 0.0], 0), (0.37, [5.0, -3.0], 1)] {
        let params = ViewParams {
            origin: [0.0, 0.0],
            size: [320.0, 240.0],
            offset,
            scale,
            rotation: 0.2,
            checker: 8.0,
            stage: [0.0, 0.0, 1.0, 1.0],
            checker_a: [0.85, 0.85, 0.85, 1.0],
            checker_b: [0.64, 0.64, 0.64, 1.0],
            time: 0.0,
        };
        let [a, b] = [&mut one, &mut split].map(|gpu| {
            gpu.set_lod(lod);
            gpu.set_view(&queue, &params);
            render(gpu, &device, &queue, 320, 240)
        });
        let worst = a.iter().zip(&b).map(|(x, y)| x.abs_diff(*y)).max().unwrap();
        assert!(worst <= 2, "scale {scale}: off by {worst}");
    }
}
