//! Frame selection with batch retiming, and out of pegs (spec 24;
//! happypaint `animation-store.ts:589-850`, `animation-bar.tsx:734-940`).
//! Both are view state: the selection and peg mode live here, while the ops
//! and the pegs themselves go through the engine.

use crate::App;
use egui::RichText;
use onepaint_engine::Command;
use onepaint_engine::anim::Peg;
use onepaint_ui_kit::{Tokens, theme, widgets};

pub struct Timing {
    /// The ops bar is open; plain taps toggle frames.
    pub select_mode: bool,
    pub selected: Vec<String>,
    /// The Set / Split duration field.
    pub duration: u32,
    /// Canvas drags slide `peg_target`'s ghost instead of painting.
    pub peg_mode: bool,
    pub peg_target: Option<String>,
}

impl Default for Timing {
    fn default() -> Self {
        Self { select_mode: false, selected: Vec::new(), duration: 2, peg_mode: false, peg_target: None }
    }
}

/// Frame ids in timeline order and the one under the playhead.
fn frames(app: &App) -> (Vec<String>, Option<String>) {
    let slot = app.session.anim_slot();
    let fs = app.session.anim_frames();
    let cur = fs.iter().find(|f| slot >= f.start && slot < f.start + f.hold).map(|f| f.id.clone());
    (fs.into_iter().map(|f| f.id).collect(), cur)
}

impl Timing {
    /// What the ops act on: the live selection, else the current frame
    /// (`selectedFrameIds`).
    pub fn targets(&self, app: &App) -> Vec<String> {
        let (all, cur) = frames(app);
        let picked: Vec<String> = all.into_iter().filter(|f| self.selected.contains(f)).collect();
        if picked.is_empty() { cur.into_iter().collect() } else { picked }
    }

    /// Leaving select mode drops the selection (`setSelectMode`).
    pub fn set_select_mode(&mut self, on: bool) {
        self.select_mode = on;
        if !on {
            self.selected.clear();
        }
    }

    pub fn set_peg_mode(&mut self, on: bool) {
        self.peg_mode = on;
        if !on {
            self.peg_target = None;
        }
    }
}

/// A tap on a frame (`tapFrame`): Shift selects the range from the current
/// frame, Ctrl/⌘ (or any tap in select mode) toggles it. True when it
/// selected instead of navigating.
pub fn tap(app: &mut App, id: &str, mods: egui::Modifiers) -> bool {
    if mods.shift {
        let (all, cur) = frames(app);
        let from = cur.and_then(|c| all.iter().position(|f| *f == c));
        let to = all.iter().position(|f| f == id);
        if let (Some(a), Some(b)) = (from, to) {
            app.timing.set_select_mode(true);
            app.timing.selected = all[a.min(b)..=a.max(b)].to_vec();
        }
        return true;
    }
    if mods.command || app.timing.select_mode {
        app.timing.set_select_mode(true);
        let sel = &mut app.timing.selected;
        match sel.iter().position(|s| s == id) {
            Some(i) => {
                sel.remove(i);
            }
            None => sel.push(id.to_string()),
        }
        return true;
    }
    false
}

/// Drop ids that no longer exist (`afterStructureChange`).
fn prune(app: &mut App) {
    let (all, _) = frames(app);
    app.timing.selected.retain(|s| all.contains(s));
}

/// The timing ops bar, shown in select mode.
pub fn ops_bar(app: &mut App, ui: &mut egui::Ui) {
    if !app.timing.select_mode {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let n = app.timing.selected.len();
        let what = if n == 0 { "Current frame".to_string() } else { format!("{n} selected") };
        ui.label(RichText::new(what).font(theme::medium(11.0)).color(t.fg_muted));
        if widgets::secondary_button(ui, "All", 0.0).clicked() {
            app.timing.selected = frames(app).0;
        }
        if widgets::secondary_button(ui, "Invert", 0.0).clicked() {
            let sel = std::mem::take(&mut app.timing.selected);
            app.timing.selected = frames(app).0.into_iter().filter(|f| !sel.contains(f)).collect();
        }
        if widgets::secondary_button(ui, "Clear", 0.0).clicked() {
            app.timing.selected.clear();
        }
        widgets::vline(ui, 20.0);
        let mut d = app.timing.duration;
        ui.add(egui::DragValue::new(&mut d).range(1..=99).speed(0.1).suffix(" fr"));
        app.timing.duration = d;
        let ids = app.timing.targets(app);
        let run = |app: &mut App, cmd: Command| {
            let _ = app.session.execute(cmd);
            prune(app);
        };
        if widgets::secondary_button(ui, "Set duration", 0.0).on_hover_text("Hold each frame this long").clicked() {
            run(app, Command::SetFramesDuration { ids: ids.clone(), hold: d });
        }
        if widgets::secondary_button(ui, "Split", 0.0).on_hover_text("Cut long holds into sheets this long, same timing").clicked() {
            run(app, Command::SplitFrames { ids: ids.clone(), duration: d });
        }
        if widgets::secondary_button(ui, "In-betweens", 0.0).on_hover_text("An empty frame after each").clicked() {
            run(app, Command::AddInbetweens { ids: ids.clone() });
        }
        if widgets::secondary_button(ui, "Duplicate", 0.0).clicked() {
            run(app, Command::DuplicateFrames { ids: ids.clone() });
        }
        if widgets::secondary_button(ui, "Delete", 0.0).clicked() {
            run(app, Command::DeleteFrames { ids: ids.clone() });
        }
        widgets::vline(ui, 20.0);
        if widgets::primary_button(ui, "Done", 0.0).clicked() {
            app.timing.set_select_mode(false);
        }
    });
}

fn set_peg(app: &mut App, id: &str, f: impl FnOnce(&mut Peg)) {
    crate::timeline::set_settings(app, |s| {
        let p = s.out_of_pegs.entry(id.to_string()).or_default();
        f(p);
    });
}

/// Out of pegs (inside the onion menu): pick a ghost, then drag the canvas
/// to slide it, or turn and scale it here.
pub fn pegs_panel(app: &mut App, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    widgets::hairline(ui);
    let mut on = app.timing.peg_mode;
    if widgets::toggle(ui, &mut on, "Out of pegs (move ghosts)").changed() {
        app.timing.set_peg_mode(on);
        if on && app.timing.peg_target.is_none() {
            app.timing.peg_target = ghosts(app).first().map(|g| g.0.clone());
        }
    }
    if !app.timing.peg_mode {
        return;
    }
    let pegs = app.session.anim_settings().out_of_pegs.clone();
    ui.horizontal_wrapped(|ui| {
        for (id, d) in ghosts(app) {
            let label = format!("{}{d}{}", if d > 0 { "+" } else { "" }, if pegs.contains_key(&id) { " •" } else { "" });
            let sel = app.timing.peg_target.as_deref() == Some(id.as_str());
            if ui.selectable_label(sel, RichText::new(label).font(theme::mono(11.0))).clicked() {
                app.timing.peg_target = Some(id);
            }
        }
    });
    let Some(id) = app.timing.peg_target.clone() else {
        ui.label(RichText::new("Turn on onion skins to pick a ghost").font(theme::medium(10.5)).color(t.fg_faint));
        return;
    };
    ui.label(RichText::new("Drag the canvas to slide the ghost").font(theme::medium(10.5)).color(t.fg_faint));
    let p = pegs.get(&id).copied().unwrap_or_default();
    let mut deg = p.rot.to_degrees() as f32;
    if widgets::slider_row(ui, "Rotate", &mut deg, -180.0..=180.0, |v| format!("{v:.0}°")).changed() {
        set_peg(app, &id, |p| p.rot = (deg as f64).to_radians());
    }
    let mut pct = (p.scale * 100.0) as f32;
    if widgets::slider_row(ui, "Scale", &mut pct, 10.0..=400.0, |v| format!("{v:.0}%")).changed() {
        set_peg(app, &id, |p| p.scale = pct as f64 / 100.0);
    }
    ui.horizontal(|ui| {
        if ui.add_enabled_ui(pegs.contains_key(&id), |ui| widgets::secondary_button(ui, "Reset", 0.0)).inner.clicked() {
            crate::timeline::set_settings(app, |s| {
                s.out_of_pegs.remove(&id);
            });
        }
        if ui.add_enabled_ui(!pegs.is_empty(), |ui| widgets::secondary_button(ui, "Reset all", 0.0)).inner.clicked() {
            crate::timeline::set_settings(app, |s| s.out_of_pegs.clear());
        }
    });
}

/// The frames around the current one that show as ghosts, with their
/// distance (`onionGhosts`).
fn ghosts(app: &App) -> Vec<(String, i32)> {
    let s = app.session.anim_settings();
    if !s.onion_enabled {
        return Vec::new();
    }
    let (all, cur) = frames(app);
    let Some(at) = cur.and_then(|c| all.iter().position(|f| *f == c)) else { return Vec::new() };
    let mut out = Vec::new();
    for d in (1..=s.onion_before as i32).rev().map(|d| -d).chain(1..=s.onion_after as i32) {
        if s.onion_muted.contains(&d) {
            continue;
        }
        if let Some(f) = usize::try_from(at as i32 + d).ok().and_then(|i| all.get(i)) {
            out.push((f.clone(), d));
        }
    }
    out
}
