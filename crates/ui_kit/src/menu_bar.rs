//! A menu bar (happypaint `components/menu-bar/`): menus as data, titles in
//! a row. Click a title to open its menu; while one is open, hovering
//! another title switches to it. ←/→ move between menus, ↑/↓ between
//! rows, Enter picks, Esc or a click outside closes. Toggles show ✓,
//! submenus open beside their row. The caller owns what an item does: the
//! bar returns the id of the item picked.

use crate::theme::{self, Tokens};
use egui::{Align2, Area, Id, Key, Order, Pos2, Rect, Sense, Ui, pos2, vec2};

/// One menu row.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub id: u32,
    pub label: String,
    pub shortcut: String,
    /// `Some` for a toggle: shows ✓ when on.
    pub checked: Option<bool>,
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(id: u32, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), shortcut: String::new(), checked: None, enabled: true }
    }

    pub fn shortcut(mut self, s: impl Into<String>) -> Self {
        self.shortcut = s.into();
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        self.checked = Some(on);
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MenuEntry {
    Item(MenuItem),
    Sep,
    /// A small caption over the rows below it.
    Header(String),
    /// A row that opens its own list beside it.
    Sub(String, Vec<MenuEntry>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Menu {
    pub title: String,
    pub entries: Vec<MenuEntry>,
}

#[derive(Clone, Debug, Default)]
struct State {
    open: Option<usize>,
    /// The submenu row open in the menu, by entry index.
    sub: Option<usize>,
    /// The row the keyboard is on, by entry index.
    focus: Option<usize>,
    /// Where the open lists were drawn last frame (clicks there stay inside).
    rects: Vec<Rect>,
}

fn state_id() -> Id {
    Id::new("onepaint-menu-bar")
}

/// Whether a menu is open (its click outside should close it, not paint).
pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<State>(state_id())).is_some_and(|s| s.open.is_some())
}

/// Open menu `i` (a debug hook and tests).
pub fn open(ctx: &egui::Context, i: usize) {
    ctx.data_mut(|d| d.insert_temp(state_id(), State { open: Some(i), ..Default::default() }));
}

/// Close any open menu.
pub fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(state_id(), State::default()));
}

const ROW_H: f32 = 28.0;
const TITLE_H: f32 = 26.0;

fn pickable(e: &MenuEntry) -> bool {
    match e {
        MenuEntry::Item(i) => i.enabled,
        MenuEntry::Sub(..) => true,
        _ => false,
    }
}

/// The next pickable row after `from` (`step` ±1), wrapping.
fn step_focus(entries: &[MenuEntry], from: Option<usize>, step: i32) -> Option<usize> {
    let n = entries.len() as i32;
    if n == 0 {
        return None;
    }
    let mut i = from.map_or(if step > 0 { -1 } else { n }, |f| f as i32);
    for _ in 0..n {
        i = (i + step).rem_euclid(n);
        if pickable(&entries[i as usize]) {
            return Some(i as usize);
        }
    }
    None
}

/// The menu titles; returns the id of the item picked this frame.
pub fn menu_bar(ui: &mut Ui, menus: &[Menu]) -> Option<u32> {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let mut st: State = ctx.data(|d| d.get_temp(state_id())).unwrap_or_default();
    let mut picked = None;
    let mut title_rects = Vec::with_capacity(menus.len());
    for (i, m) in menus.iter().enumerate() {
        let galley = ui.painter().layout_no_wrap(m.title.clone(), theme::medium(13.0), t.fg);
        let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, TITLE_H), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &m.title));
        let open = st.open == Some(i);
        if resp.clicked() {
            st.open = if open { None } else { Some(i) };
            st.sub = None;
            st.focus = None;
        } else if st.open.is_some() && !open && resp.hovered() {
            st.open = Some(i);
            st.sub = None;
            st.focus = None;
        }
        let fill = if st.open == Some(i) {
            Some(t.pressed)
        } else if resp.hovered() {
            Some(t.hover)
        } else {
            None
        };
        if let Some(f) = fill {
            ui.painter().rect_filled(rect, t.radius_sm, f);
        }
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.fg);
        title_rects.push(rect);
    }
    let Some(open) = st.open.filter(|&i| i < menus.len()) else {
        ctx.data_mut(|d| d.insert_temp(state_id(), State::default()));
        return None;
    };
    let entries = &menus[open].entries;
    // Keys: taken here so the app's shortcuts don't see them.
    let key = |k: Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, k));
    if key(Key::Escape) {
        ctx.data_mut(|d| d.insert_temp(state_id(), State::default()));
        return None;
    }
    if key(Key::ArrowRight) {
        match st.focus.map(|f| &entries[f]) {
            Some(MenuEntry::Sub(..)) if st.sub != st.focus => st.sub = st.focus,
            _ => {
                st.open = Some((open + 1) % menus.len());
                st.focus = None;
                st.sub = None;
            }
        }
    }
    if key(Key::ArrowLeft) {
        if st.sub.is_some() {
            st.sub = None;
        } else {
            st.open = Some((open + menus.len() - 1) % menus.len());
            st.focus = None;
        }
    }
    if key(Key::ArrowDown) {
        st.focus = step_focus(entries, st.focus, 1);
    }
    if key(Key::ArrowUp) {
        st.focus = step_focus(entries, st.focus, -1);
    }
    let enter = key(Key::Enter);
    if st.open != Some(open) {
        ctx.data_mut(|d| d.insert_temp(state_id(), st));
        return None;
    }
    if enter && let Some(f) = st.focus {
        match &entries[f] {
            MenuEntry::Item(it) if it.enabled => picked = Some(it.id),
            MenuEntry::Sub(..) => st.sub = Some(f),
            _ => {}
        }
    }
    // The open list, under its title.
    let at = title_rects[open].left_bottom() + vec2(0.0, 4.0);
    let mut rects = title_rects.clone();
    let (list, sub_row) = list_area(&ctx, Id::new(("menu-list", open)), at, entries, st.focus, st.sub, &mut picked, &mut st);
    rects.push(list);
    if let (Some(s), Some(row)) = (st.sub, sub_row)
        && let Some(MenuEntry::Sub(_, items)) = entries.get(s)
    {
        let mut ignore = st.clone();
        let (r, _) =
            list_area(&ctx, Id::new(("menu-sub", open, s)), row.right_top() + vec2(4.0, -6.0), items, None, None, &mut picked, &mut ignore);
        rects.push(r);
    }
    // A click outside the titles and lists closes.
    let press = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
    if let Some(p) = press
        && !st.rects.iter().chain(&rects).any(|r| r.contains(p))
    {
        st = State::default();
    }
    if picked.is_some() {
        st = State::default();
    } else {
        st.rects = rects;
    }
    ctx.data_mut(|d| d.insert_temp(state_id(), st));
    picked
}

/// One list of rows at `at`; returns its rect and the open submenu row's rect.
#[allow(clippy::too_many_arguments)]
fn list_area(
    ctx: &egui::Context,
    id: Id,
    at: Pos2,
    entries: &[MenuEntry],
    focus: Option<usize>,
    sub: Option<usize>,
    picked: &mut Option<u32>,
    st: &mut State,
) -> (Rect, Option<Rect>) {
    let t = Tokens::get(ctx);
    let checks = entries.iter().any(|e| matches!(e, MenuEntry::Item(MenuItem { checked: Some(_), .. })));
    let mut sub_row = None;
    // As wide as its longest row, as tall as the window allows (then it scrolls).
    let width = entries
        .iter()
        .map(|e| {
            let text = |s: &str, f: egui::FontId| ctx.fonts_mut(|fo| fo.layout_no_wrap(s.to_string(), f, t.fg).size().x);
            match e {
                MenuEntry::Item(i) => text(&i.label, egui::FontId::proportional(13.0)) + text(&i.shortcut, theme::mono(11.0)) + 64.0,
                MenuEntry::Sub(l, _) => text(l, egui::FontId::proportional(13.0)) + 64.0,
                _ => 0.0,
            }
        })
        .fold(200.0f32, f32::max);
    let max_h = (ctx.content_rect().bottom() - at.y - 24.0).max(120.0);
    let r = Area::new(id).order(Order::Foreground).fixed_pos(at).constrain(false).show(ctx, |ui| {
        crate::widgets::popover_frame(ui).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, e) in entries.iter().enumerate() {
                    match e {
                        MenuEntry::Sep => {
                            ui.add_space(4.0);
                            crate::widgets::hairline(ui);
                            ui.add_space(4.0);
                        }
                        MenuEntry::Header(h) => {
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new(h.to_uppercase()).font(theme::medium(10.0)).color(t.fg_faint));
                            ui.add_space(2.0);
                        }
                        MenuEntry::Item(it) => {
                            let r = row(ui, &it.label, &it.shortcut, it.checked, checks, it.enabled, focus == Some(i), false);
                            if r.hovered() {
                                st.sub = None;
                            }
                            if r.clicked() && it.enabled {
                                *picked = Some(it.id);
                            }
                        }
                        MenuEntry::Sub(label, _) => {
                            let open = sub == Some(i);
                            let r = row(ui, label, "", None, checks, true, focus == Some(i) || open, true);
                            if r.hovered() || r.clicked() {
                                st.sub = Some(i);
                            }
                            if open || r.hovered() {
                                sub_row = Some(r.rect);
                            }
                        }
                    }
                }
            });
        });
    });
    (r.response.rect, sub_row)
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut Ui,
    label: &str,
    shortcut: &str,
    checked: Option<bool>,
    checks: bool,
    enabled: bool,
    focused: bool,
    sub: bool,
) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| {
        let kind = if checked.is_some() { egui::WidgetType::Checkbox } else { egui::WidgetType::Button };
        let mut info = egui::WidgetInfo::labeled(kind, enabled, label);
        info.selected = checked;
        info
    });
    if enabled && (resp.hovered() || focused) {
        ui.painter().rect_filled(rect, t.radius_sm, t.hover);
    }
    let fg = if enabled { t.fg } else { t.fg_faint };
    let mut x = rect.left() + 8.0;
    if checks {
        if checked == Some(true) {
            crate::icons::paint(ui, Rect::from_center_size(pos2(x + 7.0, rect.center().y), vec2(14.0, 14.0)), "check", 14.0, t.accent);
        }
        x += 22.0;
    }
    ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(13.0), fg);
    if sub {
        crate::icons::paint(
            ui,
            Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(14.0, 14.0)),
            "chevron-right",
            14.0,
            t.fg_muted,
        );
    } else if !shortcut.is_empty() {
        ui.painter().text(pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, shortcut, theme::mono(11.0), t.fg_faint);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_skips_what_cannot_be_picked_and_wraps() {
        let e = vec![
            MenuEntry::Header("A".into()),
            MenuEntry::Item(MenuItem::new(1, "one")),
            MenuEntry::Sep,
            MenuEntry::Item(MenuItem::new(2, "two").enabled(false)),
            MenuEntry::Sub("more".into(), vec![]),
        ];
        assert_eq!(step_focus(&e, None, 1), Some(1));
        assert_eq!(step_focus(&e, Some(1), 1), Some(4), "past the separator and the disabled row");
        assert_eq!(step_focus(&e, Some(4), 1), Some(1), "wraps");
        assert_eq!(step_focus(&e, None, -1), Some(4));
        assert_eq!(step_focus(&[MenuEntry::Sep], None, 1), None);
    }
}
