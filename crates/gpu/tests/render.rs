#![allow(clippy::unwrap_used)]
//! Headless render of the present pass; skipped when no GPU adapter exists.
use onepaint_gpu::{CanvasGpu, ViewParams};
use onepaint_raster::Tile;

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
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
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
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
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
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
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
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
    assert_eq!(px(20, 32), [255, 0, 0, 255], "red document pixel");
    let chk = px(40, 32);
    assert!(chk[0] == chk[1] && chk[0] > 150, "checker {chk:?}");
    assert_eq!(px(2, 2), [0, 0, 255, 255], "stage");
}
