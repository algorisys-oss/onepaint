//! Pen tablets in the app (spec 07): the Wayland tablet backend's samples
//! become the pointer and touch-with-force events egui-winit makes for pens
//! elsewhere, so the UI and canvas need no second path. Each sample's
//! pressure, tilt and twist rides along for the canvas; the eraser end
//! switches to the eraser while it is in range.

#![allow(unsafe_code)]

use crate::App;
use egui::{Event, PointerButton, TouchDeviceId, TouchId, TouchPhase, pos2};
use onepaint_engine::Tool;
use onepaint_input::tablet::{PenEvent, PenTool, Tablet};

/// The tablet for the main window, if the platform has a backend.
pub fn connect(cc: &eframe::CreationContext) -> Option<Tablet> {
    // SAFETY: the main window outlives `App` (eframe creates the app after
    // the window and drops it first), and `App::on_exit` drops the tablet
    // before the window closes.
    let t = unsafe { Tablet::new(cc) };
    if t.is_some() {
        log::info!("pen tablet: Wayland tablet protocol");
    }
    t
}

const PEN: TouchDeviceId = TouchDeviceId(0x7ab1e7);
const NIB: TouchId = TouchId(0x7ab1e7);

/// Turn this frame's pen events into egui input.
pub fn feed(app: &mut App, ctx: &egui::Context, raw: &mut egui::RawInput) {
    let Some(t) = app.tablet.as_mut() else { return };
    let events = t.pump();
    if events.is_empty() {
        return;
    }
    let (zoom, mods) = (ctx.zoom_factor(), ctx.input(|i| i.modifiers));
    raw.events.extend(translate(app, &events, zoom, mods));
}

/// The egui events for pen events (and the canvas's per-sample queue).
pub fn translate(app: &mut App, events: &[PenEvent], zoom: f32, mods: egui::Modifiers) -> Vec<Event> {
    // Tablet positions are logical window px; egui points divide by its zoom.
    let at = |p: [f32; 2]| pos2(p[0] / zoom, p[1] / zoom);
    let mut out = Vec::new();
    for e in events {
        match *e {
            PenEvent::In(PenTool::Eraser) => {
                let tool = app.session.tool();
                if tool != Tool::Eraser {
                    app.eraser_from = Some(tool);
                    app.set_tool(Tool::Eraser);
                }
            }
            PenEvent::In(PenTool::Pen) => {}
            PenEvent::Move(s) => {
                let pos = at(s.pos);
                app.canvas.pen_queue.push_back((pos, s));
                if app.canvas.pen_down {
                    out.push(Event::Touch { device_id: PEN, id: NIB, phase: TouchPhase::Move, pos, force: s.pressure });
                }
                out.push(Event::PointerMoved(pos));
            }
            PenEvent::Down(s) => {
                let pos = at(s.pos);
                app.canvas.pen_down = true;
                app.canvas.pen_queue.push_back((pos, s));
                out.push(Event::PointerMoved(pos));
                out.push(Event::Touch { device_id: PEN, id: NIB, phase: TouchPhase::Start, pos, force: s.pressure.or(Some(0.0)) });
                out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: mods });
            }
            PenEvent::Up => {
                app.canvas.pen_down = false;
                let pos = app.canvas.pen_queue.back().map(|(p, _)| *p).unwrap_or_default();
                out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: mods });
                out.push(Event::Touch { device_id: PEN, id: NIB, phase: TouchPhase::End, pos, force: None });
            }
            PenEvent::Out => {
                out.push(Event::PointerGone);
                if let Some(tool) = app.eraser_from.take() {
                    app.set_tool(tool);
                }
            }
        }
    }
    out
}
