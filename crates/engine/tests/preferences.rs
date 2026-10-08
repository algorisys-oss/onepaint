#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

/// A horizontal stroke at `pressure`; returns the painted band's height at x = 100.
fn band(s: &mut Session, pressure: f64) -> i32 {
    let pt = |x: f64| StrokePoint { x, y: 100.0, pressure, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None };
    s.stroke_begin(pt(40.0));
    let pts: Vec<_> = (0..80).map(|i| pt(40.0 + i as f64 * 2.0)).collect();
    s.stroke_extend(&pts);
    s.stroke_end();
    let l = s.doc().layer(s.active()).unwrap();
    let ys: Vec<i32> = (0..200).filter(|&y| l.pixels.get(100, y)[3] > 0.5).collect();
    let h = ys.len() as i32;
    s.execute(Command::Undo).unwrap();
    h
}

fn session() -> Session {
    let mut s = Session::new(240, 200, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetBrushSize { size: 40.0 }).unwrap();
    let mut b = s.current_brush().clone();
    (b.size_pressure, b.dynamics) = (0.0, None);
    s.execute(Command::SetBrushPreset { preset: Box::new(b) }).unwrap();
    s
}

#[test]
fn forced_size_pressure_overrides_the_brush() {
    let mut s = session();
    let free = band(&mut s, 0.3);
    s.set_preferences(Preferences { force_size_pressure: true, ..Default::default() });
    let forced = band(&mut s, 0.3);
    assert!(free >= 36 && forced < free * 2 / 3, "free {free}, forced {forced}");
}

#[test]
fn screen_relative_size_divides_by_the_zoom() {
    let mut s = session();
    let canvas = band(&mut s, 1.0);
    s.set_preferences(Preferences { screen_zoom: Some(2.0), ..Default::default() });
    let screen = band(&mut s, 1.0);
    assert!((screen * 2 - canvas).abs() <= 3, "canvas {canvas}, screen {screen}");
}

#[test]
fn size_ceiling_and_history_cap() {
    let mut s = session();
    s.execute(Command::SetBrushSize { size: 5000.0 }).unwrap();
    assert_eq!(s.current_brush().size, 1000.0);
    s.set_preferences(Preferences { max_brush_size: 4000.0, max_history_steps: 5, ..Default::default() });
    s.execute(Command::SetBrushSize { size: 5000.0 }).unwrap();
    assert_eq!(s.current_brush().size, 4000.0);
    s.set_preferences(Preferences { max_brush_size: 50.0, max_history_steps: 5, ..Default::default() });
    assert_eq!(s.current_brush().size, 50.0, "lowering the ceiling shrinks the brush");
    for _ in 0..8 {
        s.execute(Command::AddLayer).unwrap();
    }
    let mut undos = 0;
    while s.can_undo() {
        s.execute(Command::Undo).unwrap();
        undos += 1;
    }
    assert_eq!(undos, 5);
}
