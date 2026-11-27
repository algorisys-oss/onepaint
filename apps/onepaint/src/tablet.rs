//! Pen tablets in the app (spec 07): the Wayland tablet backend's samples
//! become the pointer and touch-with-force events egui-winit makes for pens
//! elsewhere, so the UI and canvas need no second path. Each sample's
//! pressure, tilt and twist rides along for the canvas; the eraser end
//! switches to the eraser while it is in range. A lift mid-stroke is held
//! back across tip dips (`onepaint_input::dip`).

#![allow(unsafe_code)]

use crate::App;
use egui::{Event, PointerButton, TouchDeviceId, TouchId, TouchPhase, pos2};
use onepaint_engine::Tool;
use onepaint_input::dip::{Contact, Hover};
use onepaint_input::tablet::{PenEvent, PenSample, PenTool, Tablet};

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
    let (zoom, mods, ms) = (ctx.zoom_factor(), ctx.input(|i| i.modifiers), ctx.input(|i| i.time) * 1000.0);
    if !events.is_empty() {
        raw.events.extend(translate(app, &events, zoom, mods, ms));
    }
    raw.events.extend(tick(app, mods, ms));
    if app.canvas.dip.pending() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}

/// The deferred end of a stroke whose dip window ran out (a real lift).
pub fn tick(app: &mut App, mods: egui::Modifiers, ms: f64) -> Vec<Event> {
    app.canvas.dip.tick(ms).map(|lift| lifted(pos2(lift[0], lift[1]), mods)).unwrap_or_default()
}

/// The release a pen lift makes.
fn lifted(pos: egui::Pos2, modifiers: egui::Modifiers) -> Vec<Event> {
    vec![
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers },
        Event::Touch { device_id: PEN, id: NIB, phase: TouchPhase::End, pos, force: None },
    ]
}

/// A contact move of the stroke (and its sample for the canvas).
fn contact_move(app: &mut App, out: &mut Vec<Event>, pos: egui::Pos2, s: PenSample) {
    app.canvas.pen_at = pos;
    app.canvas.pen_queue.push_back((pos, s));
    out.push(Event::Touch { device_id: PEN, id: NIB, phase: TouchPhase::Move, pos, force: s.pressure });
    out.push(Event::PointerMoved(pos));
}

/// The egui events for pen events (and the canvas's per-sample queue). `ms`
/// is the time they arrived, for the dip window.
pub fn translate(app: &mut App, events: &[PenEvent], zoom: f32, mods: egui::Modifiers, ms: f64) -> Vec<Event> {
    // Tablet positions are logical window px; egui points divide by its zoom.
    let at = |p: [f32; 2]| pos2(p[0] / zoom, p[1] / zoom);
    let mut out = Vec::new();
    for e in events {
        // A stroke's end held back across a dip: hovers are buffered, a
        // re-contact near them continues the stroke through their path.
        if app.canvas.dip.pending() {
            match *e {
                PenEvent::Move(s) if !app.canvas.pen_down => match app.canvas.dip.hover([at(s.pos).x, at(s.pos).y], s, ms) {
                    Hover::Held => continue,
                    Hover::Commit { lift } => out.extend(lifted(pos2(lift[0], lift[1]), mods)),
                },
                PenEvent::Down(s) => {
                    let pos = at(s.pos);
                    match app.canvas.dip.contact([pos.x, pos.y]) {
                        Contact::Resume(path) => {
                            app.canvas.pen_down = true;
                            // The dip's hovers carry no pressure: they take the re-contact's.
                            for (p, h) in path {
                                contact_move(app, &mut out, pos2(p[0], p[1]), PenSample { pressure: s.pressure, ..h });
                            }
                            contact_move(app, &mut out, pos, s);
                            continue;
                        }
                        Contact::Commit { lift } => out.extend(lifted(pos2(lift[0], lift[1]), mods)),
                    }
                }
                _ => {
                    if let Some(lift) = app.canvas.dip.flush() {
                        out.extend(lifted(pos2(lift[0], lift[1]), mods));
                    }
                }
            }
        }
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
                app.canvas.pen_at = pos;
                app.canvas.pen_queue.push_back((pos, s));
                if app.canvas.pen_down {
                    out.push(Event::Touch { device_id: PEN, id: NIB, phase: TouchPhase::Move, pos, force: s.pressure });
                }
                out.push(Event::PointerMoved(pos));
            }
            PenEvent::Down(s) => {
                let pos = at(s.pos);
                app.canvas.pen_at = pos;
                app.canvas.pen_down = true;
                app.canvas.pen_queue.push_back((pos, s));
                out.push(Event::PointerMoved(pos));
                out.push(Event::Touch { device_id: PEN, id: NIB, phase: TouchPhase::Start, pos, force: s.pressure.or(Some(0.0)) });
                out.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: mods });
            }
            PenEvent::Up => {
                app.canvas.pen_down = false;
                let pos = app.canvas.pen_at;
                // Mid-stroke, with dip recovery on, hold the end back: it may
                // be a tip dip. Off (the default since happypaint v0.211.7),
                // the lift is final: no rule tells a dip from a short, slow
                // hop to the next mark, so close hatching would fuse.
                if app.canvas.painting() && app.settings.pen_dip_recovery {
                    app.canvas.dip.lift([pos.x, pos.y], ms);
                } else {
                    out.extend(lifted(pos, mods));
                }
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
