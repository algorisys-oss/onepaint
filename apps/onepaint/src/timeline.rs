//! Animation timeline (spec 24): a bottom bar with transport, onion settings
//! and the frame strip, plus the playback clock.

use crate::App;
use egui::{Align2, Area, Id, Order, RichText, pos2};
use onepaint_engine::Command;
use onepaint_engine::anim::{AnimSettings, LoopMode, step_slot};
use onepaint_ui_kit::{Tokens, icons, theme, widgets};

/// Playback clock: seconds banked toward the next frame and the ping-pong direction.
#[derive(Default)]
pub struct Playback {
    pub playing: bool,
    last: Option<f64>,
    acc: f64,
    dir: i32,
}

fn set_settings(app: &mut App, f: impl FnOnce(&mut AnimSettings)) {
    let mut s = app.session.anim_settings().clone();
    f(&mut s);
    let _ = app.session.execute(Command::SetAnimation { settings: Box::new(s) });
}

pub fn toggle(app: &mut App) {
    let on = !app.session.anim_settings().enabled;
    set_settings(app, |s| s.enabled = on);
    if !on {
        set_playing(app, false);
    }
}

pub fn set_playing(app: &mut App, playing: bool) {
    app.playback = Playback { playing, last: None, acc: 0.0, dir: 1 };
    let _ = app.session.execute(Command::SetAnimPlaying { playing });
}

pub fn step(app: &mut App, delta: i32) {
    let total = app.session.anim_total_slots();
    if total == 0 {
        return;
    }
    let slot = (app.session.anim_slot() as i64 + delta as i64).clamp(0, total as i64 - 1) as usize;
    let _ = app.session.execute(Command::SetAnimSlot { slot });
}

/// Advance playback by wall-clock time (call every frame).
pub fn tick(app: &mut App, ctx: &egui::Context) {
    if !app.playback.playing {
        return;
    }
    let now = ctx.input(|i| i.time);
    let s = app.session.anim_settings().clone();
    let total = app.session.anim_total_slots();
    if !s.enabled || total == 0 {
        set_playing(app, false);
        return;
    }
    let dt = app.playback.last.map_or(0.0, |l| now - l);
    app.playback.last = Some(now);
    app.playback.acc += dt;
    let period = 1.0 / s.fps.max(1.0);
    let mut slot = app.session.anim_slot();
    let mut moved = false;
    while app.playback.acc >= period {
        app.playback.acc -= period;
        let (next, dir, done) = step_slot(slot, 0, total - 1, s.loop_mode, app.playback.dir);
        (slot, app.playback.dir) = (next, dir);
        moved = true;
        if done {
            set_playing(app, false);
            break;
        }
    }
    if moved {
        let _ = app.session.execute(Command::SetAnimSlot { slot });
    }
    ctx.request_repaint_after(std::time::Duration::from_secs_f64((period - app.playback.acc).max(0.004)));
}

pub fn show(app: &mut App, ctx: &egui::Context, screen: egui::Rect) {
    if !app.session.anim_settings().enabled {
        return;
    }
    let t = Tokens::get(ctx);
    Area::new(Id::new("timeline"))
        .order(Order::Middle)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(screen.center().x, screen.bottom() - 12.0))
        .show(ctx, |ui| {
            widgets::glass_frame(ui).show(ui, |ui| {
                ui.set_max_width((screen.width() - 420.0).max(360.0));
                ui.horizontal(|ui| {
                    if icons::button(ui, "skip-back", 28.0, false, "First frame").clicked() {
                        let _ = app.session.execute(Command::SetAnimSlot { slot: 0 });
                    }
                    if icons::button(ui, "chevron-left", 28.0, false, "Previous frame (,)").clicked() {
                        step(app, -1);
                    }
                    let playing = app.playback.playing;
                    if icons::button(ui, if playing { "pause" } else { "play" }, 28.0, playing, if playing { "Pause" } else { "Play" })
                        .clicked()
                    {
                        set_playing(app, !playing);
                    }
                    if icons::button(ui, "chevron-right", 28.0, false, "Next frame (.)").clicked() {
                        step(app, 1);
                    }
                    if icons::button(ui, "skip-forward", 28.0, false, "Last frame").clicked() {
                        let last = app.session.anim_total_slots().saturating_sub(1);
                        let _ = app.session.execute(Command::SetAnimSlot { slot: last });
                    }
                    widgets::vline(ui, 22.0);
                    let s = app.session.anim_settings().clone();
                    let mut fps = s.fps;
                    if ui.add(egui::DragValue::new(&mut fps).range(1.0..=60.0).speed(0.2).suffix(" fps")).changed() {
                        set_settings(app, |x| x.fps = fps.round());
                    }
                    let modes = [(LoopMode::Loop, "Loop"), (LoopMode::Pingpong, "Ping-pong"), (LoopMode::Once, "Once")];
                    egui::ComboBox::from_id_salt("loop")
                        .width(90.0)
                        .selected_text(modes.iter().find(|m| m.0 == s.loop_mode).map_or("Loop", |m| m.1))
                        .show_ui(ui, |ui| {
                            for (m, name) in modes {
                                if ui.selectable_label(s.loop_mode == m, name).clicked() {
                                    set_settings(app, |x| x.loop_mode = m);
                                }
                            }
                        });
                    ui.menu_button(RichText::new(if s.onion_enabled { "Onion ●" } else { "Onion ○" }), |ui| {
                        let mut x = s.clone();
                        let mut changed = widgets::toggle(ui, &mut x.onion_enabled, "Onion skins").changed();
                        let mut b = x.onion_before as f32;
                        changed |= widgets::slider_row(ui, "Before", &mut b, 0.0..=8.0, |v| format!("{v:.0}")).changed();
                        let mut a = x.onion_after as f32;
                        changed |= widgets::slider_row(ui, "After", &mut a, 0.0..=8.0, |v| format!("{v:.0}")).changed();
                        let mut o = x.onion_opacity * 100.0;
                        changed |= widgets::slider_row(ui, "Opacity", &mut o, 0.0..=100.0, |v| format!("{v:.0}%")).changed();
                        changed |= widgets::toggle(ui, &mut x.onion_tint, "Tint (red before, green after)").changed();
                        changed |= widgets::toggle(ui, &mut x.onion_front, "Over the current frame").changed();
                        if changed {
                            (x.onion_before, x.onion_after, x.onion_opacity) = (b.round() as u32, a.round() as u32, o / 100.0);
                            let _ = app.session.execute(Command::SetAnimation { settings: Box::new(x) });
                        }
                    });
                    ui.label(
                        RichText::new(format!("{} / {}", app.session.anim_slot() + 1, app.session.anim_total_slots().max(1)))
                            .font(theme::mono(11.5))
                            .color(t.fg_muted),
                    );
                });
                ui.add_space(4.0);
                egui::ScrollArea::horizontal().id_salt("frames").max_width(ui.available_width()).show(ui, |ui| {
                    ui.horizontal(|ui| frames_strip(app, ui));
                });
            });
        });
}

fn frames_strip(app: &mut App, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let frames = app.session.anim_frames();
    let slot = app.session.anim_slot();
    let doc_size = [app.session.doc().width, app.session.doc().height];
    for (i, f) in frames.iter().enumerate() {
        let current = slot >= f.start && slot < f.start + f.hold;
        let w = 52.0 + (f.hold.min(6) as f32 - 1.0) * 14.0;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 54.0), egui::Sense::click());
        let stroke = if current { egui::Stroke::new(2.0, t.accent) } else { egui::Stroke::new(1.0, t.border) };
        ui.painter().rect(rect, 6.0, t.field, stroke, egui::StrokeKind::Inside);
        let thumb = egui::Rect::from_min_size(rect.min + egui::vec2(4.0, 4.0), egui::vec2(44.0, 33.0));
        widgets::checker(ui.painter(), thumb, 4.0);
        match app.session.doc().find(&f.id) {
            Some(onepaint_doc::Node::Layer(l)) => {
                let tex = app.thumbs.layer(ui.ctx(), l, doc_size);
                ui.painter().image(tex.id(), thumb, egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            _ => icons::paint(ui, thumb.shrink(6.0), "folder-open", 20.0, t.fg_muted),
        }
        let label = if f.hold > 1 { format!("{} ×{}", i + 1, f.hold) } else { format!("{}", i + 1) };
        ui.painter().text(
            pos2(rect.left() + 6.0, rect.bottom() - 4.0),
            Align2::LEFT_BOTTOM,
            label,
            theme::mono(10.5),
            if current { t.fg } else { t.fg_muted },
        );
        let id = f.id.clone();
        if resp.clicked() {
            let _ = app.session.execute(Command::SetActiveLayer { id: id.clone() });
            let _ = app.session.execute(Command::SetAnimSlot { slot: f.start });
        }
        resp.on_hover_text("Click to select · right-click for frame options").context_menu(|ui| {
            if ui.button("Hold +1").clicked() {
                let _ = app.session.execute(Command::SetFrameHold { id: id.clone(), hold: f.hold as u32 + 1 });
            }
            if ui.add_enabled(f.hold > 1, egui::Button::new("Hold −1")).clicked() {
                let _ = app.session.execute(Command::SetFrameHold { id: id.clone(), hold: f.hold as u32 - 1 });
            }
            ui.separator();
            if ui.button("Duplicate").clicked() {
                let _ = app.session.execute(Command::DuplicateLayer { id: id.clone() });
                ui.close();
            }
            if ui.button("Delete").clicked() {
                let _ = app.session.execute(Command::DeleteLayer { id: id.clone() });
                ui.close();
            }
            ui.separator();
            for (role, label) in [(Some("background"), "Make background"), (Some("foreground"), "Make foreground")] {
                if ui.button(label).clicked() {
                    let _ = app.session.execute(Command::SetFrameRole { id: id.clone(), role: role.map(String::from) });
                    ui.close();
                }
            }
        });
    }
    if icons::button(ui, "plus", 32.0, false, "Add frame").clicked() {
        let _ = app.session.execute(Command::AddFrame);
    }
    // Background / foreground nodes show under or over every frame.
    let roles: Vec<(String, String, String)> = app
        .session
        .doc()
        .root
        .iter()
        .filter_map(|n| onepaint_engine::anim::role_of(n).map(|r| (n.id().to_string(), n.name().to_string(), r.to_string())))
        .collect();
    for (id, name, role) in roles {
        let label = format!("{} {name}", if role == "background" { "BG" } else { "FG" });
        let r = ui.add(egui::Button::new(RichText::new(label).font(theme::medium(11.0)).color(t.fg_muted)));
        if r.clicked() {
            let _ = app.session.execute(Command::SetActiveLayer { id: id.clone() });
        }
        r.on_hover_text(format!("Shown under or over every frame ({role})")).context_menu(|ui| {
            if ui.button("Make a frame").clicked() {
                let _ = app.session.execute(Command::SetFrameRole { id: id.clone(), role: None });
                ui.close();
            }
        });
    }
}
