//! The tool bar (happypaint `components/toolbar/toolbar.tsx`): every tool
//! has a button. It floats where you drag it by its grip, flips between a
//! horizontal and a vertical grid (double-click the grip), docks to a side
//! as a full-height rail, and resizes by whole button columns. Narrow
//! windows turn it into a scrolling strip above the status bar.

use crate::layout::{self, DockSide, Orientation};
use crate::{App, Popover, dock};
use egui::{Align2, Area, Id, Order, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use onepaint_engine::Tool;
use onepaint_ui_kit::widgets::{self, glass_frame};
use onepaint_ui_kit::{Tokens, icons};

/// Button size; a cell is this plus the gap (`layout::TOOL_CELL`).
const BTN: f32 = 34.0;
const GAP: f32 = layout::TOOL_CELL - BTN;
/// The grip's thickness across the bar.
const GRIP: f32 = 14.0;
/// Margin from the window edges.
const M: f32 = 12.0;
/// Height of the compact strip.
const STRIP_H: f32 = 46.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Item {
    Tool(Tool, &'static str, &'static str),
    /// The five selection tools share one button; its popover picks the shape.
    Select,
    Animation,
    Adjustments,
    Sep,
}

const ITEMS: &[Item] = &[
    Item::Tool(Tool::Move, "move", "Move / transform (V)"),
    Item::Select,
    Item::Tool(Tool::Fill, "paint-bucket", "Fill (G)"),
    Item::Tool(Tool::Gradient, "droplet", "Gradient"),
    Item::Tool(Tool::Shape, "square", "Shape (U)"),
    Item::Tool(Tool::Pen, "pen-tool", "Pen (P)"),
    Item::Tool(Tool::Panel, "layout-panel-top", "Panel (manga)"),
    Item::Tool(Tool::Text, "type", "Text (T)"),
    Item::Tool(Tool::Eyedropper, "pipette", "Eyedropper (I)"),
    Item::Tool(Tool::Hand, "hand", PAN_LABEL),
    Item::Animation,
    Item::Adjustments,
    Item::Sep,
    Item::Tool(Tool::Brush, "brush", "Brush (B)"),
    Item::Tool(Tool::Eraser, "eraser", "Eraser (E)"),
    Item::Tool(Tool::Smudge, "fingerprint", "Smudge (S)"),
    Item::Tool(Tool::Mixer, "palette", "Mixer brush (Shift+B)"),
    Item::Tool(Tool::Liquify, "waves", "Liquify"),
    Item::Sep,
];

pub const PAN_LABEL: &str = "Pan (H) — drag to move the canvas. Hold Space with any tool, or drag with the middle mouse button";
pub const SELECT_LABEL: &str = "Select (M, L, W)";
pub const GRIP_LABEL: &str = "Drag to move · double-click to flip orientation";

/// Tools without a button of their own, and how they are reached.
#[cfg(test)]
pub const NO_BUTTON: &[(Tool, &str)] = &[
    (Tool::SelectRect, "The Select button; its popover picks the shape (M)."),
    (Tool::SelectEllipse, "The Select button; its popover picks the shape."),
    (Tool::SelectLasso, "The Select button; its popover picks the shape (L)."),
    (Tool::SelectPoly, "The Select button; its popover picks the shape."),
    (Tool::SelectWand, "The Select button; its popover picks the shape (W)."),
];

/// The tools with their own button, and its accessible label.
#[cfg(test)]
pub fn tool_buttons() -> Vec<(Tool, &'static str)> {
    ITEMS
        .iter()
        .filter_map(|i| match i {
            Item::Tool(t, _, l) => Some((*t, *l)),
            _ => None,
        })
        .collect()
}

fn button_count() -> usize {
    ITEMS.iter().filter(|i| **i != Item::Sep).count()
}

/// The rail's width: the user's columns, or two.
pub fn rail_width(app: &App) -> f32 {
    app.layout.toolbar.size.unwrap_or(layout::toolbar_width(2))
}

/// The tool bar's drag in progress: grab offset from its top-left.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolbarUi {
    drag: Option<Vec2>,
    /// Where the bar was drawn last frame.
    pub rect: Option<Rect>,
}

/// Draw the tool bar; `topbar_right` is where the top-left bar ends (the
/// default horizontal bar never reaches past it).
pub fn show(app: &mut App, ctx: &egui::Context, view: Rect, stage_bottom: f32, topbar_right: f32) -> Rect {
    if dock::compact(view) {
        return strip(app, ctx, view, stage_bottom);
    }
    let rail = app.layout.tools_dock;
    let res = dock::reserved(app, view);
    let vertical = rail.is_some() || app.layout.toolbar.orientation == Orientation::Vertical;
    let n = button_count();
    let user_cols = app.layout.toolbar.size.map(layout::toolbar_cols);
    let mut cols = user_cols.unwrap_or(if vertical { 2 } else { n }).min(n);
    let pos = app.layout.toolbar.pos;
    // Keep the default bar clear of the top-left bar.
    let room = if rail.is_none() && pos.is_none() && !vertical {
        view.right() - res.right - M - topbar_right - 8.0 - GRIP
    } else {
        view.width() - res.left - res.right - 2.0 * M
    };
    cols = cols.min(layout::toolbar_cols(room.max(layout::toolbar_width(1)))).max(1);
    let rows = rows_for(cols, vertical);
    let grid = vec2(cols as f32 * layout::TOOL_CELL - GAP, rows.len() as f32 * layout::TOOL_CELL - GAP);
    let size = if vertical { grid + vec2(12.0, 12.0 + GRIP) } else { grid + vec2(12.0 + GRIP, 12.0) };
    let (at, pivot) = match (rail, pos) {
        // The rail sits at the window edge, outside any dock.
        (Some(DockSide::Left), _) => (pos2(view.left(), view.top() + dock::TOP), Align2::LEFT_TOP),
        (Some(DockSide::Right), _) => (pos2(view.right(), view.top() + dock::TOP), Align2::RIGHT_TOP),
        (None, Some(p)) => (clamp(pos2(p[0], p[1]), size, view, stage_bottom), Align2::LEFT_TOP),
        (None, None) => {
            let y = view.top() + M;
            (pos2(view.right() - res.right - M, y), Align2::RIGHT_TOP)
        }
    };
    let rail_h = stage_bottom - view.top() - dock::TOP;
    let t = Tokens::get(ctx);
    let r = Area::new(Id::new("toolbar")).order(Order::Middle).pivot(pivot).fixed_pos(at).show(ctx, |ui| {
        let mut frame = glass_frame(ui).inner_margin(6.0);
        if rail.is_some() {
            frame = frame.corner_radius(0.0);
        }
        frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(GAP, GAP);
            if rail.is_some() {
                ui.set_min_height(rail_h - 12.0);
                ui.set_width(rail_width(app) - 12.0);
            }
            let body = |ui: &mut Ui, app: &mut App| {
                for row in &rows {
                    ui.horizontal(|ui| {
                        for it in row {
                            if *it == Item::Sep {
                                widgets::vline(ui, 24.0);
                            } else {
                                item(app, ui, *it, view);
                            }
                        }
                    });
                }
            };
            // "With the tools": the size and opacity sliders ride in the bar.
            let sliders = app.settings.side_sliders && app.settings.side_sliders_mode == crate::side_sliders::Mode::Tools;
            if vertical {
                ui.vertical(|ui| {
                    grip(app, ui, vec2(grid.x, GRIP), "grip-horizontal");
                    body(ui, app);
                    if sliders {
                        widgets::hairline(ui);
                        ui.vertical_centered(|ui| crate::side_sliders::body(app, ui, true));
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    grip(app, ui, vec2(GRIP, grid.y.max(BTN)), "grip-vertical");
                    ui.vertical(|ui| body(ui, app));
                    if sliders {
                        widgets::vline(ui, 24.0);
                        crate::side_sliders::body(app, ui, false);
                    }
                });
            }
        });
    });
    let rect = r.response.rect;
    if app.settings.side_sliders && app.settings.side_sliders_mode == crate::side_sliders::Mode::Tools {
        crate::side_sliders::entry_popover(app, ctx, vertical);
    }
    resize_handle(app, ctx, rect, rail.is_some());
    drag_tick(app, ctx, rect, view, stage_bottom);
    let _ = t;
    app.toolbar.rect = Some(rect);
    rect
}

/// Rows of items: `cols` per row; a vertical bar starts each group on a
/// new row, and a one-row bar keeps its separators.
fn rows_for(cols: usize, vertical: bool) -> Vec<Vec<Item>> {
    let one_row = !vertical && cols >= button_count();
    let mut rows: Vec<Vec<Item>> = vec![Vec::new()];
    for it in ITEMS {
        let row = rows.last_mut().expect("rows starts non-empty");
        if *it == Item::Sep {
            if one_row {
                row.push(*it);
            } else if vertical && !row.is_empty() {
                rows.push(Vec::new());
            }
            continue;
        }
        if row.iter().filter(|i| **i != Item::Sep).count() >= cols {
            rows.push(vec![*it]);
        } else {
            row.push(*it);
        }
    }
    rows.retain(|r| !r.is_empty());
    rows
}

fn clamp(p: Pos2, size: Vec2, view: Rect, stage_bottom: f32) -> Pos2 {
    let x = p.x.clamp(view.left() + 4.0, (view.right() - size.x - 4.0).max(view.left() + 4.0));
    let y = p.y.clamp(view.top() + 4.0, (stage_bottom - size.y - 4.0).max(view.top() + 4.0));
    pos2(x, y)
}

fn grip(app: &mut App, ui: &mut Ui, size: Vec2, icon: &str) {
    let t = Tokens::get(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(size, Sense::click_and_drag());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, GRIP_LABEL));
    let r = r.on_hover_text(GRIP_LABEL).on_hover_cursor(egui::CursorIcon::Grab);
    icons::paint(
        ui,
        Rect::from_center_size(rect.center(), vec2(14.0, 14.0)),
        icon,
        14.0,
        if r.hovered() { t.fg_muted } else { t.fg_faint },
    );
    if r.double_clicked() && app.layout.tools_dock.is_none() {
        let o = &mut app.layout.toolbar.orientation;
        *o = if *o == Orientation::Vertical { Orientation::Horizontal } else { Orientation::Vertical };
        app.layout.save();
    }
    if r.drag_started()
        && let (Some(p), Some(bar)) = (r.interact_pointer_pos(), app.toolbar.rect)
    {
        // Tear a rail off where it stands.
        if app.layout.tools_dock.take().is_some() {
            app.layout.toolbar.orientation = Orientation::Vertical;
        }
        app.layout.toolbar.pos = Some([bar.left(), bar.top()]);
        app.toolbar.drag = Some(p - bar.min);
    }
}

fn drag_tick(app: &mut App, ctx: &egui::Context, rect: Rect, view: Rect, stage_bottom: f32) {
    let Some(grab) = app.toolbar.drag else { return };
    let (pointer, down) = ctx.input(|i| (i.pointer.latest_pos(), i.pointer.primary_down()));
    let Some(p) = pointer else { return };
    let local = pos2(p.x - view.left(), p.y - view.top());
    let side = layout::drop_dock_side(local, Some(rect.translate(-view.min.to_vec2())), view.size());
    if down {
        let at = clamp(p - grab, rect.size(), view, stage_bottom);
        app.layout.toolbar.pos = Some([at.x, at.y]);
        if let Some(side) = side {
            dock::rail_preview(ctx, side, view, stage_bottom);
        }
        ctx.request_repaint();
    } else {
        app.layout.tools_dock = side;
        app.toolbar.drag = None;
        app.layout.save();
    }
}

/// The grip on the trailing edge: drag to set whole columns; double-click
/// for the automatic width.
fn resize_handle(app: &mut App, ctx: &egui::Context, rect: Rect, rail: bool) {
    let t = Tokens::get(ctx);
    let x = if rail && app.layout.tools_dock == Some(DockSide::Right) { rect.left() } else { rect.right() - 6.0 };
    let h = if rail { 60.0 } else { rect.height().min(42.0) };
    let handle = Rect::from_center_size(pos2(x + 3.0, rect.center().y), vec2(6.0, h));
    Area::new(Id::new("toolbar-resize")).order(Order::Middle).fixed_pos(handle.min).show(ctx, |ui| {
        let r = ui
            .interact(handle, Id::new("toolbar-resize-grip"), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Resize the tool bar"));
        if r.hovered() || r.dragged() {
            ui.painter().rect_filled(handle.shrink2(vec2(1.5, 8.0)), 2.0, t.accent);
        }
        if r.dragged()
            && let Some(p) = r.interact_pointer_pos()
        {
            let w = if app.layout.tools_dock == Some(DockSide::Right) { rect.right() - p.x } else { p.x - rect.left() };
            let cols = layout::toolbar_cols(w).min(button_count());
            app.layout.toolbar.size = Some(layout::toolbar_width(cols));
        }
        if r.drag_stopped() {
            app.layout.save();
        }
        if r.double_clicked() {
            app.layout.toolbar.size = None;
            app.layout.save();
        }
    });
}

/// Narrow windows: one scrolling row above the status bar.
fn strip(app: &mut App, ctx: &egui::Context, view: Rect, stage_bottom: f32) -> Rect {
    let at = pos2(view.left(), stage_bottom);
    let r = Area::new(Id::new("toolbar")).order(Order::Middle).pivot(Align2::LEFT_BOTTOM).fixed_pos(at).show(ctx, |ui| {
        glass_frame(ui).corner_radius(0.0).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            // The frame adds its margin and a 1 px stroke on each side.
            ui.set_width(view.width() - 18.0);
            ui.set_height(STRIP_H - 12.0);
            egui::ScrollArea::horizontal().id_salt("toolbar-strip").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = GAP;
                    for it in ITEMS {
                        if *it == Item::Sep {
                            widgets::vline(ui, 24.0);
                        } else {
                            item(app, ui, *it, view);
                        }
                    }
                });
            });
        });
    });
    app.toolbar.rect = Some(r.response.rect);
    r.response.rect
}

/// Where popovers open: beside the tool bar, on the canvas side.
pub fn popover_anchor(app: &App, view: Rect) -> (Pos2, Align2) {
    let Some(r) = app.toolbar.rect else {
        return (pos2(view.right() - M, view.top() + 64.0), Align2::RIGHT_TOP);
    };
    let wide = r.width() >= r.height();
    let right_half = r.center().x > view.center().x;
    if dock::compact(view) {
        return (pos2(view.right() - 8.0, r.top() - 8.0), Align2::RIGHT_BOTTOM);
    }
    match (wide, right_half) {
        (true, true) => (pos2(r.right(), r.bottom() + 8.0), Align2::RIGHT_TOP),
        (true, false) => (pos2(r.left(), r.bottom() + 8.0), Align2::LEFT_TOP),
        (false, true) => (pos2(r.left() - 8.0, r.top()), Align2::RIGHT_TOP),
        (false, false) => (pos2(r.right() + 8.0, r.top()), Align2::LEFT_TOP),
    }
}

fn item(app: &mut App, ui: &mut Ui, it: Item, view: Rect) {
    let tool = app.session.tool();
    match it {
        Item::Tool(t, icon, label) => {
            if icons::button(ui, icon, BTN, tool == t, label).clicked() {
                // Again on the brush or eraser: the brushes. Other tools'
                // options are in the options row (spec 65).
                if tool == t && matches!(t, Tool::Brush | Tool::Eraser) {
                    dock::toggle(app, Popover::Brushes, view);
                }
                app.set_tool(t);
            }
        }
        Item::Select => {
            let selecting = crate::chrome::is_select_tool(tool);
            let icon = crate::chrome::select_icon(app.last_select_tool);
            if icons::button(ui, icon, BTN, selecting, SELECT_LABEL).clicked() {
                if selecting {
                    dock::toggle(app, Popover::Selection, view);
                } else {
                    let t = app.last_select_tool;
                    app.set_tool(t);
                }
            }
        }
        Item::Animation => {
            if icons::button(ui, "film", BTN, app.session.anim_settings().enabled, "Animation").clicked() {
                crate::timeline::toggle(app);
            }
        }
        Item::Adjustments => {
            let on = matches!(app.popover, Some(Popover::Adjustments | Popover::Filter));
            if icons::button(ui, "sparkles", BTN, on, "Adjustments").clicked() {
                if app.session.filter_active() {
                    app.popover = Some(Popover::Filter);
                } else {
                    app.popover = if app.popover == Some(Popover::Adjustments) { None } else { Some(Popover::Adjustments) };
                }
            }
        }
        Item::Sep => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_has_a_button_or_a_written_exemption() {
        let buttons = tool_buttons();
        for t in Tool::ALL {
            let has = buttons.iter().any(|(b, _)| *b == t);
            let exempt = NO_BUTTON.iter().find(|(e, _)| *e == t);
            assert!(has ^ exempt.is_some(), "{t:?}: a button or an exemption, not both or neither");
            if let Some((_, why)) = exempt {
                assert!(why.len() > 20, "{t:?}: say how it is reached");
            }
        }
        assert!(buttons.iter().any(|(t, _)| *t == Tool::Panel) && buttons.iter().any(|(t, _)| *t == Tool::Hand));
    }

    #[test]
    fn rows_wrap_by_columns_and_groups() {
        let n = button_count();
        assert_eq!(rows_for(n, false).len(), 1, "one row keeps everything");
        assert!(rows_for(n, false)[0].contains(&Item::Sep));
        let two = rows_for(2, true);
        assert!(two.iter().all(|r| r.len() <= 2 && !r.contains(&Item::Sep)));
        let total: usize = two.iter().map(|r| r.len()).sum();
        assert_eq!(total, n, "nothing lost");
        // The vertical bar starts the brush group on a fresh row.
        assert!(two.iter().any(|r| r.first() == Some(&Item::Tool(Tool::Brush, "brush", "Brush (B)"))));
    }
}
