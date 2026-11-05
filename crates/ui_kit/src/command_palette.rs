//! A command palette (happypaint `components/command-palette/`, v0.204.11):
//! a search field over every command, ranked as you type. ↑/↓ move, Enter
//! runs, Esc or a click outside closes. Commands are data; the caller runs
//! the one picked, as with the menu bar.

use crate::theme::{self, Tokens};
use crate::widgets::popover_frame;
use egui::{Align2, Area, Id, Key, Order, Pos2, Rect, Sense, Ui, pos2, vec2};

/// One searchable command.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaletteItem {
    pub title: String,
    /// Shown as a badge: Tools, Layer, Adjust…
    pub category: String,
    /// Where it lives in the menus ("Layer ▸ Masks"), searched and shown faint.
    pub path: String,
    pub shortcut: String,
    /// Other words it answers to ("bucket" for Fill).
    pub keywords: Vec<String>,
}

/// What the palette did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Open,
    /// Run `items[i]`; the palette has closed.
    Run(usize),
    Close,
}

#[derive(Clone, Debug, Default)]
struct State {
    query: String,
    active: usize,
    /// The active row moved by key: scroll it into view.
    scroll: bool,
}

fn state_id() -> Id {
    Id::new("onepaint-command-palette")
}

/// Start afresh: an empty query, the first row active. Call when opening.
pub fn reset(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<State>(state_id()));
}

const WIDTH: f32 = 560.0;
const ROW_H: f32 = 32.0;
const LIST_H: f32 = 360.0;

/// How well `it` answers `q` (lower-case, trimmed); lower is better.
fn rank(it: &PaletteItem, q: &str) -> Option<u32> {
    let t = it.title.to_lowercase();
    if t == q {
        return Some(0);
    }
    if t.starts_with(q) {
        return Some(1);
    }
    let word_start = |s: &str| s.split(|c: char| !c.is_alphanumeric()).any(|w| !w.is_empty() && w.starts_with(q));
    if word_start(&t) {
        return Some(2);
    }
    if t.contains(q) {
        return Some(3);
    }
    let keywords = it.keywords.iter().map(|k| k.to_lowercase()).collect::<Vec<_>>();
    if keywords.iter().any(|k| k.starts_with(q) || word_start(k)) {
        return Some(4);
    }
    // Several words: each must turn up somewhere (title, path, category, keywords).
    let hay = format!("{t} {} {} {}", it.path.to_lowercase(), it.category.to_lowercase(), keywords.join(" "));
    if q.split_whitespace().all(|w| hay.contains(w)) {
        return Some(5);
    }
    // Letters in order ("gblur" for Gaussian Blur).
    let mut chars = t.chars();
    if q.chars().filter(|c| !c.is_whitespace()).all(|c| chars.any(|x| x == c)) {
        return Some(6);
    }
    None
}

/// The items answering `query`, best first (ties keep their order). An empty
/// query lists everything.
pub fn search(items: &[PaletteItem], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..items.len()).collect();
    }
    let mut hits: Vec<(u32, usize)> = items.iter().enumerate().filter_map(|(i, it)| rank(it, &q).map(|r| (r, i))).collect();
    hits.sort();
    hits.into_iter().map(|(_, i)| i).collect()
}

/// Draw the palette with its top centre at `top`. Returns what happened.
pub fn show(ctx: &egui::Context, items: &[PaletteItem], top: Pos2) -> Outcome {
    let mut st = ctx.data(|d| d.get_temp::<State>(state_id())).unwrap_or_default();
    let hits = search(items, &st.query);
    let key = |k: Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
    if key(Key::Escape) {
        reset(ctx);
        return Outcome::Close;
    }
    if !hits.is_empty() {
        if key(Key::ArrowDown) {
            (st.active, st.scroll) = ((st.active + 1) % hits.len(), true);
        }
        if key(Key::ArrowUp) {
            (st.active, st.scroll) = ((st.active + hits.len() - 1) % hits.len(), true);
        }
    }
    st.active = st.active.min(hits.len().saturating_sub(1));
    if key(Key::Enter)
        && let Some(&i) = hits.get(st.active)
    {
        reset(ctx);
        return Outcome::Run(i);
    }
    let mut picked = None;
    let area = Area::new(Id::new(("popover", "command-palette"))).order(Order::Foreground).pivot(Align2::CENTER_TOP).fixed_pos(top).show(
        ctx,
        |ui| {
            popover_frame(ui).show(ui, |ui| {
                ui.set_width(WIDTH);
                let t = Tokens::get(ui.ctx());
                let field = egui::TextEdit::singleline(&mut st.query)
                    .hint_text("Search tools and commands")
                    .font(theme::medium(15.0))
                    .desired_width(f32::INFINITY)
                    .margin(vec2(10.0, 8.0));
                let r = ui.add(field);
                r.request_focus();
                if r.changed() {
                    st.active = 0;
                }
                ui.add_space(6.0);
                if hits.is_empty() {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("No matches").color(t.fg_faint));
                    ui.add_space(8.0);
                    return;
                }
                egui::ScrollArea::vertical().max_height(LIST_H).auto_shrink([false, true]).show(ui, |ui| {
                    for (n, &i) in hits.iter().enumerate() {
                        let active = n == st.active;
                        let resp = row(ui, &items[i], active);
                        if active && std::mem::take(&mut st.scroll) {
                            resp.scroll_to_me(None);
                        }
                        if resp.hovered() && resp.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                            st.active = n;
                        }
                        if resp.clicked() {
                            picked = Some(i);
                        }
                    }
                });
            })
        },
    );
    if let Some(i) = picked {
        reset(ctx);
        return Outcome::Run(i);
    }
    // A press outside closes it.
    let outside = ctx.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
    if outside {
        reset(ctx);
        return Outcome::Close;
    }
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
    Outcome::Open
}

fn row(ui: &mut Ui, it: &PaletteItem, active: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &it.title));
    if active {
        ui.painter().rect_filled(rect, t.radius_sm, t.accent_soft);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius_sm, t.hover);
    }
    let y = rect.center().y;
    let title = ui.painter().layout_no_wrap(it.title.clone(), egui::FontId::proportional(13.5), t.fg);
    let mut x = rect.left() + 10.0;
    let title_w = title.size().x;
    ui.painter().galley(pos2(x, y - title.size().y / 2.0), title, t.fg);
    x += title_w + 10.0;
    // The menu path, faint, when there is room before the shortcut.
    let shortcut = (!it.shortcut.is_empty()).then(|| ui.painter().layout_no_wrap(it.shortcut.clone(), theme::mono(11.0), t.fg_muted));
    let badge = ui.painter().layout_no_wrap(it.category.to_uppercase(), theme::medium(9.5), t.fg_muted);
    let right_w = shortcut.as_ref().map_or(0.0, |g| g.size().x + 14.0) + badge.size().x + 22.0;
    let room = rect.right() - right_w - x;
    if !it.path.is_empty() && room > 40.0 {
        let path = ui.painter().layout(it.path.clone(), egui::FontId::proportional(12.0), t.fg_faint, room);
        if path.rows.len() == 1 {
            ui.painter().galley(pos2(x, y - path.size().y / 2.0), path, t.fg_faint);
        }
    }
    let mut r = rect.right() - 10.0;
    if let Some(g) = shortcut {
        let w = g.size().x;
        let pill = Rect::from_min_max(pos2(r - w - 10.0, y - 9.0), pos2(r, y + 9.0));
        ui.painter().rect_filled(pill, t.radius_sm, t.field);
        ui.painter().galley(pos2(pill.left() + 5.0, y - g.size().y / 2.0), g, t.fg_muted);
        r = pill.left() - 6.0;
    }
    let w = badge.size().x;
    let chip = Rect::from_min_max(pos2(r - w - 12.0, y - 8.0), pos2(r, y + 8.0));
    ui.painter().rect_stroke(chip, t.radius_sm, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    ui.painter().galley(pos2(chip.left() + 6.0, y - badge.size().y / 2.0), badge, t.fg_muted);
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, category: &str, path: &str, keywords: &[&str]) -> PaletteItem {
        PaletteItem {
            title: title.into(),
            category: category.into(),
            path: path.into(),
            shortcut: String::new(),
            keywords: keywords.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn items() -> Vec<PaletteItem> {
        vec![
            item("Brush", "Tools", "", &[]),
            item("Eraser", "Tools", "", &["rubber"]),
            item("Fill (bucket)", "Tools", "", &["paint bucket", "flood"]),
            item("Gaussian Blur", "Filter", "Adjust ▸ Blur", &[]),
            item("Motion Blur", "Filter", "Adjust ▸ Blur", &[]),
            item("Lasso select", "Tools", "", &[]),
            item("Print preview", "View", "View ▸ Proof", &["cmyk", "soft proof"]),
            item("Add layer mask", "Layer", "Layer ▸ Masks", &[]),
        ]
    }

    fn titles(q: &str) -> Vec<String> {
        let v = items();
        search(&v, q).into_iter().map(|i| v[i].title.clone()).collect()
    }

    #[test]
    fn an_empty_query_lists_everything_in_order() {
        assert_eq!(search(&items(), "  "), (0..items().len()).collect::<Vec<_>>());
    }

    #[test]
    fn the_exact_title_ranks_first() {
        assert_eq!(titles("Eraser")[0], "Eraser");
        assert_eq!(titles("brush")[0], "Brush");
    }

    #[test]
    fn prefixes_words_and_keywords_match() {
        assert!(titles("lasso").contains(&"Lasso select".to_string()));
        assert_eq!(titles("bucket")[0], "Fill (bucket)");
        assert_eq!(titles("cmyk"), vec!["Print preview"]);
        assert_eq!(titles("rubber"), vec!["Eraser"]);
        let blur = titles("blur");
        assert_eq!(blur.len(), 2, "{blur:?}");
    }

    #[test]
    fn several_words_each_match_somewhere() {
        assert_eq!(titles("mask layer"), vec!["Add layer mask"]);
        assert_eq!(titles("blur adjust").len(), 2);
    }

    #[test]
    fn letters_in_order_match_last() {
        assert_eq!(titles("gblr"), vec!["Gaussian Blur"]);
        assert!(titles("zzz").is_empty());
    }
}
