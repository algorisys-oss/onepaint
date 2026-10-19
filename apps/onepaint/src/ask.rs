//! In-app questions (happypaint `ask-dialog`, v0.199.3): a confirm before
//! something can't be undone, or a name prompt. One at a time; asking again
//! replaces the open one. Enter answers, Escape or the backdrop cancels.

use crate::App;
use egui::RichText;
use onepaint_ui_kit::{Tokens, theme, widgets};

/// What the answer does.
#[derive(Clone, Debug, PartialEq)]
pub enum AskFor {
    DeletePalette(String),
    SaveSelection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    pub title: String,
    pub message: Option<String>,
    pub confirm: String,
    /// Destructive: the button is red and Cancel takes the focus.
    pub danger: bool,
    /// A name prompt's text (`None` for a plain confirm).
    pub text: Option<String>,
    pub what: AskFor,
}

impl Ask {
    pub fn confirm(title: impl Into<String>, message: &str, confirm: &str, danger: bool, what: AskFor) -> Self {
        Ask { title: title.into(), message: Some(message.into()), confirm: confirm.into(), danger, text: None, what }
    }

    pub fn text(title: impl Into<String>, initial: String, confirm: &str, what: AskFor) -> Self {
        Ask { title: title.into(), message: None, confirm: confirm.into(), danger: false, text: Some(initial), what }
    }
}

/// A name prompt's answer: trimmed, `None` when blank (`askText`).
pub fn answer(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Show the open question and act on its answer.
pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(mut ask) = app.ask.take() else { return };
    let t = Tokens::get(ctx);
    let id = egui::Id::new("ask");
    let mut done: Option<bool> = None;
    let modal = egui::Modal::new(id).backdrop_color(t.shadow).frame(egui::Frame::NONE).show(ctx, |ui| {
        widgets::popover_frame(ui).show(ui, |ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(&ask.title).font(theme::semibold(15.0)).color(t.fg));
            if let Some(m) = &ask.message {
                ui.add_space(4.0);
                ui.label(RichText::new(m).color(t.fg_muted));
            }
            if let Some(text) = ask.text.as_mut() {
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                if !ui.data(|d| d.get_temp::<bool>(id.with("focused")).unwrap_or(false)) {
                    r.request_focus();
                    ui.data_mut(|d| d.insert_temp(id.with("focused"), true));
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = Some(true);
                }
            } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                // A danger confirm answers Enter with Cancel, its focused button.
                done = Some(!ask.danger);
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = if ask.danger {
                    widgets::danger_button(ui, &ask.confirm, 88.0)
                } else {
                    widgets::primary_button(ui, &ask.confirm, 88.0)
                };
                if ok.clicked() {
                    done = Some(true);
                }
                if widgets::secondary_button(ui, "Cancel", 88.0).clicked() {
                    done = Some(false);
                }
            });
        });
    });
    if modal.should_close() {
        done = done.or(Some(false));
    }
    match done {
        None => app.ask = Some(ask),
        Some(yes) => {
            ctx.data_mut(|d| d.remove::<bool>(id.with("focused")));
            if yes {
                answered(app, ctx, ask);
            }
        }
    }
}

fn answered(app: &mut App, ctx: &egui::Context, ask: Ask) {
    let now = ctx.input(|i| i.time);
    match ask.what {
        AskFor::DeletePalette(id) => app.palettes.delete(&id),
        AskFor::SaveSelection => {
            let Some(name) = ask.text.as_deref().and_then(answer) else { return };
            match app.session.execute(onepaint_engine::Command::SaveSelection { name: name.clone() }) {
                Ok(()) => app.notify(format!("Saved “{name}”"), now),
                Err(e) => app.notify(e.to_string(), now),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // happypaint ask.test.ts: answers are trimmed, blank is no answer.
    #[test]
    fn a_name_is_trimmed_and_blank_is_none() {
        assert_eq!(super::answer("  New name  ").as_deref(), Some("New name"));
        assert_eq!(super::answer("   "), None);
    }
}
