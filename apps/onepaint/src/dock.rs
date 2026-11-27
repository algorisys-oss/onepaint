//! Docked and floating panels (happypaint `components/dock`, `float-drag.ts`):
//! side docks with stacked or tabbed sections, floating windows that fold,
//! resize and tab together, and the drag that moves a panel between them.
//! The data model is `layout.rs`; panel bodies come from `chrome::panel_body`.

use crate::layout::{self, DockSide, Edge, PanelId, Placement};
use crate::{App, Popover};
use egui::{Area, Id, Order, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use onepaint_engine::Tool;
use onepaint_ui_kit::widgets::popover_frame;
use onepaint_ui_kit::{Tokens, icons, theme};

/// Header strip of a docked section or a floating window.
const HEAD_H: f32 = 28.0;
/// Section splitter thickness.
const SPLIT_H: f32 = 7.0;
/// Width splitter at a dock's inner edge.
const EDGE_W: f32 = 8.0;
/// A docked section's body never gets shorter than this.
const MIN_BODY_H: f32 = 82.0;
/// Resize grips along a floating window's border.
const GRIP: f32 = 6.0;
/// A tab must travel this far before it tears off its group.
const TEAR_OFF: f32 = 6.0;
/// The top of the docks and rails: below the top bar.
pub const TOP: f32 = 0.0;

/// What a floating title or tab strip stands for as a drop target.
#[derive(Clone, Debug, PartialEq)]
pub enum FloatTarget {
    Panel(PanelId),
    Group(String),
}

/// Where a panel drag would land.
#[derive(Clone, Debug, PartialEq)]
pub enum Drop {
    Float,
    Join(FloatTarget),
    DockTab(DockSide),
    Dock(DockSide, usize),
}

#[derive(Clone, Debug)]
pub struct PanelDrag {
    pub id: PanelId,
    /// Pointer offset from the window's top-left.
    grab: Vec2,
}

#[derive(Clone, Debug)]
struct Resize {
    target: FloatTarget,
    edge: Edge,
    start: Rect,
    from: Pos2,
}

/// Per-frame UI state for the docks: the drag in progress and the rects
/// drop targets were drawn at (last frame).
#[derive(Default)]
pub struct DockUi {
    pub drag: Option<PanelDrag>,
    /// A floating tab group's strip being dragged (moves it, never docks).
    group_drag: Option<(String, Vec2)>,
    resize: Option<Resize>,
    heads: Vec<(DockSide, Rect)>,
    titles: Vec<(FloatTarget, Rect)>,
    mids: [Vec<f32>; 2],
    pub preview: Option<Drop>,
    /// Where floating windows were drawn, for drop and resize maths.
    windows: Vec<(FloatTarget, Rect)>,
}

/// Window-edge space the docks and the tool rail take, in px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reserved {
    pub left: f32,
    pub right: f32,
    /// Dock widths alone (the rail sits outside them).
    pub dock: [f32; 2],
}

/// Below 900 px the docks give way to popovers (`isCompact`).
pub fn compact(view: Rect) -> bool {
    view.width() < crate::status_bar::COMPACT_W
}

/// The panel a popover shows, when that panel can live elsewhere.
pub fn panel_of(p: Popover) -> Option<PanelId> {
    Some(match p {
        Popover::Layers => PanelId::Layers,
        Popover::Brushes => PanelId::Brush,
        Popover::Color => PanelId::Color,
        Popover::Guides => PanelId::Guides,
        Popover::AdjustLayer => PanelId::Properties,
        Popover::MoveOptions
        | Popover::Selection
        | Popover::Fill
        | Popover::GradientOptions
        | Popover::ShapeOptions
        | Popover::PenOptions
        | Popover::PanelOptions
        | Popover::SmudgeOptions
        | Popover::MixerOptions
        | Popover::LiquifyOptions => PanelId::ToolOptions,
        _ => return None,
    })
}

/// The options popover of a tool (brush and eraser use the brush panel).
pub fn tool_options(t: Tool) -> Option<Popover> {
    Some(match t {
        Tool::Move => Popover::MoveOptions,
        Tool::SelectRect | Tool::SelectEllipse | Tool::SelectLasso | Tool::SelectPoly | Tool::SelectWand => Popover::Selection,
        Tool::Fill => Popover::Fill,
        Tool::Gradient => Popover::GradientOptions,
        Tool::Shape => Popover::ShapeOptions,
        Tool::Pen => Popover::PenOptions,
        Tool::Panel => Popover::PanelOptions,
        Tool::Smudge => Popover::SmudgeOptions,
        Tool::Mixer => Popover::MixerOptions,
        Tool::Liquify => Popover::LiquifyOptions,
        _ => return None,
    })
}

/// The panel lives outside its popover: docked, floating or grouped (or
/// it has no popover). Narrow windows put everything back home.
pub fn hosted(app: &App, id: PanelId, view: Rect) -> bool {
    if compact(view) {
        return !id.home_is_popover();
    }
    !(id.home_is_popover() && app.layout.placement(id) == Placement::Home)
}

pub fn panel_open(app: &App, id: PanelId) -> bool {
    match id {
        PanelId::Reference => app.reference.open,
        _ => app.layout.is_open(id),
    }
}

pub fn set_panel_open(app: &mut App, id: PanelId, open: bool) {
    match id {
        PanelId::Reference => app.reference.open = open,
        _ => app.layout.set_open(id, open),
    }
    app.layout.save();
}

/// Open or close what a toolbar button shows: its popover at home, else
/// the docked or floating panel.
pub fn toggle(app: &mut App, p: Popover, view: Rect) {
    match panel_of(p).filter(|id| hosted(app, *id, view)) {
        Some(id) => {
            let open = panel_open(app, id);
            set_panel_open(app, id, !open);
            if !open
                && let Placement::Group { group } = app.layout.placement(id)
                && let Some(g) = app.layout.groups.get_mut(&group)
            {
                g.active = Some(id);
            }
        }
        None => app.popover = if app.popover == Some(p) { None } else { Some(p) },
    }
}

/// The button's panel is showing (popover or hosted).
pub fn showing(app: &App, p: Popover, view: Rect) -> bool {
    match panel_of(p).filter(|id| hosted(app, *id, view)) {
        Some(id) => panel_open(app, id),
        None => app.popover == Some(p),
    }
}

/// A request to open a panel's popover opens the panel where it lives.
pub fn normalise(app: &mut App, view: Rect) {
    let Some(p) = app.popover else { return };
    let Some(id) = panel_of(p) else { return };
    if hosted(app, id, view) {
        app.popover = None;
        set_panel_open(app, id, true);
        if let Placement::Group { group } = app.layout.placement(id)
            && let Some(g) = app.layout.groups.get_mut(&group)
        {
            g.active = Some(id);
        }
    }
}

/// Space the docks and the rail take from the stage.
pub fn reserved(app: &App, view: Rect) -> Reserved {
    if app.hide_ui || compact(view) {
        return Reserved::default();
    }
    let mut r = Reserved::default();
    for side in [DockSide::Left, DockSide::Right] {
        if !open_docked(app, side).is_empty() {
            r.dock[side.i()] = app.layout.width[side.i()];
        }
    }
    let rail = app.layout.tools_dock.map(|_| crate::toolbar::rail_width(app) + 2.0).unwrap_or(0.0);
    r.left = r.dock[0] + if app.layout.tools_dock == Some(DockSide::Left) { rail } else { 0.0 };
    r.right = r.dock[1] + if app.layout.tools_dock == Some(DockSide::Right) { rail } else { 0.0 };
    r
}

fn open_docked(app: &App, side: DockSide) -> Vec<PanelId> {
    app.layout.docked_to(side).into_iter().filter(|id| panel_open(app, *id)).collect()
}

/// Bodies size themselves to `w` in a popover and fill their host elsewhere.
pub fn fit_width(ui: &mut Ui, w: f32) {
    if ui.ctx().data(|d| d.get_temp::<bool>(hosted_id())).unwrap_or(false) {
        ui.set_width(ui.available_width());
    } else {
        ui.set_width(w);
    }
}

/// The body is in a dock or floating window, which shows its title.
pub fn is_hosted(ui: &Ui) -> bool {
    ui.ctx().data(|d| d.get_temp::<bool>(hosted_id())).unwrap_or(false)
}

fn hosted_id() -> Id {
    Id::new("panel-hosted")
}

fn host_body(app: &mut App, ui: &mut Ui, id: PanelId) {
    ui.ctx().data_mut(|d| d.insert_temp(hosted_id(), true));
    crate::chrome::panel_body(app, ui, id);
    ui.ctx().data_mut(|d| d.insert_temp(hosted_id(), false));
}

/// Draw the docks, floating windows and drop previews; track drags.
pub fn show(app: &mut App, ctx: &egui::Context, view: Rect, stage_bottom: f32) {
    app.dock.heads.clear();
    app.dock.titles.clear();
    app.dock.windows.clear();
    app.dock.mids = [Vec::new(), Vec::new()];
    if !compact(view) {
        let res = reserved(app, view);
        for side in [DockSide::Left, DockSide::Right] {
            let w = res.dock[side.i()];
            if w <= 0.0 {
                continue;
            }
            // Docks sit inside the tool rail, if there is one.
            let x = match side {
                DockSide::Left => view.left() + res.left - w,
                DockSide::Right => view.right() - res.right,
            };
            dock_zone(app, ctx, side, Rect::from_min_max(pos2(x, view.top() + TOP), pos2(x + w, stage_bottom)));
        }
    }
    floats(app, ctx, view, stage_bottom);
    drag_tick(app, ctx, view, stage_bottom);
}

// ── Docks ──

fn dock_zone(app: &mut App, ctx: &egui::Context, side: DockSide, rect: Rect) {
    let t = Tokens::get(ctx);
    let open = open_docked(app, side);
    let tabbed = app.layout.tabbed[side.i()] && open.len() > 1;
    Area::new(Id::new(("dock", side))).order(Order::Middle).fixed_pos(rect.min).show(ctx, |ui| {
        ui.set_min_size(rect.size());
        ui.set_max_size(rect.size());
        ui.painter().rect_filled(rect, 0.0, t.panel);
        let edge_x = match side {
            DockSide::Left => rect.right(),
            DockSide::Right => rect.left(),
        };
        ui.painter().vline(edge_x, rect.y_range(), Stroke::new(1.0, t.border));
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        if tabbed {
            tabbed_dock(app, ui, side, rect, &open);
        } else {
            stacked_dock(app, ui, side, rect, &open);
        }
        width_splitter(app, ui, side, rect);
    });
}

fn stacked_dock(app: &mut App, ui: &mut Ui, side: DockSide, rect: Rect, open: &[PanelId]) {
    let expanded: Vec<PanelId> = open.iter().copied().filter(|id| !app.layout.is_collapsed(*id)).collect();
    let splits = open.len().saturating_sub(1) as f32 * SPLIT_H;
    let free = (rect.height() - open.len() as f32 * HEAD_H - splits).max(0.0);
    let total_w: f32 = expanded.iter().map(|id| app.layout.weight(*id)).sum::<f32>().max(1e-3);
    let mut y = rect.top();
    let mut body_h = std::collections::BTreeMap::new();
    for (i, id) in open.iter().copied().enumerate() {
        let collapsed = app.layout.is_collapsed(id);
        let h = if collapsed { 0.0 } else { (free * app.layout.weight(id) / total_w).max(MIN_BODY_H.min(free)) };
        body_h.insert(id, h);
        let head = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), HEAD_H));
        app.dock.heads.push((side, head));
        app.dock.mids[side.i()].push(head.top() + (HEAD_H + h) / 2.0);
        if section_header(app, ui, side, id, head, i == 0 && open.len() > 1) {
            float_together(app, side, open, rect, None);
            return;
        }
        y += HEAD_H;
        if h > 0.0 {
            let body = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), h));
            section_body(app, ui, id, body);
            y += h;
        }
        if i + 1 < open.len() {
            let split = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), SPLIT_H));
            let next = open[i + 1];
            if !(collapsed && app.layout.is_collapsed(next)) {
                section_splitter(app, ui, split, id, next, &body_h, free, total_w);
            }
            y += SPLIT_H;
        }
    }
}

/// A stacked dock's section header; true when its Float button (on the
/// first header, `tabs_button`) asks the side's panels to float together.
fn section_header(app: &mut App, ui: &mut Ui, side: DockSide, id: PanelId, head: Rect, tabs_button: bool) -> bool {
    let mut float = false;
    let t = Tokens::get(ui.ctx());
    ui.painter().rect_filled(head, 0.0, t.bg_elevated);
    ui.painter().hline(head.x_range(), head.bottom(), Stroke::new(1.0, t.border));
    let collapsed = app.layout.is_collapsed(id);
    let mut child = ui
        .new_child(egui::UiBuilder::new().max_rect(head.shrink2(vec2(6.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let drag = child.interact(head, Id::new(("dock-head", id)), Sense::click_and_drag());
    icons::paint(
        &child,
        Rect::from_center_size(pos2(head.left() + 12.0, head.center().y), vec2(14.0, 14.0)),
        "grip-vertical",
        14.0,
        t.fg_faint,
    );
    child.add_space(18.0);
    child.label(RichText::new(id.label()).font(theme::semibold(12.0)).color(t.fg));
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if icons::button(ui, "x", 22.0, false, &format!("Close {}", id.label())).clicked() {
            set_panel_open(app, id, false);
        }
        let (icon, tip) = if collapsed { ("chevron-down", "Expand") } else { ("chevron-up", "Collapse") };
        if icons::button(ui, icon, 22.0, false, tip).clicked() {
            app.layout.collapsed.insert(id, !collapsed);
            app.layout.save();
        }
        if tabs_button && icons::button(ui, "chevrons-up-down", 22.0, false, "Show as tabs").clicked() {
            app.layout.tabbed[side.i()] = true;
            app.layout.save();
        }
        float = tabs_button && icons::button(ui, "maximize-2", 22.0, false, "Float these panels together, as one group").clicked();
    });
    if drag.double_clicked() {
        app.layout.collapsed.insert(id, !collapsed);
        app.layout.save();
    }
    if drag.drag_started()
        && let Some(p) = drag.interact_pointer_pos()
    {
        begin_drag(app, id, p, p - head.min);
    }
    drag.on_hover_cursor(egui::CursorIcon::Grab);
    float
}

/// Panels that keep a header pinned and scroll the rest themselves (the
/// Brushes panel's search and actions, happypaint v0.204.18).
fn scrolls_itself(id: PanelId) -> bool {
    id == PanelId::Brush
}

fn section_body(app: &mut App, ui: &mut Ui, id: PanelId, body: Rect) {
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(body.shrink2(vec2(10.0, 6.0))).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(body);
    if scrolls_itself(id) {
        child.spacing_mut().item_spacing = vec2(8.0, 6.0);
        host_body(app, &mut child, id);
        return;
    }
    egui::ScrollArea::vertical().id_salt(("dock-body", id)).auto_shrink([false, false]).max_height(body.height() - 12.0).show(
        &mut child,
        |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            host_body(app, ui, id);
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn section_splitter(
    app: &mut App,
    ui: &mut Ui,
    split: Rect,
    a: PanelId,
    b: PanelId,
    body_h: &std::collections::BTreeMap<PanelId, f32>,
    free: f32,
    total_w: f32,
) {
    let t = Tokens::get(ui.ctx());
    let r = ui.interact(split, Id::new(("dock-split", a, b)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
    let c = if r.hovered() || r.dragged() { t.accent } else { t.border };
    ui.painter().hline(split.shrink2(vec2(split.width() * 0.4, 0.0)).x_range(), split.center().y, Stroke::new(2.0, c));
    if r.dragged() && free > 0.0 {
        let dy = r.drag_delta().y;
        let px_per_w = free / total_w;
        let (wa, wb) = (app.layout.weight(a), app.layout.weight(b));
        let (ha, hb) = (body_h.get(&a).copied().unwrap_or(0.0), body_h.get(&b).copied().unwrap_or(0.0));
        if ha + dy >= MIN_BODY_H && hb - dy >= MIN_BODY_H {
            let dw = dy / px_per_w;
            app.layout.set_weights(a, b, wa + dw, wb - dw);
        }
    }
    if r.drag_stopped() {
        app.layout.save();
    }
}

fn tabbed_dock(app: &mut App, ui: &mut Ui, side: DockSide, rect: Rect, open: &[PanelId]) {
    let t = Tokens::get(ui.ctx());
    let active = app.layout.active_dock_tab(side, open).unwrap_or(open[0]);
    let strip = Rect::from_min_size(rect.min, vec2(rect.width(), HEAD_H + 2.0));
    app.dock.heads.push((side, strip));
    ui.painter().rect_filled(strip, 0.0, t.bg_elevated);
    ui.painter().hline(strip.x_range(), strip.bottom(), Stroke::new(1.0, t.border));
    // The strip's empty space drags the open panels out as one group
    // (happypaint v0.211.9); the tabs and buttons, laid over it, win.
    let pull = ui.interact(strip, Id::new(("dock-strip", side.i())), Sense::drag());
    if open.len() > 1
        && pull.drag_started()
        && let Some(p) = pull.interact_pointer_pos()
    {
        let grab = p - strip.min;
        if let Some(key) = float_together(app, side, open, rect, Some(strip.min)) {
            app.dock.group_drag = Some((key, grab));
        }
        return;
    }
    let mut child = ui
        .new_child(egui::UiBuilder::new().max_rect(strip.shrink2(vec2(4.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.spacing_mut().item_spacing.x = 2.0;
    for id in open {
        let sel = *id == active;
        let text = RichText::new(id.label()).font(theme::medium(11.5)).color(if sel { t.fg } else { t.fg_muted });
        let r = child.add(egui::Button::new(text).selected(sel).sense(Sense::click_and_drag()));
        if r.clicked() {
            app.layout.set_active_dock_tab(side, *id);
            app.layout.save();
        }
        if sel
            && r.drag_started()
            && let Some(p) = r.interact_pointer_pos()
        {
            begin_drag(app, *id, p, vec2(20.0, 12.0));
        }
        if sel && icons::button(&mut child, "x", 18.0, false, &format!("Close {}", id.label())).clicked() {
            set_panel_open(app, *id, false);
        }
    }
    let mut float = false;
    child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if icons::button(ui, "chevrons-up-down", 22.0, false, "Stack the panels").clicked() {
            app.layout.tabbed[side.i()] = false;
            app.layout.save();
        }
        float = open.len() > 1 && icons::button(ui, "maximize-2", 22.0, false, "Float these panels together, as one group").clicked();
    });
    if float {
        float_together(app, side, open, rect, None);
        return;
    }
    let body = Rect::from_min_max(pos2(rect.left(), strip.bottom()), rect.max);
    app.dock.mids[side.i()].push(body.center().y);
    section_body(app, ui, active, body);
}

fn width_splitter(app: &mut App, ui: &mut Ui, side: DockSide, rect: Rect) {
    let x = match side {
        DockSide::Left => rect.right() - EDGE_W / 2.0,
        DockSide::Right => rect.left() - EDGE_W / 2.0,
    };
    let strip = Rect::from_min_size(pos2(x, rect.top()), vec2(EDGE_W, rect.height()));
    let r = ui.interact(strip, Id::new(("dock-width", side)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if r.dragged() {
        let dx = r.drag_delta().x;
        let w = app.layout.width[side.i()] + if side == DockSide::Left { dx } else { -dx };
        app.layout.set_dock_width(side, w);
    }
    if r.drag_stopped() {
        app.layout.save();
    }
}

// ── Floating windows ──

fn default_width(id: PanelId) -> f32 {
    match id {
        PanelId::Brush => 340.0,
        PanelId::Color => 280.0,
        PanelId::Align => 260.0,
        PanelId::Guides => 330.0,
        PanelId::Text => 350.0,
        PanelId::Reference => 320.0,
        _ => 300.0,
    }
}

fn float_pos(app: &App, id: PanelId, view: Rect) -> Option<Pos2> {
    match app.layout.placement(id) {
        Placement::Float { x, y } => Some(pos2(x, y)),
        Placement::Home if !id.home_is_popover() || compact(view) => Some(id.default_float(view.size())),
        _ => None,
    }
}

fn floats(app: &mut App, ctx: &egui::Context, view: Rect, stage_bottom: f32) {
    for id in PanelId::ALL {
        if !panel_open(app, id) || !hosted(app, id, view) {
            continue;
        }
        // A floating Text panel belongs to the Text tool; docked or grouped
        // it stays (happypaint v0.210.0).
        if id == PanelId::Text && app.session.tool() != onepaint_engine::Tool::Text {
            continue;
        }
        let Some(pos) = float_pos(app, id, view) else { continue };
        let size = app.layout.sizes.get(&id).map(|s| vec2(s[0], s[1]));
        let folded = app.layout.is_float_collapsed(id);
        let rect = float_window(app, ctx, FloatTarget::Panel(id), &[id], id, pos, size, folded, view, stage_bottom);
        app.dock.windows.push((FloatTarget::Panel(id), rect));
    }
    if compact(view) {
        return;
    }
    let keys: Vec<String> = app.layout.groups.keys().cloned().collect();
    for key in keys {
        let members: Vec<PanelId> = app.layout.groups[&key].members.iter().copied().filter(|m| panel_open(app, *m)).collect();
        let Some(active) = app.layout.active_group_tab(&key, |m| members.contains(&m)) else { continue };
        let g = &app.layout.groups[&key];
        let (pos, size, folded) = (pos2(g.x, g.y), g.w.zip(g.h).map(|(w, h)| vec2(w, h)), g.collapsed);
        let rect = float_window(app, ctx, FloatTarget::Group(key.clone()), &members, active, pos, size, folded, view, stage_bottom);
        app.dock.windows.push((FloatTarget::Group(key), rect));
    }
}

#[allow(clippy::too_many_arguments)]
fn float_window(
    app: &mut App,
    ctx: &egui::Context,
    target: FloatTarget,
    members: &[PanelId],
    active: PanelId,
    pos: Pos2,
    size: Option<Vec2>,
    folded: bool,
    view: Rect,
    stage_bottom: f32,
) -> Rect {
    let t = Tokens::get(ctx);
    let w = size.map_or(default_width(active), |s| s.x);
    let max_h = (stage_bottom - pos.y - 24.0).max(120.0);
    let body_h = size.map(|s| (s.y - HEAD_H - 24.0).max(40.0)).unwrap_or(max_h - HEAD_H - 24.0).min(max_h);
    let area_id = Id::new(("float", format!("{target:?}")));
    let r = Area::new(area_id).order(Order::Middle).fixed_pos(pos).constrain(false).show(ctx, |ui| {
        let framed = popover_frame(ui).show(ui, |ui| {
            ui.set_width(w - 24.0);
            let head = title_row(app, ui, &target, members, active, folded);
            app.dock.titles.push((target.clone(), head));
            if !folded {
                ui.add_space(4.0);
                if scrolls_itself(active) {
                    ui.allocate_ui(vec2(ui.available_width(), body_h), |ui| host_body(app, ui, active));
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt(("float-body", active))
                    .max_height(body_h)
                    .auto_shrink([false, size.is_none()])
                    .show(ui, |ui| host_body(app, ui, active));
            }
        });
        if !folded {
            resize_grips(app, ui, &target, framed.response.rect, members, view);
        }
    });
    let _ = t;
    r.response.rect
}

/// The title (or tab strip) of a floating window; returns its rect.
fn title_row(app: &mut App, ui: &mut Ui, target: &FloatTarget, members: &[PanelId], active: PanelId, folded: bool) -> Rect {
    let t = Tokens::get(ui.ctx());
    let r = ui.horizontal(|ui| {
        ui.set_height(HEAD_H - 4.0);
        let (grip, _) = ui.allocate_exact_size(vec2(14.0, 20.0), Sense::hover());
        icons::paint(ui, grip, "grip-vertical", 14.0, t.fg_faint);
        match target {
            FloatTarget::Panel(id) => {
                ui.label(RichText::new(id.label()).font(theme::semibold(13.0)).color(t.fg));
            }
            FloatTarget::Group(key) => {
                for m in members {
                    let sel = *m == active;
                    let text = RichText::new(m.label()).font(theme::medium(11.5)).color(if sel { t.fg } else { t.fg_muted });
                    let tab = ui.add(egui::Button::new(text).selected(sel).sense(Sense::click_and_drag()));
                    if tab.clicked()
                        && let Some(g) = app.layout.groups.get_mut(key)
                    {
                        g.active = Some(*m);
                        app.layout.save();
                    }
                    // Tear the active tab off once it travels a few px.
                    if sel
                        && tab.dragged()
                        && tab.drag_delta().length()
                            + ui.input(|i| i.pointer.press_origin().zip(i.pointer.latest_pos()).map_or(0.0, |(a, b)| a.distance(b)))
                            > TEAR_OFF
                        && app.dock.drag.is_none()
                        && let Some(p) = tab.interact_pointer_pos()
                    {
                        app.layout.leave_group(*m);
                        begin_drag(app, *m, p, vec2(24.0, 12.0));
                    }
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = match target {
                FloatTarget::Panel(id) => format!("Close {}", id.label()),
                FloatTarget::Group(_) => "Close these panels".to_string(),
            };
            if icons::button(ui, "x", 22.0, false, &label).clicked() {
                for m in members {
                    set_panel_open(app, *m, false);
                }
            }
            let (icon, tip) = if folded { ("chevron-down", "Unfold") } else { ("chevron-up", "Fold") };
            if icons::button(ui, icon, 22.0, false, tip).clicked() {
                fold(app, target, !folded);
            }
        });
    });
    let head = r.response.rect;
    let drag = ui.interact(head, ui.id().with("title-drag"), Sense::click_and_drag());
    if drag.double_clicked() {
        fold(app, target, !folded);
    }
    if drag.drag_started()
        && let Some(p) = drag.interact_pointer_pos()
    {
        match target {
            FloatTarget::Panel(id) => begin_drag(app, *id, p, p - head.min + vec2(12.0, 12.0)),
            FloatTarget::Group(key) => {
                if let Some(g) = app.layout.groups.get(key) {
                    app.dock.group_drag = Some((key.clone(), p - pos2(g.x, g.y)));
                }
            }
        }
    }
    head
}

fn fold(app: &mut App, target: &FloatTarget, on: bool) {
    match target {
        FloatTarget::Panel(id) => {
            app.layout.float_collapsed.insert(*id, on);
        }
        FloatTarget::Group(key) => {
            if let Some(g) = app.layout.groups.get_mut(key) {
                g.collapsed = on;
            }
        }
    }
    app.layout.save();
}

/// Resize grips along the window's border (drawn inside its own area, so
/// they never cover the window's controls).
fn resize_grips(app: &mut App, ui: &mut Ui, target: &FloatTarget, rect: Rect, members: &[PanelId], view: Rect) {
    let height = members.iter().all(|m| m.float_resize().height);
    let min = members.iter().fold(Vec2::ZERO, |a, m| a.max(m.float_resize().min)).max(if members.len() > 1 {
        vec2(200.0, 160.0)
    } else {
        Vec2::ZERO
    });
    {
        for edge in Edge::ALL {
            if !height && !edge.horizontal_only() {
                continue;
            }
            let g = grip_rect(edge, rect);
            let cursor = match edge {
                Edge::N | Edge::S => egui::CursorIcon::ResizeVertical,
                Edge::E | Edge::W => egui::CursorIcon::ResizeHorizontal,
                Edge::Ne | Edge::Sw => egui::CursorIcon::ResizeNeSw,
                Edge::Nw | Edge::Se => egui::CursorIcon::ResizeNwSe,
            };
            let r = ui.interact(g, Id::new(("grip", format!("{target:?}"), format!("{edge:?}"))), Sense::drag()).on_hover_cursor(cursor);
            if r.drag_started()
                && let Some(p) = r.interact_pointer_pos()
            {
                app.dock.resize = Some(Resize { target: target.clone(), edge, start: rect, from: p });
            }
            if r.dragged()
                && let (Some(rs), Some(p)) = (app.dock.resize.clone(), r.interact_pointer_pos())
                && rs.target == *target
            {
                let mut nr = layout::resize_rect(rs.edge, rs.start, p - rs.from, min, view.size());
                if !height {
                    nr = Rect::from_min_size(nr.min, vec2(nr.width(), rs.start.height()));
                }
                apply_rect(app, target, nr, height);
            }
            if r.drag_stopped() {
                app.dock.resize = None;
                app.layout.save();
            }
        }
    }
}

fn grip_rect(edge: Edge, r: Rect) -> Rect {
    let (g, c) = (GRIP, GRIP * 2.0);
    match edge {
        Edge::N => Rect::from_min_max(pos2(r.left() + c, r.top()), pos2(r.right() - c, r.top() + g)),
        Edge::S => Rect::from_min_max(pos2(r.left() + c, r.bottom() - g), pos2(r.right() - c, r.bottom())),
        Edge::W => Rect::from_min_max(pos2(r.left(), r.top() + c), pos2(r.left() + g, r.bottom() - c)),
        Edge::E => Rect::from_min_max(pos2(r.right() - g, r.top() + c), pos2(r.right(), r.bottom() - c)),
        Edge::Nw => Rect::from_min_size(r.min, vec2(c, c)),
        Edge::Ne => Rect::from_min_size(pos2(r.right() - c, r.top()), vec2(c, c)),
        Edge::Sw => Rect::from_min_size(pos2(r.left(), r.bottom() - c), vec2(c, c)),
        Edge::Se => Rect::from_min_size(r.max - vec2(c, c), vec2(c, c)),
    }
}

fn apply_rect(app: &mut App, target: &FloatTarget, r: Rect, height: bool) {
    match target {
        FloatTarget::Panel(id) => {
            app.layout.placements.insert(*id, Placement::Float { x: r.left(), y: r.top() });
            let h = if height { r.height() } else { 0.0 };
            app.layout.sizes.insert(*id, [r.width(), h]);
        }
        FloatTarget::Group(key) => {
            if let Some(g) = app.layout.groups.get_mut(key) {
                (g.x, g.y, g.w, g.h) = (r.left(), r.top(), Some(r.width()), Some(r.height()));
            }
        }
    }
}

// ── Dragging panels around ──

/// Start moving a panel: it floats under the pointer from now on.
pub fn begin_drag(app: &mut App, id: PanelId, pointer: Pos2, grab: Vec2) {
    let at = pointer - grab;
    if matches!(app.layout.placement(id), Placement::Group { .. }) {
        app.layout.leave_group(id);
    }
    app.layout.set_placement(id, Placement::Float { x: at.x, y: at.y }, None);
    if app.popover.and_then(panel_of) == Some(id) {
        app.popover = None;
    }
    set_panel_open(app, id, true);
    app.dock.drag = Some(PanelDrag { id, grab });
}

/// Where a drop at `p` of a window at `win` lands (`applyPanelDrop`).
fn resolve(app: &App, id: PanelId, p: Pos2, win: Option<Rect>, view: Rect) -> Drop {
    let own_group = match app.layout.placement(id) {
        Placement::Group { group } => Some(group),
        _ => None,
    };
    let titles: Vec<(FloatTarget, Rect)> = app
        .dock
        .titles
        .iter()
        .filter(|(t, _)| match t {
            FloatTarget::Panel(p) => *p != id,
            FloatTarget::Group(g) => Some(g) != own_group.as_ref(),
        })
        .cloned()
        .collect();
    if let Some(t) = layout::float_target_at(p, &titles) {
        return Drop::Join(t);
    }
    if let Some(side) = layout::tab_drop_side(p, &app.dock.heads) {
        return Drop::DockTab(side);
    }
    let local = pos2(p.x - view.left(), p.y - view.top());
    let win = win.map(|r| r.translate(-view.min.to_vec2()));
    match layout::drop_dock_side(local, win, view.size()) {
        Some(side) => Drop::Dock(side, layout::dock_insert_index(&app.dock.mids[side.i()], p.y)),
        None => Drop::Float,
    }
}

/// Where a floating group dropped at `p` lands: a docked header (tabs) or
/// a side's edge band; `None` keeps it floating.
fn group_drop(app: &App, p: Pos2, win: Option<Rect>, view: Rect) -> Option<Drop> {
    if let Some(side) = layout::tab_drop_side(p, &app.dock.heads) {
        return Some(Drop::DockTab(side));
    }
    let local = pos2(p.x - view.left(), p.y - view.top());
    let win = win.map(|r| r.translate(-view.min.to_vec2()));
    layout::drop_dock_side(local, win, view.size()).map(|side| Drop::Dock(side, layout::dock_insert_index(&app.dock.mids[side.i()], p.y)))
}

/// Float a side's open panels together as one group with its top-left at
/// `at` (default: just inside the dock), the showing tab still showing.
fn float_together(app: &mut App, side: DockSide, open: &[PanelId], dock: Rect, at: Option<Pos2>) -> Option<String> {
    let at = at.unwrap_or(dock.min + vec2(24.0, 24.0));
    let rect = Rect::from_min_size(at, vec2(dock.width(), dock.height().min(520.0)));
    let key = app.layout.float_docked_as_group(side, open, rect)?;
    app.layout.save();
    Some(key)
}

fn drag_tick(app: &mut App, ctx: &egui::Context, view: Rect, stage_bottom: f32) {
    let (pointer, down) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
    if let Some((key, grab)) = app.dock.group_drag.clone() {
        // A group docks whole on a side's edge or a docked header (happypaint
        // v0.211.9); it never joins another floating window.
        let win = app.dock.windows.iter().find(|(t, _)| *t == FloatTarget::Group(key.clone())).map(|(_, r)| *r);
        let drop = pointer.and_then(|p| group_drop(app, p, win, view));
        match (down, pointer) {
            (true, Some(p)) => {
                if let Some(g) = app.layout.groups.get_mut(&key) {
                    let at = clamp_float(p - grab, vec2(g.w.unwrap_or(300.0), HEAD_H), view, stage_bottom);
                    (g.x, g.y) = (at.x, at.y);
                }
                if let Some(d) = &drop {
                    draw_preview(app, ctx, d, view, stage_bottom);
                }
                ctx.request_repaint();
            }
            _ => {
                match drop {
                    Some(Drop::DockTab(side)) => app.layout.dock_group(&key, side, true, None),
                    Some(Drop::Dock(side, index)) => app.layout.dock_group(&key, side, false, Some(index)),
                    _ => {}
                }
                app.dock.group_drag = None;
                app.layout.save();
            }
        }
    }
    let Some(d) = app.dock.drag.clone() else {
        app.dock.preview = None;
        return;
    };
    let Some(p) = pointer else { return };
    let win = app.dock.windows.iter().find(|(t, _)| *t == FloatTarget::Panel(d.id)).map(|(_, r)| *r);
    let drop = resolve(app, d.id, p, win, view);
    if down {
        let size = win.map_or(vec2(default_width(d.id), HEAD_H), |r| r.size());
        let at = clamp_float(p - d.grab, size, view, stage_bottom);
        app.layout.placements.insert(d.id, Placement::Float { x: at.x, y: at.y });
        draw_preview(app, ctx, &drop, view, stage_bottom);
        app.dock.preview = Some(drop);
        ctx.request_repaint();
        return;
    }
    // Released: land it.
    match drop {
        Drop::Float => {}
        Drop::Join(FloatTarget::Panel(target)) => {
            let r = app.dock.windows.iter().find(|(t, _)| *t == FloatTarget::Panel(target)).map_or(Rect::NOTHING, |(_, r)| *r);
            app.layout.join_group(d.id, target, r);
        }
        Drop::Join(FloatTarget::Group(key)) => {
            if let Some(first) = app.layout.groups.get(&key).and_then(|g| g.members.first().copied()) {
                app.layout.join_group(d.id, first, Rect::NOTHING);
            }
        }
        Drop::DockTab(side) => {
            let others = !open_docked(app, side).is_empty();
            app.layout.set_placement(d.id, Placement::Dock { side }, None);
            if others {
                app.layout.tabbed[side.i()] = true;
            }
            app.layout.set_active_dock_tab(side, d.id);
        }
        Drop::Dock(side, index) => app.layout.set_placement(d.id, Placement::Dock { side }, Some(index)),
    }
    set_panel_open(app, d.id, true);
    app.dock.drag = None;
    app.dock.preview = None;
    app.layout.save();
}

fn clamp_float(at: Pos2, size: Vec2, view: Rect, stage_bottom: f32) -> Pos2 {
    let m = layout::FLOAT_MARGIN;
    let x = at.x.clamp(view.left() + m, (view.right() - size.x - m).max(view.left() + m));
    let y = at.y.clamp(view.top() + m, (stage_bottom - HEAD_H - m).max(view.top() + m));
    pos2(x.round(), y.round())
}

fn draw_preview(app: &App, ctx: &egui::Context, drop: &Drop, view: Rect, stage_bottom: f32) {
    let t = Tokens::get(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("dock-preview")));
    let accent = Stroke::new(2.0, t.accent);
    match drop {
        Drop::Float => {}
        Drop::Join(target) => {
            if let Some((_, r)) = app.dock.windows.iter().find(|(w, _)| w == target) {
                painter.rect_stroke(*r, t.radius_lg, accent, egui::StrokeKind::Outside);
            }
        }
        Drop::DockTab(side) => {
            for (_, r) in app.dock.heads.iter().filter(|(s, _)| s == side) {
                painter.rect_filled(*r, 0.0, t.accent_soft);
            }
        }
        Drop::Dock(side, index) => {
            let w = layout::EDGE_BAND;
            let x = match side {
                DockSide::Left => view.left(),
                DockSide::Right => view.right() - w,
            };
            let band = Rect::from_min_size(pos2(x, view.top() + TOP), vec2(w, stage_bottom - view.top() - TOP));
            painter.rect_filled(band, 0.0, t.accent_soft);
            painter.rect_stroke(band, 0.0, accent, egui::StrokeKind::Inside);
            // The insertion line inside an existing dock.
            let heads: Vec<Rect> = app.dock.heads.iter().filter(|(s, _)| s == side).map(|(_, r)| *r).collect();
            if !heads.is_empty() {
                let y = heads.get(*index).map_or(stage_bottom - 2.0, |r| r.top());
                let xr = heads[0].x_range();
                painter.hline(xr, y, Stroke::new(3.0, t.accent));
            }
        }
    }
}

/// A popover body with a drag grip on top: pull it to float the panel,
/// then drop it on a dock or another window. The grip is laid out after
/// the body so it spans the popover without widening it.
pub fn with_grip(app: &mut App, ui: &mut Ui, id: PanelId, body: impl FnOnce(&mut App, &mut Ui)) {
    let t = Tokens::get(ui.ctx());
    let top = ui.cursor().top();
    ui.add_space(12.0);
    body(app, ui);
    let rect = Rect::from_min_max(pos2(ui.min_rect().left(), top), pos2(ui.min_rect().right(), top + 12.0));
    let r = ui.interact(rect, Id::new(("popover-grip", id)), Sense::drag());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, format!("Drag {}", id.label())));
    let r = r.on_hover_text(format!("Drag to float {} · drop on a side to dock", id.label())).on_hover_cursor(egui::CursorIcon::Grab);
    icons::paint(
        ui,
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        "grip-horizontal",
        14.0,
        if r.hovered() { t.fg_muted } else { t.fg_faint },
    );
    if r.drag_started()
        && let Some(p) = r.interact_pointer_pos()
    {
        let popover_min = pos2(rect.left(), top) - vec2(12.0, 12.0);
        begin_drag(app, id, p, p - popover_min);
    }
}

/// The edge band shown while the tool bar is dragged near a side.
pub fn rail_preview(ctx: &egui::Context, side: DockSide, view: Rect, stage_bottom: f32) {
    let t = Tokens::get(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("rail-preview")));
    let w = layout::EDGE_BAND;
    let x = match side {
        DockSide::Left => view.left(),
        DockSide::Right => view.right() - w,
    };
    let band = Rect::from_min_size(pos2(x, view.top() + TOP), vec2(w, stage_bottom - view.top() - TOP));
    painter.rect_filled(band, 0.0, t.accent_soft);
    painter.rect_stroke(band, 0.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
}
