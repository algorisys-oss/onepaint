//! The Layers panel's bar for the picked layers (happypaint
//! `layer-panel__multibar`): with layers Ctrl/⌘- or Shift-clicked it offers
//! Group, Merge (scattered picks too, v0.208.3), Duplicate, Lock, Hide, a
//! colour label and Delete for all of them at once, each one undo step.

use crate::App;
use egui::{RichText, Ui};
use onepaint_engine::{Command, LayerProps};
use onepaint_ui_kit::{Tokens, theme, widgets};

#[derive(Clone)]
enum Act {
    Group,
    Merge,
    Duplicate,
    Lock(bool),
    Show(bool),
    Label(Option<String>),
    Delete,
    Cancel,
}

/// The picks in panel order (top first), dropping any that no longer exist.
fn ordered(app: &mut App) -> Vec<String> {
    let order = app.session.panel_order();
    app.layer_picks.retain(|id| order.contains(id));
    order.into_iter().filter(|id| app.layer_picks.contains(id)).collect()
}

/// The picks not already inside a picked group, which travel with it.
fn outermost(app: &App, picks: &[String]) -> Vec<String> {
    let doc = app.session.doc();
    picks
        .iter()
        .filter(|id| {
            let mut p = doc.parent_of(id).flatten();
            while let Some(g) = p {
                if picks.contains(&g) {
                    return false;
                }
                p = doc.parent_of(&g).flatten();
            }
            true
        })
        .cloned()
        .collect()
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let picks = ordered(app);
    if picks.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let doc = app.session.doc();
    let layers: Vec<&String> = picks.iter().filter(|id| doc.layer(id).is_some()).collect();
    let all_locked = !layers.is_empty() && layers.iter().all(|id| doc.layer(id).is_some_and(|l| l.locked));
    let all_hidden = picks.iter().all(|id| doc.find(id).is_some_and(|n| !n.visible()));
    let labels: Vec<Option<String>> = picks.iter().map(|id| app.session.color_label(id)).collect();
    let shared = labels.iter().all(|l| *l == labels[0]).then(|| labels[0].clone());
    let mergeable = app.session.mergeable_run(&picks).is_some();
    let mut act = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{} selected", picks.len())).font(theme::medium(12.0)).color(t.fg_muted));
        ui.add_enabled_ui(picks.len() >= 2, |ui| {
            if widgets::icon_label_button(ui, "folder-plus", "Group", false, true).on_hover_text("Group the selected layers").clicked() {
                act = Some(Act::Group);
            }
        });
        ui.add_enabled_ui(mergeable, |ui| {
            let tip = if mergeable { "Merge the selected layers into one" } else { "Merge needs two or more paint layers selected" };
            if widgets::icon_label_button(ui, "merge", "Merge", false, true).on_hover_text(tip).on_disabled_hover_text(tip).clicked() {
                act = Some(Act::Merge);
            }
        });
        if widgets::icon_label_button(ui, "copy", "Duplicate", false, true).on_hover_text("Duplicate the selected layers").clicked() {
            act = Some(Act::Duplicate);
        }
        ui.add_enabled_ui(!layers.is_empty(), |ui| {
            let (icon, label) = if all_locked { ("lock-open", "Unlock") } else { ("lock", "Lock") };
            if widgets::icon_label_button(ui, icon, label, false, true).clicked() {
                act = Some(Act::Lock(!all_locked));
            }
        });
        let (icon, label) = if all_hidden { ("eye", "Show") } else { ("eye-off", "Hide") };
        if widgets::icon_label_button(ui, icon, label, false, true).clicked() {
            act = Some(Act::Show(all_hidden));
        }
        let current = shared.as_ref().map(|l| l.as_deref());
        if let Some(l) = crate::chrome::label_swatches(ui, current) {
            act = Some(Act::Label(l));
        }
        if widgets::icon_label_button(ui, "trash", "Delete", false, true).on_hover_text("Delete the selected layers").clicked() {
            act = Some(Act::Delete);
        }
        if widgets::icon_label_button(ui, "x", "Cancel", false, true).on_hover_text("Clear the selection").clicked() {
            act = Some(Act::Cancel);
        }
    });
    widgets::hairline(ui);
    if let Some(a) = act {
        run(app, ui, a, &picks);
    }
}

fn run(app: &mut App, ui: &Ui, act: Act, picks: &[String]) {
    let batch = |commands: Vec<Command>| Command::Batch { commands };
    let props = |f: &dyn Fn(&mut LayerProps)| {
        let mut p = LayerProps::default();
        f(&mut p);
        p
    };
    let (cmd, clear) = match act {
        Act::Group => (Some(Command::GroupLayers { ids: picks.to_vec() }), true),
        Act::Merge => (Some(Command::MergeLayers { ids: picks.to_vec() }), true),
        Act::Duplicate => {
            let ids = outermost(app, picks);
            (Some(batch(ids.into_iter().rev().map(|id| Command::DuplicateLayer { id }).collect())), true)
        }
        Act::Lock(on) => {
            // Groups can't be locked.
            let ids = picks.iter().filter(|id| app.session.doc().layer(id).is_some_and(|l| l.locked != on));
            let cmds = ids.map(|id| Command::SetLayerProps { id: id.clone(), props: props(&|p| p.locked = Some(on)) }).collect();
            (Some(batch(cmds)), false)
        }
        Act::Show(on) => {
            let ids = picks.iter().filter(|id| app.session.doc().find(id).is_some_and(|n| n.visible() != on));
            let cmds = ids.map(|id| Command::SetLayerProps { id: id.clone(), props: props(&|p| p.visible = Some(on)) }).collect();
            (Some(batch(cmds)), false)
        }
        Act::Label(label) => {
            let cmds = picks.iter().map(|id| Command::SetColorLabel { id: id.clone(), label: label.clone() }).collect();
            (Some(batch(cmds)), false)
        }
        Act::Delete => {
            let ids = outermost(app, picks);
            (Some(batch(ids.into_iter().map(|id| Command::DeleteLayer { id }).collect())), true)
        }
        Act::Cancel => (None, true),
    };
    if let Some(cmd) = cmd
        && let Err(e) = app.session.execute(cmd)
    {
        let now = ui.input(|i| i.time);
        app.notify(e.to_string(), now);
    }
    if clear {
        app.layer_picks.clear();
    }
}
