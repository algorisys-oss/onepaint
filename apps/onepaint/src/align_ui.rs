//! Align & distribute (happypaint `components/align-panel`): line up the
//! layers picked in the Layers panel (Ctrl/⌘-click, Shift-click) to the
//! canvas or the selection, space their centres evenly, or even out the gaps.

use crate::App;
use egui::{RichText, Ui};
use onepaint_engine::Command;
use onepaint_engine::align::{self, AlignMode, Axis};
use onepaint_ui_kit::{Tokens, icons, theme, widgets};

const ALIGN_BUTTONS: [(AlignMode, &str, &str); 6] = [
    (AlignMode::Left, "align-start-vertical", "Align left edges"),
    (AlignMode::HCenter, "align-center-vertical", "Centre horizontally"),
    (AlignMode::Right, "align-end-vertical", "Align right edges"),
    (AlignMode::Top, "align-start-horizontal", "Align top edges"),
    (AlignMode::VCenter, "align-center-horizontal", "Centre vertically"),
    (AlignMode::Bottom, "align-end-horizontal", "Align bottom edges"),
];

#[derive(Clone, Copy)]
enum Op {
    Align(AlignMode),
    Distribute(Axis),
    Space(Axis),
}

/// Click a layer row with modifiers: Ctrl/⌘ toggles it into the picks
/// (the first time with the active layer too), Shift picks the range from
/// the anchor, a plain click clears them (`layer-panel.tsx:487-516`).
pub fn pick_row(app: &mut App, id: &str, rows: &[String], ctrl: bool, shift: bool) {
    let active = app.session.active().to_string();
    if shift {
        let anchor = app.pick_anchor.clone().unwrap_or(active);
        let (a, b) = (rows.iter().position(|r| *r == anchor), rows.iter().position(|r| r == id));
        if let (Some(a), Some(b)) = (a, b) {
            let (lo, hi) = (a.min(b), a.max(b));
            app.layer_picks = rows[lo..=hi].to_vec();
        }
    } else if ctrl {
        if app.layer_picks.is_empty() {
            app.layer_picks.push(active);
        }
        if let Some(i) = app.layer_picks.iter().position(|p| p == id) {
            app.layer_picks.remove(i);
        } else {
            app.layer_picks.push(id.to_string());
        }
        app.pick_anchor = Some(id.to_string());
    } else {
        app.layer_picks.clear();
        app.pick_anchor = Some(id.to_string());
    }
}

/// The panel body.
pub fn panel(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    crate::dock::fit_width(ui, 240.0);
    app.layer_picks.retain(|id| app.session.doc().find(id).is_some());
    // With nothing multi-selected the panel acts on the active layer, as the
    // Move tool does (happypaint v0.204.1): a tap makes a layer active, not
    // picked, and a tablet has no Ctrl/Shift-click.
    let using_active = app.layer_picks.is_empty();
    let picks = if using_active { vec![app.session.active().to_string()] } else { app.layer_picks.clone() };
    let subjects = app.session.transformable_ids(&picks);
    let count = subjects.len();
    let mut op = None;
    widgets::section_label(ui, "Align");
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(count >= 1, |ui| {
            for (mode, icon, label) in ALIGN_BUTTONS {
                if icons::button(ui, icon, 30.0, false, label).clicked() {
                    op = Some(Op::Align(mode));
                }
            }
        });
    });
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            widgets::section_label(ui, "Distribute centres");
            ui.horizontal(|ui| {
                ui.add_enabled_ui(count >= 3, |ui| {
                    if icons::button(
                        ui,
                        "align-horizontal-distribute-center",
                        30.0,
                        false,
                        "Space the centres evenly across (needs 3+ layers)",
                    )
                    .clicked()
                    {
                        op = Some(Op::Distribute(Axis::Horizontal));
                    }
                    if icons::button(ui, "align-vertical-distribute-center", 30.0, false, "Space the centres evenly down (needs 3+ layers)")
                        .clicked()
                    {
                        op = Some(Op::Distribute(Axis::Vertical));
                    }
                });
            });
        });
        ui.add_space(8.0);
        ui.vertical(|ui| {
            widgets::section_label(ui, "Even gaps");
            ui.horizontal(|ui| {
                ui.add_enabled_ui(count >= 3, |ui| {
                    if icons::button(
                        ui,
                        "align-horizontal-space-around",
                        30.0,
                        false,
                        "Equal gaps left to right — differs from distribute when the layers are different widths",
                    )
                    .clicked()
                    {
                        op = Some(Op::Space(Axis::Horizontal));
                    }
                    if icons::button(
                        ui,
                        "align-vertical-space-around",
                        30.0,
                        false,
                        "Equal gaps top to bottom — differs from distribute when the layers are different heights",
                    )
                    .clicked()
                    {
                        op = Some(Op::Space(Axis::Vertical));
                    }
                });
            });
        });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Align to").color(t.fg_muted));
        if let Some(i) = crate::chrome::segmented(ui, &["Canvas", "Selection"], usize::from(app.align_to_selection)) {
            app.align_to_selection = i == 1;
        }
    });
    let hint = if count == 0 {
        "Choose a visible, unlocked layer in the Layers panel to align it.".to_string()
    } else if using_active {
        "Aligning the current layer. To align several, Ctrl/⌘- or Shift-click their rows.".to_string()
    } else {
        format!("{count} layer{} selected · moved by their artwork's bounding box", if count == 1 { "" } else { "s" })
    };
    ui.label(RichText::new(hint).font(theme::medium(10.5)).color(t.fg_faint));
    if let Some(op) = op {
        run(app, ui, op, &subjects);
    }
}

fn run(app: &mut App, ui: &Ui, op: Op, subjects: &[String]) {
    let now = ui.input(|i| i.time);
    let items = app.session.layer_content_bounds(subjects);
    if items.is_empty() {
        app.notify("Select layers with artwork on them", now);
        return;
    }
    if app.canvas.xform != onepaint_engine::transform::TransformParams::default() {
        app.notify("Finish the move first (Enter), then align", now);
        return;
    }
    let (moves, done) = match op {
        Op::Align(mode) => {
            // One layer aligned to its own bounds never moves: say what would help.
            if app.align_to_selection && items.len() == 1 && app.session.selection_bounds().is_none() {
                app.notify("Draw a selection to align to, or set Align to: Canvas", now);
                return;
            }
            let target = if app.align_to_selection {
                app.session.selection_bounds().or_else(|| align::union_bounds(&items))
            } else {
                Some(app.session.doc().bounds())
            };
            let Some(target) = target else { return };
            (align::align_offsets(&items, mode, target), "Aligned")
        }
        Op::Distribute(axis) | Op::Space(axis) if items.len() < 3 => {
            let _ = axis;
            let msg = if matches!(op, Op::Distribute(_)) {
                "Distributing needs three layers or more"
            } else {
                "Even spacing needs three layers or more"
            };
            app.notify(msg, now);
            return;
        }
        Op::Distribute(axis) => (align::distribute_offsets(&items, axis), "Distributed"),
        Op::Space(axis) => (align::spacing_offsets(&items, axis), "Spacing evened"),
    };
    if moves.is_empty() {
        app.notify("Nothing to move", now);
        return;
    }
    if moves.iter().all(|m| (m.dx, m.dy) == (0, 0)) {
        app.notify("Already aligned", now);
        return;
    }
    let _ = app.session.execute(Command::AlignLayers { moves });
    app.notify(done, now);
}
