//! Help ▸ Manual (spec 65 phase 6; happypaint `routes/docs.tsx`): the
//! manual (`assets/manual.md`) in a window, its contents on the left and
//! one section at a time on the right. The text is a small Markdown subset:
//! `#`/`##` headings, paragraphs, `- ` lists, `**bold**` and `` `keys` ``.

use crate::App;
use egui::text::{LayoutJob, TextFormat};
use egui::{RichText, Ui};
use onepaint_ui_kit::{Tokens, theme, widgets};

const SOURCE: &str = include_str!("../assets/manual.md");

#[derive(Debug, PartialEq)]
pub enum Block {
    Para(String),
    List(Vec<String>),
}

#[derive(Debug)]
pub struct Section {
    pub title: String,
    pub blocks: Vec<Block>,
}

/// The manual's sections, in order (the `#` title line is not one).
pub fn sections() -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut para = String::new();
    let mut list: Vec<String> = Vec::new();
    fn flush(out: &mut [Section], para: &mut String, list: &mut Vec<String>) {
        let Some(s) = out.last_mut() else {
            para.clear();
            list.clear();
            return;
        };
        if !para.trim().is_empty() {
            s.blocks.push(Block::Para(std::mem::take(para).trim().to_string()));
        }
        para.clear();
        if !list.is_empty() {
            s.blocks.push(Block::List(std::mem::take(list)));
        }
    }
    for line in SOURCE.lines() {
        if let Some(t) = line.strip_prefix("## ") {
            flush(&mut out, &mut para, &mut list);
            out.push(Section { title: t.trim().to_string(), blocks: Vec::new() });
        } else if line.starts_with("# ") {
            continue;
        } else if let Some(item) = line.strip_prefix("- ") {
            if !para.trim().is_empty() {
                flush(&mut out, &mut para, &mut list);
            }
            list.push(item.trim().to_string());
        } else if line.trim().is_empty() {
            flush(&mut out, &mut para, &mut list);
        } else if !list.is_empty() {
            // A wrapped list item continues on the next line.
            if let Some(last) = list.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(line.trim());
        }
    }
    flush(&mut out, &mut para, &mut list);
    out
}

/// How a run of text shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    Body,
    Bold,
    Key,
}

/// `text` split into runs: `**bold**`, `` `key` ``, and the rest (an
/// unclosed marker is plain text).
pub fn runs(text: &str) -> Vec<(&str, Style)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let Some(at) = [rest.find("**"), rest.find('`')].into_iter().flatten().min() else {
            out.push((rest, Style::Body));
            break;
        };
        if at > 0 {
            out.push((&rest[..at], Style::Body));
        }
        rest = &rest[at..];
        let (mark, style) = if rest.starts_with("**") { ("**", Style::Bold) } else { ("`", Style::Key) };
        let after = &rest[mark.len()..];
        match after.find(mark) {
            Some(end) => {
                out.push((&after[..end], style));
                rest = &after[end + mark.len()..];
            }
            None => {
                out.push((rest, Style::Body));
                break;
            }
        }
    }
    out
}

/// One paragraph laid out with its runs styled.
fn inline(ui: &Ui, text: &str, width: f32) -> LayoutJob {
    let t = Tokens::get(ui.ctx());
    let mut job = LayoutJob { wrap: egui::text::TextWrapping { max_width: width, ..Default::default() }, ..Default::default() };
    for (s, style) in runs(text) {
        let fmt = match style {
            Style::Body => TextFormat { font_id: egui::FontId::proportional(13.5), color: t.fg, ..Default::default() },
            Style::Bold => TextFormat { font_id: theme::semibold(13.5), color: t.fg, ..Default::default() },
            Style::Key => TextFormat { font_id: theme::mono(12.0), color: t.fg, background: t.field, ..Default::default() },
        };
        job.append(s, 0.0, fmt);
    }
    job
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let all = sections();
    let at = app.manual_section.min(all.len().saturating_sub(1));
    let h = (ui.ctx().content_rect().height() - 140.0).clamp(320.0, 720.0);
    ui.set_width(800.0);
    ui.horizontal(|ui| {
        widgets::section_label(ui, "Manual");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if onepaint_ui_kit::icons::button(ui, "x", 24.0, false, "Close the manual").clicked() {
                app.popover = None;
            }
        });
    });
    ui.horizontal_top(|ui| {
        // Contents.
        ui.vertical(|ui| {
            ui.set_width(210.0);
            egui::ScrollArea::vertical().id_salt("manual-toc").max_height(h).show(ui, |ui| {
                for (i, s) in all.iter().enumerate() {
                    let text = RichText::new(&s.title).color(if i == at { t.accent } else { t.fg_muted });
                    if ui.add(egui::Button::new(text).frame(false).selected(i == at)).clicked() {
                        app.manual_section = i;
                    }
                }
            });
        });
        widgets::vline(ui, h);
        // The section.
        ui.vertical(|ui| {
            let w = 560.0;
            ui.set_width(w);
            egui::ScrollArea::vertical().id_salt(("manual-body", at)).max_height(h - 40.0).show(ui, |ui| {
                let Some(s) = all.get(at) else { return };
                ui.label(RichText::new(&s.title).font(theme::semibold(20.0)).color(t.fg));
                ui.add_space(8.0);
                for b in &s.blocks {
                    match b {
                        Block::Para(p) => {
                            ui.add(egui::Label::new(inline(ui, p, w - 16.0)).wrap());
                        }
                        Block::List(items) => {
                            for item in items {
                                ui.horizontal_top(|ui| {
                                    ui.label(RichText::new("•").color(t.fg_muted));
                                    ui.add(egui::Label::new(inline(ui, item, w - 36.0)).wrap());
                                });
                            }
                        }
                    }
                    ui.add_space(8.0);
                }
            });
            ui.horizontal(|ui| {
                ui.add_enabled_ui(at > 0, |ui| {
                    if widgets::secondary_button(ui, "Previous", 90.0).clicked() {
                        app.manual_section = at - 1;
                    }
                });
                ui.add_enabled_ui(at + 1 < all.len(), |ui| {
                    if widgets::secondary_button(ui, "Next", 90.0).clicked() {
                        app.manual_section = at + 1;
                    }
                });
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manual_parses_into_its_sections() {
        let s = sections();
        assert!(s.len() >= 20, "{} sections", s.len());
        assert_eq!(s[0].title, "Overview");
        assert!(s.iter().all(|x| !x.blocks.is_empty()), "every section has text");
        let ws = s.iter().find(|x| x.title == "The workspace").unwrap();
        assert!(matches!(&ws.blocks[1], Block::List(items) if items.len() == 4 && items[0].starts_with("**The header**")));
        // Runs: bold and keys styled, an unclosed marker left as it is.
        assert_eq!(
            runs("a **b** `c` d"),
            vec![("a ", Style::Body), ("b", Style::Bold), (" ", Style::Body), ("c", Style::Key), (" d", Style::Body)]
        );
        assert_eq!(runs("x `y"), vec![("x ", Style::Body), ("`y", Style::Body)]);
    }
}
