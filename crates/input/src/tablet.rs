//! Pen tablets (AGENTS.md: per-OS backends behind one `PenEvent`).
//!
//! Wayland's tablet protocol (`zwp_tablet_v2`, via `octotablet`) gives
//! pressure, tilt, twist and the eraser end. Once bound, the compositor
//! sends the pen here instead of as pointer events, so the app turns these
//! events back into pointer input for the UI.

/// Which end of the pen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenTool {
    Pen,
    Eraser,
}

/// One pen sample. `pos` is in logical window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenSample {
    pub pos: [f32; 2],
    /// 0..1, `None` when the tool reports none.
    pub pressure: Option<f32>,
    /// The pen body projected on the surface: `[sin x, sin y]` of the tilt
    /// from perpendicular, each −1..1 (`+,+` = right and towards the user).
    pub tilt: [f32; 2],
    /// Rotation around the pen's axis, degrees 0..360.
    pub twist: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PenEvent {
    /// The tool came into range over the window.
    In(PenTool),
    /// Moved, touching or hovering.
    Move(PenSample),
    /// The nib touched down.
    Down(PenSample),
    Up,
    /// The tool left range or the window.
    Out,
}

/// The platform tablet backend, when there is one.
pub struct Tablet {
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    manager: octotablet::Manager,
    /// The last pose, for Down (which carries none).
    last: Option<PenSample>,
    down: bool,
}

impl Tablet {
    /// Connect to the window's tablet protocol. `None` where unsupported
    /// (X11, Windows, macOS: winit's own pen input is used there).
    ///
    /// # Safety
    /// `window` must keep its display and window handles valid for as long
    /// as the returned `Tablet` lives: drop it before the window closes.
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    #[allow(unsafe_code)]
    pub unsafe fn new(window: impl raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle) -> Option<Self> {
        // SAFETY: forwarded to this function's contract.
        let manager = unsafe { octotablet::Builder::new().build_raw(window) }.ok()?;
        matches!(manager.backed(), octotablet::Backend::WaylandTabletUnstableV2).then_some(Self { manager, last: None, down: false })
    }

    /// # Safety
    /// See the Linux version; there is no backend here.
    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    #[allow(unsafe_code)]
    pub unsafe fn new(_window: impl raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle) -> Option<Self> {
        None
    }

    /// Events since the last call, in order (every sample, not one per frame).
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    pub fn pump(&mut self) -> Vec<PenEvent> {
        use octotablet::events::{Event, ToolEvent};
        use octotablet::tool::Type;
        let mut out = Vec::new();
        let Ok(events) = self.manager.pump() else { return out };
        for e in events {
            let Event::Tool { tool, event } = e else { continue };
            match event {
                ToolEvent::In { .. } => {
                    let t = if tool.tool_type == Some(Type::Eraser) { PenTool::Eraser } else { PenTool::Pen };
                    out.push(PenEvent::In(t));
                }
                ToolEvent::Pose(p) => {
                    let s = sample_of(&p);
                    self.last = Some(s);
                    out.push(PenEvent::Move(s));
                }
                ToolEvent::Down => {
                    self.down = true;
                    if let Some(s) = self.last {
                        out.push(PenEvent::Down(s));
                    }
                }
                ToolEvent::Up => {
                    self.down = false;
                    out.push(PenEvent::Up);
                }
                ToolEvent::Out => {
                    if std::mem::take(&mut self.down) {
                        out.push(PenEvent::Up);
                    }
                    out.push(PenEvent::Out);
                }
                _ => {}
            }
        }
        out
    }

    #[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
    pub fn pump(&mut self) -> Vec<PenEvent> {
        Vec::new()
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn sample_of(p: &octotablet::axis::Pose) -> PenSample {
    let tilt = p.tilt.map_or([0.0; 2], |[x, y]| [x.sin(), y.sin()]);
    let twist = p.roll.get().map_or(0.0, |r| r.to_degrees().rem_euclid(360.0));
    PenSample { pos: p.position, pressure: p.pressure.get().map(|v| v.clamp(0.0, 1.0)), tilt, twist }
}
