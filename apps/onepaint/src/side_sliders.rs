//! The size and opacity sliders beside the canvas (happypaint
//! `components/side-sliders`, `utils/side-sliders-placement.ts`,
//! `utils/size-curve.ts`, `utils/slider-entry.ts`; v0.204.0–v0.204.7): on any
//! edge, floating, or with the tools; an end icon types an exact value.

use serde::{Deserialize, Serialize};

/// Where the strip lives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
    Float,
    Tools,
}

impl Mode {
    pub const ALL: [Mode; 6] = [Mode::Left, Mode::Right, Mode::Top, Mode::Bottom, Mode::Float, Mode::Tools];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Left => "Left edge",
            Mode::Right => "Right edge",
            Mode::Top => "Top edge",
            Mode::Bottom => "Bottom edge",
            Mode::Float => "Floating",
            Mode::Tools => "With the tools",
        }
    }

    pub fn is_edge(self) -> bool {
        matches!(self, Mode::Left | Mode::Right | Mode::Top | Mode::Bottom)
    }
}

/// A floating strip: its stage-relative top-left and orientation.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Float {
    pub x: f32,
    pub y: f32,
    pub horizontal: bool,
}

impl Default for Float {
    fn default() -> Self {
        Self { x: 60.0, y: 80.0, horizontal: false }
    }
}

impl Float {
    /// Rounded, and the default where a value isn't a number (`coerceFloat`).
    pub fn coerced(self) -> Self {
        if !self.x.is_finite() || !self.y.is_finite() {
            return Self::default();
        }
        Self { x: self.x.round(), y: self.y.round(), horizontal: self.horizontal }
    }
}

/// Whether the strip runs vertically (`orientationOf`); `tools_vertical`
/// says whether the tool panel is a narrow column.
pub fn vertical(mode: Mode, float: Float, tools_vertical: bool) -> bool {
    match mode {
        Mode::Left | Mode::Right => true,
        Mode::Top | Mode::Bottom => false,
        Mode::Float => !float.horizontal,
        Mode::Tools => tools_vertical,
    }
}

/// Pulling an edge strip's grip this far off its edge floats it.
pub const TEAR_OFF_PX: f32 = 48.0;

/// Whether a grip drag of `(dx, dy)` pulls an edge strip off (`shouldTearOff`).
pub fn should_tear_off(mode: Mode, dx: f32, dy: f32) -> bool {
    match mode {
        Mode::Left | Mode::Right => dx.abs() > TEAR_OFF_PX,
        Mode::Top | Mode::Bottom => dy.abs() > TEAR_OFF_PX,
        Mode::Float | Mode::Tools => false,
    }
}

pub const EDGE_MARGIN: f32 = 6.0;
pub const RULER_INSET: f32 = 18.0;

/// Keep a floating strip of `strip` size on a `stage` (`clampFloat`).
pub fn clamp_float(p: [f32; 2], stage: [f32; 2], strip: [f32; 2], rulers: bool) -> [f32; 2] {
    let lo = EDGE_MARGIN + if rulers { RULER_INSET } else { 0.0 };
    let axis = |v: f32, s: f32, k: f32| {
        let hi = s - k - EDGE_MARGIN;
        if hi < lo { lo } else { v.clamp(lo, hi) }.round()
    };
    [axis(p[0], stage[0], strip[0]), axis(p[1], stage[1], strip[1])]
}

/// How far an edge strip may slide from the centre (`edgeRoom`).
pub fn edge_room(stage_len: f32, strip_len: f32) -> f32 {
    ((stage_len - strip_len) / 2.0 - 8.0).max(0.0)
}

/// The Size slider's position 0..1 as px: `1 + (max − 1)·t²` (`sizeFromSlider`).
pub fn size_from_slider(t: f64, max: f64) -> f64 {
    (1.0 + (max - 1.0) * t.clamp(0.0, 1.0).powi(2)).max(1.0)
}

/// The inverse of [`size_from_slider`].
pub fn slider_from_size(size: f64, max: f64) -> f64 {
    if max <= 1.0 {
        return 0.0;
    }
    ((size.clamp(1.0, max) - 1.0) / (max - 1.0)).sqrt()
}

/// A typed value's range: clamp to `min..=max`, round to `decimals`.
#[derive(Clone, Copy, Debug)]
pub struct EntryRange {
    pub min: f64,
    pub max: f64,
    pub decimals: i32,
}

fn round_to(v: f64, decimals: i32) -> f64 {
    let k = 10f64.powi(decimals);
    (v * k).round() / k
}

/// A typed value (`parseEntry`): a trailing `px` or `%` is dropped, a comma
/// reads as a point; anything else that isn't a plain number is `None`.
/// Out-of-range values clamp.
pub fn parse_entry(text: &str, r: EntryRange) -> Option<f64> {
    let mut s = text.trim().to_string();
    let lower = s.to_ascii_lowercase();
    for unit in ["px", "%"] {
        if lower.ends_with(unit) {
            s.truncate(s.len() - unit.len());
            s = s.trim_end().to_string();
            break;
        }
    }
    let s = s.replacen(',', ".", 1);
    let body = s.strip_prefix('-').unwrap_or(&s);
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    let digits = |p: &str| p.chars().all(|c| c.is_ascii_digit());
    let ok = !body.is_empty() && digits(int) && digits(frac) && (!int.is_empty() || !frac.is_empty()) && body.matches('.').count() <= 1;
    if !ok || body == "." {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    Some(round_to(v, r.decimals).clamp(r.min, r.max))
}

/// A value as the entry shows it (`formatEntry`): "12", "2.5".
pub fn format_entry(v: f64, r: EntryRange) -> String {
    let v = round_to(v, r.decimals);
    if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_floats_and_orientation() {
        let m: Mode = serde_json::from_str("\"tools\"").unwrap();
        assert_eq!(m, Mode::Tools);
        assert_eq!(Float { x: f32::NAN, y: 1.0, horizontal: true }.coerced(), Float::default());
        assert_eq!(Float { x: 120.6, y: 40.2, horizontal: true }.coerced(), Float { x: 121.0, y: 40.0, horizontal: true });
        let h = Float { horizontal: true, ..Default::default() };
        assert!(vertical(Mode::Left, h, false) && vertical(Mode::Right, h, false));
        assert!(!vertical(Mode::Top, Float::default(), true) && !vertical(Mode::Bottom, Float::default(), true));
        assert!(!vertical(Mode::Float, h, true) && vertical(Mode::Float, Float::default(), false));
        assert!(vertical(Mode::Tools, h, true) && !vertical(Mode::Tools, h, false));
        assert_eq!(Mode::ALL.map(Mode::is_edge), [true, true, true, true, false, false]);
    }

    #[test]
    fn tear_off_needs_more_than_48_px_away_from_the_edge() {
        assert!(should_tear_off(Mode::Left, 49.0, 0.0) && should_tear_off(Mode::Right, -49.0, 0.0));
        assert!(!should_tear_off(Mode::Left, 10.0, 400.0));
        assert!(should_tear_off(Mode::Top, 0.0, 49.0) && should_tear_off(Mode::Bottom, 0.0, -49.0));
        assert!(!should_tear_off(Mode::Top, 500.0, 10.0));
        assert!(!should_tear_off(Mode::Left, 48.0, 0.0));
        assert!(!should_tear_off(Mode::Float, 999.0, 999.0) && !should_tear_off(Mode::Tools, 999.0, 999.0));
    }

    #[test]
    fn floats_stay_on_the_stage() {
        let (stage, strip) = ([1000.0, 700.0], [40.0, 400.0]);
        assert_eq!(clamp_float([300.0, 100.0], stage, strip, false), [300.0, 100.0]);
        assert_eq!(clamp_float([-50.0, -50.0], stage, strip, false), [6.0, 6.0]);
        assert_eq!(clamp_float([5000.0, 5000.0], stage, strip, false), [954.0, 294.0]);
        assert_eq!(clamp_float([0.0, 0.0], stage, strip, true), [24.0, 24.0]);
        assert_eq!(clamp_float([0.0, 300.0], stage, [40.0, 900.0], false)[1], 6.0);
        assert_eq!((edge_room(700.0, 400.0), edge_room(300.0, 400.0)), (142.0, 0.0));
    }

    #[test]
    fn the_size_curve() {
        assert_eq!((size_from_slider(0.0, 1000.0), size_from_slider(1.0, 1000.0)), (1.0, 1000.0));
        assert!((size_from_slider(0.5, 1000.0) - 250.75).abs() < 1e-9);
        let t = slider_from_size(10.0, 1000.0);
        assert!(t > 0.09 && t < 0.1);
        for s in [1.0, 3.0, 20.0, 140.0, 999.0] {
            assert!((size_from_slider(slider_from_size(s, 1000.0), 1000.0) - s).abs() < 1e-9);
        }
        assert_eq!((size_from_slider(-1.0, 500.0), size_from_slider(2.0, 500.0), slider_from_size(5000.0, 500.0)), (1.0, 500.0, 1.0));
    }

    #[test]
    fn typed_entries_match_slider_entry_test() {
        let size = EntryRange { min: 1.0, max: 1000.0, decimals: 1 };
        let pct = EntryRange { min: 1.0, max: 100.0, decimals: 0 };
        assert_eq!(parse_entry("12", size), Some(12.0));
        assert_eq!(parse_entry(" 12 px ", size), Some(12.0));
        assert_eq!(parse_entry("40%", pct), Some(40.0));
        assert_eq!(parse_entry("2,5", size), Some(2.5));
        assert_eq!(parse_entry("2.46", size), Some(2.5));
        assert_eq!(parse_entry("33.6", pct), Some(34.0));
        assert_eq!(parse_entry("0", size), Some(1.0));
        assert_eq!(parse_entry("5000", size), Some(1000.0));
        assert_eq!(parse_entry("-20", pct), Some(1.0));
        assert_eq!(parse_entry("150", pct), Some(100.0));
        for bad in ["", "   ", "abc", "1e3", "1.2.3"] {
            assert_eq!(parse_entry(bad, size), None, "{bad:?}");
        }
        assert_eq!(format_entry(12.0, size), "12");
        assert_eq!(format_entry(2.46, size), "2.5");
        assert_eq!(format_entry(33.4, pct), "33");
    }
}

// ── The strip ──

use crate::App;
use egui::{Align2, Area, Id, Order, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use onepaint_engine::Command;
use onepaint_ui_kit::widgets::{self, glass_frame};
use onepaint_ui_kit::{Tokens, icons};

/// Which value a typed entry edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Size,
    Opacity,
}

/// What the strip remembers between frames.
#[derive(Default)]
pub struct UiState {
    /// The strip's last rect, to clamp its offset and float position.
    rect: Option<Rect>,
    /// A grip drag: where it started and the offset or float position then.
    drag: Option<(Pos2, [f32; 2])>,
    /// The open typed entry: which value, its text, and where it sits.
    entry: Option<(Part, String, Pos2)>,
    /// When an entry was last closed, so the click that closed it doesn't reopen it.
    closed: Option<(Part, f64)>,
}

const TRACK: f32 = 150.0;

impl UiState {
    /// A typed value is open: the next press elsewhere only closes it.
    pub fn entry_open(&self) -> bool {
        self.entry.is_some()
    }
}

fn size_range(app: &App) -> EntryRange {
    EntryRange { min: 1.0, max: app.settings.max_brush_size, decimals: 1 }
}

const PCT: EntryRange = EntryRange { min: 1.0, max: 100.0, decimals: 0 };

/// The strip for the edge and float modes; with the tools it is drawn by
/// the tool bar (`body`). `stage` is the canvas area between the docks.
pub fn show(app: &mut App, ctx: &egui::Context, stage: Rect) {
    let s = &app.settings;
    if !s.side_sliders || s.side_sliders_mode == Mode::Tools {
        return;
    }
    let mode = s.side_sliders_mode;
    let float = s.side_sliders_float;
    let vertical = vertical(mode, float, true);
    let rulers = app.session.guides().rulers;
    let inset = EDGE_MARGIN + if rulers { RULER_INSET } else { 0.0 };
    let size = app.side.rect.map_or(vec2(36.0, 300.0), |r| r.size());
    let along = if vertical { size.y } else { size.x };
    let room = edge_room(if vertical { stage.height() } else { stage.width() }, along);
    let off = s.side_sliders_offset.clamp(-room, room);
    let (at, pivot) = match mode {
        Mode::Left => (pos2(stage.left() + inset, stage.center().y + off), Align2::LEFT_CENTER),
        Mode::Right => (pos2(stage.right() - EDGE_MARGIN, stage.center().y + off), Align2::RIGHT_CENTER),
        // Below the floating top bars.
        Mode::Top => (pos2(stage.center().x + off, stage.top() + 64.0 + if rulers { RULER_INSET } else { 0.0 }), Align2::CENTER_TOP),
        Mode::Bottom => (pos2(stage.center().x + off, stage.bottom() - EDGE_MARGIN), Align2::CENTER_BOTTOM),
        _ => {
            let p = clamp_float([float.x, float.y], [stage.width(), stage.height()], [size.x, size.y], rulers);
            (stage.min + vec2(p[0], p[1]), Align2::LEFT_TOP)
        }
    };
    let r = Area::new(Id::new("side-sliders")).order(Order::Middle).pivot(pivot).fixed_pos(at).show(ctx, |ui| {
        glass_frame(ui).inner_margin(egui::Margin::symmetric(4, 6)).show(ui, |ui| {
            let mut inner = |ui: &mut Ui| {
                grip(app, ui, mode, vertical, stage);
                body(app, ui, vertical);
            };
            if vertical {
                ui.vertical_centered(|ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    inner(ui)
                });
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    inner(ui)
                });
            }
        });
    });
    app.side.rect = Some(r.response.rect);
    entry_popover(app, ctx, vertical);
}

/// The grip: slide along the edge, pull off to float, drag a float anywhere;
/// double-click turns a float or recentres an edge strip.
fn grip(app: &mut App, ui: &mut Ui, mode: Mode, vertical: bool, stage: Rect) {
    let t = Tokens::get(ui.ctx());
    let size = if vertical { vec2(28.0, 14.0) } else { vec2(14.0, 28.0) };
    let (rect, r) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let tip = if mode == Mode::Float {
        format!("Drag to move · double-click to turn {}", if vertical { "horizontal" } else { "vertical" })
    } else {
        format!("Drag to move {} · pull away from the edge to float it", if vertical { "up or down" } else { "left or right" })
    };
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &tip));
    let r = r.on_hover_text(&tip).on_hover_cursor(egui::CursorIcon::Grab);
    let icon = if vertical { "grip-horizontal" } else { "grip-vertical" };
    icons::paint(
        ui,
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        icon,
        14.0,
        if r.hovered() { t.fg_muted } else { t.fg_faint },
    );
    let s = &mut app.settings;
    if r.double_clicked() {
        if mode == Mode::Float {
            s.side_sliders_float.horizontal = !s.side_sliders_float.horizontal;
        } else {
            s.side_sliders_offset = 0.0;
        }
        s.save();
        return;
    }
    if r.drag_started()
        && let Some(p) = ui.input(|i| i.pointer.press_origin())
    {
        let base = if mode == Mode::Float { [s.side_sliders_float.x, s.side_sliders_float.y] } else { [s.side_sliders_offset, 0.0] };
        app.side.drag = Some((p, base));
    }
    if r.dragged()
        && let (Some((start, base)), Some(p)) = (app.side.drag, r.interact_pointer_pos())
    {
        let d = p - start;
        let strip = app.side.rect.unwrap_or(rect);
        if should_tear_off(mode, d.x, d.y) {
            // Float where the strip now is, keeping its orientation; the drag goes on.
            let at = strip.min + d - stage.min.to_vec2();
            s.side_sliders_mode = Mode::Float;
            s.side_sliders_offset = 0.0;
            s.side_sliders_float = Float { x: at.x.round(), y: at.y.round(), horizontal: !vertical };
            app.side.drag = Some((p, [at.x, at.y]));
        } else if mode == Mode::Float {
            let q = clamp_float([base[0] + d.x, base[1] + d.y], [stage.width(), stage.height()], [strip.width(), strip.height()], false);
            s.side_sliders_float.x = q[0];
            s.side_sliders_float.y = q[1];
        } else {
            s.side_sliders_offset = (base[0] + if vertical { d.y } else { d.x }).round();
        }
    }
    if r.drag_stopped() {
        app.side.drag = None;
        app.settings.save();
    }
}

/// The two sliders with their end icons, then undo and redo; `tools` mode
/// draws this inside the tool bar.
pub fn body(app: &mut App, ui: &mut Ui, vertical: bool) {
    let max = app.settings.max_brush_size;
    let (size, opacity) = {
        let b = app.session.current_brush();
        (b.size, b.opacity)
    };
    // Size: the small-and-big dots icon, then its track.
    let icon = end_icon(
        ui,
        Part::Size,
        app.side.entry.as_ref().is_some_and(|e| e.0 == Part::Size),
        "Brush size · tap to type a value",
        "Type a brush size",
    );
    if icon.clicked() {
        toggle_entry(app, ui, Part::Size, icon.rect);
    }
    let mut v = slider_from_size(size, max) as f32;
    let r = track(ui, &mut v, vertical, "Brush size", &format!("{} px", size.round()));
    if r.changed() {
        let _ = app.session.execute(Command::SetBrushSize { size: size_from_slider(v as f64, max).round().max(1.0) });
        app.canvas.size_flash = 1.0;
    }
    if r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        toggle_entry(app, ui, Part::Size, icon.rect);
    }
    // Keep the two tracks apart, so they don't read as one.
    ui.add_space(10.0);
    let mut o = opacity as f32;
    let r = track(ui, &mut o, vertical, "Opacity", &format!("{}%", (opacity * 100.0).round()));
    if r.changed() {
        let _ = app.session.execute(Command::SetBrushOpacity { opacity: o.clamp(0.01, 1.0) as f64 });
    }
    let icon = end_icon(
        ui,
        Part::Opacity,
        app.side.entry.as_ref().is_some_and(|e| e.0 == Part::Opacity),
        "Opacity · tap to type a value",
        "Type an opacity",
    );
    if icon.clicked() || (r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
        toggle_entry(app, ui, Part::Opacity, icon.rect);
    }
    ui.add_enabled_ui(app.session.can_undo(), |ui| {
        if icons::button(ui, "undo-2", 28.0, false, "Undo (Ctrl+Z · two-finger tap)").clicked() {
            let _ = app.session.execute(Command::Undo);
        }
    });
    ui.add_enabled_ui(app.session.can_redo(), |ui| {
        if icons::button(ui, "redo-2", 28.0, false, "Redo (Ctrl+Shift+Z · three-finger tap)").clicked() {
            let _ = app.session.execute(Command::Redo);
        }
    });
}

/// A slider track; the value shows beside the pointer while it is dragged.
fn track(ui: &mut Ui, v: &mut f32, vertical: bool, label: &str, readout: &str) -> egui::Response {
    let r = if vertical {
        widgets::vslider(ui, v, 0.0..=1.0, TRACK, "")
    } else {
        ui.allocate_ui(vec2(TRACK, 20.0), |ui| widgets::slider(ui, v, 0.0..=1.0, None)).inner
    };
    let value = (*v * 100.0).round() as f64;
    r.widget_info(|| egui::WidgetInfo::slider(true, value, label));
    if r.dragged() {
        egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), r.id.with("readout"), egui::PopupAnchor::Pointer).show(|ui| {
            ui.label(egui::RichText::new(readout).font(onepaint_ui_kit::theme::mono(12.0)));
        });
        r
    } else {
        r.on_hover_text(label)
    }
}

/// An end icon: two dots (small and big) for size, Blend for opacity.
fn end_icon(ui: &mut Ui, part: Part, active: bool, tip: &str, label: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let color = if active {
        t.accent
    } else if r.hovered() {
        t.fg
    } else {
        t.fg_muted
    };
    if active || r.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.hover);
    }
    match part {
        Part::Size => {
            // happypaint's glyph: circle (5.5, 17) r 3 and (15, 10) r 7 in a 24 box, at 14 px.
            let k = 14.0 / 24.0;
            let o = rect.center() - vec2(7.0, 7.0);
            let stroke = egui::Stroke::new(1.5, color);
            ui.painter().circle_stroke(o + vec2(5.5, 17.0) * k, 3.0 * k, stroke);
            ui.painter().circle_stroke(o + vec2(15.0, 10.0) * k, 7.0 * k, stroke);
        }
        Part::Opacity => icons::paint(ui, Rect::from_center_size(rect.center(), vec2(14.0, 14.0)), "blend", 14.0, color),
    }
    r.on_hover_text(tip)
}

fn toggle_entry(app: &mut App, ui: &Ui, part: Part, at: Rect) {
    let now = ui.input(|i| i.time);
    if let Some((p, _, _)) = &app.side.entry
        && *p == part
    {
        commit_entry(app);
        app.side.closed = Some((part, now));
        return;
    }
    // The press that closed this entry also clicks its icon: don't reopen.
    if app.side.closed.is_some_and(|(p, at)| p == part && now - at < 0.4) {
        return;
    }
    commit_entry(app);
    let brush = app.session.current_brush();
    let text = match part {
        Part::Size => format_entry(brush.size, size_range(app)),
        Part::Opacity => format_entry(brush.opacity * 100.0, PCT),
    };
    app.side.entry = Some((part, text, at.center()));
}

/// Apply the open entry, if its text reads as a number.
fn commit_entry(app: &mut App) {
    let Some((part, text, _)) = app.side.entry.take() else { return };
    match part {
        Part::Size => {
            if let Some(v) = parse_entry(&text, size_range(app)) {
                let _ = app.session.execute(Command::SetBrushSize { size: v });
            }
        }
        Part::Opacity => {
            if let Some(v) = parse_entry(&text, PCT) {
                let _ = app.session.execute(Command::SetBrushOpacity { opacity: (v / 100.0).clamp(0.01, 1.0) });
            }
        }
    }
}

/// The typed entry beside its icon, toward the screen's centre: Enter or a
/// click elsewhere commits, Esc cancels.
pub fn entry_popover(app: &mut App, ctx: &egui::Context, vertical: bool) {
    let Some((part, _, at)) = app.side.entry.clone() else { return };
    let screen = ctx.content_rect();
    let (pos, pivot) = if vertical {
        if at.x < screen.center().x { (at + vec2(20.0, 0.0), Align2::LEFT_CENTER) } else { (at - vec2(20.0, 0.0), Align2::RIGHT_CENTER) }
    } else if at.y < screen.center().y {
        (at + vec2(0.0, 20.0), Align2::CENTER_TOP)
    } else {
        (at - vec2(0.0, 20.0), Align2::CENTER_BOTTOM)
    };
    let (label, unit) = match part {
        Part::Size => ("Brush size in pixels", "px"),
        Part::Opacity => ("Opacity in percent", "%"),
    };
    let mut done = None;
    let r = Area::new(Id::new("side-entry")).order(Order::Foreground).pivot(pivot).fixed_pos(pos).show(ctx, |ui| {
        widgets::popover_frame(ui).show(ui, |ui| {
            ui.horizontal(|ui| {
                let Some((_, text, _)) = app.side.entry.as_mut() else { return };
                // The field takes Escape itself (egui would drop its focus first), to cancel.
                let filter = egui::EventFilter { escape: true, ..Default::default() };
                let te = ui.add(
                    egui::TextEdit::singleline(text).desired_width(48.0).font(onepaint_ui_kit::theme::mono(12.5)).event_filter(filter),
                );
                te.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, label));
                if !te.has_focus() && !te.lost_focus() {
                    te.request_focus();
                }
                ui.label(unit);
                let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if esc {
                    done = Some(false);
                } else if enter || te.lost_focus() {
                    done = Some(true);
                }
            });
        });
    });
    // A press anywhere else commits.
    let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !r.response.rect.contains(p)));
    let now = ctx.input(|i| i.time);
    match done.or(outside.then_some(true)) {
        Some(true) => {
            commit_entry(app);
            app.side.closed = Some((part, now));
        }
        Some(false) => {
            app.side.entry = None;
            app.side.closed = Some((part, now));
        }
        None => {}
    }
}
