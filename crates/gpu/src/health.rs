//! GPU health: whether the device was lost (a driver reset, a removed GPU,
//! a timeout). eframe owns the device and can't make a new one, so the app
//! saves the work and asks for a restart (todo §2.1).

use std::sync::{Arc, Mutex};

/// Watches one device for loss.
#[derive(Clone, Default)]
pub struct Health {
    lost: Arc<Mutex<Option<String>>>,
}

impl Health {
    /// Start watching `device`.
    pub fn watch(device: &wgpu::Device) -> Self {
        let h = Self::default();
        let lost = h.lost.clone();
        device.set_device_lost_callback(move |reason, msg| {
            log::error!("GPU device lost ({reason:?}): {msg}");
            *lost.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("{reason:?}: {msg}"));
        });
        h
    }

    /// Why the device was lost, once it has been.
    pub fn lost(&self) -> Option<String> {
        self.lost.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Record a loss by hand (tests of what the app then does).
    #[doc(hidden)]
    pub fn mark_lost(&self, why: &str) {
        *self.lost.lock().unwrap_or_else(|e| e.into_inner()) = Some(why.to_string());
    }
}
