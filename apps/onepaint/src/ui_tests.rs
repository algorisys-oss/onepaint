//! UI tests for the tool bar, docks and panels (real pointer events
//! through egui_kittest).

use crate::layout::{DockSide, Orientation, PanelId, Placement};
use crate::{App, Popover, dock, toolbar};
use egui::{Event, PointerButton, Pos2, pos2, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use onepaint_engine::Tool;

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
    let top = h.get_by_label("Menu").rect();
    let bar = h.state().toolbar.rect.unwrap();
    assert!(bar.left() > top.right(), "tool bar {bar:?} clear of the top bar {top:?}");
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
