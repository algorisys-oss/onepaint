#![allow(clippy::unwrap_used)]
//! Live wet media (spec 35 B): fluid strokes stay wet after the pen lifts,
//! dry on their own, and commit as one undo step.
use onepaint_color::Rgba;
use onepaint_engine::recording::Replay;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;

fn pt(x: f64, y: f64) -> StrokePoint {
    StrokePoint { x, y, pressure: 0.7, tilt_x: 0.0, tilt_y: 0.0, twist: 0.0, t: None }
}

fn session() -> Session {
    let mut s = Session::new(240, 160, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.1, 0.3, 0.8, 1.0) }).unwrap();
    s
}

/// A fluid brush; `water` small enough to dry within a short test.
fn fluid_brush(s: &mut Session, id: &str, water: Option<f64>) {
    let mut b = onepaint_brush::preset(id).unwrap();
    if let Some(w) = water {
        b.extra.get_mut("fluid").unwrap()["water"] = serde_json::json!(w);
    }
    s.execute(Command::SetTool { tool: Tool::Brush }).unwrap();
    s.execute(Command::SetBrushPreset { preset: Box::new(b) }).unwrap();
    s.execute(Command::SetBrushSize { size: 30.0 }).unwrap();
}

fn stroke(s: &mut Session, y: f64) {
    s.stroke_begin(pt(60.0, y));
    s.stroke_extend(&(0..=60).map(|i| pt(60.0 + i as f64 * 2.0, y)).collect::<Vec<_>>());
    s.stroke_end();
}

fn layer_alpha(s: &Session, x: i32, y: i32) -> f32 {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)[3]
}

/// Tick the sim at 60 fps until it dries (or `max` frames pass).
fn run(s: &mut Session, from: f64, max: usize) -> f64 {
    let mut now = from;
    for _ in 0..max {
        if !s.wet_active() {
            break;
        }
        now += 1000.0 / 60.0;
        s.wet_tick(now);
    }
    now
}

#[test]
fn a_wash_shows_while_wet_dries_by_itself_and_undoes_as_one_step() {
    let mut s = session();
    fluid_brush(&mut s, "watercolor-live", Some(0.05));
    stroke(&mut s, 80.0);
    assert!(s.wet_active(), "still wet after the pen lifts");
    // The canvas shows the suspended paint before it has settled.
    let shown = s.composite(&[(0, 0)]);
    let t = &shown[0].1;
    let p = t.get(120, 80);
    assert!(p[2] > p[0] + 0.05, "wet paint on screen: {p:?}");
    let now = run(&mut s, 0.0, 600);
    assert!(!s.wet_active(), "dried on its own");
    assert!(layer_alpha(&s, 120, 80) > 0.05, "settled into the layer");
    assert!(s.can_undo());
    s.execute(Command::Undo).unwrap();
    assert_eq!(layer_alpha(&s, 120, 80), 0.0, "the whole session is one step");
    s.execute(Command::Redo).unwrap();
    assert!(layer_alpha(&s, 120, 80) > 0.05);
    let _ = now;
}

#[test]
fn dry_commits_now_and_other_work_dries_first() {
    let mut s = session();
    fluid_brush(&mut s, "watercolor-live", None);
    stroke(&mut s, 50.0);
    run(&mut s, 0.0, 10);
    assert!(s.wet_active());
    s.execute(Command::DryWet).unwrap();
    assert!(!s.wet_active() && layer_alpha(&s, 120, 50) > 0.05);
    // Painting more wet paint, then a dry brush stroke: the wash dries first.
    stroke(&mut s, 110.0);
    assert!(s.wet_active());
    s.execute(Command::SetBrush { preset_id: "hard-round".into() }).unwrap();
    assert!(s.wet_active(), "choosing a brush keeps it wet");
    s.stroke_begin(pt(20.0, 20.0));
    s.stroke_end();
    assert!(!s.wet_active(), "a dry stroke dries the wash first");
    // Undo: the dry stroke, then the second wash.
    s.execute(Command::Undo).unwrap();
    s.execute(Command::Undo).unwrap();
    assert_eq!(layer_alpha(&s, 120, 110), 0.0);
    assert!(layer_alpha(&s, 120, 50) > 0.05);
}

#[test]
fn live_oils_build_relief_and_never_dry_on_their_own() {
    let mut s = session();
    fluid_brush(&mut s, "oil-paint-live", None);
    stroke(&mut s, 80.0);
    run(&mut s, 0.0, 200);
    assert!(s.wet_active(), "oils stay workable");
    let h = s.doc().layer(s.active()).unwrap().height.as_ref().map_or(0.0, |h| h.get(120, 80)[0]);
    assert!(h > 0.05, "relief: {h}");
    // Saving dries it into the file.
    let bytes = s.save_happypaint().unwrap();
    assert!(!s.wet_active());
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    let painted = t.doc().root.iter().filter_map(|n| n.as_layer()).any(|l| l.pixels.get(120, 80)[3] > 0.05);
    assert!(painted);
}

#[test]
fn switching_layers_dries_and_tilt_is_kept() {
    let mut s = session();
    s.execute(Command::SetWetTilt { x: 0.0, y: 1.0 }).unwrap();
    assert_eq!(s.wet_tilt(), [0.0, 1.0]);
    fluid_brush(&mut s, "watercolor-live", None);
    stroke(&mut s, 60.0);
    let first = s.active().to_string();
    s.execute(Command::AddLayer).unwrap();
    assert!(!s.wet_active(), "a new layer dries the wash");
    assert!(s.doc().layer(&first).unwrap().pixels.get(120, 60)[3] > 0.05);
}

#[test]
fn a_recorded_wet_session_replays_exactly() {
    let mut s = session();
    fluid_brush(&mut s, "watercolor-live", Some(0.05));
    s.start_recording().unwrap();
    stroke(&mut s, 80.0);
    run(&mut s, 0.0, 600);
    assert!(!s.wet_active());
    let rec = s.stop_recording().unwrap();
    let mut r = Replay::new(&rec).unwrap();
    r.step(usize::MAX / 2);
    assert!(r.done());
    let id = s.active().to_string();
    let a = s.doc().layer(&id).unwrap().pixels.read_rect(s.doc().bounds());
    let b = r.session.doc().layer(&id).unwrap().pixels.read_rect(s.doc().bounds());
    assert!(a == b, "replayed pixels match");
}

fn wash_opts(pigment: f32) -> WashFillOptions {
    WashFillOptions { tolerance: 0.1, grow: 0, sample_all: false, water: 0.6, pigment, grade: 0.0, soft: 4.8, speed: 10.0 }
}

/// The left half painted black: the "line art" a wash fills beside.
fn half_black() -> Session {
    let mut s = session();
    let left = onepaint_selection::rect_polygon([0.0, 0.0], [120.0, 160.0]);
    s.execute(Command::SelectShape { contours: vec![left], mode: onepaint_selection::Mode::Replace, antialias: false }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.9, 0.2, 0.1, 1.0) }).unwrap();
    s
}

/// Spec 58 C: a wash spreads over the region the tap picks, stays inside
/// it, dries by itself, and undoes as one step.
#[test]
fn a_wash_fill_spreads_in_its_region_and_dries() {
    let mut s = half_black();
    let undo_before = s.history_len();
    s.execute(Command::WashFill { x: 200, y: 80, options: wash_opts(0.7) }).unwrap();
    assert!(s.wet_active(), "the wash is wet");
    run(&mut s, 0.0, 4000);
    assert!(!s.wet_active(), "and dries by itself");
    let px = |x, y| s.doc().layer(s.active()).unwrap().pixels.get(x, y);
    let (near, far) = (px(200, 80), px(130, 20));
    assert!(near[3] > 0.2 && far[3] > 0.05, "the whole region took paint: {near:?} {far:?}");
    assert!(near[0] > near[2], "in the wash's colour: {near:?}");
    // The black side is untouched.
    assert_eq!(px(60, 80), s.doc().space().pixel(Rgba::new(0.0, 0.0, 0.0, 1.0)));
    assert_eq!(s.history_len(), undo_before + 1);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.doc().layer(s.active()).unwrap().pixels.get(200, 80)[3], 0.0);
}

/// Water only wets; a tap outside the canvas or on a locked layer does nothing.
#[test]
fn water_only_washes_and_refusals() {
    let mut s = half_black();
    s.execute(Command::WashFill { x: 200, y: 80, options: wash_opts(0.0) }).unwrap();
    assert!(s.wet_active());
    run(&mut s, 0.0, 4000);
    assert_eq!(s.doc().layer(s.active()).unwrap().pixels.get(200, 80)[3], 0.0, "water leaves no pigment");
    s.execute(Command::WashFill { x: 500, y: 80, options: wash_opts(0.7) }).unwrap();
    assert!(!s.wet_active());
    let id = s.active().to_string();
    s.execute(Command::SetLayerProps { id, props: LayerProps { locked: Some(true), ..Default::default() } }).unwrap();
    s.execute(Command::WashFill { x: 200, y: 80, options: wash_opts(0.7) }).unwrap();
    assert!(!s.wet_active());
}
