//! Transform & Align (happypaint `components/align-panel`). The Transform
//! section types exact X / Y / W / H and an angle into the Move tool's live
//! box (v0.209.0; the maths is `engine::transform_numeric`): editing a field
//! arms the Move tool, so the result is the same pending transform a handle
//! drag makes, applied with Enter / Apply and dropped with Esc / Cancel.
//! The Align section lines up the layers picked in the Layers panel
//! (Ctrl/⌘-click, Shift-click) to the canvas or the selection, spaces their
//! centres evenly, or evens out the gaps.

use crate::App;
use egui::{RichText, Ui};
use onepaint_engine::align::{self, AlignMode, Axis};
use onepaint_engine::transform::TransformParams;
use onepaint_engine::transform_numeric::FieldsPatch;
use onepaint_engine::{Command, Tool};
use onepaint_raster::Rect;
use onepaint_ui_kit::inline::inline_number;
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
    transform_section(app, ui);
    widgets::hairline(ui);
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

/// What a Move session on the active layer would lift: its artwork, inside
/// the selection when there is one (`Session::begin_transform`).
fn idle_box(app: &App) -> Option<Rect> {
    let active = app.session.active().to_string();
    let (_, content) = app.session.layer_content_bounds(&[active]).into_iter().next()?;
    match app.session.clip_mask().and_then(|m| m.bounds()) {
        Some((x, y, w, h)) => content.intersect(&Rect::new(x as i32, y as i32, w as i32, h as i32)),
        None => Some(content),
    }
}

/// Transform: typed position, size and angle for the live box.
fn transform_section(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    widgets::section_label(ui, "Transform");
    let doc = [f64::from(app.session.doc().width), f64::from(app.session.doc().height)];
    // Distort and Warp move corners freely: there are no such numbers.
    let busy = app.session.transform_warp().is_some();
    let live = app.session.transform_state().map(|(_, b)| b).filter(|_| !busy);
    // Before a Move session the fields show the active layer's box as it stands.
    let shown = match live {
        Some(b) => Some(app.canvas.xform.fields(doc, b)),
        None if !busy => idle_box(app).map(|b| TransformParams::default().fields(doc, b)),
        None => None,
    };
    let mut v = shown.unwrap_or_default();
    let mut patch = FieldsPatch::default();
    let far = -1.0e6..=1.0e6;
    let size = 1.0..=1.0e6;
    ui.add_enabled_ui(shown.is_some(), |ui| {
        ui.horizontal(|ui| {
            if inline_number(ui, "X", &mut v.x, far.clone(), " px", 2).on_hover_text("Left edge of the box on the canvas").changed() {
                patch.x = Some(v.x);
            }
            if inline_number(ui, "Y", &mut v.y, far.clone(), " px", 2).on_hover_text("Top edge of the box on the canvas").changed() {
                patch.y = Some(v.y);
            }
        });
        ui.horizontal(|ui| {
            if inline_number(ui, "W", &mut v.w, size.clone(), " px", 2).on_hover_text("Width in pixels").changed() {
                patch.w = Some(v.w);
            }
            if inline_number(ui, "H", &mut v.h, size.clone(), " px", 2).on_hover_text("Height in pixels").changed() {
                patch.h = Some(v.h);
            }
            let linked = app.xform_link_aspect;
            let (icon, tip) = if linked {
                ("link-2", "Width and height linked: changing one keeps the aspect ratio")
            } else {
                ("link-2-off", "Width and height unlinked: tap to keep the aspect ratio")
            };
            if icons::button(ui, icon, 24.0, linked, tip).clicked() {
                app.xform_link_aspect = !linked;
            }
        });
        ui.horizontal(|ui| {
            let r = inline_number(ui, "Angle", &mut v.angle, -180.0..=180.0, "°", 1);
            if r.on_hover_text("Rotation in degrees, about the box centre").changed() {
                patch.angle = Some(v.angle);
            }
        });
    });
    if patch != FieldsPatch::default() {
        set_fields(app, patch);
    }
    let pending = app.session.transform_active() && (busy || !app.canvas.xform.is_identity());
    if pending {
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Apply", 64.0).on_hover_text("Apply the transform (Enter)").clicked() {
                app.commit_transform();
            }
            if widgets::secondary_button(ui, "Cancel", 64.0).on_hover_text("Cancel the transform (Esc)").clicked() {
                app.cancel_transform();
            }
        });
    }
    let hint = if busy {
        Some("Distort and Warp move corners freely; apply them to type numbers again.")
    } else if shown.is_none() {
        Some("Choose a layer with artwork on it to transform.")
    } else {
        None
    };
    if let Some(h) = hint {
        ui.label(RichText::new(h).font(theme::medium(10.5)).color(t.fg_faint));
    }
}

/// Typing is a transform: it arms the Move tool, whose session the numbers then edit.
pub fn set_fields(app: &mut App, patch: FieldsPatch) {
    if app.session.tool() != Tool::Move {
        app.set_tool(Tool::Move);
    } else if !app.session.transform_active() {
        app.begin_transform();
    }
    if app.session.transform_warp().is_some() {
        return;
    }
    let Some((_, b)) = app.session.transform_state() else { return };
    let doc = [f64::from(app.session.doc().width), f64::from(app.session.doc().height)];
    let next = app.canvas.xform.with_fields(doc, b, patch, app.xform_link_aspect);
    app.canvas.set_xform(&mut app.session, next);
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
