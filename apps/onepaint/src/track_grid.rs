//! The exposure-sheet grid (happypaint `track-grid.tsx`): a ruler and one
//! row per track, top-first like the Layers panel. Click a sheet to draw on
//! it, drag it to retime (clamped between its neighbours), double-click a
//! gap for a new drawing there.

use crate::App;
use egui::{Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use onepaint_engine::Command;
use onepaint_engine::anim::{is_tracks_doc, tracks_of};
use onepaint_ui_kit::{Tokens, icons, theme, widgets};

const LABEL_W: f32 = 132.0;
const ROW_H: f32 = 24.0;
const RULER_H: f32 = 18.0;
const MIN_COL: f32 = 14.0;

/// A sheet drag in progress: its id, start column, press x, and the column
/// width at the press (the grid rescales as the timeline grows; a live
/// width would run the sheet away from the pointer).
#[derive(Clone)]
struct SheetDrag {
    id: String,
    start: usize,
    x: f32,
    col: f32,
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    if !is_tracks_doc(&app.session.doc().root) {
        if widgets::secondary_button(ui, "Convert to tracks", 130.0)
            .on_hover_text("One row per track, each with its own drawings and timing")
            .clicked()
        {
            notify(app, ui, Command::ConvertToTracks);
        }
        return;
    }
    let tracks = tracks_of(&app.session.doc().root);
    let total = app.session.anim_total_slots().max(1);
    let slot = app.session.anim_slot();
    let active = app.session.active().to_string();
    let anim = app.session.anim_settings().clone();
    let (lo, hi) = anim.play_range(total);
    let width = ui.available_width().max(LABEL_W + MIN_COL * 4.0);
    let col = ((width - LABEL_W) / total as f32).max(MIN_COL);
    let grid_w = LABEL_W + col * total as f32;

    ui.horizontal(|ui| {
        if ui.button(RichText::new("+ Track").font(theme::medium(11.0))).clicked() {
            notify(app, ui, Command::AddTrack);
        }
        let track_of_active =
            tracks.iter().find(|tr| tr.id == active || tr.sheets.as_ref().is_some_and(|s| s.iter().any(|f| f.id == active)));
        if let Some(tr) = track_of_active.filter(|tr| tr.sheets.is_some())
            && ui.button(RichText::new("+ Drawing").font(theme::medium(11.0))).on_hover_text("A new drawing at the playhead").clicked()
        {
            notify(app, ui, Command::AddSheet { track: tr.id.clone(), column: slot });
        }
    });
    egui::ScrollArea::both().id_salt("track-grid").max_height(190.0).show(ui, |ui| {
        let rows = tracks.len();
        let (rect, _) = ui.allocate_exact_size(vec2(grid_w, RULER_H + rows as f32 * (ROW_H + 2.0)), Sense::hover());
        let x_of = |c: usize| rect.left() + LABEL_W + c as f32 * col;
        let col_at = |x: f32| (((x - rect.left() - LABEL_W) / col).floor().max(0.0) as usize).min(total - 1);
        let painter = ui.painter_at(rect);

        // Ruler: scrub on press and drag.
        let ruler = Rect::from_min_size(rect.min, vec2(grid_w, RULER_H));
        let r = ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Timeline ruler"));
        if let Some(p) = r.interact_pointer_pos().filter(|_| r.clicked() || r.dragged()) {
            let _ = app.session.execute(Command::SetAnimSlot { slot: col_at(p.x) });
        }
        painter.text(pos2(rect.left() + 4.0, ruler.center().y), egui::Align2::LEFT_CENTER, "Tracks", theme::medium(11.0), t.fg_muted);
        for c in 0..total {
            let cell = Rect::from_min_size(pos2(x_of(c), rect.top()), vec2(col - 1.0, RULER_H));
            if c == slot {
                painter.rect_filled(cell, 2.0, t.accent);
            } else if anim.has_mark_range() && (c < lo || c > hi) {
                painter.rect_filled(cell, 0.0, t.hover);
            }
            if c % 5 == 0 || total <= 12 {
                let fg = if c == slot { t.on_accent } else { t.fg_faint };
                painter.text(cell.center(), egui::Align2::CENTER_CENTER, format!("{}", c + 1), theme::mono(9.5), fg);
            }
            if let Some(m) = anim.markers.iter().find(|m| m.slot == c) {
                let mc = onepaint_color::Rgba::from_hex(&m.color).map_or(t.accent, widgets::to_color32);
                painter.rect_filled(Rect::from_min_size(cell.min, vec2(cell.width(), 3.0)), 0.0, mc);
            }
        }

        // Rows, top-first.
        for (row, tr) in tracks.iter().rev().enumerate() {
            let y = rect.top() + RULER_H + 2.0 + row as f32 * (ROW_H + 2.0);
            let name = app.session.doc().find(&tr.id).map_or_else(String::new, |n| n.name().to_string());
            let current = tr.id == active || tr.sheets.as_ref().is_some_and(|s| s.iter().any(|f| f.id == active));
            let label = Rect::from_min_size(pos2(rect.left(), y), vec2(LABEL_W - 4.0, ROW_H));
            let lr = ui.interact(label, ui.id().with(("track", &tr.id)), Sense::click());
            lr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Track {name}")));
            painter.rect_filled(
                label,
                4.0,
                if current {
                    t.accent_soft
                } else if lr.hovered() {
                    t.hover
                } else {
                    t.field
                },
            );
            painter.text(label.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, &name, theme::medium(11.0), t.fg);
            if lr.clicked() {
                pick_track(app, tr, slot);
            }
            let Some(sheets) = &tr.sheets else {
                let body = Rect::from_min_size(pos2(x_of(0), y), vec2(col * total as f32 - 1.0, ROW_H));
                painter.rect_filled(body, 3.0, t.bg_elevated);
                painter.text(body.center(), egui::Align2::CENTER_CENTER, "every frame", theme::medium(10.5), t.fg_faint);
                continue;
            };
            // Track buttons: up / down / delete, at the label's right end.
            let mut bx = label.right() - 18.0;
            for (icon, tip, cmd) in [
                ("trash", "Delete track", Command::DeleteTrack { id: tr.id.clone() }),
                ("chevron-down", "Move track down", Command::MoveTrack { id: tr.id.clone(), delta: -1 }),
                ("chevron-up", "Move track up", Command::MoveTrack { id: tr.id.clone(), delta: 1 }),
            ] {
                let b = Rect::from_center_size(pos2(bx, label.center().y), vec2(16.0, 16.0));
                let br = ui.interact(b, ui.id().with(("tb", &tr.id, icon)), Sense::click());
                br.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{tip} {name}")));
                icons::paint(ui, b, icon, 13.0, if br.hovered() { t.fg } else { t.fg_faint });
                if br.on_hover_text(tip).clicked() {
                    notify(app, ui, cmd);
                }
                bx -= 18.0;
            }
            // Gaps: click moves the playhead (and picks the track), double-click adds a drawing.
            for c in 0..total {
                if sheets.iter().any(|f| c >= f.start && c < f.start + f.hold) {
                    continue;
                }
                let g = Rect::from_min_size(pos2(x_of(c), y), vec2(col - 1.0, ROW_H));
                let gr = ui.interact(g, ui.id().with(("gap", &tr.id, c)), Sense::click());
                gr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{name} column {}", c + 1)));
                if gr.hovered() {
                    painter.rect_filled(g, 2.0, t.hover);
                }
                if gr.double_clicked() {
                    notify(app, ui, Command::AddSheet { track: tr.id.clone(), column: c });
                } else if gr.clicked() {
                    pick_track(app, tr, c);
                    let _ = app.session.execute(Command::SetAnimSlot { slot: c });
                }
            }
            // Playhead behind the cells.
            painter.rect_filled(Rect::from_min_size(pos2(x_of(slot), y), vec2(col - 1.0, ROW_H)), 0.0, t.accent_soft);
            // Sheets: click to draw on, drag to retime.
            for (i, f) in sheets.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(x_of(f.start), y), vec2(col * f.hold as f32 - 1.0, ROW_H));
                let cr = ui.interact(cell, ui.id().with(("sheet", &f.id)), Sense::click_and_drag());
                cr.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Sheet {} of {name}", i + 1)));
                let sheet_name = app.session.doc().find(&f.id).map_or_else(String::new, |n| n.name().to_string());
                let cr = cr.on_hover_text(format!("{sheet_name} · frames {}–{}", f.start + 1, f.start + f.hold));
                let under = slot >= f.start && slot < f.start + f.hold;
                painter.rect_filled(cell, 3.0, if f.id == active { t.accent_soft } else { t.field });
                painter.rect_stroke(
                    cell,
                    3.0,
                    Stroke::new(if f.id == active { 2.0 } else { 1.0 }, if under || f.id == active { t.accent } else { t.border }),
                    egui::StrokeKind::Inside,
                );
                painter.text(cell.left_center() + vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, format!("{}", i + 1), theme::mono(10.0), t.fg);
                let key = ui.id().with("sheet-drag");
                if cr.drag_started()
                    && let Some(p) = cr.interact_pointer_pos()
                {
                    ui.data_mut(|d| d.insert_temp(key, SheetDrag { id: f.id.clone(), start: f.start, x: p.x, col }));
                }
                if cr.dragged()
                    && let (Some(d), Some(p)) = (ui.data(|d| d.get_temp::<SheetDrag>(key)), cr.interact_pointer_pos())
                    && d.id == f.id
                {
                    let delta = ((p.x - d.x) / d.col).round() as i64;
                    let _ = app.session.execute(Command::MoveSheet { id: d.id.clone(), column: d.start as i64 + delta });
                }
                if cr.drag_stopped() {
                    ui.data_mut(|d| d.remove::<SheetDrag>(key));
                    let _ = app.session.execute(Command::SealHistory);
                }
                if cr.clicked() {
                    let _ = app.session.execute(Command::SetActiveLayer { id: f.id.clone() });
                    let _ = app.session.execute(Command::SetAnimSlot { slot: f.start });
                }
            }
        }
    });
}

/// Activate a track: the sheet at `slot`, else its first sheet, else the track itself.
fn pick_track(app: &mut App, tr: &onepaint_engine::anim::Track, slot: usize) {
    let id = match &tr.sheets {
        Some(s) => s.iter().find(|f| slot >= f.start && slot < f.start + f.hold).or(s.first()).map_or(tr.id.clone(), |f| f.id.clone()),
        None => tr.id.clone(),
    };
    let _ = app.session.execute(Command::SetActiveLayer { id });
}

/// Run a command; a refusal is shown as a notice.
fn notify(app: &mut App, ui: &Ui, cmd: Command) {
    if let Err(e) = app.session.execute(cmd) {
        let now = ui.input(|i| i.time);
        app.notify(e.to_string(), now);
    }
}
