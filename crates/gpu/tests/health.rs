#![allow(clippy::unwrap_used)]
//! GPU health (todo §2.1): a lost device is noticed; skipped without an adapter.
use onepaint_gpu::health::Health;

#[test]
fn a_destroyed_device_reads_as_lost() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())) else {
        eprintln!("skipping: no GPU adapter");
        return;
    };
    let (device, _queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let h = Health::watch(&device);
    assert!(h.lost().is_none());
    device.destroy();
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    assert!(h.lost().is_some_and(|w| w.contains("Destroyed")), "{:?}", h.lost());
}
