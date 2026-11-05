//! The pulled-string stabilizer's string is measured on screen (happypaint
//! `stabilizer.test.ts`, v0.204.17).
use onepaint_stroke::*;

/// Feed the cursor along x to `to` (doc px); how far the brush trails it.
fn trail(strength: f64, view_scale: f64, to: f64) -> f64 {
    let mut s = Stabilizer::new(strength, view_scale);
    let mut brush = 0.0;
    let mut x = 0.0;
    while x <= to {
        if let Some(p) = s.next(StrokePoint { x, pressure: 0.5, ..Default::default() }) {
            brush = p.x;
        }
        x += 0.5;
    }
    to - brush
}

#[test]
fn the_string_is_the_same_on_screen_at_every_zoom() {
    // Strength 0.5: a 60 px string on screen.
    for zoom in [0.5, 1.0, 2.0, 4.0, 8.0] {
        let on_screen = trail(0.5, zoom, 400.0) * zoom;
        assert!((on_screen - 60.0).abs() < 0.5, "zoom {zoom}: {on_screen} px on screen");
    }
}

#[test]
fn a_bad_view_scale_falls_back_to_100_percent() {
    for z in [0.0, -2.0, f64::NAN, f64::INFINITY] {
        assert!((trail(0.5, z, 400.0) - 60.0).abs() < 0.5, "zoom {z}");
    }
}
