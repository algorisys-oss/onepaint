//! Debug automation driven by environment variables, for visual checks:
//!
//! - `ONEPAINT_SCREENSHOT=out.png` saves a screenshot of the window after a few frames.
//! - `ONEPAINT_DEMO=1` paints a small demo scene first.
//! - `ONEPAINT_POPOVER=layers|brushes|color|menu|new` opens a popover.
//! - `ONEPAINT_THEME=light` starts in the light theme.
//! - `ONEPAINT_TRANSFORM=1` starts a rotated move on the active layer.
//! - `ONEPAINT_TEXT=1` adds a text object and opens its editor.
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
}

impl Debug {
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
                _ => None,
            }),
            exit: var("ONEPAINT_EXIT").is_some(),
            light: var("ONEPAINT_THEME").is_some_and(|t| t == "light"),
            transform: var("ONEPAINT_TRANSFORM").is_some(),
            text: var("ONEPAINT_TEXT").is_some(),
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
            ..Default::default()
        };
        let _ = app.session.execute(Command::AddText { spec: spec.clone() });
        app.set_tool(onepaint_engine::Tool::Text);
        let id = app.session.active().to_string();
        app.canvas.text_edit = Some(crate::canvas::TextEdit { target: Some((id, 0)), spec });
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
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
