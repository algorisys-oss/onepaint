//! Panel tool (spec 30, happypaint paint-canvas.tsx:3517-3596, 4222-4265):
//! one press, four gestures chosen by what was grabbed, plus the layout
//! presets and page style in the panel options popover.

use crate::App;
use egui::{Color32, Pos2, Stroke, Ui};
use onepaint_engine::{Command, Session};
use onepaint_panel::geometry::{LAYOUT_PRESETS, Pt, distance_to_polygon_edge, nearest_vertex, translate_outline};
use onepaint_ui_kit::{Tokens, theme, widgets};

/// Handle grab radius in screen px, so handles keep their size at any zoom (PC:318).
pub const GRAB_PX: f64 = 12.0;
/// A drag shorter than this (doc px) commits nothing (PC:320).
pub const MIN_DRAG: f64 = 4.0;

/// Sticky panel options: what new frames and layouts get.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelOptions {
    pub gutter: f32,
    pub border: f32,
    pub margin: f32,
}

impl Default for PanelOptions {
    fn default() -> Self {
        Self {
            gutter: onepaint_panel::DEFAULT_GUTTER as f32,
            border: onepaint_panel::DEFAULT_BORDER as f32,
            margin: onepaint_panel::DEFAULT_MARGIN as f32,
        }
    }
}

/// What the press grabbed (PC:3517-3521).
#[derive(Clone, Debug, PartialEq)]
pub enum Mode {
    /// Bare page: draw a new frame.
    Create,
    /// Inside a frame: cut it along the drag.
    Split { id: String },
    /// A corner handle: reshape that corner.
    Vertex { id: String, index: usize, before: Vec<Pt> },
    /// The frame line: move the whole frame (the art stays put).
    Move { id: String, before: Vec<Pt> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PanelDrag {
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub mode: Mode,
    /// A reshape or move has been applied (it passed the drag threshold).
    pub live: bool,
}

impl PanelDrag {
    fn moved(&self) -> f64 {
        (self.to[0] - self.from[0]).hypot(self.to[1] - self.from[1])
    }

    /// The outline a reshape or move would leave.
    fn edited(&self) -> Option<(&str, Vec<Pt>)> {
        match &self.mode {
            Mode::Vertex { id, index, before } => {
                let mut o = before.clone();
                o[*index] = Pt::new(self.to[0], self.to[1]);
                Some((id, o))
            }
            Mode::Move { id, before } => Some((id, translate_outline(before, self.to[0] - self.from[0], self.to[1] - self.from[1]))),
            _ => None,
        }
    }
}

/// Classify a press at doc point `d`: corner, then frame line, then interior,
/// then bare page. Topmost frame first, matching `panel_at` (PC:3533-3553).
pub fn press(session: &Session, d: [f64; 2], scale: f64) -> PanelDrag {
    let grab = GRAB_PX / scale.max(0.01);
    let at = Pt::new(d[0], d[1]);
    let frames = session.panel_frames();
    let mode = frames
        .iter()
        .rev()
        .find_map(|(id, o)| nearest_vertex(o, at, grab).map(|index| Mode::Vertex { id: id.clone(), index, before: o.clone() }))
        .or_else(|| {
            frames
                .iter()
                .rev()
                .find(|(_, o)| distance_to_polygon_edge(o, at) <= grab)
                .map(|(id, o)| Mode::Move { id: id.clone(), before: o.clone() })
        })
        .unwrap_or_else(|| match session.panel_at(d) {
            Some(id) => Mode::Split { id },
            None => Mode::Create,
        });
    PanelDrag { from: d, to: d, mode, live: false }
}

/// Pointer moved: reshape and move apply live (one undo step per gesture).
pub fn drag(pd: &mut PanelDrag, session: &mut Session, d: [f64; 2]) {
    pd.to = d;
    if !pd.live && pd.moved() < MIN_DRAG {
        return;
    }
    if let Some((id, outline)) = pd.edited() {
        let cmd = Command::SetPanelOutline { id: id.to_string(), outline: outline.iter().map(|p| p.to_array()).collect() };
        let _ = session.execute(cmd);
        pd.live = true;
    }
}

/// Release: commit a create or split; close a reshape or move (PC:3577-3596).
pub fn release(pd: PanelDrag, session: &mut Session, o: &PanelOptions) {
    let far = pd.moved() >= MIN_DRAG;
    match &pd.mode {
        Mode::Vertex { id, before, .. } | Mode::Move { id, before } if pd.live => {
            if !far {
                // Dragged back under the threshold: put the frame back exactly.
                let outline = before.iter().map(|p| p.to_array()).collect();
                let _ = session.execute(Command::SetPanelOutline { id: id.clone(), outline });
            }
            let _ = session.execute(Command::SealHistory);
        }
        Mode::Split { id } if far => {
            let _ = session.execute(Command::SplitPanel { id: id.clone(), a: pd.from, b: pd.to });
        }
        Mode::Create if far => {
            let _ = session.execute(Command::CreatePanel { a: pd.from, b: pd.to, gutter: o.gutter as f64, border: o.border as f64 });
        }
        _ => {}
    }
}

/// Corner handles on every frame, and the dashed preview of a create or a cut
/// (PC:4222-4265). `to_screen` maps doc px to screen points.
pub fn overlay(ui: &Ui, session: &Session, pd: Option<&PanelDrag>, to_screen: impl Fn([f64; 2]) -> Pos2) {
    let painter = ui.painter();
    let accent = Tokens::get(ui.ctx()).accent;
    for (_, o) in session.panel_frames() {
        for q in o {
            let r = egui::Rect::from_center_size(to_screen(q.to_array()), egui::vec2(8.0, 8.0));
            painter.rect(
                r,
                1.0,
                Color32::from_white_alpha(242),
                Stroke::new(1.0, Color32::from_black_alpha(140)),
                egui::StrokeKind::Middle,
            );
        }
    }
    let Some(d) = pd else { return };
    let (a, b) = (to_screen(d.from), to_screen(d.to));
    let stroke = Stroke::new(1.5, accent);
    match d.mode {
        Mode::Split { .. } => {
            // Extend the cut well past the drag so it reads as a line through the panel.
            let v = b - a;
            let len = v.length();
            if len > 0.0 {
                let far = ui.clip_rect().size().max_elem() * 2.0;
                let u = v / len;
                painter.add(egui::Shape::dashed_line(&[a - u * far, a + u * far], stroke, 6.0, 4.0));
            }
        }
        Mode::Create => {
            let r = egui::Rect::from_two_pos(a, b);
            let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.add(egui::Shape::dashed_line(&pts, stroke, 6.0, 4.0));
        }
        _ => {}
    }
}

/// The panel options popover: layout presets, page style and help.
pub fn options(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let now = ui.input(|i| i.time);
    crate::dock::fit_width(ui, 300.0);
    widgets::section_label(ui, "Page layout");
    ui.horizontal_wrapped(|ui| {
        for l in LAYOUT_PRESETS {
            let label = format!("{}×{}", l.rows, l.cols);
            if widgets::secondary_button(ui, &label, 44.0).on_hover_text(l.tip).clicked() {
                let o = app.canvas.panel_options;
                let cmd = Command::CreatePanelGrid {
                    rows: l.rows,
                    cols: l.cols,
                    gutter: o.gutter as f64,
                    margin: o.margin as f64,
                    border: o.border as f64,
                };
                if let Err(e) = app.session.execute(cmd) {
                    app.notify(e.to_string(), now);
                }
            }
        }
    });
    widgets::hairline(ui);
    widgets::section_label(ui, "Frames");
    let mut o = app.canvas.panel_options;
    let px = |v: f32| format!("{v:.0} px");
    let g = widgets::slider_row(ui, "Gutter", &mut o.gutter, 0.0..=120.0, px);
    g.widget_info(|| egui::WidgetInfo::slider(true, o.gutter as f64, "Gutter width"));
    if g.changed() {
        let _ = app.session.execute(Command::SetPanelStyle { id: None, gutter: Some(o.gutter.round() as f64), border: None });
    }
    let b = widgets::slider_row(ui, "Border", &mut o.border, 0.0..=30.0, px);
    b.widget_info(|| egui::WidgetInfo::slider(true, o.border as f64, "Border width"));
    if b.changed() {
        let _ = app.session.execute(Command::SetPanelStyle { id: None, gutter: None, border: Some(o.border.round() as f64) });
    }
    if g.drag_stopped() || b.drag_stopped() || g.clicked() || b.clicked() {
        let _ = app.session.execute(Command::SealHistory);
    }
    widgets::slider_row(ui, "Page margin", &mut o.margin, 0.0..=200.0, px);
    app.canvas.panel_options = o;
    ui.label(
        egui::RichText::new(
            "Drag on the page to draw a frame; drag across a frame to cut it in two. Drag a corner to reshape, the frame line to move it. Art inside a panel is clipped to its frame; gutter and border apply to every panel on the page.",
        )
        .font(theme::medium(10.5))
        .color(t.fg_faint),
    );
}

#[cfg(test)]
mod tests {
    use crate::{App, Popover};
    use egui::{Event, PointerButton, Pos2, pos2, vec2};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use onepaint_engine::Tool;

    fn harness() -> Harness<'static, App> {
        Harness::builder().with_size(vec2(1200.0, 800.0)).with_max_steps(400).build_eframe(|cc| App::new(cc, None))
    }

    fn press(h: &mut Harness<'_, App>, p: Pos2, pressed: bool) {
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() });
        h.step();
    }

    fn drag(h: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
        h.event(Event::PointerMoved(from));
        h.step();
        press(h, from, true);
        for i in 1..=10 {
            h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
            h.step();
        }
        press(h, to, false);
        h.run();
    }

    fn screen(h: &Harness<'_, App>, d: [f64; 2]) -> Pos2 {
        let s = h.state().canvas.doc_to_win(d);
        pos2(s[0] as f32, s[1] as f32)
    }

    fn centre(o: &[onepaint_panel::geometry::Pt]) -> [f64; 2] {
        let n = o.len() as f64;
        [o.iter().map(|p| p.x).sum::<f64>() / n, o.iter().map(|p| p.y).sum::<f64>() / n]
    }

    #[test]
    fn panel_tool_creates_lays_out_splits_and_reshapes() {
        let mut h = harness();
        h.run();
        h.get_by_label("Panel (manga)").click();
        h.run();
        assert_eq!(h.state().session.tool(), Tool::Panel);

        // A drag on bare page draws a frame.
        drag(&mut h, pos2(400.0, 250.0), pos2(700.0, 500.0));
        assert_eq!(h.state().session.panel_frames().len(), 1, "drag on bare page creates a frame");
        let _ = h.state_mut().session.execute(onepaint_engine::Command::Undo);
        assert!(h.state().session.panel_frames().is_empty());

        // A preset lays out a page.
        h.state_mut().popover = Some(Popover::PanelOptions);
        h.run();
        h.get_by_label("2×2").click();
        h.run();
        assert_eq!(h.state().session.panel_frames().len(), 4);
        h.state_mut().popover = None;
        h.run();

        // A drag inside a frame cuts it in two.
        let (id, outline) = h.state().session.panel_frames().remove(0);
        let c = screen(&h, centre(&outline));
        drag(&mut h, c - vec2(0.0, 30.0), c + vec2(0.0, 30.0));
        let frames = h.state().session.panel_frames();
        assert_eq!(frames.len(), 5, "split");
        assert_eq!(frames[0].0, id);

        // A corner drag reshapes that corner only.
        let (id, before) = h.state().session.panel_frames().remove(0);
        let h0 = h.state().session.history_len();
        // Bring the page's top corner out from under the top bar.
        h.state_mut().canvas.view.pan(0.0, 60.0);
        h.run();
        let corner = screen(&h, before[0].to_array());
        drag(&mut h, corner, corner + vec2(30.0, 20.0));
        let after = h.state().session.panel(&id).map(|p| p.outline).unwrap_or_default();
        let want = h.state().canvas.win_to_doc([corner.x as f64 + 30.0, corner.y as f64 + 20.0]);
        assert!((after[0].x - want[0]).abs() < 1.0 && (after[0].y - want[1]).abs() < 1.0, "{:?}", after[0]);
        assert_eq!(after[1..], before[1..], "only the grabbed corner moves");
        assert_eq!(h.state().session.history_len(), h0 + 1, "one step per drag");

        // A tap on a handle changes nothing.
        let corner = screen(&h, after[2].to_array());
        drag(&mut h, corner, corner);
        assert_eq!(h.state().session.history_len(), h0 + 1);
    }

    #[test]
    fn gutter_slider_restyles_every_panel_in_one_step() {
        let mut h = harness();
        h.run();
        h.state_mut().set_tool(Tool::Panel);
        h.state_mut().popover = Some(Popover::PanelOptions);
        h.run();
        h.get_by_label("1×2").click();
        h.run();
        let h0 = h.state().session.history_len();
        let r = h.get_by_label("Gutter width").rect();
        drag(&mut h, r.center(), pos2(r.right() - 4.0, r.center().y));
        let s = &h.state().session;
        let gutters: Vec<f64> = s.panel_frames().iter().filter_map(|(id, _)| s.panel(id)).map(|p| p.gutter()).collect();
        assert_eq!(gutters.len(), 2);
        assert!(gutters.iter().all(|g| *g > 100.0), "{gutters:?}");
        assert_eq!(s.history_len(), h0 + 1, "one step per slider drag");
        assert!(h.state().canvas.panel_options.gutter > 100.0);
    }

    #[test]
    fn picking_the_tool_shows_its_layouts_and_the_frame_line_moves_a_frame() {
        let mut h = harness();
        h.run();
        h.get_by_label("Panel (manga)").click();
        h.run();
        assert_eq!(h.state().popover, Some(Popover::PanelOptions), "the layouts show on first pick");
        h.get_by_label("1×2").click();
        h.run();
        h.state_mut().popover = None;
        h.state_mut().canvas.view.pan(0.0, 60.0);
        h.run();
        // Grab the middle of the first frame's top edge and drag it down.
        let (id, before) = h.state().session.panel_frames().remove(0);
        let mid = [(before[0].x + before[1].x) / 2.0, (before[0].y + before[1].y) / 2.0];
        let at = screen(&h, mid);
        drag(&mut h, at, at + vec2(0.0, 40.0));
        let after = h.state().session.panel(&id).map(|p| p.outline).unwrap_or_default();
        let dy = after[0].y - before[0].y;
        assert!(dy > 10.0, "moved down {dy}");
        assert!(after.iter().zip(&before).all(|(a, b)| (a.y - b.y - dy).abs() < 1e-6 && (a.x - b.x).abs() < 1e-6), "the whole frame moves");
        // Art painted in a panel stops at its frame.
        let art = h.state().session.active().to_string();
        let o = h.state().session.panel(&id).map(|p| p.outline).unwrap_or_default();
        let (x0, x1) = (o.iter().map(|p| p.x).fold(f64::MAX, f64::min), o.iter().map(|p| p.x).fold(f64::MIN, f64::max));
        let y = (o[0].y + o[2].y) / 2.0;
        h.state_mut().set_tool(Tool::Brush);
        h.run();
        let (a, b) = (screen(&h, [x0 - 80.0, y]), screen(&h, [x1 + 80.0, y]));
        drag(&mut h, a, b);
        let s = &h.state().session;
        assert_eq!(s.active(), art, "painting goes to the panel's Art layer");
        let shown = |x: f64| s.pick_color(x as i32, y as i32);
        assert!(shown((x0 + x1) / 2.0).g < 0.5, "paint shows inside the frame");
        assert!(shown(x0 - 40.0).g > 0.99, "and stops at it: {:?}", shown(x0 - 40.0));
    }
}
