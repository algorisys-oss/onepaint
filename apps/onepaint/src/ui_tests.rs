//! UI tests for the tool bar, docks and panels (real pointer events
//! through egui_kittest).

use crate::layout::{DockSide, Orientation, PanelId, Placement};
use crate::{App, Popover, dock, toolbar};
use egui::{Event, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use onepaint_engine::{Command, Tool};

fn harness_sized(w: f32, h: f32) -> Harness<'static, App> {
    Harness::builder().with_size(vec2(w, h)).with_max_steps(400).build_eframe(|cc| App::new(cc, None))
}

fn harness() -> Harness<'static, App> {
    harness_sized(1200.0, 800.0)
}

fn press(h: &mut Harness<'_, App>, p: Pos2, pressed: bool, modifiers: egui::Modifiers) {
    h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers });
    h.step();
}

fn drag(h: &mut Harness<'_, App>, from: Pos2, to: Pos2) {
    h.event(Event::PointerMoved(from));
    h.step();
    press(h, from, true, Default::default());
    for i in 1..=12 {
        h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 12.0)));
        h.step();
    }
    press(h, to, false, Default::default());
    h.run();
}

fn click_with(h: &mut Harness<'_, App>, p: Pos2, modifiers: egui::Modifiers) {
    h.event(Event::PointerMoved(p));
    h.step();
    for pressed in [true, false] {
        h.event_modifiers(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers }, modifiers);
        h.step();
    }
    h.run();
}

fn double_click(h: &mut Harness<'_, App>, p: Pos2) {
    h.event(Event::PointerMoved(p));
    h.step();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Default::default() });
            h.step();
        }
    }
    h.run();
}

/// The tool bar's Layers button (the docked panel's title is "Layers" too).
fn layers_button(h: &Harness<'_, App>) -> egui::Rect {
    h.get_by_role_and_label(egui::accesskit::Role::Button, "Layers").rect()
}

#[test]
fn every_tool_is_reachable_from_the_tool_bar() {
    let mut h = harness();
    h.run();
    for (tool, label) in toolbar::tool_buttons() {
        h.get_by_label(label).click();
        h.run();
        assert_eq!(h.state().session.tool(), tool, "{label}");
        // Leave Move so its transform doesn't linger between tools.
        h.state_mut().set_tool(Tool::Brush);
        h.run();
    }
    // The selection shapes: the Select button, then its popover.
    for (tool, _) in toolbar::NO_BUTTON {
        h.get_by_label(toolbar::SELECT_LABEL).click();
        h.run();
        if h.state().popover != Some(Popover::Selection) {
            h.get_by_label(toolbar::SELECT_LABEL).click();
            h.run();
        }
        let name = match tool {
            Tool::SelectRect => "Rectangle (M)",
            Tool::SelectEllipse => "Ellipse",
            Tool::SelectLasso => "Lasso (L)",
            Tool::SelectPoly => "Polygon",
            _ => "Magic wand (W)",
        };
        h.get_by_label(name).click();
        h.run();
        assert_eq!(h.state().session.tool(), *tool, "{name}");
        h.state_mut().popover = None;
        h.run();
    }
}

#[test]
fn the_pan_button_pans_without_painting() {
    let mut h = harness();
    h.run();
    h.get_by_label(toolbar::PAN_LABEL).click();
    h.run();
    let (n, before) = (h.state().session.history_len(), h.state().canvas.view);
    drag(&mut h, pos2(600.0, 400.0), pos2(500.0, 350.0));
    assert_eq!(h.state().session.history_len(), n, "no paint");
    assert_ne!(h.state().canvas.view, before, "the view moved");
}

#[test]
fn dragging_the_layers_popover_to_the_edge_docks_it() {
    let mut h = harness();
    h.run();
    h.get_by_label("Layers").click();
    h.run();
    assert_eq!(h.state().popover, Some(Popover::Layers));
    let grip = h.get_by_label("Drag Layers").rect().center();
    drag(&mut h, grip, pos2(1190.0, 400.0));
    let app = h.state();
    assert_eq!(app.popover, None);
    assert_eq!(app.layout.placement(PanelId::Layers), Placement::Dock { side: DockSide::Right });
    assert!(dock::panel_open(app, PanelId::Layers));
    // The canvas makes room for the dock.
    assert!(app.canvas.stage.right() <= 1200.0 - crate::layout::DOCK_MIN_W + 0.5, "{:?}", app.canvas.stage);
    // The toolbar button now opens and closes the docked panel.
    let b = layers_button(&h).center();
    click_with(&mut h, b, Default::default());
    assert!(!dock::panel_open(h.state(), PanelId::Layers));
    assert_eq!(h.state().popover, None);
    // (The bar follows the dock's edge, so find it again.)
    let b = layers_button(&h).center();
    click_with(&mut h, b, Default::default());
    assert!(dock::panel_open(h.state(), PanelId::Layers));
    // Its header drags it back out to float.
    let head = h.get_by_label("Close Layers").rect().center() - vec2(150.0, 0.0);
    drag(&mut h, head, pos2(600.0, 300.0));
    assert!(matches!(h.state().layout.placement(PanelId::Layers), Placement::Float { .. }));
}

#[test]
fn the_tool_bar_docks_as_a_rail_and_flips() {
    // A real frame rate, so two clicks count as a double-click.
    let mut h = Harness::builder()
        .with_size(vec2(1200.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|cc| App::new(cc, None));
    h.run();
    let grip = h.get_by_label(toolbar::GRIP_LABEL).rect().center();
    // Double-click flips it to vertical.
    double_click(&mut h, grip);
    assert_eq!(h.state().layout.toolbar.orientation, Orientation::Vertical);
    // Drag it to the left edge: a rail that takes room from the canvas.
    let grip = h.get_by_label(toolbar::GRIP_LABEL).rect().center();
    drag(&mut h, grip, pos2(20.0, 400.0));
    let app = h.state();
    assert_eq!(app.layout.tools_dock, Some(DockSide::Left));
    assert!(app.canvas.stage.left() >= toolbar::rail_width(app), "{:?}", app.canvas.stage);
    // Tools still work from the rail.
    h.get_by_label("Eraser (E)").click();
    h.run();
    assert_eq!(h.state().session.tool(), Tool::Eraser);
    // Pull it off again.
    let grip = h.get_by_label(toolbar::GRIP_LABEL).rect().center();
    drag(&mut h, grip, pos2(600.0, 300.0));
    assert_eq!(h.state().layout.tools_dock, None);
}

#[test]
fn ctrl_click_picks_layers_and_align_moves_them() {
    let mut h = harness();
    h.run();
    // Two blocks on two layers.
    let mut ids = Vec::new();
    for x in [100.0, 600.0] {
        let s = &mut h.state_mut().session;
        let _ = s.execute(onepaint_engine::Command::AddLayer);
        let poly = vec![[x, 100.0], [x + 200.0, 100.0], [x + 200.0, 300.0], [x, 300.0]];
        let _ = s.execute(onepaint_engine::Command::SelectShape { contours: vec![poly], mode: onepaint_selection::Mode::Replace });
        let _ = s.execute(onepaint_engine::Command::FillSelection);
        let _ = s.execute(onepaint_engine::Command::Deselect);
        ids.push(s.active().to_string());
    }
    h.run();
    h.get_by_label("Layers").click();
    h.run();
    let names: Vec<String> = ids.iter().map(|id| h.state().session.doc().find(id).unwrap().name().to_string()).collect();
    // The second block's layer is active; Ctrl-clicking the first picks both.
    let at = h.get_by_label(&names[0]).rect().center();
    click_with(&mut h, at, egui::Modifiers::COMMAND);
    assert_eq!(h.state().layer_picks.len(), 2, "{:?}", h.state().layer_picks);
    h.state_mut().popover = None;
    dock::set_panel_open(h.state_mut(), PanelId::Align, true);
    h.run();
    h.get_by_label("Align left edges").click();
    h.run();
    let s = &h.state().session;
    let left = |id: &str| s.doc().layer(id).unwrap().pixels.content_bounds().unwrap().x;
    assert_eq!((left(&ids[0]), left(&ids[1])), (0, 0));
    // A plain click clears the picks.
    h.get_by_label("Layers").click();
    h.run();
    let at = h.get_by_label(&names[0]).rect().center();
    click_with(&mut h, at, Default::default());
    assert!(h.state().layer_picks.is_empty());
}

#[test]
fn narrow_windows_keep_the_bars_apart_and_compact_folds_the_docks() {
    let mut h = harness_sized(1000.0, 700.0);
    h.run();
    let top = h.get_by_label("Help").rect();
    let bar = h.state().toolbar.rect.unwrap();
    assert!(bar.top() >= top.bottom(), "tool bar {bar:?} below the header {top:?}");
    assert!(bar.right() <= 1000.0);
    // Below 900 px the tool bar is a strip and docked panels go home.
    let mut h = harness_sized(820.0, 700.0);
    h.state_mut().layout.set_placement(PanelId::Layers, Placement::Dock { side: DockSide::Right }, None);
    h.run();
    let app = h.state();
    let bar = app.toolbar.rect.unwrap();
    assert!(bar.bottom() <= 700.0 - crate::status_bar::HEIGHT + 0.5 && bar.width() > 700.0, "{bar:?}");
    assert_eq!(app.canvas.stage.width(), 820.0, "no dock room on a narrow window");
    h.get_by_label("Layers").click();
    h.run();
    assert_eq!(h.state().popover, Some(Popover::Layers), "the docked panel opens as its popover");
}

#[test]
fn panels_join_into_a_floating_tab_group() {
    let mut h = harness();
    h.run();
    let l = &mut h.state_mut().layout;
    l.set_placement(PanelId::Align, Placement::Float { x: 300.0, y: 120.0 }, None);
    l.set_placement(PanelId::Palettes, Placement::Float { x: 700.0, y: 300.0 }, None);
    l.set_open(PanelId::Align, true);
    l.set_open(PanelId::Palettes, true);
    h.run();
    let from = h.get_by_label("Close Palettes").rect().center() - vec2(120.0, 0.0);
    let onto = h.get_by_label("Close Align").rect().center() - vec2(100.0, 0.0);
    drag(&mut h, from, onto);
    let app = h.state();
    assert_eq!(app.layout.groups.len(), 1, "{:?}", app.layout.placements);
    let g = app.layout.groups.values().next().unwrap();
    assert_eq!(g.members, vec![PanelId::Align, PanelId::Palettes]);
    assert!(h.query_by_label("Close these panels").is_some());
}

#[test]
fn help_has_a_guide_to_the_panel_tool() {
    let mut h = harness();
    h.run();
    h.state_mut().popover = Some(Popover::Help);
    h.run();
    h.get_by_label("Guide").click();
    h.run();
    assert!(h.query_by_label("Drag inside a panel").is_some());
    assert!(h.query_by_label("Drag the status-bar swatch onto the canvas").is_some());
}

/// Spec 56: Properties edits the active layer's style. A built-in style
/// applies in one click; an effect can be added, opened and switched off.
#[test]
fn properties_edit_the_layer_style() {
    let mut h = harness();
    h.state_mut().layout.set_placement(PanelId::Properties, Placement::Dock { side: DockSide::Right }, None);
    dock::set_panel_open(h.state_mut(), PanelId::Properties, true);
    h.run();
    let id = h.state().session.active().to_string();
    let styles = |h: &Harness<'_, App>| h.state().session.doc().find(&id).and_then(|n| n.as_layer()).and_then(|l| l.styles());
    assert!(styles(&h).is_none());
    h.get_by_label("Ink Outline").click();
    h.run();
    let st = styles(&h).expect("styled");
    assert_eq!(st.effects.len(), 1);
    assert_eq!(st.effects[0].kind, onepaint_doc::styles::EffectKind::Stroke);
    // Add a drop shadow: it goes on top, open for editing.
    h.get_by_label("Drop Shadow").click();
    h.run();
    let st = styles(&h).unwrap();
    assert_eq!(st.effects[0].kind, onepaint_doc::styles::EffectKind::DropShadow);
    assert!(h.get_all_by_label("Distance").count() > 0, "its controls are open");
    // Remove it again, with the × on its row.
    let row = h.get_all_by_label("Drop Shadow").map(|n| n.rect().center().y).fold(f32::MAX, f32::min);
    let x = h.get_all_by_label("×").min_by(|a, b| (a.rect().center().y - row).abs().total_cmp(&(b.rect().center().y - row).abs())).unwrap();
    x.click();
    h.run();
    assert_eq!(styles(&h).unwrap().effects.len(), 1);
    // One undo step per edit.
    h.state_mut().session.execute(onepaint_engine::Command::Undo).unwrap();
    assert_eq!(styles(&h).unwrap().effects.len(), 2);
}

/// Spec 56: the current style saves as the user's own, applies in a click
/// like the built-ins, and deletes with its ×.
#[test]
fn a_style_saves_as_a_preset_and_applies() {
    let mut h = harness();
    h.state_mut().layout.set_placement(PanelId::Properties, Placement::Dock { side: DockSide::Right }, None);
    dock::set_panel_open(h.state_mut(), PanelId::Properties, true);
    h.run();
    let id = h.state().session.active().to_string();
    let styles = |h: &Harness<'_, App>| h.state().session.doc().find(&id).and_then(|n| n.as_layer()).and_then(|l| l.styles());
    h.get_by_label("Neon").click();
    h.run();
    h.state_mut().style_name = "Glow".into();
    h.get_by_label("Save style").click();
    h.run();
    assert_eq!(h.state().style_presets.list.len(), 1);
    assert_eq!(h.state().style_presets.list[0].name, "Glow");
    assert!(h.state().style_name.is_empty());
    h.get_by_label("Ink Outline").click();
    h.run();
    assert_eq!(styles(&h).unwrap().effects.len(), 1);
    h.get_by_label("Glow").click();
    h.run();
    assert_eq!(styles(&h), h.state().style_presets.list.first().map(|p| p.styles.clone()));
    let chip = h.get_by_label("Glow").rect().right_center();
    let x = h.get_all_by_label("×").min_by(|a, b| a.rect().center().distance(chip).total_cmp(&b.rect().center().distance(chip))).unwrap();
    x.click();
    h.run();
    assert!(h.state().style_presets.list.is_empty());
}

/// Spec 56: a gradient overlay's stops edit on a bar: a click adds a stop,
/// which the current colour can then recolour.
#[test]
fn gradient_overlay_stops_edit_on_the_bar() {
    let mut h = harness();
    h.state_mut().layout.set_placement(PanelId::Properties, Placement::Dock { side: DockSide::Right }, None);
    dock::set_panel_open(h.state_mut(), PanelId::Properties, true);
    h.run();
    let id = h.state().session.active().to_string();
    let stops = |h: &Harness<'_, App>| {
        let st = h.state().session.doc().find(&id).and_then(|n| n.as_layer()).and_then(|l| l.styles()).unwrap();
        st.effects.iter().find(|e| e.kind == onepaint_doc::styles::EffectKind::GradientOverlay).unwrap().stops.clone()
    };
    h.get_by_label("Gold").click();
    h.run();
    assert_eq!(stops(&h).len(), 3);
    // The effect's row, above the Add chips.
    h.get_all_by_label("Gradient Overlay").min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).unwrap().click();
    h.run();
    let bar = h.get_by_label("Gradient: 3 stops").rect();
    click_with(&mut h, egui::pos2(bar.left() + 7.0 + (bar.width() - 14.0) * 0.25, bar.top() + 8.0), Default::default());
    let s = stops(&h);
    assert_eq!(s.len(), 4);
    assert!((s[3]["pos"].as_f64().unwrap() - 0.25).abs() < 0.02, "{s:?}");
    h.state_mut().session.execute(onepaint_engine::Command::SetColor { rgba: onepaint_color::Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    h.run();
    // The swatch beside the label.
    let l = h.get_by_label("Stop colour").rect();
    click_with(&mut h, egui::pos2(l.right() + 16.0, l.center().y), Default::default());
    assert_eq!(stops(&h)[3]["color"], serde_json::json!([1.0, 0.0, 0.0]));
    // Drag its handle along; the others stay.
    let bar = h.get_by_label("Gradient: 4 stops").rect();
    let x = |p: f32| bar.left() + 7.0 + (bar.width() - 14.0) * p;
    drag(&mut h, egui::pos2(x(0.25), bar.top() + 26.0), egui::pos2(x(0.6), bar.top() + 26.0));
    let s = stops(&h);
    assert!((s[3]["pos"].as_f64().unwrap() - 0.6).abs() < 0.02, "{s:?}");
    assert_eq!(s[1]["pos"].as_f64(), Some(0.5));
}

/// Every id in `menus`, submenus included.
fn menu_ids(menus: &[onepaint_ui_kit::menu_bar::Menu]) -> std::collections::HashSet<u32> {
    use onepaint_ui_kit::menu_bar::MenuEntry;
    fn walk(e: &[MenuEntry], out: &mut std::collections::HashSet<u32>) {
        for x in e {
            match x {
                MenuEntry::Item(i) => {
                    out.insert(i.id);
                }
                MenuEntry::Sub(_, s) => walk(s, out),
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    for m in menus {
        walk(&m.entries, &mut out);
    }
    out
}

/// Spec 65: everything the menus can do shows up, in the state it belongs
/// to (happypaint `menu-reachability.test.ts`); the old "…" menu is gone.
#[test]
fn every_menu_command_is_reachable() {
    use crate::menus::{Cmd, Picked, build, decode};
    let mut h = harness();
    h.run();
    let mut seen = menu_ids(&build(h.state()));
    // Print preview, animation, a mask, a smart filter.
    {
        let app = h.state_mut();
        let mut v = app.canvas.view_modes();
        v.proof = true;
        app.canvas.set_view_modes(v);
        crate::timeline::toggle(app);
        let _ = app.session.execute(Command::AddMask);
        let id = app.session.active().to_string();
        let params = onepaint_filters::default_params(onepaint_filters::Kind::Invert);
        let _ = app.session.execute(Command::AddSmartFilter { id, kind: "invert".into(), params });
    }
    seen.extend(menu_ids(&build(h.state())));
    // A group, then a text layer.
    let _ = h.state_mut().session.execute(Command::AddGroup);
    seen.extend(menu_ids(&build(h.state())));
    let spec = onepaint_text::TextSpec { text: "Hi".into(), ..Default::default() };
    let _ = h.state_mut().session.execute(Command::AddText { spec });
    seen.extend(menu_ids(&build(h.state())));
    let missing: Vec<Cmd> =
        Cmd::ALL.iter().copied().filter(|c| !seen.iter().any(|id| matches!(decode(*id), Some(Picked::Cmd(x)) if x == *c))).collect();
    assert!(missing.is_empty(), "not in any menu: {missing:?}");
    // Every filter and adjustment kind is in Adjust.
    let kinds = seen.iter().filter(|id| matches!(decode(**id), Some(Picked::Kind(_)))).count();
    assert_eq!(kinds, onepaint_filters::Kind::ALL.len());
    assert!(h.query_by_label("Menu").is_none(), "no second action list");
}

#[test]
fn menus_run_their_command_and_escape_closes() {
    let mut h = harness();
    h.run();
    let n = h.state().session.doc().root.len();
    crate::tests::menu_pick(&mut h, "Layer", "New layer");
    assert_eq!(h.state().session.doc().root.len(), n + 1);
    assert!(!onepaint_ui_kit::menu_bar::is_open(&h.ctx), "picking closes the menu");
    // Esc closes an open menu; the canvas keeps its tool.
    h.get_by_label("Image").click();
    h.run();
    assert!(onepaint_ui_kit::menu_bar::is_open(&h.ctx));
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!onepaint_ui_kit::menu_bar::is_open(&h.ctx));
    // A click on the canvas that closes a menu doesn't paint.
    h.get_by_label("Edit").click();
    h.run();
    let before = h.state().session.history_len();
    click_with(&mut h, pos2(640.0, 420.0), Default::default());
    h.run();
    assert!(!onepaint_ui_kit::menu_bar::is_open(&h.ctx));
    assert_eq!(h.state().session.history_len(), before, "no dab");
}

/// Spec 65 phase 2: the options row drives the brush, the tool and the view.
#[test]
fn the_options_row_drives_the_brush_the_tool_and_the_zoom() {
    use egui::accesskit::Role;
    let mut h = harness();
    h.run();
    // The brush strip: pressure overrides and the stabiliser.
    h.get_by_label("Size").click();
    h.run();
    assert!(h.state().settings.force_size_pressure);
    h.get_by_label("Stabilizer").click();
    h.run();
    assert!(h.state().session.current_brush().stabilizer.as_ref().is_some_and(|s| s.enabled));
    // The foreground swatch opens the colour popover under itself.
    let fg = h.get_by_label("Foreground colour").rect();
    h.get_by_label("Foreground colour").click();
    h.run();
    assert_eq!(h.state().popover, Some(Popover::Color));
    let at = h.state().row_anchor.unwrap();
    assert!(at.0 == Popover::Color && (at.1.x - fg.left()).abs() < 1.0 && at.1.y > fg.bottom());
    h.state_mut().popover = None;
    // Fill: Wash shows its own settings.
    h.state_mut().set_tool(Tool::Fill);
    h.run();
    assert!(h.query_by_role_and_label(Role::Slider, "Tolerance").is_some());
    assert!(h.query_by_role_and_label(Role::Slider, "Water").is_none());
    h.get_by_label("Wash").click();
    h.run();
    assert!(h.state().canvas.options.fill_wash);
    assert!(h.query_by_role_and_label(Role::Slider, "Water").is_some());
    // The zoom menu at the end of the row.
    h.get_by_label("Zoom").click();
    h.run();
    h.get_by_label("100%").click();
    h.run();
    assert!((h.state().canvas.view.scale - 1.0).abs() < 1e-9);
}
