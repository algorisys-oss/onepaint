//! The painting surface: GPU display through an `egui_wgpu` paint callback,
//! plus pointer input (painting, pan, zoom, rotate).

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;
use egui::{Color32, CursorIcon, Event, PointerButton, Rect, Sense, Stroke, TouchPhase, pos2};
use onepaint_engine::view::View;
use onepaint_engine::{Command, Session};
use onepaint_gpu::{CanvasGpu, ViewParams};
use onepaint_input::{PointerKind, PressureProvider};
use onepaint_raster::Tile;
use onepaint_stroke::StrokePoint;
use onepaint_ui_kit::Tokens;
use std::sync::Arc;

/// Register GPU resources once at startup.
pub fn init_gpu(cc: &eframe::CreationContext) -> bool {
    let Some(rs) = cc.wgpu_render_state.as_ref() else {
        return false;
    };
    let gpu = CanvasGpu::new(&rs.device, rs.target_format);
    rs.renderer.write().callback_resources.insert(gpu);
    log::info!(
        "canvas GPU: {:?} ({:?})",
        rs.adapter.get_info().name,
        rs.adapter.get_info().backend
    );
    true
}

struct CanvasCallback {
    doc_size: [u32; 2],
    uploads: Arc<Vec<((i32, i32), Tile)>>,
    params: ViewParams,
}

impl CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        res: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(gpu) = res.get_mut::<CanvasGpu>() {
            gpu.ensure_size(device, self.doc_size[0], self.doc_size[1]);
            gpu.upload(queue, &self.uploads);
            gpu.set_view(queue, &self.params);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        res: &CallbackResources,
    ) {
        if let Some(gpu) = res.get::<CanvasGpu>() {
            gpu.draw(pass);
        }
    }
}

fn rgba(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    [
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ]
}

#[derive(Default)]
enum Drag {
    #[default]
    None,
    Paint,
    Pan,
    Rotate {
        last_angle: f64,
    },
    Size {
        start_x: f32,
        start_size: f64,
    },
    Pick,
}

pub struct Canvas {
    pub view: View,
    pub doc_size: [u32; 2],
    drag: Drag,
    pressure: Option<PressureProvider>,
    last_pen_force: Option<f32>,
    pub hover_doc: Option<[f64; 2]>,
    last_pos: Option<egui::Pos2>,
    /// The size slider/keys changed the brush recently: show a preview ring.
    pub size_flash: f64,
}

impl Canvas {
    pub fn new(doc: [u32; 2]) -> Self {
        Self {
            view: View::new([1200.0, 800.0], [doc[0] as f64, doc[1] as f64]),
            doc_size: doc,
            drag: Drag::None,
            pressure: None,
            last_pen_force: None,
            hover_doc: None,
            last_pos: None,
            size_flash: 0.0,
        }
    }

    pub fn reset_for(&mut self, doc: [u32; 2]) {
        self.doc_size = doc;
        self.view.doc_size = [doc[0] as f64, doc[1] as f64];
        self.view.fit();
    }

    fn stroke_point(&mut self, pos: egui::Pos2, t: f64, force: Option<f32>) -> StrokePoint {
        let d = self.view.screen_to_doc([pos.x as f64, pos.y as f64]);
        let p = self
            .pressure
            .get_or_insert_with(|| PressureProvider::new(PointerKind::Mouse));
        let pressure = p.pressure(pos.x as f64, pos.y as f64, t, force.map(|f| f as f64));
        StrokePoint {
            x: d[0],
            y: d[1],
            pressure,
            t: Some(t),
            ..Default::default()
        }
    }

    /// Draw the canvas into `rect` and handle its input.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        session: &mut Session,
        space_held: bool,
        r_held: bool,
        gpu: bool,
    ) {
        let t = Tokens::get(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        self.view.view_size = [rect.width() as f64, rect.height() as f64];
        let resp = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
        let time_ms = ui.input(|i| i.time) * 1000.0;

        // ── Input ──
        let events = ui.input(|i| i.events.clone());
        for ev in &events {
            if let Event::Touch {
                phase, pos, force, ..
            } = ev
            {
                if force.is_some() {
                    self.last_pen_force = *force;
                }
                if *phase == TouchPhase::End || *phase == TouchPhase::Cancel {
                    self.last_pen_force = None;
                }
                let _ = pos;
            }
        }
        let local = |p: egui::Pos2| pos2(p.x - rect.left(), p.y - rect.top());
        // Raw events, so a stroke starts on press (no drag threshold) and uses every sample.
        for ev in &events {
            match ev {
                Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers: m,
                } if resp.hovered() && matches!(self.drag, Drag::None) => {
                    let p = local(*pos);
                    self.drag = if *button == PointerButton::Middle || space_held {
                        Drag::Pan
                    } else if r_held {
                        Drag::Rotate {
                            last_angle: self.angle_to(p),
                        }
                    } else if *button == PointerButton::Secondary || (m.ctrl && m.alt) {
                        Drag::Size {
                            start_x: p.x,
                            start_size: session.current_brush().size,
                        }
                    } else if m.alt {
                        self.pick(session, p);
                        Drag::Pick
                    } else if *button == PointerButton::Primary {
                        let kind = if self.last_pen_force.is_some() {
                            PointerKind::Pen
                        } else {
                            PointerKind::Mouse
                        };
                        self.pressure = Some(PressureProvider::new(kind));
                        let sp = self.stroke_point(p, time_ms, self.last_pen_force);
                        session.stroke_begin(sp);
                        Drag::Paint
                    } else {
                        Drag::None
                    };
                    self.last_pos = Some(p);
                }
                Event::PointerMoved(pos) => {
                    let p = local(*pos);
                    match self.drag {
                        Drag::Paint => {
                            let sp = self.stroke_point(p, time_ms, self.last_pen_force);
                            session.stroke_extend(&[sp]);
                        }
                        Drag::Pan => {
                            if let Some(lp) = self.last_pos {
                                self.view.pan((p.x - lp.x) as f64, (p.y - lp.y) as f64);
                            }
                        }
                        Drag::Rotate { last_angle } => {
                            let a = self.angle_to(p);
                            let c = [self.view.view_size[0] / 2.0, self.view.view_size[1] / 2.0];
                            self.view.rotate_at(c, a - last_angle);
                            self.drag = Drag::Rotate { last_angle: a };
                        }
                        Drag::Size {
                            start_x,
                            start_size,
                        } => {
                            let size = (start_size + (p.x - start_x) as f64 / self.view.scale)
                                .clamp(1.0, 1000.0);
                            let _ = session.execute(Command::SetBrushSize { size });
                            self.size_flash = 1.0;
                        }
                        Drag::Pick => self.pick(session, p),
                        Drag::None => {}
                    }
                    self.last_pos = Some(p);
                }
                Event::PointerButton { pressed: false, .. } | Event::PointerGone => {
                    match self.drag {
                        Drag::Paint => session.stroke_end(),
                        Drag::Rotate { .. } => {
                            let c = [self.view.view_size[0] / 2.0, self.view.view_size[1] / 2.0];
                            let snapped = View::snap_rotation(self.view.rotation);
                            self.view.rotate_at(c, snapped - self.view.rotation);
                        }
                        _ => {}
                    }
                    self.drag = Drag::None;
                    self.pressure = None;
                    self.last_pos = None;
                }
                _ => {}
            }
        }
        if resp.hovered() {
            let (scroll, zoom, rot) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.rotation_delta()));
            let at = ui
                .input(|i| i.pointer.hover_pos())
                .map(local)
                .unwrap_or(pos2(rect.width() / 2.0, rect.height() / 2.0));
            if zoom != 1.0 {
                self.view.zoom_at([at.x as f64, at.y as f64], zoom as f64);
            } else if scroll != egui::Vec2::ZERO {
                self.view.pan(scroll.x as f64, scroll.y as f64);
            }
            if rot != 0.0 {
                self.view.rotate_at([at.x as f64, at.y as f64], rot as f64);
            }
        }
        self.hover_doc = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            .map(|p| {
                let l = local(p);
                self.view.screen_to_doc([l.x as f64, l.y as f64])
            });

        // ── Display ──
        let dirty = session.take_dirty_tiles();
        let uploads = if dirty.is_empty() {
            Vec::new()
        } else {
            session.composite(&dirty)
        };
        let params = ViewParams {
            origin: [rect.left() * ppp, rect.top() * ppp],
            size: [rect.width() * ppp, rect.height() * ppp],
            offset: [
                self.view.offset[0] as f32 * ppp,
                self.view.offset[1] as f32 * ppp,
            ],
            scale: self.view.scale as f32 * ppp,
            rotation: self.view.rotation as f32,
            checker: (10.0 * ppp).max(2.0),
            stage: rgba(t.stage),
            checker_a: [0.85, 0.85, 0.85, 1.0],
            checker_b: [0.64, 0.64, 0.64, 1.0],
        };
        if gpu {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                CanvasCallback {
                    doc_size: self.doc_size,
                    uploads: Arc::new(uploads),
                    params,
                },
            ));
        }

        // Brush cursor ring.
        if let Some(hp) = ui
            .input(|i| i.pointer.hover_pos())
            .filter(|p| rect.contains(*p))
            && !space_held
            && !r_held
        {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
            let r = (session.current_brush().size / 2.0 * self.view.scale) as f32;
            if r > 2.0 {
                ui.painter()
                    .circle_stroke(hp, r, Stroke::new(1.5, Color32::from_black_alpha(140)));
                ui.painter().circle_stroke(
                    hp,
                    r,
                    Stroke::new(0.75, Color32::from_white_alpha(220)),
                );
            }
        } else if space_held && resp.hovered() {
            ui.ctx().set_cursor_icon(if matches!(self.drag, Drag::Pan) {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            });
        }
        if self.size_flash > 0.0 {
            let r = (session.current_brush().size / 2.0 * self.view.scale) as f32;
            let a = (self.size_flash.min(1.0) * 200.0) as u8;
            ui.painter().circle_stroke(
                rect.center(),
                r,
                Stroke::new(1.5, Color32::from_white_alpha(a)),
            );
            self.size_flash -= ui.input(|i| i.stable_dt) as f64 * 1.5;
            ui.ctx().request_repaint();
        }
    }

    fn angle_to(&self, p: egui::Pos2) -> f64 {
        let c = [self.view.view_size[0] / 2.0, self.view.view_size[1] / 2.0];
        (p.y as f64 - c[1]).atan2(p.x as f64 - c[0])
    }

    fn pick(&mut self, session: &mut Session, p: egui::Pos2) {
        let d = self.view.screen_to_doc([p.x as f64, p.y as f64]);
        let (x, y) = (d[0].floor() as i32, d[1].floor() as i32);
        if x >= 0 && y >= 0 && (x as u32) < self.doc_size[0] && (y as u32) < self.doc_size[1] {
            let c = session.pick_color(x, y);
            let _ = session.execute(Command::SetColor { rgba: c });
        }
    }
}
