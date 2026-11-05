#![allow(clippy::unwrap_used)]
//! Live exit-taper tail (happypaint v0.204.15): a tapered brush draws right up
//! to the pen while the stroke is live, and lifting leaves exactly what was
//! on screen.
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn pt(x: f64) -> StrokePoint {
    StrokePoint { x, y: 60.0, pressure: 1.0, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None }
}

fn session(preset: &str) -> Session {
    let mut s = Session::new(320, 120, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.1, 0.1, 0.1, 1.0) }).unwrap();
    s.execute(Command::SetBrushPreset { preset: Box::new(onepaint_brush::preset(preset).unwrap()) }).unwrap();
    s
}

/// Every tile of the 320×120 document, composited with the live stroke as
/// the next frame shows it.
fn screen(s: &mut Session) -> Vec<Vec<f32>> {
    s.take_dirty_tiles();
    let keys = [(0, 0), (1, 0)];
    let mut tiles = s.composite(&keys);
    tiles.sort_by_key(|(k, _)| *k);
    tiles.into_iter().map(|(_, t)| t.to_f32()).collect()
}

fn ink_at(s: &mut Session, x: usize) -> f32 {
    // White paper, dark ink: how far the red channel dropped at (x, 60).
    let t = &screen(s)[x / 256];
    1.0 - t[((60 * 256) + (x % 256)) * 4]
}

#[test]
fn ink_reaches_the_pen_while_drawing() {
    // G-pen tapers its last 34 px; the pen is at x = 260.
    let mut s = session("manga-g-pen");
    s.stroke_begin(pt(20.0));
    s.stroke_extend(&(0..=120).map(|i| pt(20.0 + i as f64 * 2.0)).collect::<Vec<_>>());
    assert!(ink_at(&mut s, 240) > 0.3, "ink 20 px behind the nib: {}", ink_at(&mut s, 240));
    s.stroke_cancel();
}

#[test]
fn lifting_leaves_what_was_on_screen() {
    for preset in ["manga-g-pen", "comic-inker"] {
        let mut s = session(preset);
        s.stroke_begin(pt(20.0));
        for i in 1..=60 {
            s.stroke_extend(&[pt(20.0 + i as f64 * 4.0)]);
        }
        let live = screen(&mut s);
        s.stroke_end();
        let after = screen(&mut s);
        let diff = live.iter().flatten().zip(after.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(diff <= 1.0 / 255.0, "{preset}: lift changed the screen by {diff}");
    }
}

#[test]
fn the_live_tail_is_tapered_to_a_point_at_the_pen() {
    let mut s = session("manga-g-pen");
    s.stroke_begin(pt(20.0));
    s.stroke_extend(&(0..=120).map(|i| pt(20.0 + i as f64 * 2.0)).collect::<Vec<_>>());
    // Full width in the body, thinning toward the nib.
    let width = |s: &mut Session, x: usize| {
        let t = &screen(s)[x / 256];
        (0..120).filter(|y| 1.0 - t[((y * 256) + (x % 256)) * 4] > 0.5).count()
    };
    let (body, near) = (width(&mut s, 200), width(&mut s, 240));
    assert!(near > 0 && near < body, "tail at the nib ({near}) inked but thinner than the body ({body})");
    s.stroke_cancel();
}
