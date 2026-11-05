#![allow(clippy::unwrap_used)]
//! The stabilizer's string is measured on screen (happypaint v0.204.17):
//! Studio Pen trails the pen by the same screen distance at every zoom, and a
//! recording replays with the zoom it was drawn at.
use onepaint_color::Rgba;
use onepaint_engine::recording::Replay;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn pt(x: f64, t: f64) -> StrokePoint {
    StrokePoint { x, y: 60.0, pressure: 0.7, t: Some(t), ..Default::default() }
}

fn session(zoom: f64) -> Session {
    let mut s = Session::new(400, 120, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SetBrush { preset_id: "studio-pen".into() }).unwrap();
    s.set_preferences(Preferences { view_zoom: zoom, ..Default::default() });
    s
}

/// Draw from x = 20 to x = 300 and report how far behind the pen the ink
/// ends before lift (doc px).
fn trail(zoom: f64) -> f64 {
    let mut s = session(zoom);
    s.stroke_begin(pt(20.0, 0.0));
    s.stroke_extend(&(1..=280).map(|i| pt(20.0 + i as f64, i as f64 * 4.0)).collect::<Vec<_>>());
    s.take_dirty_tiles();
    let keys = [(0, 0), (1, 0)];
    let tiles = s.composite(&keys);
    let ink = |x: i32| {
        let (_, t) = tiles.iter().find(|(k, _)| *k == ((x / 256), 0)).unwrap();
        1.0 - t.to_f32()[((60 * 256 + x % 256) * 4) as usize] > 0.2
    };
    let end = (0..400).rev().find(|x| ink(*x)).unwrap();
    s.stroke_cancel();
    300.0 - end as f64
}

#[test]
fn studio_pen_trails_by_the_same_screen_distance_at_every_zoom() {
    // Strength 0.5: a 60 px string on screen. Smoothing and the 12 px exit
    // taper add a little lag of their own, in doc px, at every zoom (so the
    // on-screen trail still creeps up a little, as happypaint measured).
    let at_100 = trail(1.0);
    let rest = at_100 - 60.0;
    assert!((0.0..20.0).contains(&rest), "60 px string plus a little: {at_100}");
    for zoom in [2.0, 4.0, 8.0] {
        let t = trail(zoom);
        assert!((t - (rest + 60.0 / zoom)).abs() < 3.0, "zoom {zoom}: trails {t} doc px, want {} ", rest + 60.0 / zoom);
    }
}

#[test]
fn a_recording_replays_with_the_zoom_it_was_drawn_at() {
    let mut s = session(4.0);
    s.start_recording().unwrap();
    s.stroke_begin(pt(20.0, 0.0));
    s.stroke_extend(&(1..=60).map(|i| pt(20.0 + i as f64 * 4.0, i as f64 * 8.0)).collect::<Vec<_>>());
    s.stroke_end();
    let rec = s.stop_recording().unwrap();
    // The replaying session sits at 100%.
    let mut r = Replay::new(&rec).unwrap();
    r.step(usize::MAX / 2);
    assert!(r.done());
    assert_eq!(r.session.flatten(), s.flatten(), "pixel-identical");
}

#[test]
fn the_zoom_is_not_stored_in_the_brush() {
    let mut s = session(3.0);
    s.stroke_begin(pt(20.0, 0.0));
    s.stroke_end();
    assert_eq!(s.current_brush().stabilizer.as_ref().unwrap().view_scale, None);
}
