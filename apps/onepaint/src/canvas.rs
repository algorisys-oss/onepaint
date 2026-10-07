//! The painting surface: GPU display through an `egui_wgpu` paint callback,
//! plus pointer input per tool (paint, fill, pick, select, pan, zoom, rotate).

use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor};
use eframe::wgpu;
use egui::{Color32, CursorIcon, Event, Key, PointerButton, Pos2, Rect, Sense, Stroke, TouchPhase, pos2};
use onepaint_engine::transform::{Handle, TransformParams};
use onepaint_engine::view::View;
use onepaint_engine::{Command, SelectMode, Session, Tool};
use onepaint_filters::gradient::{GradStop, Shape as GShape, fg_bg, fg_transparent, reverse_stops};
use onepaint_gpu::{CanvasGpu, ViewParams};
use onepaint_input::{PointerKind, PressureProvider};
use onepaint_raster::Tile;
use onepaint_selection::{Pt, constrain_45, constrain_square, ellipse_polygon, rect_polygon};
use onepaint_stroke::StrokePoint;
use onepaint_stroke::shape::{DragOpts, Shape, ShapeKind, drag_shape, recognize, sample_path};
use onepaint_ui_kit::Tokens;
use std::sync::Arc;

/// Register GPU resources once at startup.
pub fn init_gpu(cc: &eframe::CreationContext) -> bool {
    let Some(rs) = cc.wgpu_render_state.as_ref() else {
        return false;
    };
    let gpu = CanvasGpu::new(&rs.device, rs.target_format);
    rs.renderer.write().callback_resources.insert(gpu);
    log::info!("canvas GPU: {:?} ({:?})", rs.adapter.get_info().name, rs.adapter.get_info().backend);
    true
}

/// A selection upload: `Some(Some(bytes))` sets it, `Some(None)` clears it.
type SelectionUpload = Option<Option<Arc<Vec<u8>>>>;

struct CanvasCallback {
    doc_size: [u32; 2],
    uploads: Arc<Vec<((i32, i32), Tile)>>,
    selection: SelectionUpload,
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
            if let Some(sel) = &self.selection {
                gpu.set_selection(queue, sel.as_deref().map(|v| v.as_slice()));
            }
            gpu.set_view(queue, &self.params);
        }
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, res: &CallbackResources) {
        if let Some(gpu) = res.get::<CanvasGpu>() {
            gpu.draw(pass);
        }
    }
}

fn rgba(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0]
}

/// Sticky tool options (happypaint tool-store defaults).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolOptions {
    pub select_mode: SelectMode,
    pub wand_tolerance: f32,
    pub wand_contiguous: bool,
    pub wand_sample_all: bool,
    pub fill_tolerance: f32,
    pub fill_grow: u32,
    pub gradient_shape: GShape,
    /// Foreground → transparent (false) or foreground → background (true).
    pub gradient_to_bg: bool,
    pub gradient_reverse: bool,
    pub shape_kind: ShapeKind,
    pub shape_sides: u32,
    pub shape_fill: bool,
    pub background: onepaint_color::Rgba,
}

impl Default for ToolOptions {
    fn default() -> Self {
        Self {
            select_mode: SelectMode::Replace,
            wand_tolerance: 0.15,
            wand_contiguous: true,
            wand_sample_all: false,
            fill_tolerance: 0.15,
            fill_grow: 1,
            gradient_shape: GShape::Linear,
            gradient_to_bg: false,
            gradient_reverse: false,
            shape_kind: ShapeKind::Line,
            shape_sides: 6,
            shape_fill: false,
            background: onepaint_color::Rgba::WHITE,
        }
    }
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
    /// Rect or ellipse selection from `start` (doc px).
    Shape {
        start: Pt,
        end: Pt,
        mode: SelectMode,
    },
    Lasso {
        points: Vec<Pt>,
        mode: SelectMode,
    },
    Transform {
        handle: Handle,
        grab: Pt,
        start: TransformParams,
    },
    Gradient {
        start: Pt,
    },
    ShapeTool {
        start: Pt,
        shape: Option<Shape>,
    },
}

/// Raw points of the brush stroke in progress, for quick-shape.
#[derive(Default)]
struct HoldTracker {
    points: Vec<Pt>,
    anchor: Pos2,
    since: f64,
    fired: bool,
}

pub struct Canvas {
    pub view: View,
    pub doc_size: [u32; 2],
    pub options: ToolOptions,
    drag: Drag,
    /// Polygon-lasso vertices in progress (doc px) and their mode.
    poly: Option<(Vec<Pt>, SelectMode)>,
    pressure: Option<PressureProvider>,
    last_pen_force: Option<f32>,
    pub hover_doc: Option<[f64; 2]>,
    last_pos: Option<Pos2>,
    /// The size slider/keys changed the brush recently: show a preview ring.
    pub size_flash: f64,
    uploaded_selection: Option<(u64, [u32; 2])>,
    /// Parameters of the transform in progress (the engine holds the matrix).
    pub xform: TransformParams,
    hold: HoldTracker,
}

impl Canvas {
    pub fn new(doc: [u32; 2]) -> Self {
        Self {
            view: View::new([1200.0, 800.0], [doc[0] as f64, doc[1] as f64]),
            doc_size: doc,
            options: ToolOptions::default(),
            drag: Drag::None,
            poly: None,
            pressure: None,
            last_pen_force: None,
            hover_doc: None,
            last_pos: None,
            size_flash: 0.0,
            uploaded_selection: None,
            xform: TransformParams::default(),
            hold: HoldTracker::default(),
        }
    }

    pub fn reset_for(&mut self, doc: [u32; 2]) {
        self.doc_size = doc;
        self.view.doc_size = [doc[0] as f64, doc[1] as f64];
        self.view.fit();
        self.poly = None;
        self.uploaded_selection = None;
    }

    fn doc_pt(&self, p: Pos2) -> Pt {
        self.view.screen_to_doc([p.x as f64, p.y as f64])
    }

    fn stroke_point(&mut self, pos: Pos2, t: f64, force: Option<f32>) -> StrokePoint {
        let d = self.doc_pt(pos);
        let p = self.pressure.get_or_insert_with(|| PressureProvider::new(PointerKind::Mouse));
        let pressure = p.pressure(pos.x as f64, pos.y as f64, t, force.map(|f| f as f64));
        StrokePoint { x: d[0], y: d[1], pressure, t: Some(t), ..Default::default() }
    }

    /// Finish the polygon lasso (Enter, or a click near the first vertex).
    pub fn commit_poly(&mut self, session: &mut Session) {
        if let Some((pts, mode)) = self.poly.take()
            && pts.len() > 2
        {
            let _ = session.execute(Command::SelectShape { contours: vec![pts], mode });
        }
    }

    pub fn cancel_poly(&mut self) -> bool {
        self.poly.take().is_some()
    }

    #[allow(clippy::too_many_arguments)]
    fn press(
        &mut self,
        session: &mut Session,
        p: Pos2,
        button: PointerButton,
        m: egui::Modifiers,
        time_ms: f64,
        space: bool,
        r: bool,
    ) -> Drag {
        if button == PointerButton::Middle || space {
            return Drag::Pan;
        }
        if r {
            return Drag::Rotate { last_angle: self.angle_to(p) };
        }
        let tool = session.tool();
        let painting = matches!(tool, Tool::Brush | Tool::Eraser);
        if painting && (button == PointerButton::Secondary || (m.ctrl && m.alt)) {
            return Drag::Size { start_x: p.x, start_size: session.current_brush().size };
        }
        if button != PointerButton::Primary {
            return Drag::None;
        }
        let selecting = matches!(tool, Tool::SelectRect | Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly | Tool::SelectWand);
        if (m.alt && !selecting) || tool == Tool::Eyedropper {
            self.pick(session, p);
            return Drag::Pick;
        }
        let d = self.doc_pt(p);
        let mode = SelectMode::from_modifiers(m.shift, m.alt, self.options.select_mode);
        if tool == Tool::Move {
            let Some((_, b)) = session.transform_state() else {
                return Drag::None;
            };
            let s = self.view.scale;
            let handle = self.xform.hit(self.doc_f(), b, d, 12.0 / s, 28.0 / s);
            return Drag::Transform { handle, grab: d, start: self.xform };
        }
        match tool {
            Tool::Brush | Tool::Eraser => {
                let kind = if self.last_pen_force.is_some() { PointerKind::Pen } else { PointerKind::Mouse };
                self.pressure = Some(PressureProvider::new(kind));
                let sp = self.stroke_point(p, time_ms, self.last_pen_force);
                session.stroke_begin(sp);
                self.hold = HoldTracker { points: vec![d], anchor: p, since: time_ms, fired: false };
                Drag::Paint
            }
            Tool::Gradient => {
                session.gradient_begin();
                let stops = self.gradient_stops(session);
                session.gradient_update(d, d, self.options.gradient_shape, &stops);
                Drag::Gradient { start: d }
            }
            Tool::Shape => Drag::ShapeTool { start: d, shape: None },
            Tool::Fill => {
                let o = self.options;
                let _ = session.execute(Command::Fill {
                    x: d[0].floor() as i32,
                    y: d[1].floor() as i32,
                    tolerance: o.fill_tolerance,
                    grow: o.fill_grow,
                });
                Drag::None
            }
            Tool::SelectWand => {
                let o = self.options;
                let _ = session.execute(Command::SelectByColor {
                    x: d[0].floor() as i32,
                    y: d[1].floor() as i32,
                    tolerance: o.wand_tolerance,
                    mode,
                    contiguous: o.wand_contiguous,
                    sample_all: o.wand_sample_all,
                });
                Drag::None
            }
            Tool::SelectRect | Tool::SelectEllipse => Drag::Shape { start: d, end: d, mode },
            Tool::SelectLasso => Drag::Lasso { points: vec![d], mode },
            Tool::SelectPoly => {
                let close_dist = 12.0 / self.view.scale;
                match &mut self.poly {
                    None => self.poly = Some((vec![d], mode)),
                    Some((pts, _)) => {
                        let first = pts[0];
                        if pts.len() > 2 && (d[0] - first[0]).hypot(d[1] - first[1]) <= close_dist {
                            self.commit_poly(session);
                        } else {
                            let last = *pts.last().unwrap_or(&first);
                            pts.push(if m.shift { constrain_45(last, d) } else { d });
                        }
                    }
                }
                Drag::None
            }
            Tool::Eyedropper | Tool::Move => Drag::Pick,
        }
    }

    fn gradient_stops(&self, session: &Session) -> Vec<GradStop> {
        let c = session.color();
        let fg = [c.r as f64, c.g as f64, c.b as f64];
        let b = self.options.background;
        let stops = if self.options.gradient_to_bg { fg_bg(fg, [b.r as f64, b.g as f64, b.b as f64]) } else { fg_transparent(fg) };
        if self.options.gradient_reverse { reverse_stops(&stops) } else { stops }
    }

    /// Show `shape` as a live (uncommitted) brush stroke.
    fn preview_shape(session: &mut Session, shape: &Shape) {
        session.stroke_cancel();
        let path = sample_path(shape, 2.0);
        if path.len() < 2 {
            return;
        }
        let pts: Vec<StrokePoint> = path.iter().enumerate().map(|(i, p)| StrokePoint::at(p[0], p[1], 1.0, i as f64 * 4.0)).collect();
        session.stroke_begin(pts[0]);
        session.stroke_extend(&pts[1..]);
    }

    fn doc_f(&self) -> [f64; 2] {
        [self.doc_size[0] as f64, self.doc_size[1] as f64]
    }

    /// Push new transform parameters to the engine.
    pub fn set_xform(&mut self, session: &mut Session, mut p: TransformParams) {
        if p.sx == 1.0 && p.sy == 1.0 && p.rotation == 0.0 {
            // A pure translate snaps to whole pixels (PE:2203-2205).
            p.dx = p.dx.round();
            p.dy = p.dy.round();
        }
        self.xform = p;
        let _ = session.execute(Command::SetTransform { matrix: p.to_affine(self.doc_f()) });
    }

    /// Apply `f` to the parameters, keeping the transformed box centre in place.
    pub fn xform_about_centre(&mut self, session: &mut Session, f: impl FnOnce(TransformParams) -> TransformParams) {
        let Some((_, b)) = session.transform_state() else {
            return;
        };
        let doc = self.doc_f();
        let c0 = self.xform.handles(doc, b).centre;
        let next = f(self.xform);
        let c1 = next.handles(doc, b).centre;
        let moved = next.move_drag(c1, c0, false);
        self.set_xform(session, moved);
    }

    fn shape_contour(tool: Tool, start: Pt, end: Pt) -> Vec<Pt> {
        if tool == Tool::SelectEllipse { ellipse_polygon(start, end) } else { rect_polygon(start, end) }
    }

    /// Draw the canvas into `rect` and handle its input.
    #[allow(clippy::too_many_arguments)]
    pub fn show(&mut self, ui: &mut egui::Ui, rect: Rect, session: &mut Session, space_held: bool, r_held: bool, gpu: bool) {
        let t = Tokens::get(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        self.view.view_size = [rect.width() as f64, rect.height() as f64];
        let resp = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
        let time_ms = ui.input(|i| i.time) * 1000.0;
        let shift = ui.input(|i| i.modifiers.shift);

        // ── Input ──
        let events = ui.input(|i| i.events.clone());
        for ev in &events {
            if let Event::Touch { phase, force, .. } = ev {
                if force.is_some() {
                    self.last_pen_force = *force;
                }
                if *phase == TouchPhase::End || *phase == TouchPhase::Cancel {
                    self.last_pen_force = None;
                }
            }
        }
        let local = |p: Pos2| pos2(p.x - rect.left(), p.y - rect.top());
        // Raw events, so a stroke starts on press (no drag threshold) and uses every sample.
        for ev in &events {
            match ev {
                Event::PointerButton { pos, button, pressed: true, modifiers: m } if resp.hovered() && matches!(self.drag, Drag::None) => {
                    let p = local(*pos);
                    self.drag = self.press(session, p, *button, *m, time_ms, space_held, r_held);
                    self.last_pos = Some(p);
                }
                Event::PointerMoved(pos) => {
                    let p = local(*pos);
                    let d = self.doc_pt(p);
                    match &mut self.drag {
                        Drag::Paint => {
                            if self.hold.fired {
                                // A quick-shape replaced the stroke; further motion is ignored.
                            } else {
                                let sp = self.stroke_point(p, time_ms, self.last_pen_force);
                                session.stroke_extend(&[sp]);
                                self.hold.points.push(d);
                                if p.distance(self.hold.anchor) >= 3.0 {
                                    self.hold.anchor = p;
                                    self.hold.since = time_ms;
                                }
                            }
                        }
                        Drag::Gradient { start } => {
                            let start = *start;
                            let stops = self.gradient_stops(session);
                            session.gradient_update(start, d, self.options.gradient_shape, &stops);
                        }
                        Drag::ShapeTool { start, shape } => {
                            let alt = ui.input(|i| i.modifiers.alt);
                            let o = DragOpts { constrain: shift, from_center: alt, sides: self.options.shape_sides };
                            *shape = drag_shape(self.options.shape_kind, *start, d, o);
                            if let Some(sh) = shape.clone() {
                                Self::preview_shape(session, &sh);
                            } else {
                                session.stroke_cancel();
                            }
                        }
                        Drag::Pan => {
                            if let Some(lp) = self.last_pos {
                                self.view.pan((p.x - lp.x) as f64, (p.y - lp.y) as f64);
                            }
                        }
                        Drag::Rotate { last_angle } => {
                            let last = *last_angle;
                            let a = self.angle_to(p);
                            let c = [self.view.view_size[0] / 2.0, self.view.view_size[1] / 2.0];
                            self.view.rotate_at(c, a - last);
                            self.drag = Drag::Rotate { last_angle: a };
                        }
                        Drag::Size { start_x, start_size } => {
                            let size = (*start_size + (p.x - *start_x) as f64 / self.view.scale).clamp(1.0, 1000.0);
                            let _ = session.execute(Command::SetBrushSize { size });
                            self.size_flash = 1.0;
                        }
                        Drag::Pick => self.pick(session, p),
                        Drag::Shape { start, end, .. } => *end = if shift { constrain_square(*start, d) } else { d },
                        Drag::Lasso { points, .. } => points.push(d),
                        Drag::Transform { handle, grab, start } => {
                            let (handle, grab, start) = (*handle, *grab, *start);
                            if let Some((_, b)) = session.transform_state() {
                                let alt = ui.input(|i| i.modifiers.alt);
                                let doc = self.doc_f();
                                let next = match handle {
                                    Handle::Body => start.move_drag(grab, d, shift),
                                    Handle::Rotate => start.rotate_drag(doc, b, grab, d),
                                    h => start.scale_drag(doc, b, h, grab, d, shift, alt),
                                };
                                self.set_xform(session, next);
                            }
                        }
                        Drag::None => {}
                    }
                    self.last_pos = Some(p);
                }
                Event::PointerButton { pressed: false, .. } | Event::PointerGone => {
                    match std::mem::take(&mut self.drag) {
                        Drag::Paint => session.stroke_end(),
                        Drag::Gradient { .. } => session.gradient_end(),
                        Drag::ShapeTool { shape, .. } => {
                            session.stroke_cancel();
                            if let Some(sh) = shape {
                                session.stroke_shape(&sh, self.options.shape_fill);
                            }
                        }
                        Drag::Rotate { .. } => {
                            let c = [self.view.view_size[0] / 2.0, self.view.view_size[1] / 2.0];
                            let snapped = View::snap_rotation(self.view.rotation);
                            self.view.rotate_at(c, snapped - self.view.rotation);
                        }
                        Drag::Shape { start, end, mode } => {
                            let contour = Self::shape_contour(session.tool(), start, end);
                            let _ = session.execute(Command::SelectShape { contours: vec![contour], mode });
                        }
                        Drag::Lasso { points, mode } => {
                            let _ = session.execute(Command::SelectShape { contours: vec![points], mode });
                        }
                        _ => {}
                    }
                    self.pressure = None;
                    self.last_pos = None;
                }
                Event::Key { key: Key::Enter, pressed: true, .. } if self.poly.is_some() => self.commit_poly(session),
                _ => {}
            }
        }
        if session.tool() != Tool::SelectPoly {
            self.poly = None;
        }
        // Quick-shape: hold still for 600 ms while painting to straighten the stroke.
        if matches!(self.drag, Drag::Paint) && !self.hold.fired {
            if time_ms - self.hold.since >= 600.0 {
                self.hold.fired = true;
                if let Some(sh) = recognize(&self.hold.points, shift) {
                    Self::preview_shape(session, &sh);
                }
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));
            }
        }
        if resp.hovered() {
            let (scroll, zoom, rot) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.rotation_delta()));
            let at = ui.input(|i| i.pointer.hover_pos()).map(local).unwrap_or(pos2(rect.width() / 2.0, rect.height() / 2.0));
            if zoom != 1.0 {
                self.view.zoom_at([at.x as f64, at.y as f64], zoom as f64);
            } else if scroll != egui::Vec2::ZERO {
                self.view.pan(scroll.x as f64, scroll.y as f64);
            }
            if rot != 0.0 {
                self.view.rotate_at([at.x as f64, at.y as f64], rot as f64);
            }
        }
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
        self.hover_doc = hover.map(|p| self.doc_pt(local(p)));

        // ── Display ──
        let dirty = session.take_dirty_tiles();
        let uploads = if dirty.is_empty() { Vec::new() } else { session.composite(&dirty) };
        let key = (session.selection_version(), self.doc_size);
        let selection = if self.uploaded_selection != Some(key) {
            self.uploaded_selection = Some(key);
            Some(session.clip_mask().map(|m| Arc::new(m.data().to_vec())))
        } else {
            None
        };
        let now = ui.input(|i| i.time);
        let params = ViewParams {
            origin: [rect.left() * ppp, rect.top() * ppp],
            size: [rect.width() * ppp, rect.height() * ppp],
            offset: [self.view.offset[0] as f32 * ppp, self.view.offset[1] as f32 * ppp],
            scale: self.view.scale as f32 * ppp,
            rotation: self.view.rotation as f32,
            checker: (10.0 * ppp).max(2.0),
            stage: rgba(t.stage),
            checker_a: [0.85, 0.85, 0.85, 1.0],
            checker_b: [0.64, 0.64, 0.64, 1.0],
            time: (now % 1000.0) as f32,
        };
        if gpu {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                CanvasCallback { doc_size: self.doc_size, uploads: Arc::new(uploads), selection, params },
            ));
        }
        if session.selection().is_some() {
            // Marching ants animate; ~20 fps is plenty and keeps idle CPU low.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.draw_overlays(ui, rect, session, hover, space_held, r_held, &resp);
    }

    fn to_screen(&self, rect: Rect, d: Pt) -> Pos2 {
        let s = self.view.doc_to_screen(d);
        pos2(rect.left() + s[0] as f32, rect.top() + s[1] as f32)
    }

    fn outline(&self, ui: &egui::Ui, rect: Rect, pts: &[Pt], closed: bool) {
        let mut sp: Vec<Pos2> = pts.iter().map(|&d| self.to_screen(rect, d)).collect();
        if closed && let Some(&f) = sp.first() {
            sp.push(f);
        }
        ui.painter().add(egui::Shape::line(sp.clone(), Stroke::new(2.5, Color32::from_black_alpha(160))));
        ui.painter().add(egui::Shape::line(sp, Stroke::new(1.0, Color32::WHITE)));
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_overlays(
        &mut self,
        ui: &egui::Ui,
        rect: Rect,
        session: &Session,
        hover: Option<Pos2>,
        space: bool,
        r: bool,
        resp: &egui::Response,
    ) {
        if let Some((_, b)) = session.transform_state() {
            let h = self.xform.handles(self.doc_f(), b);
            self.outline(ui, rect, &h.corners, true);
            let knob = self.xform.rotate_knob(self.doc_f(), b, 28.0 / self.view.scale);
            let (top, k) = (self.to_screen(rect, h.edges[0]), self.to_screen(rect, knob));
            ui.painter().line_segment([top, k], Stroke::new(1.0, Color32::WHITE));
            ui.painter().circle(k, 5.0, Color32::WHITE, Stroke::new(1.0, Color32::from_black_alpha(160)));
            for c in h.corners.iter().chain(h.edges.iter()) {
                let s = self.to_screen(rect, *c);
                let r = Rect::from_center_size(s, egui::vec2(9.0, 9.0));
                ui.painter().rect(r, 2.0, Color32::WHITE, Stroke::new(1.0, Color32::from_black_alpha(160)), egui::StrokeKind::Middle);
            }
        }
        match &self.drag {
            Drag::Shape { start, end, .. } => self.outline(ui, rect, &Self::shape_contour(session.tool(), *start, *end), true),
            Drag::Lasso { points, .. } => self.outline(ui, rect, points, false),
            _ => {}
        }
        if let Some((pts, _)) = &self.poly {
            let mut p = pts.clone();
            if let Some(h) = hover {
                let d = self.doc_pt(pos2(h.x - rect.left(), h.y - rect.top()));
                let last = *p.last().unwrap_or(&d);
                p.push(if ui.input(|i| i.modifiers.shift) { constrain_45(last, d) } else { d });
            }
            self.outline(ui, rect, &p, false);
            let f = self.to_screen(rect, pts[0]);
            ui.painter().circle_stroke(f, 5.0, Stroke::new(1.5, Color32::WHITE));
        }
        let Some(hp) = hover else { return };
        if space {
            ui.ctx().set_cursor_icon(if matches!(self.drag, Drag::Pan) { CursorIcon::Grabbing } else { CursorIcon::Grab });
            return;
        }
        if r {
            ui.ctx().set_cursor_icon(CursorIcon::Alias);
            return;
        }
        let _ = resp;
        ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        if matches!(session.tool(), Tool::Brush | Tool::Eraser) {
            let rad = (session.current_brush().size / 2.0 * self.view.scale) as f32;
            if rad > 2.0 {
                ui.painter().circle_stroke(hp, rad, Stroke::new(1.5, Color32::from_black_alpha(140)));
                ui.painter().circle_stroke(hp, rad, Stroke::new(0.75, Color32::from_white_alpha(220)));
            }
        }
        if self.size_flash > 0.0 {
            let rad = (session.current_brush().size / 2.0 * self.view.scale) as f32;
            let a = (self.size_flash.min(1.0) * 200.0) as u8;
            ui.painter().circle_stroke(rect.center(), rad, Stroke::new(1.5, Color32::from_white_alpha(a)));
            self.size_flash -= ui.input(|i| i.stable_dt) as f64 * 1.5;
            ui.ctx().request_repaint();
        }
    }

    fn angle_to(&self, p: Pos2) -> f64 {
        let c = [self.view.view_size[0] / 2.0, self.view.view_size[1] / 2.0];
        (p.y as f64 - c[1]).atan2(p.x as f64 - c[0])
    }

    fn pick(&mut self, session: &mut Session, p: Pos2) {
        let d = self.doc_pt(p);
        if let Some(c) = session.sample_color(d[0].floor() as i32, d[1].floor() as i32) {
            let _ = session.execute(Command::SetColor { rgba: c });
        }
    }
}
