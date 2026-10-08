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
    /// Pen: closed paths get a fill in the current colour.
    pub pen_fill: bool,
    /// Gradient: add a live gradient-fill layer instead of painting pixels.
    pub gradient_layer: bool,
    pub stroke_path_tool: onepaint_engine::path_ops::StrokePathTool,
    pub stroke_path_pressure: bool,
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
            pen_fill: false,
            gradient_layer: false,
            stroke_path_tool: Default::default(),
            stroke_path_pressure: true,
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
    TextMove {
        grab: Pt,
        start: Box<onepaint_text::TextSpec>,
    },
    /// Hard perspective: a straight line from `origin`, re-aimed every move.
    PerspLine {
        origin: Pt,
    },
    /// Dragging a vanishing point (perspective edit mode).
    Vp {
        index: usize,
    },
    /// Moving the symmetry centre (symmetry edit mode).
    SymAxis,
    /// Moving guide `index`; `from_ruler` drags may drop back on a bar to delete.
    GuideMove {
        index: usize,
        offset: Pt,
        from_ruler: bool,
    },
    /// Alt-drag in edit-guides mode: swing guide `index` about its anchor.
    GuideRotate {
        index: usize,
    },
    /// A drag on empty canvas in edit-guides mode draws a guide along it.
    GuideDraw {
        start: Pt,
        index: Option<usize>,
    },
    /// Measure from `start` (doc px).
    Measure {
        start: Pt,
    },
    /// Dragging a distort corner or warp lattice point (`None` = the whole set).
    WarpPoint {
        index: Option<usize>,
        grab: Pt,
        start: onepaint_engine::transform::WarpMode,
    },
    /// Pen: pulling the handle of the anchor just placed.
    PenNew {
        index: usize,
    },
    /// Pen: dragging an anchor or handle of the active path layer.
    PenEdit {
        obj: usize,
        sub: usize,
        anchor: usize,
        part: crate::pen::Part,
    },
}

/// The text object being created or edited with the text tool.
#[derive(Clone, Debug)]
pub struct TextEdit {
    /// `(layer id, index)` once the object exists in the document.
    pub target: Option<(String, usize)>,
    pub spec: onepaint_text::TextSpec,
}

/// Raw points of the brush stroke in progress, for quick-shape.
#[derive(Default)]
struct HoldTracker {
    points: Vec<Pt>,
    anchor: Pos2,
    since: f64,
    fired: bool,
    /// Quick-shape applies to brush and eraser strokes only.
    enabled: bool,
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
    /// Composite tiles still waiting for a frame's budget.
    pending: std::collections::BTreeSet<(i32, i32)>,
    pub text_edit: Option<TextEdit>,
    /// Style for new text objects (happypaint's text-store style).
    pub text_style: onepaint_text::TextSpec,
    /// Freehand stroke snapping (H/V, grid, guides, soft perspective).
    snapper: Option<onepaint_stroke::snap::Snapper>,
    /// Transient guide edit modes: canvas drags move handles instead of painting.
    pub persp_editing: bool,
    pub sym_editing: bool,
    /// Edit guides: drags move, draw and swing guide lines.
    pub lines_editing: bool,
    /// Measure: drags report length and angle.
    pub measuring: bool,
    /// The last measurement (doc px), shown until the next one or Escape.
    pub measure: Option<(Pt, Pt)>,
    pub pen: crate::pen::PenState,
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
            pending: Default::default(),
            text_edit: None,
            text_style: onepaint_text::TextSpec::default(),
            snapper: None,
            persp_editing: false,
            sym_editing: false,
            lines_editing: false,
            measuring: false,
            measure: None,
            pen: Default::default(),
        }
    }

    pub fn reset_for(&mut self, doc: [u32; 2]) {
        self.doc_size = doc;
        self.view.doc_size = [doc[0] as f64, doc[1] as f64];
        self.view.fit();
        self.poly = None;
        self.uploaded_selection = None;
        self.pending.clear();
        self.persp_editing = false;
        self.sym_editing = false;
        self.lines_editing = false;
        self.measuring = false;
        self.measure = None;
    }

    /// Pen press: close or extend the path being built, edit the active path
    /// layer, or start a new path.
    fn pen_press(&mut self, session: &mut Session, p: Pos2, d: Pt, m: egui::Modifiers, time_ms: f64) -> Drag {
        use onepaint_stroke::bezier::{PathAnchor, closest_segment, insert_anchor_at, toggle_anchor_kind};
        let tol = 10.0 / self.view.scale;
        let double = self.pen.last_press.is_some_and(|(t, q)| time_ms - t < 350.0 && (q[0] - d[0]).hypot(q[1] - d[1]) <= tol);
        self.pen.last_press = Some((time_ms, d));
        let _ = p;
        if let Some(b) = self.pen.building.as_mut() {
            let first = b[0];
            if b.len() >= 2 && (first.x - d[0]).hypot(first.y - d[1]) <= tol {
                crate::pen::finish(&mut self.pen, session, true, self.options.pen_fill, (time_ms as u32).wrapping_mul(2_654_435_761));
                return Drag::None;
            }
            if double && b.len() >= 2 {
                crate::pen::finish(&mut self.pen, session, false, self.options.pen_fill, (time_ms as u32).wrapping_mul(2_654_435_761));
                return Drag::None;
            }
            b.push(PathAnchor::corner(d[0], d[1]));
            return Drag::PenNew { index: b.len() - 1 };
        }
        let active = session.active().to_string();
        if session.is_path_layer(&active) {
            let specs = session.paths(&active);
            if let Some((obj, sub, anchor, part)) = crate::pen::hit(&specs, d, tol) {
                self.pen.last = Some((obj, sub, anchor));
                if m.alt && part == crate::pen::Part::Anchor {
                    crate::pen::edit_object(session, obj, |s| toggle_anchor_kind(&mut s.subpaths[sub].anchors, anchor));
                    let _ = session.execute(Command::SealHistory);
                    return Drag::None;
                }
                return Drag::PenEdit { obj, sub, anchor, part };
            }
            if m.command {
                for (obj, spec) in specs.iter().enumerate() {
                    for (sub, s) in spec.subpaths.iter().enumerate() {
                        if let Some((seg, t, _)) = closest_segment(s, d, tol) {
                            crate::pen::edit_object(session, obj, |sp| insert_anchor_at(&mut sp.subpaths[sub], seg, t));
                            let _ = session.execute(Command::SealHistory);
                            return Drag::None;
                        }
                    }
                }
            }
        }
        self.pen.building = Some(vec![PathAnchor::corner(d[0], d[1])]);
        Drag::PenNew { index: 0 }
    }

    /// Path outlines, anchors and handles (active path layer and the path being built).
    fn pen_overlay(&self, ui: &egui::Ui, rect: Rect, session: &Session, hover: Option<Pos2>) {
        use onepaint_stroke::bezier::{PathSpec, Subpath, flatten_subpath};
        if session.tool() != Tool::Pen {
            return;
        }
        let accent = onepaint_ui_kit::Tokens::get(ui.ctx()).accent;
        let painter = ui.painter();
        let draw = |sub: &Subpath| {
            let pts: Vec<Pos2> = flatten_subpath(sub, 2.0).into_iter().map(|q| self.to_screen(rect, q)).collect();
            painter.add(egui::Shape::line(pts.clone(), Stroke::new(2.5, Color32::from_black_alpha(120))));
            painter.add(egui::Shape::line(pts, Stroke::new(1.0, accent)));
            for a in &sub.anchors {
                let c = self.to_screen(rect, [a.x, a.y]);
                for (has, h) in [(a.has_in(), a.in_handle()), (a.has_out(), a.out_handle())] {
                    if has {
                        let e = self.to_screen(rect, [a.x + h[0], a.y + h[1]]);
                        painter.line_segment([c, e], Stroke::new(1.0, accent.gamma_multiply(0.8)));
                        painter.circle(e, 3.5, Color32::WHITE, Stroke::new(1.0, accent));
                    }
                }
                let r = Rect::from_center_size(c, egui::vec2(7.0, 7.0));
                let fill = if a.kind == onepaint_stroke::bezier::AnchorKind::Smooth { accent } else { Color32::WHITE };
                painter.rect(r, 1.0, fill, Stroke::new(1.0, accent), egui::StrokeKind::Middle);
            }
        };
        let specs: Vec<PathSpec> = session.paths(session.active());
        for spec in &specs {
            spec.subpaths.iter().for_each(&draw);
        }
        if let Some(b) = &self.pen.building {
            draw(&Subpath { anchors: b.clone(), closed: false });
            if let (Some(last), Some(h)) = (b.last(), hover) {
                let to = pos2(h.x, h.y);
                painter.line_segment([self.to_screen(rect, [last.x, last.y]), to], Stroke::new(1.0, accent.gamma_multiply(0.6)));
            }
        }
    }

    /// Which pinned ruler bar a canvas-local point is on (rulers on, unrotated).
    fn ruler_band(&self, session: &Session, p: Pos2) -> Option<bool> {
        use onepaint_stroke::guide_lines::{RULER_PX, rulers_pinned};
        if !session.guides().rulers || !session.guides().visible || !rulers_pinned(self.view.rotation) {
            return None;
        }
        let (r, w, h) = (RULER_PX as f32, self.view.view_size[0] as f32, self.view.view_size[1] as f32);
        if p.y >= 0.0 && p.y <= r && p.x > r && p.x <= w {
            Some(true)
        } else if p.x >= 0.0 && p.x <= r && p.y > r && p.y <= h {
            Some(false)
        } else {
            None
        }
    }

    fn edit_lines(session: &mut Session, lines: Vec<onepaint_engine::guides::GuideLine>) {
        let _ = session.execute(Command::EditGuideLines { lines });
    }

    /// The vanishing point under screen position `p` (16 px hit radius).
    fn vp_hit(&self, session: &Session, p: Pos2) -> Option<usize> {
        let (w, h) = (self.doc_size[0] as f64, self.doc_size[1] as f64);
        session.guides().perspective.points.iter().position(|v| {
            let s = self.view.doc_to_screen([v.x * w, v.y * h]);
            pos2(s[0] as f32, s[1] as f32).distance(p) <= 16.0
        })
    }

    fn snap_config(&self, session: &Session) -> onepaint_stroke::snap::SnapConfig {
        session.guides().snap_config(self.doc_size[0], self.doc_size[1], self.view.scale, self.persp_editing)
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
        if button == PointerButton::Middle || space || (button == PointerButton::Primary && session.tool() == Tool::Hand) {
            return Drag::Pan;
        }
        if r {
            return Drag::Rotate { last_angle: self.angle_to(p) };
        }
        let tool = session.tool();
        let painting = matches!(tool, Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Mixer | Tool::Liquify);
        if painting && (button == PointerButton::Secondary || (m.ctrl && m.alt)) {
            return Drag::Size { start_x: p.x, start_size: session.current_brush().size };
        }
        if button != PointerButton::Primary {
            return Drag::None;
        }
        // A press on a ruler bar drags out a guide, ahead of every tool.
        if let Some(top) = self.ruler_band(session, p) {
            let d = self.doc_pt(p);
            let g = onepaint_engine::guides::GuideLine::new(
                onepaint_doc::new_id(),
                d[0],
                d[1],
                if top { 0.0 } else { std::f64::consts::FRAC_PI_2 },
            );
            let mut lines = session.guides().lines.clone();
            lines.push(g);
            let index = lines.len() - 1;
            Self::edit_lines(session, lines);
            return Drag::GuideMove { index, offset: [0.0, 0.0], from_ruler: true };
        }
        if self.measuring {
            let d = self.doc_pt(p);
            self.measure = Some((d, d));
            return Drag::Measure { start: d };
        }
        if self.lines_editing {
            let d = self.doc_pt(p);
            let tol = onepaint_stroke::guide_lines::GUIDE_HIT_PX / self.view.scale;
            return match onepaint_stroke::guide_lines::nearest_guide(&session.guides().lines, d, tol) {
                Some(index) if m.alt => Drag::GuideRotate { index },
                Some(index) => {
                    let g = &session.guides().lines[index];
                    Drag::GuideMove { index, offset: [g.x - d[0], g.y - d[1]], from_ruler: false }
                }
                None => Drag::GuideDraw { start: d, index: None },
            };
        }
        if self.persp_editing {
            return match self.vp_hit(session, p) {
                Some(index) => Drag::Vp { index },
                None => Drag::None,
            };
        }
        if self.sym_editing {
            Self::move_symmetry_centre(session, self.doc_pt(p), self.doc_size);
            return Drag::SymAxis;
        }
        let selecting = matches!(tool, Tool::SelectRect | Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly | Tool::SelectWand);
        if (m.alt && !selecting) || tool == Tool::Eyedropper {
            self.pick(session, p);
            return Drag::Pick;
        }
        let d = self.doc_pt(p);
        let mode = SelectMode::from_modifiers(m.shift, m.alt, self.options.select_mode);
        if tool == Tool::Move
            && let Some(w) = session.transform_warp()
        {
            let pts = warp_points(w);
            // The nearest point within 12 px (corners of a thin box can both be in reach).
            let index = pts
                .iter()
                .enumerate()
                .map(|(i, q)| {
                    let s = self.view.doc_to_screen(*q);
                    (i, pos2(s[0] as f32, s[1] as f32).distance(p))
                })
                .filter(|(_, dist)| *dist <= 12.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            return Drag::WarpPoint { index, grab: d, start: w.clone() };
        }
        if tool == Tool::Move {
            let Some((_, b)) = session.transform_state() else {
                return Drag::None;
            };
            let s = self.view.scale;
            let handle = self.xform.hit(self.doc_f(), b, d, 12.0 / s, 28.0 / s);
            return Drag::Transform { handle, grab: d, start: self.xform };
        }
        match tool {
            Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Mixer | Tool::Liquify => {
                let kind = if self.last_pen_force.is_some() { PointerKind::Pen } else { PointerKind::Mouse };
                self.pressure = Some(PressureProvider::new(kind));
                if matches!(tool, Tool::Brush | Tool::Eraser) && session.guides().hard_perspective(self.persp_editing) {
                    session.stroke_cancel();
                    return Drag::PerspLine { origin: d };
                }
                let cfg = self.snap_config(session);
                self.snapper = if tool == Tool::Liquify { None } else { onepaint_stroke::snap::Snapper::arm(cfg, d) };
                let mut sp = self.stroke_point(p, time_ms, self.last_pen_force);
                if let Some(o) = self.snapper.as_ref().map(|s| s.origin()) {
                    (sp.x, sp.y) = (o[0], o[1]);
                }
                session.stroke_begin(sp);
                let enabled = matches!(tool, Tool::Brush | Tool::Eraser);
                self.hold = HoldTracker { points: vec![d], anchor: p, since: time_ms, fired: false, enabled };
                Drag::Paint
            }
            Tool::Pen => self.pen_press(session, p, d, m, time_ms),
            Tool::Hand => Drag::Pan,
            Tool::Gradient => {
                session.gradient_begin();
                let stops = self.gradient_stops(session);
                session.gradient_update(d, d, self.options.gradient_shape, &stops);
                Drag::Gradient { start: d }
            }
            Tool::Shape => Drag::ShapeTool { start: d, shape: None },
            Tool::Text => match session.text_at(d[0], d[1]) {
                Some((id, i)) => {
                    let spec = session.texts(&id)[i].clone();
                    self.text_edit = Some(TextEdit { target: Some((id, i)), spec: spec.clone() });
                    Drag::TextMove { grab: d, start: Box::new(spec) }
                }
                None => {
                    let c = session.color();
                    let spec =
                        onepaint_text::TextSpec { text: String::new(), x: d[0], y: d[1], color: c.to_hex(), ..self.text_style.clone() };
                    self.text_edit = Some(TextEdit { target: None, spec });
                    Drag::None
                }
            },
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
                                let mut sp = self.stroke_point(p, time_ms, self.last_pen_force);
                                if let Some(s) = self.snapper.as_mut() {
                                    [sp.x, sp.y] = s.apply([sp.x, sp.y]);
                                }
                                session.stroke_extend(&[sp]);
                                self.hold.points.push(d);
                                if p.distance(self.hold.anchor) >= 3.0 {
                                    self.hold.anchor = p;
                                    self.hold.since = time_ms;
                                }
                            }
                        }
                        Drag::PerspLine { origin } => {
                            let origin = *origin;
                            let end = onepaint_stroke::snap::hard_line_end(&self.snap_config(session), origin, d);
                            Self::preview_shape(session, &Shape::Line { a: origin, b: end });
                        }
                        Drag::Vp { index } => {
                            let index = *index;
                            let mut g = session.guides().clone();
                            if let Some(v) = g.perspective.points.get_mut(index) {
                                (v.x, v.y) = (d[0] / self.doc_size[0] as f64, d[1] / self.doc_size[1] as f64);
                                let _ = session.execute(Command::SetGuides { guides: Box::new(g) });
                            }
                        }
                        Drag::SymAxis => Self::move_symmetry_centre(session, d, self.doc_size),
                        Drag::WarpPoint { index, grab, start } => {
                            use onepaint_engine::transform::WarpMode;
                            let (dx, dy) = (d[0] - grab[0], d[1] - grab[1]);
                            let index = *index;
                            let mv = |pts: &mut [[f64; 2]]| match index {
                                Some(i) => {
                                    if let Some(p) = pts.get_mut(i) {
                                        (p[0], p[1]) = (p[0] + dx, p[1] + dy);
                                    }
                                }
                                None => pts.iter_mut().for_each(|p| (p[0], p[1]) = (p[0] + dx, p[1] + dy)),
                            };
                            let next = match start.clone() {
                                WarpMode::Quad(mut q) => {
                                    mv(&mut q);
                                    WarpMode::Quad(q)
                                }
                                WarpMode::Mesh(mut m) => {
                                    mv(&mut m);
                                    WarpMode::Mesh(m)
                                }
                            };
                            let _ = session.execute(Command::SetWarp { warp: next });
                        }
                        Drag::GuideMove { index, offset, .. } => {
                            let (index, offset) = (*index, *offset);
                            let mut lines = session.guides().lines.clone();
                            if let Some(g) = lines.get_mut(index) {
                                (g.x, g.y) = (d[0] + offset[0], d[1] + offset[1]);
                                Self::edit_lines(session, lines);
                            }
                        }
                        Drag::GuideRotate { index } => {
                            let index = *index;
                            let mut lines = session.guides().lines.clone();
                            if let Some(g) = lines.get_mut(index) {
                                let a = onepaint_stroke::guide_lines::angle_through([g.x, g.y], d);
                                g.angle = if shift { onepaint_stroke::guide_lines::snap_angle(a, 15.0) } else { a };
                                Self::edit_lines(session, lines);
                            }
                        }
                        Drag::GuideDraw { start, index } => {
                            let start = *start;
                            if (d[0] - start[0]).hypot(d[1] - start[1]) * self.view.scale >= 4.0 {
                                let a = onepaint_stroke::guide_lines::angle_through(start, d);
                                let a = if shift { onepaint_stroke::guide_lines::snap_angle(a, 15.0) } else { a };
                                let mut lines = session.guides().lines.clone();
                                match *index {
                                    Some(i) if i < lines.len() => lines[i].angle = a,
                                    _ => {
                                        lines.push(onepaint_engine::guides::GuideLine::new(onepaint_doc::new_id(), start[0], start[1], a));
                                        *index = Some(lines.len() - 1);
                                    }
                                }
                                Self::edit_lines(session, lines);
                            }
                        }
                        Drag::Measure { start } => {
                            let start = *start;
                            self.measure = Some((start, d));
                        }
                        Drag::PenNew { index } => {
                            let index = *index;
                            let anchor = self.pen.building.as_ref().and_then(|b| b.get(index)).map(|a| [a.x, a.y]);
                            if let (Some(b), Some(a)) = (self.pen.building.as_mut(), anchor)
                                && (d[0] - a[0]).hypot(d[1] - a[1]) * self.view.scale >= 3.0
                            {
                                crate::pen::pull_handle(b, index, d, shift);
                            }
                        }
                        Drag::PenEdit { obj, sub, anchor, part } => {
                            let (obj, sub, anchor, part) = (*obj, *sub, *anchor, *part);
                            crate::pen::edit_object(session, obj, |spec| {
                                let Some(s) = spec.subpaths.get_mut(sub) else { return };
                                let Some(a) = s.anchors.get(anchor).copied() else { return };
                                use onepaint_stroke::bezier::{HandleSide, constrain_handle, move_anchor, set_handle};
                                let h = |d: Pt| {
                                    let v = [d[0] - a.x, d[1] - a.y];
                                    if shift { constrain_handle(v[0], v[1]) } else { v }
                                };
                                match part {
                                    crate::pen::Part::Anchor => move_anchor(&mut s.anchors, anchor, d[0], d[1]),
                                    crate::pen::Part::In => set_handle(&mut s.anchors, anchor, HandleSide::In, h(d)),
                                    crate::pen::Part::Out => set_handle(&mut s.anchors, anchor, HandleSide::Out, h(d)),
                                }
                            });
                        }
                        Drag::Gradient { start } => {
                            let start = *start;
                            let stops = self.gradient_stops(session);
                            session.gradient_update(start, d, self.options.gradient_shape, &stops);
                        }
                        Drag::TextMove { grab, start } => {
                            let (grab, start) = (*grab, start.clone());
                            if let Some(te) = self.text_edit.as_mut()
                                && let Some((id, i)) = te.target.clone()
                            {
                                te.spec = onepaint_text::TextSpec { x: start.x + d[0] - grab[0], y: start.y + d[1] - grab[1], ..*start };
                                let _ = session.execute(Command::UpdateText { id, index: i, spec: te.spec.clone() });
                            }
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
                        Drag::Paint | Drag::PerspLine { .. } => {
                            self.snapper = None;
                            session.stroke_end();
                        }
                        Drag::GuideMove { index, from_ruler, .. } => {
                            // Dragged off the document, or (from a ruler) back onto a bar: delete.
                            let p = self.last_pos.unwrap_or_default();
                            let mut lines = session.guides().lines.clone();
                            if let Some(g) = lines.get(index) {
                                let (w, h) = (self.doc_size[0] as f64, self.doc_size[1] as f64);
                                let on_bar = from_ruler && self.ruler_band(session, p).is_some();
                                if on_bar || !g.intersects_doc(w, h) {
                                    lines.remove(index);
                                    Self::edit_lines(session, lines);
                                }
                            }
                            let _ = session.execute(Command::CommitGuideLines);
                        }
                        Drag::GuideRotate { .. } | Drag::GuideDraw { .. } => {
                            let _ = session.execute(Command::CommitGuideLines);
                        }
                        Drag::PenEdit { .. } => {
                            let _ = session.execute(Command::SealHistory);
                        }
                        Drag::Gradient { start } if self.options.gradient_layer => {
                            session.stroke_cancel();
                            let end = self.last_pos.map(|p| self.doc_pt(p)).unwrap_or(start);
                            let stops = self.gradient_stops(session);
                            let params = gradient_fill_params(start, end, self.options.gradient_shape, &stops, self.doc_size);
                            let _ = session.execute(Command::AddAdjustmentLayer { kind: "gradient-fill".into(), params });
                        }
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
        if matches!(self.drag, Drag::Paint) && session.stroke_wants_ticks() {
            session.stroke_tick(time_ms);
            ui.ctx().request_repaint();
        }
        // Quick-shape: hold still for 600 ms while painting to straighten the stroke.
        if matches!(self.drag, Drag::Paint) && self.hold.enabled && !self.hold.fired {
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
        self.pending.extend(session.take_dirty_tiles());
        let uploads = self.composite_budgeted(session, ui.ctx());
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

    /// Composite pending tiles within the frame budget, visible ones first.
    fn composite_budgeted(&mut self, session: &Session, ctx: &egui::Context) -> Vec<((i32, i32), Tile)> {
        if self.pending.is_empty() {
            return Vec::new();
        }
        let v = &self.view;
        let corners =
            [[0.0, 0.0], [v.view_size[0], 0.0], [0.0, v.view_size[1]], [v.view_size[0], v.view_size[1]]].map(|c| v.screen_to_doc(c));
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for c in corners {
            (x0, y0, x1, y1) = (x0.min(c[0]), y0.min(c[1]), x1.max(c[0]), y1.max(c[1]));
        }
        let visible = onepaint_raster::Rect::covering(x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        let centre = v.screen_to_doc([v.view_size[0] / 2.0, v.view_size[1] / 2.0]);
        let order = crate::progressive::prioritize(&self.pending, visible, centre);
        let batch = rayon::current_num_threads().max(1);
        let start = std::time::Instant::now();
        let mut out = Vec::new();
        for chunk in order.chunks(batch) {
            out.extend(session.composite(chunk));
            for k in chunk {
                self.pending.remove(k);
            }
            if start.elapsed().as_secs_f64() * 1000.0 >= crate::progressive::BUDGET_MS {
                break;
            }
        }
        if !self.pending.is_empty() {
            ctx.request_repaint();
        }
        out
    }

    fn move_symmetry_centre(session: &mut Session, d: Pt, doc: [u32; 2]) {
        let mut sym = session.guides().symmetry;
        (sym.cx, sym.cy) = ((d[0] / doc[0] as f64).clamp(0.0, 1.0), (d[1] / doc[1] as f64).clamp(0.0, 1.0));
        let _ = session.execute(Command::SetSymmetry { symmetry: sym });
    }

    /// Grid, perspective and H/V reference overlays (guides-overlay.ts).
    fn guide_overlays(&self, ui: &egui::Ui, rect: Rect, session: &Session) {
        let g = session.guides();
        if !g.visible {
            return;
        }
        let (w, h) = (self.doc_size[0] as f64, self.doc_size[1] as f64);
        let painter = ui.painter().with_clip_rect(rect);
        let hex = |s: &str| onepaint_color::Rgba::from_hex(s).map(onepaint_ui_kit::widgets::to_color32).unwrap_or(Color32::GRAY);
        let line = |a: Pt, b: Pt, c: Color32, width: f32| {
            painter.line_segment([self.to_screen(rect, a), self.to_screen(rect, b)], Stroke::new(width, c));
        };
        if g.grid.enabled {
            let step = g.grid.size.max(4.0);
            let sub = g.grid.subdivisions.max(1) as f64;
            let col = hex(&g.grid.color);
            // Skip lines closer than 3 screen px: they would only fill the canvas.
            let minor = step / sub;
            if sub > 1.0 && minor * self.view.scale >= 3.0 {
                let c = col.gamma_multiply(0.22);
                let mut x = minor;
                while x < w {
                    if (x % step).abs() > 1e-3 {
                        line([x, 0.0], [x, h], c, 1.0);
                    }
                    x += minor;
                }
                let mut y = minor;
                while y < h {
                    if (y % step).abs() > 1e-3 {
                        line([0.0, y], [w, y], c, 1.0);
                    }
                    y += minor;
                }
            }
            if step * self.view.scale >= 3.0 {
                let c = col.gamma_multiply(0.45);
                let mut x = 0.0;
                while x <= w + 0.5 {
                    line([x, 0.0], [x, h], c, 1.0);
                    x += step;
                }
                let mut y = 0.0;
                while y <= h + 0.5 {
                    line([0.0, y], [w, y], c, 1.0);
                    y += step;
                }
            }
        }
        let p = &g.perspective;
        if p.enabled && !p.points.is_empty() {
            let col = hex(&p.color);
            let fan = (p.density as usize * 3).max(6);
            let mut border = Vec::with_capacity(fan * 4);
            for k in 0..fan {
                let f = k as f64 / fan as f64;
                border.extend([[f * w, 0.0], [f * w, h], [0.0, f * h], [w, f * h]]);
            }
            let fan_col = col.gamma_multiply(0.3);
            for v in &p.points {
                for b in &border {
                    line([v.x * w, v.y * h], *b, fan_col, 1.0);
                }
            }
            if p.points.len() >= 2 {
                let (a, b) = (p.points[0], p.points[1]);
                line([a.x * w, a.y * h], [b.x * w, b.y * h], col.gamma_multiply(0.6), 1.5);
            }
            let handles = ui.painter();
            for v in &p.points {
                let c = self.to_screen(rect, [v.x * w, v.y * h]);
                handles.circle_filled(c, 9.0, col.gamma_multiply(if self.persp_editing { 0.95 } else { 0.5 }));
                if self.persp_editing {
                    handles.circle_stroke(c, 9.0, Stroke::new(1.5, Color32::WHITE));
                }
            }
        }
        if !g.lines.is_empty() {
            let col = hex(&g.line_color);
            let reach = (w.hypot(h) + (self.view.view_size[0].hypot(self.view.view_size[1])) / self.view.scale) * 2.0;
            for l in &g.lines {
                let d = l.direction();
                let a = self.to_screen(rect, [l.x - d[0] * reach, l.y - d[1] * reach]);
                let b = self.to_screen(rect, [l.x + d[0] * reach, l.y + d[1] * reach]);
                if l.is_locked() {
                    painter.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.0, col.gamma_multiply(0.55)), 6.0, 4.0));
                } else {
                    painter.line_segment([a, b], Stroke::new(1.0, col.gamma_multiply(0.9)));
                }
            }
        }
        if g.snap_hv {
            let c = Color32::from_rgba_unmultiplied(80, 150, 255, 140);
            let (cx, cy) = (w / 2.0, h / 2.0);
            for (a, b) in [([cx, 0.0], [cx, h]), ([0.0, cy], [w, cy])] {
                let pts = [self.to_screen(rect, a), self.to_screen(rect, b)];
                painter.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.5, c), 6.0, 5.0));
            }
        }
    }

    /// Pinned ruler bars (unrotated view) and the measure line, on top of everything.
    fn rulers_and_measure(&self, ui: &egui::Ui, rect: Rect, session: &Session) {
        use onepaint_stroke::guide_lines::{RULER_PX, measure_between, ruler_ticks_span, rulers_pinned};
        let g = session.guides();
        let t = onepaint_ui_kit::Tokens::get(ui.ctx());
        let dpi = session.doc().dpi;
        if let Some((a, b)) = self.measure {
            let col = Color32::from_rgba_unmultiplied(255, 90, 40, 242);
            let (sa, sb) = (self.to_screen(rect, a), self.to_screen(rect, b));
            ui.painter().line_segment([sa, sb], Stroke::new(1.5, col));
            ui.painter().circle_filled(sa, 3.0, col);
            ui.painter().circle_filled(sb, 3.0, col);
            let (len, _, deg) = measure_between(a, b, &g.units, dpi);
            ui.painter().text(
                sb + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                format!("{len}  {deg:.1}°"),
                onepaint_ui_kit::theme::mono(11.5),
                col,
            );
        }
        if !g.rulers || !g.visible || !rulers_pinned(self.view.rotation) {
            return;
        }
        let r = RULER_PX as f32;
        let painter = ui.painter();
        let bg = t.panel.gamma_multiply(0.92);
        let (top, left) = (
            Rect::from_min_max(rect.min + egui::vec2(r, 0.0), egui::pos2(rect.right(), rect.top() + r)),
            Rect::from_min_max(rect.min + egui::vec2(0.0, r), egui::pos2(rect.left() + r, rect.bottom())),
        );
        painter.rect_filled(top, 0.0, bg);
        painter.rect_filled(left, 0.0, bg);
        painter.rect_filled(Rect::from_min_size(rect.min, egui::vec2(r, r)), 0.0, bg);
        let tick = Stroke::new(1.0, t.fg_faint);
        let font = onepaint_ui_kit::theme::mono(9.0);
        let x0 = self.view.screen_to_doc([r as f64, 0.0])[0];
        let x1 = self.view.screen_to_doc([rect.width() as f64, 0.0])[0];
        let (from, to) = (x0.min(x1), x0.max(x1));
        for tk in ruler_ticks_span(from, to, self.view.scale, &g.units, dpi, 2000) {
            let sx = rect.left() + self.view.doc_to_screen([tk.pos, 0.0])[0] as f32;
            let len = if tk.major { r * 0.7 } else { r * 0.3 };
            painter.line_segment([egui::pos2(sx, rect.top() + r - len), egui::pos2(sx, rect.top() + r)], tick);
            if let Some(lbl) = tk.label {
                painter.text(egui::pos2(sx + 2.0, rect.top() + 1.0), egui::Align2::LEFT_TOP, lbl, font.clone(), t.fg_muted);
            }
        }
        let y0 = self.view.screen_to_doc([0.0, r as f64])[1];
        let y1 = self.view.screen_to_doc([0.0, rect.height() as f64])[1];
        let (from, to) = (y0.min(y1), y0.max(y1));
        for tk in ruler_ticks_span(from, to, self.view.scale, &g.units, dpi, 2000) {
            let sy = rect.top() + self.view.doc_to_screen([0.0, tk.pos])[1] as f32;
            let len = if tk.major { r * 0.7 } else { r * 0.3 };
            painter.line_segment([egui::pos2(rect.left() + r - len, sy), egui::pos2(rect.left() + r, sy)], tick);
            if let Some(lbl) = tk.label {
                let galley = painter.layout_no_wrap(lbl, font.clone(), t.fg_muted);
                let pos = egui::pos2(rect.left() + 1.0, sy + 2.0 + galley.size().x);
                painter.add(egui::epaint::TextShape::new(pos, galley, t.fg_muted).with_angle(-std::f32::consts::FRAC_PI_2));
            }
        }
        painter.line_segment([top.left_bottom(), top.right_bottom()], Stroke::new(1.0, t.border));
        painter.line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0, t.border));
    }

    /// The symmetry mirror axes (or radial spokes) through the centre.
    fn symmetry_axes(&self, ui: &egui::Ui, rect: Rect, session: &Session) {
        use onepaint_engine::SymmetryMode as M;
        let sym = session.guides().symmetry;
        if !sym.is_on() || !session.guides().visible {
            return;
        }
        let d = session.doc();
        let c = [sym.cx * d.width as f64, sym.cy * d.height as f64];
        let len = (d.width as f64).hypot(d.height as f64);
        let a = sym.angle;
        let half = std::f64::consts::FRAC_PI_2;
        let (lines, rays): (Vec<f64>, Vec<f64>) = match sym.mode {
            M::Off => (vec![], vec![]),
            M::Vertical => (vec![half + a], vec![]),
            M::Horizontal => (vec![a], vec![]),
            M::Both => (vec![a, half + a], vec![]),
            M::Radial => {
                let n = sym.radial_count.clamp(2, 24);
                (vec![], (0..n).map(|i| a - half + i as f64 / n as f64 * std::f64::consts::TAU).collect())
            }
        };
        let accent = onepaint_ui_kit::Tokens::get(ui.ctx()).accent;
        let seg = |p0: Pt, p1: Pt| {
            let (s0, s1) = (self.to_screen(rect, p0), self.to_screen(rect, p1));
            ui.painter().line_segment([s0, s1], Stroke::new(2.5, Color32::from_black_alpha(90)));
            ui.painter().line_segment([s0, s1], Stroke::new(1.0, accent.gamma_multiply(0.9)));
        };
        for t in lines {
            let (dx, dy) = (t.cos() * len, t.sin() * len);
            seg([c[0] - dx, c[1] - dy], [c[0] + dx, c[1] + dy]);
        }
        for t in rays {
            seg(c, [c[0] + t.cos() * len, c[1] + t.sin() * len]);
        }
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
        self.guide_overlays(ui, rect, session);
        self.symmetry_axes(ui, rect, session);
        self.pen_overlay(ui, rect, session, hover);
        if let Some(te) = &self.text_edit
            && session.tool() == Tool::Text
        {
            let spec =
                if te.spec.text.is_empty() { onepaint_text::TextSpec { text: " ".into(), ..te.spec.clone() } } else { te.spec.clone() };
            if let Some(q) = onepaint_text::Renderer::new().quad(&spec) {
                self.outline(ui, rect, &q, true);
            }
        }
        if let Some(w) = session.transform_warp() {
            use onepaint_engine::transform::WarpMode;
            let accent = onepaint_ui_kit::Tokens::get(ui.ctx()).accent;
            let line = |pts: Vec<Pt>| {
                let sp: Vec<Pos2> = pts.into_iter().map(|q| self.to_screen(rect, q)).collect();
                ui.painter().add(egui::Shape::line(sp, Stroke::new(1.0, accent)));
            };
            match w {
                WarpMode::Quad(q) => self.outline(ui, rect, q, true),
                WarpMode::Mesh(m) => {
                    use onepaint_engine::warp::sample_warp;
                    for k in 0..4 {
                        let t = k as f64 / 3.0;
                        line((0..=24).map(|i| sample_warp(m, i as f64 / 24.0, t)).collect());
                        line((0..=24).map(|i| sample_warp(m, t, i as f64 / 24.0)).collect());
                    }
                }
            }
            for q in warp_points(w) {
                let s = self.to_screen(rect, q);
                ui.painter().circle(s, 5.0, Color32::WHITE, Stroke::new(1.0, Color32::from_black_alpha(160)));
            }
        } else if let Some((_, b)) = session.transform_state() {
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
        self.rulers_and_measure(ui, rect, session);
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
        if matches!(session.tool(), Tool::Brush | Tool::Eraser | Tool::Smudge | Tool::Mixer | Tool::Liquify) {
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

/// The draggable points of a distort or warp.
fn warp_points(w: &onepaint_engine::transform::WarpMode) -> Vec<Pt> {
    match w {
        onepaint_engine::transform::WarpMode::Quad(q) => q.to_vec(),
        onepaint_engine::transform::WarpMode::Mesh(m) => m.clone(),
    }
}

/// A gradient drag as live gradient-fill layer params (endpoints as document fractions).
fn gradient_fill_params(
    a: Pt,
    b: Pt,
    shape: GShape,
    stops: &[onepaint_filters::gradient::GradStop],
    doc: [u32; 2],
) -> onepaint_filters::Params {
    use onepaint_filters::gradient::Shape;
    let (w, h) = (doc[0] as f64, doc[1] as f64);
    let name = match shape {
        Shape::Linear => "linear",
        Shape::Radial => "radial",
        Shape::Angle => "angle",
        Shape::Reflected => "reflected",
        Shape::Diamond => "diamond",
    };
    onepaint_filters::Params {
        grad_shape: Some(name.into()),
        fill_stops: Some(
            stops
                .iter()
                .map(|s| onepaint_filters::FillStop {
                    pos: s.pos as f32,
                    color: s.color.map(|c| c as f32),
                    opacity: s.opacity as f32,
                    mid: s.mid.map(|m| m as f32),
                })
                .collect(),
        ),
        grad_ax: Some((a[0] / w) as f32),
        grad_ay: Some((a[1] / h) as f32),
        grad_bx: Some((b[0] / w) as f32),
        grad_by: Some((b[1] / h) as f32),
        grad_reverse: Some(false),
        ..Default::default()
    }
}
