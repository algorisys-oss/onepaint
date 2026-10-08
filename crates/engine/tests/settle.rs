#![allow(clippy::unwrap_used)]
//! Watercolour settle (spec 35 D) runs when a paint stroke flattens.
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn paint(preset: onepaint_brush::BrushPreset) -> Session {
    let mut s = Session::new(240, 140, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.1, 0.3, 0.7, 1.0) }).unwrap();
    s.execute(Command::SetBrushPreset { preset: Box::new(preset) }).unwrap();
    s.execute(Command::SetBrushSize { size: 60.0 }).unwrap();
    let pt = |x: f64| StrokePoint { x, y: 70.0, pressure: 1.0, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None };
    s.stroke_begin(pt(30.0));
    s.stroke_extend(&(0..=90).map(|i| pt(30.0 + i as f64 * 2.0)).collect::<Vec<_>>());
    s.stroke_end();
    s
}

fn alpha(s: &Session, x: i32, y: i32) -> f32 {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)[3]
}

#[test]
fn a_watercolour_wash_dries_with_a_darker_rim() {
    let wash = onepaint_brush::preset("watercolor-wash").unwrap();
    assert!(wash.settle().is_some());
    let mut plain = wash.clone();
    plain.extra.remove("settle");
    let (settled, flat) = (paint(wash), paint(plain));
    // Across the stroke, the settled rim outweighs the plain one relative to the body.
    let profile = |s: &Session| (40..=100).map(|y| alpha(s, 120, y)).collect::<Vec<_>>();
    let (a, b) = (profile(&settled), profile(&flat));
    let edge = |p: &[f32]| p.iter().copied().fold(0.0, f32::max) / (p[22..39].iter().sum::<f32>() / 17.0);
    assert!(edge(&a) > edge(&b) + 0.05, "settled {a:?}\nplain {b:?}");
    assert!(a.iter().zip(&b).all(|(x, y)| *x >= y * 0.45 - 1e-3), "never thinner than the floor");
    // One undo step, as any stroke.
    let mut settled = settled;
    settled.execute(Command::Undo).unwrap();
    assert!(settled.doc().layer(settled.active()).unwrap().pixels.tile_count() == 0);
}
