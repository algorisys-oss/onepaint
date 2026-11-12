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
    assert!(toolbar::NO_BUTTON.is_empty(), "every tool has its own button (spec 65)");
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
    // Layers docks by default (spec 65): start it from its popover.
    h.state_mut().layout.set_placement(PanelId::Layers, Placement::Home, None);
    h.state_mut().layout.set_open(PanelId::Layers, false);
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
    // A rail on the left by default (spec 65), taking room from the canvas.
    let app = h.state();
    assert_eq!(app.layout.tools_dock, Some(DockSide::Left));
    assert!(app.canvas.stage.left() >= toolbar::rail_width(app), "{:?}", app.canvas.stage);
    h.get_by_label("Eraser (E)").click();
    h.run();
    assert_eq!(h.state().session.tool(), Tool::Eraser);
    // Pull it off: it floats, vertical; a double-click flips it.
    let grip = h.get_by_label(toolbar::GRIP_LABEL).rect().center();
    drag(&mut h, grip, pos2(600.0, 300.0));
    assert_eq!(h.state().layout.tools_dock, None);
    assert_eq!(h.state().layout.toolbar.orientation, Orientation::Vertical);
    // Let the drag's release age out of the double-click window first.
    for _ in 0..40 {
        h.step();
    }
    let grip = h.get_by_label(toolbar::GRIP_LABEL).rect().center();
    double_click(&mut h, grip);
    assert_eq!(h.state().layout.toolbar.orientation, Orientation::Horizontal);
    // Dropped at the left edge, it docks again.
    let grip = h.get_by_label(toolbar::GRIP_LABEL).rect().center();
    drag(&mut h, grip, pos2(20.0, 400.0));
    assert_eq!(h.state().layout.tools_dock, Some(DockSide::Left));
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
        let _ = s.execute(onepaint_engine::Command::SelectShape {
            contours: vec![poly],
            mode: onepaint_selection::Mode::Replace,
            antialias: false,
        });
        let _ = s.execute(onepaint_engine::Command::FillSelection);
        let _ = s.execute(onepaint_engine::Command::Deselect);
        ids.push(s.active().to_string());
    }
    // Layers is docked and open.
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
    let onto = h.get_by_label("Close Transform & Align").rect().center() - vec2(100.0, 0.0);
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
    // Properties gets the dock's height to itself.
    dock::set_panel_open(h.state_mut(), PanelId::Layers, false);
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
    // Properties gets the dock's height to itself.
    dock::set_panel_open(h.state_mut(), PanelId::Layers, false);
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
    // Properties gets the dock's height to itself.
    dock::set_panel_open(h.state_mut(), PanelId::Layers, false);
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
    let z = h.get_by_label("Zoom").rect();
    h.get_by_label("Zoom").click();
    h.run();
    crate::tests::click_near(&mut h, "100%", z);
    assert!((h.state().canvas.view.scale - 1.0).abs() < 1e-9);
}

/// Spec 65 phase 4: as the title bar, the header has the window controls,
/// which drive the window; with the system's title bar it doesn't.
#[test]
fn the_header_stands_in_for_the_title_bar() {
    let mut h = harness();
    h.run();
    assert!(h.query_by_label("Maximize").is_none(), "the system's title bar has them");
    h.state_mut().custom_title = true;
    h.run();
    let sent = |h: &mut Harness<'_, App>, label: &str| {
        h.get_by_label(label).click();
        h.step();
        h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default()
    };
    assert!(sent(&mut h, "Maximize").contains(&egui::ViewportCommand::Maximized(true)));
    assert!(sent(&mut h, "Minimize").contains(&egui::ViewportCommand::Minimized(true)));
    assert!(sent(&mut h, "Close").contains(&egui::ViewportCommand::Close));
}

/// Spec 65 phase 5: below 900 px the menus fold into one and the zoom
/// leaves the options row for the status bar.
#[test]
fn narrow_windows_fold_the_menus_and_the_zoom() {
    let mut h = harness_sized(850.0, 700.0);
    h.run();
    assert!(h.query_by_label("File").is_none() && h.query_by_label("Zoom").is_none());
    assert!(h.query_by_label("Zoom in").is_some(), "the status bar's zoom");
    crate::tests::menu_pick(&mut h, "Menu", "New canvas…");
    assert_eq!(h.state().popover, Some(Popover::NewDoc));
    // Wide again: the eight menus.
    let mut h = harness();
    h.run();
    assert!(h.query_by_label("Menu").is_none() && h.query_by_label("File").is_some());
}

/// Spec 65 phase 6: Help ▸ Manual opens the manual; Next and the contents
/// move between its sections.
#[test]
fn help_opens_the_manual() {
    let mut h = harness();
    h.run();
    crate::tests::menu_pick(&mut h, "Help", "Manual");
    assert_eq!(h.state().popover, Some(Popover::Manual));
    assert_eq!(h.state().manual_section, 0);
    h.get_by_label("Next").click();
    h.run();
    assert_eq!(h.state().manual_section, 1);
    h.get_by_label("Comic panels").click();
    h.run();
    let want = crate::manual::sections().iter().position(|s| s.title == "Comic panels").unwrap();
    assert_eq!(h.state().manual_section, want);
}

fn key(h: &mut Harness<'_, App>, key: egui::Key, modifiers: egui::Modifiers) {
    h.event(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.step();
    h.event(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    h.step();
}

/// Ctrl/⌘+K opens the command palette over every tool and menu command;
/// typing ranks them, Enter runs the first, Esc closes (happypaint v0.204.11).
/// Both themes.
#[test]
fn the_command_palette_finds_and_runs_commands() {
    for light in [false, true] {
        let mut h = harness();
        h.run();
        if light {
            let ctx = h.ctx.clone();
            h.state_mut().toggle_theme(&ctx);
            h.run();
        }
        let open = |h: &mut Harness<'_, App>, query: &str| {
            key(h, egui::Key::K, egui::Modifiers::COMMAND);
            assert_eq!(h.state().popover, Some(Popover::CommandPalette));
            h.event(Event::Text(query.into()));
            h.step();
            h.step();
        };
        // A tool, by an artist's word for it.
        open(&mut h, "bucket");
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        h.run();
        assert_eq!(h.state().popover, None);
        assert_eq!(h.state().session.tool(), Tool::Fill);
        // A menu command.
        let layers = h.state().session.doc().root.len();
        open(&mut h, "new layer");
        key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
        h.run();
        assert_eq!(h.state().session.doc().root.len(), layers + 1);
        // ↓ moves to the next match; Esc closes without running anything.
        open(&mut h, "blur");
        assert!(h.query_by_label("Gaussian Blur").is_some(), "filters are listed");
        key(&mut h, egui::Key::ArrowDown, egui::Modifiers::NONE);
        key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
        h.run();
        assert_eq!(h.state().popover, None);
        // The menus open it too, and Ctrl+K closes it again.
        crate::tests::menu_pick(&mut h, "Edit", "Command palette…");
        assert_eq!(h.state().popover, Some(Popover::CommandPalette));
        assert!(h.query_by_label("Eraser").is_some(), "an empty query lists everything");
        key(&mut h, egui::Key::K, egui::Modifiers::COMMAND);
        assert_eq!(h.state().popover, None);
    }
}

#[test]
fn the_palette_lists_tools_and_menu_rows_once() {
    let mut h = harness();
    h.run();
    let app = h.state();
    assert!(matches!(crate::palette::find(app, "Eraser"), Some(crate::palette::Target::Action(_))));
    assert!(matches!(crate::palette::find(app, "New layer"), Some(crate::palette::Target::Menu(_))));
    assert_eq!(crate::palette::find(app, "Command palette"), None, "it leaves itself out");
    let (items, _) = crate::palette::items(app);
    let mut titles: Vec<_> = items.iter().map(|i| (i.category.clone(), i.path.clone(), i.title.clone())).collect();
    let n = titles.len();
    titles.sort();
    titles.dedup();
    assert_eq!(titles.len(), n, "no row twice");
    assert!(n > 100, "{n} commands");
}

/// Scrolling the Brushes panel leaves its search and library actions in
/// place, docked and floating (happypaint v0.204.18); the sliders scroll up
/// with the list.
#[test]
fn the_brushes_panel_keeps_its_header_while_scrolling() {
    for placement in [Placement::Dock { side: DockSide::Right }, Placement::Float { x: 500.0, y: 90.0 }] {
        let mut h = harness_sized(1400.0, 640.0);
        h.state_mut().layout.set_placement(PanelId::Brush, placement.clone(), None);
        h.state_mut().layout.set_open(PanelId::Brush, true);
        h.run();
        let pinned = h.get_by_label("Import brushes…").rect();
        let studio = h.get_by_label("Brush studio…").rect();
        // Wheel over the list, well below the header.
        let over = pinned.center() + vec2(0.0, 160.0);
        h.event(Event::PointerMoved(over));
        for _ in 0..20 {
            h.event(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -400.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
        h.run();
        let after = h.get_by_label("Import brushes…").rect();
        assert_eq!(after.top(), pinned.top(), "{placement:?}: the actions stay put");
        assert!(h.get_by_label("Brush studio…").rect().top() < studio.top(), "{placement:?}: the body scrolled");
    }
}

/// Document Properties gives the saved file's real size (happypaint v0.208.2,
/// review F25), and says when unsaved work isn't in it.
#[test]
fn document_properties_report_the_stored_size() {
    let mut h = harness();
    h.run();
    h.state_mut().popover = Some(Popover::Paper);
    h.run();
    assert!(h.query_by_label("Not saved to a file yet.").is_some());

    let path = std::env::temp_dir().join(format!("onepaint-stored-size-{}.happypaint", std::process::id()));
    let bytes = onepaint_formats::write_happypaint(h.state().session.doc(), None, "t").unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let len = h.state().session.history_len();
    let app = h.state_mut();
    app.path = Some(path.clone());
    app.saved_history_len = Some(len);
    h.run();
    let size = crate::memory::format_file_size(bytes.len() as u64);
    assert!(h.query_by_label(&format!("The saved file is {size}.")).is_some(), "{size}");

    h.state_mut().saved_history_len = None;
    h.run();
    assert!(h.query_by_label(&format!("The saved file is {size}; unsaved changes aren't in it yet.")).is_some());
    let _ = std::fs::remove_file(&path);
}

/// happypaint v0.209.0: Ellipse has an Anti-alias chip (on by default) that
/// gives the committed selection a soft edge; off, the edge is hard. The
/// Feather field takes any radius up to 1000 px, with or without a selection.
#[test]
fn ellipse_selection_anti_alias_and_the_feather_field() {
    for light in [false, true] {
        let mut h = harness();
        h.run();
        if light {
            let ctx = h.ctx.clone();
            h.state_mut().toggle_theme(&ctx);
        }
        h.state_mut().set_tool(Tool::SelectEllipse);
        h.run();
        assert!(h.query_by_label("Feather").is_some(), "the feather field shows before any selection");
        assert!(h.query_by_label("Anti-alias").is_some());
        assert!(h.state().canvas.options.select_antialias, "on by default");
        let screen = |h: &Harness<'_, App>, d: [f64; 2]| {
            let (rect, v) = (h.state().canvas.screen_rect, h.state().canvas.view);
            let s = v.doc_to_screen(d);
            pos2(rect.left() + s[0] as f32, rect.top() + s[1] as f32)
        };
        let soft =
            |h: &Harness<'_, App>| h.state().session.selection().map_or(0, |m| m.data().iter().filter(|&&v| v != 0 && v != 255).count());
        let (a, b) = (screen(&h, [100.0, 100.0]), screen(&h, [400.0, 300.0]));
        // Marching ants keep repainting once there is a selection: fixed steps.
        let drag = |h: &mut Harness<'_, App>| {
            h.event(Event::PointerMoved(a));
            h.step();
            press(h, a, true, Default::default());
            for i in 1..=12 {
                h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 12.0)));
                h.step();
            }
            press(h, b, false, Default::default());
            h.run_steps(3);
        };
        drag(&mut h);
        assert!(soft(&h) > 50, "anti-aliased edge: {}", soft(&h));

        h.get_by_label("Anti-alias").click();
        h.run_steps(3);
        assert!(!h.state().canvas.options.select_antialias);
        drag(&mut h);
        assert!(h.state().session.selection().is_some());
        assert_eq!(soft(&h), 0, "hard edge");

        h.state_mut().set_tool(Tool::SelectRect);
        h.run_steps(3);
        assert!(h.query_by_label("Anti-alias").is_none(), "the rectangle has no such option");
        h.state_mut().session.execute(Command::SetFeather { radius: 750.0 }).unwrap();
        assert_eq!(h.state().session.feather(), 750.0);
    }
}

/// happypaint v0.209.0: the Transform & Align panel types exact numbers into
/// the Move tool's box. Typing arms the Move tool; Apply commits one step.
#[test]
fn the_transform_fields_edit_the_move_box() {
    for light in [false, true] {
        let mut h = harness();
        h.run();
        if light {
            let ctx = h.ctx.clone();
            h.state_mut().toggle_theme(&ctx);
        }
        let s = &mut h.state_mut().session;
        s.execute(Command::AddLayer).unwrap();
        let poly = onepaint_selection::rect_polygon([100.0, 100.0], [300.0, 200.0]);
        s.execute(Command::SelectShape { contours: vec![poly], mode: onepaint_selection::Mode::Replace, antialias: false }).unwrap();
        s.execute(Command::FillSelection).unwrap();
        s.execute(Command::Deselect).unwrap();
        let id = s.active().to_string();
        let undo = s.history_len();
        let l = &mut h.state_mut().layout;
        l.set_placement(PanelId::Align, Placement::Float { x: 300.0, y: 120.0 }, None);
        l.set_open(PanelId::Align, true);
        h.run();
        // Before any move the fields read the layer's box.
        let spin = egui::accesskit::Role::SpinButton;
        assert!(h.query_by_role_and_label(spin, "W").is_some(), "a W field");
        assert_ne!(h.state().session.tool(), Tool::Move);

        // Typing W (aspect linked) arms the Move tool and scales both sides.
        h.get_by_label("Width and height unlinked: tap to keep the aspect ratio").click();
        h.run();
        assert!(h.state().xform_link_aspect);
        let patch = onepaint_engine::transform_numeric::FieldsPatch { w: Some(400.0), ..Default::default() };
        crate::align_ui::set_fields(h.state_mut(), patch);
        h.run();
        assert_eq!(h.state().session.tool(), Tool::Move);
        let (_, b) = h.state().session.transform_state().unwrap();
        let doc = [f64::from(h.state().session.doc().width), f64::from(h.state().session.doc().height)];
        let f = h.state().canvas.xform.fields(doc, b);
        assert!((f.w - 400.0).abs() < 1e-6 && (f.h - 200.0).abs() < 1e-6, "{f:?}");
        assert!((f.x - 100.0).abs() < 1e-6 && (f.y - 100.0).abs() < 1e-6, "the top-left stays: {f:?}");

        // Apply commits it as one step.
        h.get_all_by_label("Apply").last().unwrap().click();
        h.run();
        let s = &h.state().session;
        assert_eq!(s.history_len(), undo + 1);
        let r = s.doc().layer(&id).unwrap().pixels.content_bounds().unwrap();
        assert_eq!((r.x, r.y, r.w, r.h), (100, 100, 400, 200));

        // Typed into the field itself: X moves the box.
        h.get_by_role_and_label(spin, "X").click();
        h.run();
        h.event(Event::Key { key: egui::Key::A, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND });
        h.event(Event::Text("40".into()));
        h.event(Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() });
        h.run();
        let (_, b) = h.state().session.transform_state().unwrap();
        let f = h.state().canvas.xform.fields(doc, b);
        assert!((f.x - 40.0).abs() < 1e-6, "{f:?}");
        assert!((f.w - 400.0).abs() < 1e-6, "{f:?}");
    }
}

/// happypaint v0.168: layers Ctrl-clicked in the Layers panel move together
/// with the Move tool, under one box, as one undo step.
#[test]
fn the_move_tool_moves_every_picked_layer() {
    let mut h = harness();
    h.run();
    let mut ids = Vec::new();
    for x in [100.0, 400.0] {
        let s = &mut h.state_mut().session;
        s.execute(Command::AddLayer).unwrap();
        let poly = onepaint_selection::rect_polygon([x, 100.0], [x + 100.0, 200.0]);
        s.execute(Command::SelectShape { contours: vec![poly], mode: onepaint_selection::Mode::Replace, antialias: false }).unwrap();
        s.execute(Command::FillSelection).unwrap();
        s.execute(Command::Deselect).unwrap();
        ids.push(s.active().to_string());
    }
    h.run();
    let name = h.state().session.doc().find(&ids[0]).unwrap().name().to_string();
    let at = h.get_by_label(&name).rect().center();
    click_with(&mut h, at, egui::Modifiers::COMMAND);
    assert_eq!(h.state().layer_picks.len(), 2);
    h.state_mut().set_tool(Tool::Move);
    h.run();
    let mut moving = h.state().session.transform_layers();
    moving.sort();
    let mut want = ids.clone();
    want.sort();
    assert_eq!(moving, want, "both picked layers are lifted");
    let (_, b) = h.state().session.transform_state().unwrap();
    assert_eq!((b.x, b.w), (100, 400), "one box around both");

    let undo = h.state().session.history_len();
    let from = h.state().canvas.doc_to_win([150.0, 150.0]);
    let to = h.state().canvas.doc_to_win([150.0, 250.0]);
    drag(&mut h, pos2(from[0] as f32, from[1] as f32), pos2(to[0] as f32, to[1] as f32));
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    h.run();
    let s = &h.state().session;
    assert_eq!(s.history_len(), undo + 1, "one undo step");
    for (id, x) in ids.iter().zip([150, 450]) {
        let l = s.doc().layer(id).unwrap();
        assert!(l.pixels.get(x, 150)[3] < 0.5 && l.pixels.get(x, 260)[3] > 0.99, "{id} moved down");
    }
}

/// happypaint's multi-select bar: Ctrl/⌘-clicked layers get Group, Merge
/// (scattered picks too, v0.208.3), Hide and a colour label, each one step.
#[test]
fn the_layers_bar_acts_on_the_picked_layers() {
    for light in [false, true] {
        let mut h = harness();
        h.run();
        if light {
            let ctx = h.ctx.clone();
            h.state_mut().toggle_theme(&ctx);
        }
        let mut ids = Vec::new();
        for x in [100.0, 300.0, 500.0] {
            let s = &mut h.state_mut().session;
            s.execute(Command::AddLayer).unwrap();
            let poly = onepaint_selection::rect_polygon([x, 100.0], [x + 100.0, 200.0]);
            s.execute(Command::SelectShape { contours: vec![poly], mode: onepaint_selection::Mode::Replace, antialias: false }).unwrap();
            s.execute(Command::FillSelection).unwrap();
            s.execute(Command::Deselect).unwrap();
            ids.push(s.active().to_string());
        }
        h.run();
        assert!(h.query_by_label("Merge").is_none(), "no bar without picks");
        // The top layer is active; Ctrl-click the bottom one: a scattered pair.
        let name = h.state().session.doc().find(&ids[0]).unwrap().name().to_string();
        let at = h.get_by_label(&name).rect().center();
        click_with(&mut h, at, egui::Modifiers::COMMAND);
        assert_eq!(h.state().layer_picks.len(), 2);
        assert!(h.query_by_label("2 selected").is_some());

        let n = h.state().session.history_len();
        h.get_by_label("Hide").click();
        h.run();
        let shown = |h: &Harness<'_, App>, id: &str| h.state().session.doc().find(id).unwrap().visible();
        assert!(!shown(&h, &ids[0]) && !shown(&h, &ids[2]) && shown(&h, &ids[1]));
        assert_eq!(h.state().session.history_len(), n + 1, "one step");
        h.get_by_label("Show").click();
        h.run();
        assert!(shown(&h, &ids[0]) && shown(&h, &ids[2]));
        // The bar's swatch, not the Properties panel's.
        let y = h.get_by_label("2 selected").rect().center().y;
        let at = h.get_all_by_label("green").map(|n| n.rect().center()).min_by(|a, b| (a.y - y).abs().total_cmp(&(b.y - y).abs())).unwrap();
        click_with(&mut h, at, Default::default());
        assert_eq!(h.state().session.color_label(&ids[2]).as_deref(), Some("green"));
        assert_eq!(h.state().session.color_label(&ids[0]).as_deref(), Some("green"));

        if light {
            h.get_by_label("Group").click();
            h.run();
            let g = h.state().session.active().to_string();
            assert_eq!(h.state().session.doc().parent_of(&ids[0]).flatten(), Some(g.clone()));
            assert_eq!(h.state().session.doc().parent_of(&ids[2]).flatten(), Some(g));
            assert_eq!(h.state().session.doc().parent_of(&ids[1]).flatten(), None, "the layer between stays out");
        } else {
            h.get_by_label("Merge").click();
            h.run();
            let s = &h.state().session;
            assert!(s.doc().find(&ids[2]).is_none(), "the top pick merged down");
            let l = s.doc().layer(&ids[0]).unwrap();
            assert!(l.pixels.get(150, 150)[3] > 0.99 && l.pixels.get(550, 150)[3] > 0.99);
            assert_eq!(l.pixels.get(350, 150)[3], 0.0, "the layer between stays its own");
            assert!(s.doc().layer(&ids[1]).is_some());
        }
        assert!(h.state().layer_picks.is_empty(), "the picks clear");
    }
}
