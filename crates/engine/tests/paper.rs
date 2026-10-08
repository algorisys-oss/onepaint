#![allow(clippy::unwrap_used)]
//! Document paper (spec 36): dry brushes bite its tooth, the canvas shows it,
//! the wet sim soaks into it, and it travels with the file.
use onepaint_color::Rgba;
use onepaint_doc::{PAPER_PRESETS, Paper};
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn rough() -> Option<Paper> {
    PAPER_PRESETS.iter().find(|p| p.id == "rough").and_then(|p| p.paper())
}

fn session(paper: Option<Paper>) -> Session {
    let mut s = Session::new(300, 120, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetPaper { paper }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.2, 0.2, 0.2, 1.0) }).unwrap();
    s
}

/// A straight stroke; returns the alpha along its centre line.
fn stroke(s: &mut Session, preset: &str) -> Vec<f32> {
    s.execute(Command::SetBrushPreset { preset: Box::new(onepaint_brush::preset(preset).unwrap()) }).unwrap();
    s.execute(Command::SetBrushSize { size: 30.0 }).unwrap();
    let pt = |x: f64| StrokePoint { x, y: 60.0, pressure: 1.0, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None };
    s.stroke_begin(pt(20.0));
    s.stroke_extend(&(0..=130).map(|i| pt(20.0 + i as f64 * 2.0)).collect::<Vec<_>>());
    s.stroke_end();
    let l = s.doc().layer(s.active()).unwrap();
    (60..240).map(|x| l.pixels.get(x, 60)[3]).collect()
}

fn spread(v: &[f32]) -> f32 {
    let m = v.iter().sum::<f32>() / v.len() as f32;
    (v.iter().map(|a| (a - m) * (a - m)).sum::<f32>() / v.len() as f32).sqrt()
}

#[test]
fn dry_media_bite_the_tooth_and_inks_ignore_it() {
    let (smooth, paper) = (stroke(&mut session(None), "hard-round"), stroke(&mut session(rough()), "hard-round"));
    assert!(spread(&paper) > spread(&smooth) + 0.05, "the tooth breaks up a dry stroke: {} vs {}", spread(&paper), spread(&smooth));
    assert!(paper.iter().all(|a| *a > 0.0), "never fully skipped (0.12 floor)");
    // Ink has paper response 0: identical on any paper.
    assert_eq!(stroke(&mut session(None), "ink"), stroke(&mut session(rough()), "ink"));
}

#[test]
fn painting_over_paint_fills_the_tooth() {
    let mut s = session(rough());
    let first = stroke(&mut s, "hard-round");
    let second = stroke(&mut s, "hard-round");
    assert!(spread(&second) < spread(&first), "a second pass fills the valleys");
}

#[test]
fn the_canvas_shows_the_tooth_but_the_pixels_stay_clean() {
    let mut s = session(rough());
    let shown: Vec<f32> = (0..100).map(|x| onepaint_compose::composite_px(s.doc(), x, 10)[0]).collect();
    assert!(shown.iter().all(|v| *v <= 1.0) && spread(&shown) > 0.01, "white paper shows its tooth");
    let bg = s.doc().root[0].as_layer().unwrap().pixels.get(10, 10);
    assert_eq!(bg, [1.0; 4], "the background layer is untouched");
    // Smooth again: plain white.
    s.execute(Command::SetPaper { paper: None }).unwrap();
    assert_eq!(onepaint_compose::composite_px(s.doc(), 10, 10), [1.0; 4]);
}

#[test]
fn paper_travels_with_the_file_and_has_no_undo_step() {
    let mut s = session(None);
    let had_undo = s.can_undo();
    s.execute(Command::SetPaper { paper: rough() }).unwrap();
    assert_eq!(s.can_undo(), had_undo, "choosing paper isn't an undo step");
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    assert_eq!(t.doc().paper(), rough());
}

#[test]
fn wet_washes_granulate_on_textured_paper() {
    let wash = |paper| {
        let mut s = session(paper);
        s.execute(Command::SetBrushPreset { preset: Box::new(onepaint_brush::preset("watercolor-live").unwrap()) }).unwrap();
        s.execute(Command::SetBrushSize { size: 40.0 }).unwrap();
        let pt = |x: f64| StrokePoint { x, y: 60.0, pressure: 0.7, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None };
        s.stroke_begin(pt(40.0));
        s.stroke_extend(&(0..=100).map(|i| pt(40.0 + i as f64 * 2.0)).collect::<Vec<_>>());
        s.stroke_end();
        let mut now = 0.0;
        for _ in 0..60 {
            now += 16.7;
            s.wet_tick(now);
        }
        s.execute(Command::DryWet).unwrap();
        let l = s.doc().layer(s.active()).unwrap();
        (100..200).map(|x| l.pixels.get(x, 60)[3]).collect::<Vec<f32>>()
    };
    let (flat, grainy) = (wash(None), wash(rough()));
    assert!(spread(&grainy) > spread(&flat) * 1.5 + 0.002, "granulation: {} vs {}", spread(&grainy), spread(&flat));
}
