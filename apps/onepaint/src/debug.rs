//! Debug automation driven by environment variables, for visual checks:
//!
//! - `ONEPAINT_SCREENSHOT=out.png` saves a screenshot of the window after a few frames.
//! - `ONEPAINT_DEMO=1` paints a small demo scene first.
//! - `ONEPAINT_POPOVER=layers|brushes|color|menu|new|studio|…` opens a popover.
//! - `ONEPAINT_THEME=light` starts in the light theme.
//! - `ONEPAINT_TRANSFORM=1` starts a rotated move on the active layer.
//! - `ONEPAINT_TEXT=1` adds a text object and opens its editor.
//! - `ONEPAINT_SMUDGE=1` smudges and mixes across the demo, plus a radial-symmetry stroke.
//! - `ONEPAINT_GUIDES=1` turns on the grid, 2-point perspective and H/V snap.
//! - `ONEPAINT_PEN=1` adds a curved, filled path layer and selects the pen.
//! - `ONEPAINT_ANIM=1` turns the demo layers into frames (background role) with onion skins;
//!   `ONEPAINT_ANIM=tracks` also converts them to exposure-sheet tracks with markers and a range;
//!   `ONEPAINT_ANIM=pegs` opens the timing ops with two frames picked and slides a ghost off its pegs.
//! - `ONEPAINT_IMPASTO=1` paints thick, wet-impasto and oil strokes, blends one, and bevels a shape.
//! - `ONEPAINT_WET=1` paints live watercolour (blue, then yellow into it) and leaves it wet.
//! - `ONEPAINT_DOCK=1` docks Layers and Properties right and Brushes left, floats Align
//!   with two layers picked, and docks the tool bar as a left rail;
//!   `ONEPAINT_DOCK=float` floats a vertical tool bar and groups Colour with Align instead.
//! - `ONEPAINT_PANELS=1` lays out a 2×2 manga page, cuts a panel, inks across a frame, and shows the Panel tool.
//! - `ONEPAINT_VIEW=gray|proof|warn` turns on a present-only view mode (spec 48).
//! - `ONEPAINT_NEW=16384x12000` starts with a blank document of that size (before the demo, if any).
//! - `ONEPAINT_MAX_SHEET=256` caps the composite's sheet side, to see sheets (spec 40) on small documents.
//! - `ONEPAINT_EXIT=1` closes the app after the screenshot.

use crate::{App, Popover};
use onepaint_color::Rgba;
use onepaint_engine::{Command, LayerProps};
use onepaint_stroke::StrokePoint;

#[derive(Default)]
pub struct Debug {
    frame: u64,
    shot: Option<String>,
    demo: bool,
    popover: Option<Popover>,
    exit: bool,
    pub light: bool,
    transform: bool,
    text: bool,
    smudge: bool,
    guides: bool,
    pen: bool,
    anim: bool,
    tracks: bool,
    pegs: bool,
    wet: bool,
    impasto: bool,
    dock: Option<String>,
    panels: bool,
    /// Start with a blank document of this size.
    new: Option<(u32, u32)>,
    view: Option<String>,
}

impl Debug {
    /// A scripted run (demo or screenshot): it never autosaves or offers recovery,
    /// so visual checks leave nothing behind in the user's autosave folder.
    pub fn scripted(&self) -> bool {
        self.demo
            || self.shot.is_some()
            || self.popover.is_some()
            || self.transform
            || self.text
            || self.smudge
            || self.guides
            || self.pen
            || self.anim
            || self.impasto
            || self.wet
            || self.dock.is_some()
            || self.panels
            || self.new.is_some()
            || self.view.is_some()
    }

    pub fn from_env() -> Self {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        Self {
            frame: 0,
            shot: var("ONEPAINT_SCREENSHOT"),
            demo: var("ONEPAINT_DEMO").is_some(),
            popover: var("ONEPAINT_POPOVER").and_then(|p| match p.as_str() {
                "layers" => Some(Popover::Layers),
                "brushes" => Some(Popover::Brushes),
                "color" => Some(Popover::Color),
                "menu" => Some(Popover::Menu),
                "new" => Some(Popover::NewDoc),
                "selection" => Some(Popover::Selection),
                "curves" => Some(Popover::Filter),
                "guides" => Some(Popover::Guides),
                "pen" => Some(Popover::PenOptions),
                "panel" => Some(Popover::PanelOptions),
                "adjustments" => Some(Popover::Adjustments),
                "mixer" => Some(Popover::MixerOptions),
                "liquify" => Some(Popover::LiquifyOptions),
                "studio" => Some(Popover::BrushStudio),
                _ => None,
            }),
            exit: var("ONEPAINT_EXIT").is_some(),
            light: var("ONEPAINT_THEME").is_some_and(|t| t == "light"),
            transform: var("ONEPAINT_TRANSFORM").is_some(),
            text: var("ONEPAINT_TEXT").is_some(),
            smudge: var("ONEPAINT_SMUDGE").is_some(),
            guides: var("ONEPAINT_GUIDES").is_some(),
            pen: var("ONEPAINT_PEN").is_some(),
            anim: var("ONEPAINT_ANIM").is_some(),
            tracks: var("ONEPAINT_ANIM").is_some_and(|v| v == "tracks"),
            pegs: var("ONEPAINT_ANIM").is_some_and(|v| v == "pegs"),
            impasto: var("ONEPAINT_IMPASTO").is_some(),
            wet: var("ONEPAINT_WET").is_some(),
            dock: var("ONEPAINT_DOCK"),
            panels: var("ONEPAINT_PANELS").is_some(),
            view: var("ONEPAINT_VIEW"),
            new: var("ONEPAINT_NEW").and_then(|v| {
                let (w, h) = v.split_once('x')?;
                Some((w.parse().ok()?, h.parse().ok()?))
            }),
        }
    }
}

fn wave(app: &mut App, y: f64, amp: f64, color: Rgba, preset: &str, size: f64) {
    let _ = app.session.execute(Command::SetBrush { preset_id: preset.into() });
    let _ = app.session.execute(Command::SetBrushSize { size });
    app.set_color(color);
    let w = app.session.doc().width as f64;
    let n = 80;
    for i in 0..=n {
        let f = i as f64 / n as f64;
        let p = StrokePoint::at(w * 0.12 + f * w * 0.76, y + (f * 9.0).sin() * amp, (f * std::f64::consts::PI).sin().max(0.1), f * 900.0);
        if i == 0 {
            app.session.stroke_begin(p);
        } else {
            app.session.stroke_extend(&[p]);
        }
    }
    app.session.stroke_end();
    app.push_recent();
}

pub fn tick(app: &mut App, ctx: &egui::Context) {
    let frame = app.debug.frame;
    app.debug.frame += 1;
    if frame == 1
        && let Some((w, h)) = app.debug.new
    {
        app.new_document(w, h);
    }
    if frame == 3
        && let Some(v) = &app.debug.view
    {
        use onepaint_color::cmyk::ViewModes;
        let modes = match v.as_str() {
            "proof" => ViewModes { proof: true, ..Default::default() },
            "warn" => ViewModes { gamut_warn: 2.0, ..Default::default() },
            _ => ViewModes { grayscale: true, ..Default::default() },
        };
        app.canvas.set_view_modes(modes);
    }
    if frame == 2 && app.debug.demo {
        let h = app.session.doc().height as f64;
        let hex = |s: &str| Rgba::from_hex(s).unwrap_or_default();
        wave(app, h * 0.3, 60.0, hex("#4c8dff"), "soft-round", 60.0);
        let _ = app.session.execute(Command::AddLayer);
        wave(app, h * 0.5, 90.0, hex("#e5484d"), "hard-round", 28.0);
        let id = app.session.active().to_string();
        let _ = app.session.execute(Command::SetLayerProps { id, props: LayerProps { opacity: Some(0.8), ..Default::default() } });
        let _ = app.session.execute(Command::AddLayer);
        wave(app, h * 0.68, 40.0, hex("#1d1d1f"), "comic-inker", 14.0);
        wave(app, h * 0.82, 20.0, hex("#ddb42c"), "soft-airbrush", 120.0);
        let _ = app.session.execute(Command::SetBrush { preset_id: "hard-round".into() });
        if let Some(mode) = app.debug.dock.clone() {
            dock_demo(app, &mode);
        }
        if app.debug.popover == Some(Popover::Selection) {
            let (w, h) = (app.session.doc().width as f64, app.session.doc().height as f64);
            let contour = onepaint_selection::ellipse_polygon([w * 0.3, h * 0.2], [w * 0.7, h * 0.75]);
            let _ = app.session.execute(Command::SelectShape { contours: vec![contour], mode: onepaint_selection::Mode::Replace });
            app.set_tool(onepaint_engine::Tool::SelectEllipse);
        }
    }
    if frame == 3 && app.debug.text {
        let spec = onepaint_text::TextSpec {
            text: "onepaint\ntext layers".into(),
            x: 700.0,
            y: 520.0,
            font_size_px: 120.0,
            bold: true,
            color: "#4c8dff".into(),
            outline_width: Some(6.0),
            outline_color: Some("#1d1d1f".into()),
            rotation: -0.12,
            balloon: Some(onepaint_text::balloon::Balloon {
                tail: Some(onepaint_text::balloon::tail_from_angle(120.0, 120.0)),
                ..onepaint_text::balloon::default_balloon(onepaint_text::balloon::BalloonShape::Oval)
            }),
            ..Default::default()
        };
        // A rich run: "text" in red italics.
        let spec = onepaint_text::runs::apply_style(&spec, 9..13, &onepaint_text::runs::StyleEdit::Color("#e5484d".into()));
        let spec = onepaint_text::runs::apply_style(&spec, 9..13, &onepaint_text::runs::StyleEdit::Italic);
        let _ = app.session.execute(Command::AddText { spec: spec.clone() });
        app.set_tool(onepaint_engine::Tool::Text);
        let id = app.session.active().to_string();
        app.canvas.text_edit = Some(crate::canvas::TextEdit { target: Some((id, 0)), spec });
    }
    if frame == 3 && app.debug.smudge {
        use onepaint_engine::{Symmetry, SymmetryMode, Tool};
        let (w, h) = (app.session.doc().width as f64, app.session.doc().height as f64);
        let drag = |app: &mut App, x: f64, y0: f64, y1: f64| {
            for i in 0..=60 {
                let p = StrokePoint::at(x + (i as f64 * 0.3).sin() * 6.0, y0 + (y1 - y0) * i as f64 / 60.0, 1.0, i as f64 * 16.0);
                if i == 0 {
                    app.session.stroke_begin(p);
                } else {
                    app.session.stroke_extend(&[p]);
                }
            }
            app.session.stroke_end();
        };
        let _ = app.session.execute(Command::SetBrush { preset_id: "soft-round".into() });
        let _ = app.session.execute(Command::SetBrushSize { size: 70.0 });
        app.set_tool(Tool::Smudge);
        let _ = app.session.execute(Command::SetSmudgeStrength { strength: 0.8 });
        for k in 0..5 {
            drag(app, w * (0.2 + k as f64 * 0.06), h * 0.55, h * 0.95);
        }
        app.set_tool(Tool::Mixer);
        app.set_color(Rgba::from_hex("#ddb42c").unwrap_or_default());
        for k in 0..4 {
            drag(app, w * (0.55 + k as f64 * 0.06), h * 0.2, h * 0.9);
        }
        let _ = app.session.execute(Command::SetSymmetry {
            symmetry: Symmetry { mode: SymmetryMode::Radial, radial_count: 8, cx: 0.5, cy: 0.5, angle: 0.0 },
        });
        app.set_tool(Tool::Liquify);
        let _ = app.session.execute(Command::SetBrushSize { size: 160.0 });
        let _ = app.session.execute(Command::SetLiquify { mode: onepaint_filters::liquify::LiquifyMode::TwirlLeft, strength: 0.3 });
        drag(app, w * 0.3, h * 0.67, h * 0.69);
        app.set_tool(Tool::Brush);
        let _ = app.session.execute(Command::SetBrush { preset_id: "hard-round".into() });
        let _ = app.session.execute(Command::SetBrushSize { size: 10.0 });
        app.set_color(Rgba::from_hex("#a06cf0").unwrap_or_default());
        for i in 0..=40 {
            let f = i as f64 / 40.0;
            let p = StrokePoint::at(w * 0.5 + 80.0 + f * 260.0, h * 0.5 + (f * 7.0).sin() * 50.0, 1.0, f * 600.0);
            if i == 0 {
                app.session.stroke_begin(p);
            } else {
                app.session.stroke_extend(&[p]);
            }
        }
        app.session.stroke_end();
    }
    if frame == 3 && app.debug.wet {
        let h = app.session.doc().height as f64;
        let hex = |s: &str| Rgba::from_hex(s).unwrap_or_default();
        let _ = app.session.execute(Command::AddLayer);
        wave(app, h * 0.45, 40.0, hex("#2f6690"), "watercolor-live", 90.0);
        wave(app, h * 0.55, 60.0, hex("#e8c547"), "watercolor-live", 70.0);
    }
    if frame == 3 && app.debug.impasto {
        let (w, h) = (app.session.doc().width as f64, app.session.doc().height as f64);
        let hex = |s: &str| Rgba::from_hex(s).unwrap_or_default();
        let _ = app.session.execute(Command::AddLayer);
        wave(app, h * 0.2, 50.0, hex("#c8553d"), "thick-paint", 70.0);
        wave(app, h * 0.38, 40.0, hex("#2f6690"), "impasto", 80.0);
        wave(app, h * 0.56, 30.0, hex("#e8c547"), "oil-paint", 90.0);
        let _ = app.session.execute(Command::SetBrush { preset_id: "oil-blender".into() });
        let _ = app.session.execute(Command::SetBrushSize { size: 120.0 });
        for i in 0..=40 {
            let p = StrokePoint::at(w * 0.7, h * 0.12 + i as f64 * h * 0.012, 1.0, i as f64 * 16.0);
            if i == 0 {
                app.session.stroke_begin(p);
            } else {
                app.session.stroke_extend(&[p]);
            }
        }
        app.session.stroke_end();
        let _ = app.session.execute(Command::AddLayer);
        let contour = onepaint_selection::ellipse_polygon([w * 0.3, h * 0.68], [w * 0.7, h * 0.92]);
        let _ = app.session.execute(Command::SelectShape { contours: vec![contour], mode: onepaint_selection::Mode::Replace });
        app.set_color(hex("#7a9e7e"));
        let _ = app.session.execute(Command::FillSelection);
        let _ = app.session.execute(Command::Deselect);
        let id = app.session.active().to_string();
        let _ = app.session.execute(Command::BevelFromAlpha { id, depth: 0.6, softness: 0.25 });
    }
    if frame == 3 && app.debug.guides {
        let mut g = app.session.guides().clone();
        g.grid.enabled = true;
        g.grid.size = 128.0;
        g.grid.subdivisions = 2;
        g.perspective.enabled = true;
        if std::env::var("ONEPAINT_GUIDES").is_ok_and(|v| v == "fisheye") {
            g.perspective.curved = true;
            g.perspective.points = onepaint_engine::guides::perspective_preset(5);
        }
        g.snap_hv = true;
        g.rulers = true;
        let (w, h) = (app.session.doc().width as f64, app.session.doc().height as f64);
        g.lines = vec![
            onepaint_engine::guides::GuideLine::new("a", w * 0.25, h * 0.4, 0.0),
            onepaint_engine::guides::GuideLine::new("b", w * 0.7, h * 0.5, 1.2),
        ];
        let _ = app.session.execute(Command::SetGuides { guides: Box::new(g) });
        app.canvas.persp_editing = true;
        app.canvas.measure = Some(([w * 0.2, h * 0.75], [w * 0.55, h * 0.6]));
    }
    if frame == 3 && app.debug.pen {
        use onepaint_stroke::bezier::*;
        let (w, h) = (app.session.doc().width as f64, app.session.doc().height as f64);
        let pts = [[w * 0.3, h * 0.35], [w * 0.5, h * 0.2], [w * 0.7, h * 0.4], [w * 0.55, h * 0.7], [w * 0.35, h * 0.6]];
        let spec = PathSpec {
            subpaths: vec![Subpath { anchors: catmull_rom_anchors(&pts, true), closed: true }],
            stroke: Some(PathStroke { brush_id: "comic-inker".into(), size: 14.0, color: [0.1, 0.1, 0.12], opacity: 1.0, seed: Some(9) }),
            fill: Some(PathFill { color: [0.3, 0.55, 1.0], opacity: 0.6 }),
        };
        let _ = app.session.execute(Command::CreatePathLayer { spec: Box::new(spec) });
        app.set_tool(onepaint_engine::Tool::Pen);
    }
    if frame == 3 && app.debug.panels {
        let o = app.canvas.panel_options;
        let _ = app.session.execute(Command::CreatePanelGrid {
            rows: 2,
            cols: 2,
            gutter: o.gutter as f64,
            margin: o.margin as f64,
            border: o.border as f64,
        });
        if let Some((id, outline)) = app.session.panel_frames().into_iter().nth(3) {
            let (cx, cy) = (outline.iter().map(|p| p.x).sum::<f64>() / 4.0, outline.iter().map(|p| p.y).sum::<f64>() / 4.0);
            let _ = app.session.execute(Command::SplitPanel { id, a: [cx - 300.0, cy - 400.0], b: [cx + 300.0, cy + 400.0] });
        }
        // Ink across the first panel's frame: the art stops at it.
        if let Some((_, o)) = app.session.panel_frames().into_iter().next() {
            let y = (o[0].y + o[2].y) / 2.0;
            let _ = app.session.execute(Command::SetBrush { preset_id: "comic-inker".into() });
            let _ = app.session.execute(Command::SetBrushSize { size: 40.0 });
            app.set_color(Rgba::new(0.1, 0.1, 0.14, 1.0));
            let (x0, x1) = (o[0].x - 120.0, o[1].x + 120.0);
            for i in 0..=40 {
                let f = i as f64 / 40.0;
                let p = StrokePoint::at(x0 + (x1 - x0) * f, y + (f * 6.0).sin() * 80.0, 0.8, f * 600.0);
                if i == 0 { app.session.stroke_begin(p) } else { app.session.stroke_extend(&[p]) }
            }
            app.session.stroke_end();
        }
        app.set_tool(onepaint_engine::Tool::Panel);
    }
    if frame == 3 && app.debug.anim {
        let bg = app.session.doc().root[0].id().to_string();
        let _ = app.session.execute(Command::SetFrameRole { id: bg, role: Some("background".into()) });
        crate::timeline::toggle(app);
        let _ = app.session.execute(Command::SetAnimSlot { slot: 1 });
        if app.debug.tracks {
            let _ = app.session.execute(Command::ConvertToTracks);
            let _ = app.session.execute(Command::AddTrack);
            let t = onepaint_engine::anim::tracks_of(&app.session.doc().root);
            if let Some(tr) = t.iter().find(|t| t.sheets.as_ref().is_some_and(|s| s.len() > 1)) {
                let last = tr.sheets.as_ref().and_then(|s| s.last()).map(|f| f.id.clone()).unwrap_or_default();
                let _ = app.session.execute(Command::MoveSheet { id: last, column: 6 });
            }
            let mut a = app.session.anim_settings().clone();
            a.add_marker(2);
            (a.mark_in, a.mark_out) = (Some(1), Some(5));
            let _ = app.session.execute(Command::SetAnimation { settings: Box::new(a) });
            let _ = app.session.execute(Command::SetAnimSlot { slot: 3 });
        }
        if app.debug.pegs {
            let ids: Vec<String> = app.session.anim_frames().into_iter().map(|f| f.id).collect();
            app.timing.set_select_mode(true);
            app.timing.selected = ids.iter().skip(1).take(2).cloned().collect();
            if let Some(first) = ids.first() {
                let mut a = app.session.anim_settings().clone();
                a.out_of_pegs.insert(first.clone(), onepaint_engine::anim::Peg { dx: 120.0, dy: 40.0, rot: 0.3, scale: 1.2 });
                let _ = app.session.execute(Command::SetAnimation { settings: Box::new(a) });
                app.timing.set_peg_mode(true);
                app.timing.peg_target = Some(first.clone());
            }
        }
    }
    if frame == 3 && app.debug.transform {
        app.set_tool(onepaint_engine::Tool::Move);
        let p = onepaint_engine::transform::TransformParams { rotation: 0.25, sx: 0.8, sy: 0.8, dx: 60.0, dy: -40.0 };
        app.canvas.xform_about_centre(&mut app.session, |_| p);
    }
    if frame == 3 && app.debug.popover == Some(Popover::Filter) {
        let _ = app.session.execute(Command::BeginFilter { kind: "curves".into() });
        let s_curve = vec![
            onepaint_stroke::CurvePoint::new(0.0, 0.0),
            onepaint_stroke::CurvePoint::new(0.25, 0.12),
            onepaint_stroke::CurvePoint::new(0.75, 0.9),
            onepaint_stroke::CurvePoint::new(1.0, 1.0),
        ];
        let _ = app.session.execute(Command::UpdateFilter {
            params: onepaint_filters::Params { curve: Some(s_curve), curve_mode: Some("smooth".into()), ..Default::default() },
        });
    }
    if frame == 3 && app.debug.popover.is_some() {
        app.popover = app.debug.popover;
    }
    if frame == 8 && app.debug.popover == Some(Popover::BrushStudio) {
        let lin = app.session.color().to_linear_premul();
        let b = app.session.current_brush().clone();
        app.studio.scribble(&b, [lin[0], lin[1], lin[2]]);
    }
    if frame == 12 && app.debug.shot.is_some() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }
    let active = app.debug.demo || app.debug.popover.is_some() || app.debug.shot.is_some();
    if active && frame < 40 {
        ctx.request_repaint();
    }
    let shots: Vec<_> = ctx.input(|i| {
        i.raw.events.iter().filter_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }).collect()
    });
    for img in shots {
        if let Some(path) = app.debug.shot.take() {
            let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            match onepaint_formats::encode_png_rgba8(img.size[0] as u32, img.size[1] as u32, &rgba).map(|b| std::fs::write(&path, b)) {
                Ok(Ok(())) => log::info!("screenshot saved to {path}"),
                e => log::error!("screenshot failed: {e:?}"),
            }
            if app.debug.exit {
                // A scripted run never asks to save its demo drawing.
                app.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

fn dock_demo(app: &mut App, mode: &str) {
    use crate::layout::{DockSide, Orientation, PanelId, Placement};
    let l = &mut app.layout;
    let ids = app.session.doc().layer_ids();
    app.layer_picks = ids.iter().rev().take(2).cloned().collect();
    if mode == "float" {
        l.toolbar.orientation = Orientation::Vertical;
        l.toolbar.pos = Some([300.0, 90.0]);
        l.set_placement(PanelId::Align, Placement::Float { x: 760.0, y: 160.0 }, None);
        l.join_group(PanelId::Color, PanelId::Align, egui::Rect::from_min_size(egui::pos2(760.0, 160.0), egui::vec2(300.0, 420.0)));
        l.set_open(PanelId::Align, true);
        l.set_open(PanelId::Color, true);
        return;
    }
    l.tools_dock = Some(DockSide::Left);
    l.set_placement(PanelId::Layers, Placement::Dock { side: DockSide::Right }, Some(0));
    l.set_placement(PanelId::Brush, Placement::Dock { side: DockSide::Left }, None);
    l.set_placement(PanelId::Align, Placement::Float { x: 520.0, y: 120.0 }, None);
    for id in [PanelId::Layers, PanelId::Properties, PanelId::Brush, PanelId::Align] {
        l.set_open(id, true);
    }
}
