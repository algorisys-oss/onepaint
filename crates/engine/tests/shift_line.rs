#![allow(clippy::unwrap_used)]
//! Shift+click straight lines (happypaint v0.204.14): a line from where the
//! last stroke ended, chained, re-aimed while held, one undo step each.
use onepaint_color::Rgba;
use onepaint_engine::recording::Replay;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn pt(x: f64, y: f64) -> StrokePoint {
    StrokePoint { x, y, pressure: 0.8, ..Default::default() }
}

fn session(preset: &str) -> Session {
    let mut s = Session::new(300, 200, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SetBrush { preset_id: preset.into() }).unwrap();
    s
}

fn alpha(s: &Session, x: i32, y: i32) -> f32 {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)[3]
}

/// A short dab-like stroke at `(x, y)`.
fn dot(s: &mut Session, x: f64, y: f64) {
    s.stroke_begin(pt(x, y));
    s.stroke_extend(&[pt(x + 0.5, y)]);
    s.stroke_end();
}

#[test]
fn a_line_joins_the_last_stroke_end_to_the_press() {
    let mut s = session("hard-round");
    assert_eq!(s.last_stroke_end(), None);
    dot(&mut s, 40.0, 100.0);
    assert_eq!(s.last_stroke_end(), Some([40.5, 100.0]));
    let from = s.last_stroke_end().unwrap();
    s.stroke_line(pt(from[0], from[1]), pt(240.0, 100.0));
    s.stroke_end();
    for x in [60, 140, 230] {
        assert!(alpha(&s, x, 100) > 0.5, "inked at x = {x}");
    }
    assert_eq!(s.last_stroke_end(), Some([240.0, 100.0]), "the next line chains from here");
}

#[test]
fn re_aiming_redraws_the_line_and_it_is_one_undo_step() {
    let mut s = session("hard-round");
    dot(&mut s, 40.0, 100.0);
    let h0 = s.history_len();
    // Aim at (240, 40), then (240, 160) before release.
    s.stroke_line(pt(40.5, 100.0), pt(240.0, 40.0));
    s.stroke_line(pt(40.5, 100.0), pt(240.0, 160.0));
    s.stroke_end();
    assert_eq!(s.history_len(), h0 + 1);
    assert!(alpha(&s, 200, 145) > 0.5, "the final aim is drawn");
    assert_eq!(alpha(&s, 200, 55), 0.0, "the first aim is gone");
    s.execute(Command::Undo).unwrap();
    assert_eq!(alpha(&s, 200, 145), 0.0);
    assert!(alpha(&s, 40, 100) > 0.5, "the dot stays");
}

#[test]
fn the_line_is_straight_and_untapered_on_an_inking_brush() {
    // G-pen: smoothing 0.3, entry 18 / exit 34 px taper.
    let mut s = session("manga-g-pen");
    s.execute(Command::SetBrushSize { size: 12.0 }).unwrap();
    s.stroke_line(pt(40.0, 100.0), pt(240.0, 100.0));
    s.stroke_end();
    let width = |s: &Session, x: i32| (80..120).filter(|y| alpha(s, x, *y) > 0.5).count();
    let (start, mid, end) = (width(&s, 44), width(&s, 140), width(&s, 236));
    assert!(start + 1 >= mid && end + 1 >= mid, "no taper: {start} / {mid} / {end}");
    assert!(alpha(&s, 239, 100) > 0.5, "reaches the press");
}

#[test]
fn a_cancelled_stroke_leaves_no_end_to_chain_from() {
    let mut s = session("hard-round");
    s.stroke_begin(pt(40.0, 100.0));
    s.stroke_extend(&[pt(80.0, 100.0)]);
    s.stroke_cancel();
    assert_eq!(s.last_stroke_end(), None);
}

#[test]
fn a_line_replays_exactly() {
    let mut s = session("manga-g-pen");
    s.start_recording().unwrap();
    dot(&mut s, 40.0, 100.0);
    s.stroke_line(pt(40.5, 100.0), pt(200.0, 60.0));
    s.stroke_line(pt(40.5, 100.0), pt(240.0, 150.0));
    s.stroke_end();
    let rec = s.stop_recording().unwrap();
    let mut r = Replay::new(&rec).unwrap();
    r.step(usize::MAX / 2);
    assert!(r.done());
    assert_eq!(r.session.flatten(), s.flatten(), "pixel-identical");
}
