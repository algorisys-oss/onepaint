#![allow(clippy::unwrap_used)]
//! Imported brushes (spec 23) are selectable through the engine and paint
//! with their image tip.

use onepaint_brush::{IMPORTED_CATEGORY, TipMask, UserLibrary, add_user_brushes, preset};
use onepaint_color::Rgba;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;
use std::sync::Arc;

/// A ring: coverage only between 60% and 95% of the radius.
fn ring(n: usize) -> TipMask {
    let c = n as f64 / 2.0;
    let data = (0..n * n)
        .map(|i| {
            let r = ((i % n) as f64 + 0.5 - c).hypot((i / n) as f64 + 0.5 - c) / c;
            if (0.6..0.95).contains(&r) { 1.0 } else { 0.0 }
        })
        .collect();
    TipMask { size: n, data }
}

fn dab(s: &mut Session) {
    s.stroke_begin(StrokePoint::at(100.0, 100.0, 1.0, 0.0));
    s.stroke_end();
}

#[test]
fn imported_preset_selects_and_paints_with_its_tip() {
    let mut p = preset("hard-round").unwrap();
    p.id = "imported-engine-ring".into();
    p.name = "Ring".into();
    p.category = IMPORTED_CATEGORY.into();
    p.tip_id = "custom:engine-ring".into();
    p.size = 60.0;
    p.size_pressure = 0.0;
    let mut lib = UserLibrary::default();
    lib.presets.push(p);
    lib.tips.insert("custom:engine-ring".into(), Arc::new(ring(128)));

    let mut s = Session::new(256, 256, Some(Rgba::WHITE));
    assert!(s.execute(Command::SetBrush { preset_id: "imported-engine-ring".into() }).is_err(), "unknown before import");
    add_user_brushes(lib);
    s.execute(Command::SetBrush { preset_id: "imported-engine-ring".into() }).unwrap();
    assert_eq!(s.current_brush().id, "imported-engine-ring");
    dab(&mut s);
    let px = |s: &Session, x, y| s.doc().layer(s.active()).unwrap().pixels.get(x, y)[3];
    assert!(px(&s, 100, 100) < 0.05, "the ring's hole: {}", px(&s, 100, 100));
    assert!(px(&s, 124, 100) > 0.5, "the ring: {}", px(&s, 124, 100));

    // The round tip would have covered the centre.
    s.execute(Command::Undo).unwrap();
    s.execute(Command::SetBrush { preset_id: "hard-round".into() }).unwrap();
    s.execute(Command::SetBrushSize { size: 60.0 }).unwrap();
    dab(&mut s);
    assert!(px(&s, 100, 100) > 0.5);
}
