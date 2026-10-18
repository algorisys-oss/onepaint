#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_doc::BlendMode;
use onepaint_engine::*;
use onepaint_filters::{Kind, Params, default_params};
use onepaint_selection::{Mode, rect_polygon};

fn red_session() -> Session {
    let mut s = Session::new(64, 64, Some(Rgba::WHITE));
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SelectAll).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

fn layer_px(s: &Session, x: i32, y: i32) -> [f32; 4] {
    s.doc().layer(s.active()).unwrap().pixels.get(x, y)
}

#[test]
fn destructive_filter_previews_commits_once_and_cancels() {
    let mut s = red_session();
    let n = s.history_len();
    s.execute(Command::BeginFilter { kind: "invert".into() }).unwrap();
    assert!(s.filter_active());
    s.execute(Command::UpdateFilter { params: Params::default() }).unwrap();
    let p = layer_px(&s, 10, 10);
    assert!(p[0] < 1e-3 && p[1] > 0.99, "preview shows cyan {p:?}");
    s.execute(Command::CancelFilter).unwrap();
    assert!(layer_px(&s, 10, 10)[0] > 0.99);
    assert_eq!(s.history_len(), n);
    s.execute(Command::BeginFilter { kind: "invert".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: Params::default() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    assert!(!s.filter_active());
    assert_eq!(s.history_len(), n + 1);
    s.execute(Command::Undo).unwrap();
    assert!(layer_px(&s, 10, 10)[0] > 0.99);
}

#[test]
fn updates_restart_from_the_original() {
    let mut s = red_session();
    s.execute(Command::BeginFilter { kind: "brightness-contrast".into() }).unwrap();
    for b in [0.3, 0.3, 0.3] {
        s.execute(Command::UpdateFilter { params: Params { brightness: Some(-b), ..Default::default() } }).unwrap();
    }
    let once = layer_px(&s, 5, 5)[0];
    s.execute(Command::CommitFilter).unwrap();
    let mut t = red_session();
    t.execute(Command::BeginFilter { kind: "brightness-contrast".into() }).unwrap();
    t.execute(Command::UpdateFilter { params: Params { brightness: Some(-0.3), ..Default::default() } }).unwrap();
    assert_eq!(once, layer_px(&t, 5, 5)[0]);
}

#[test]
fn filters_respect_the_selection_and_blur_works() {
    let mut s = red_session();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([0.0, 0.0], [32.0, 64.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::BeginFilter { kind: "invert".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: Params::default() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    assert!(layer_px(&s, 10, 10)[0] < 1e-3);
    assert!(layer_px(&s, 50, 10)[0] > 0.99);
    // Blur across the invert edge softens it.
    s.execute(Command::Deselect).unwrap();
    s.execute(Command::BeginFilter { kind: "gaussian-blur".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: Params { radius: Some(4.0), ..Default::default() } }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    let edge = layer_px(&s, 32, 10)[0];
    assert!(edge > 0.1 && edge < 0.9, "{edge}");
}

#[test]
fn adjustment_layer_composites_live_and_round_trips() {
    let mut s = red_session();
    s.execute(Command::AddAdjustmentLayer { kind: "invert".into(), params: Params::default() }).unwrap();
    let id = s.active().to_string();
    let c = s.pick_color(10, 10);
    assert!(c.r < 0.01 && c.g > 0.99, "invert layer turns red cyan {c:?}");
    s.execute(Command::SetLayerProps { id: id.clone(), props: LayerProps { opacity: Some(0.5), ..Default::default() } }).unwrap();
    let h = s.pick_color(10, 10);
    assert!(h.r > 0.3 && h.r < 0.9, "half strength {h:?}");
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(8, 8, None);
    t.open_happypaint(&bytes).unwrap();
    let back = t.doc().find(&id).unwrap().as_layer().unwrap().adjustment.clone().unwrap();
    assert_eq!(back.kind, "invert");
    assert!((t.pick_color(10, 10).r - h.r).abs() < 0.02);
}

fn layer_count(s: &Session) -> usize {
    s.doc().layer_ids().len()
}

fn clouds(scale: f32, density: f32) -> Params {
    Params { gen_scale: Some(scale), gen_density: Some(density), ..default_params(Kind::Clouds) }
}

#[test]
fn generators_render_into_a_new_layer_as_one_undo_step() {
    let mut s = red_session();
    let src = s.active().to_string();
    let (n, layers) = (s.history_len(), layer_count(&s));
    s.execute(Command::BeginFilter { kind: "clouds".into() }).unwrap();
    assert!(s.filter_active());
    assert_eq!(layer_count(&s), layers + 1);
    let new_id = s.active().to_string();
    assert_ne!(new_id, src, "the new layer is active");
    s.execute(Command::UpdateFilter { params: clouds(8.0, 0.9) }).unwrap();
    let covered = (0..64).filter(|&x| layer_px(&s, x, 20)[3] > 0.0).count();
    assert!(covered > 10, "clouds drawn on the new layer ({covered})");
    s.execute(Command::CommitFilter).unwrap();
    assert_eq!(s.history_len(), n + 1, "one undo step");
    assert_eq!(layer_count(&s), layers + 1);
    assert_eq!(s.doc().layer(&src).unwrap().pixels.get(5, 5)[0], 1.0, "the source is untouched");
    s.execute(Command::Undo).unwrap();
    assert_eq!(layer_count(&s), layers, "undo removes the layer");
    s.execute(Command::Redo).unwrap();
    assert_eq!(layer_count(&s), layers + 1);
    assert!(s.doc().layer(&new_id).is_some());
}

#[test]
fn cancelling_a_generator_restores_the_tree() {
    let mut s = red_session();
    let (src, n, layers) = (s.active().to_string(), s.history_len(), layer_count(&s));
    s.execute(Command::BeginFilter { kind: "starfield".into() }).unwrap();
    s.execute(Command::CancelFilter).unwrap();
    assert_eq!((layer_count(&s), s.history_len(), s.active()), (layers, n, src.as_str()));
    // Undo while previewing also abandons it.
    s.execute(Command::BeginFilter { kind: "lightning".into() }).unwrap();
    s.execute(Command::Undo).unwrap();
    assert_eq!((layer_count(&s), s.active()), (layers, src.as_str()));
}

#[test]
fn lens_flare_lands_on_a_screen_layer() {
    let mut s = red_session();
    s.execute(Command::BeginFilter { kind: "lens-flare".into() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    let l = s.doc().layer(s.active()).unwrap();
    assert_eq!(l.blend_mode, BlendMode::Screen);
    assert_eq!(l.name, "Lens Flare");
    assert!(l.pixels.content_bounds().is_some());
}

#[test]
fn line_art_traces_the_source_on_its_own_layer() {
    let mut s = Session::new(64, 64, Some(Rgba::WHITE));
    s.execute(Command::SetColor { rgba: Rgba::BLACK }).unwrap();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([16.0, 16.0], [48.0, 48.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    let src = s.active().to_string();
    let before = s.doc().layer(&src).unwrap().pixels.get(20, 20);
    s.execute(Command::BeginFilter { kind: "line-art".into() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    let l = s.doc().layer(s.active()).unwrap();
    assert_eq!(l.name, "Line art");
    let edge = (12..20).map(|x| l.pixels.get(x, 32)[3]).fold(0.0, f32::max);
    assert!(edge > 0.5, "the square's edge inks ({edge})");
    assert!(l.pixels.get(32, 32)[3] < 0.05 && l.pixels.get(2, 2)[3] < 0.05);
    assert_eq!(s.doc().layer(&src).unwrap().pixels.get(20, 20), before);
}

#[test]
fn global_distorts_run_over_the_whole_canvas() {
    let mut s = Session::new(64, 64, None);
    s.execute(Command::SetColor { rgba: Rgba::new(1.0, 0.0, 0.0, 1.0) }).unwrap();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([32.0, 0.0], [64.0, 64.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    assert!((20..32).all(|x| layer_px(&s, x, 24)[3] == 0.0));
    s.execute(Command::BeginFilter { kind: "twirl".into() }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    // The twirl swings red into the empty left half near the centre.
    assert!((20..32).any(|x| layer_px(&s, x, 24)[3] > 0.1));
}

#[test]
fn fill_layers_cover_the_backdrop_live() {
    let mut s = red_session();
    let p = Params { fill_color: Some([0.0, 0.0, 1.0]), ..Default::default() };
    s.execute(Command::AddAdjustmentLayer { kind: "solid-fill".into(), params: p }).unwrap();
    let c = s.pick_color(10, 10);
    assert!(c.b > 0.99 && c.r < 0.01, "{c:?}");
    // A gradient fill spans the document: black at the top, white at the bottom.
    let mut t = red_session();
    t.execute(Command::AddAdjustmentLayer { kind: "gradient-fill".into(), params: default_params(Kind::GradientFill) }).unwrap();
    assert!(t.pick_color(32, 0).r < 0.1 && t.pick_color(32, 63).r > 0.9);
}

#[test]
fn selection_masks_generators() {
    let mut s = red_session();
    s.execute(Command::SelectShape { contours: vec![rect_polygon([0.0, 0.0], [32.0, 64.0])], mode: Mode::Replace }).unwrap();
    s.execute(Command::BeginFilter { kind: "clouds".into() }).unwrap();
    s.execute(Command::UpdateFilter { params: clouds(8.0, 1.0) }).unwrap();
    s.execute(Command::CommitFilter).unwrap();
    assert!((40..64).all(|x| layer_px(&s, x, 30)[3] == 0.0));
    assert!((0..30).any(|x| layer_px(&s, x, 30)[3] > 0.0));
}

/// Spec 52: a lone top-level blur layer plans as GPU programs, live during a
/// stroke under it, and an edit redraws as far as the blur reaches.
#[test]
fn a_lone_blur_layer_runs_live_on_the_gpu_path() {
    use onepaint_compose::program::{op_contents, run};
    let mut s = Session::new(600, 400, Some(onepaint_color::Rgba::WHITE));
    let paint = s.active().to_string();
    let p = Params { radius: Some(12.0), ..Default::default() };
    s.execute(Command::AddAdjustmentLayer { kind: "gaussian-blur".into(), params: p }).unwrap();
    s.execute(Command::SetActiveLayer { id: paint }).unwrap();
    s.take_dirty_tiles();
    s.stroke_begin(onepaint_stroke::StrokePoint::at(250.0, 120.0, 1.0, 0.0));
    s.stroke_extend(&[onepaint_stroke::StrokePoint::at(254.0, 120.0, 1.0, 8.0)]);
    let keys: Vec<_> = s.doc().bounds().tiles().collect();
    let (progs, cpu) = s.plan(&keys);
    assert!(cpu.is_empty(), "no tile falls back: {cpu:?}");
    assert!(progs.iter().any(|p| op_contents(&p.ops).any(|c| c.spatial.is_some())));
    // The program shows the stroke in progress, blurred: grey near it, not white.
    let at = progs.iter().find(|p| (p.tx, p.ty) == (0, 0)).unwrap();
    let px = run(at);
    let i = (120 * 256 + 252) * 4;
    assert!(px[i] < 0.95, "blurred paint under the stroke: {:?}", &px[i..i + 4]);
    // The stroke's tiles plus the blur's reach (12 + 2 px) are dirty: tile 1 starts at 256.
    let dirty = s.take_dirty_tiles();
    assert!(dirty.contains(&(1, 0)), "{dirty:?}");
    s.stroke_end();
}

/// Spec 52 3b: two stacked blur layers both stay live (up to three do); the upper one's
/// backdrop programs carry the lower one.
#[test]
fn stacked_blur_layers_both_run_live() {
    use onepaint_compose::program::op_contents;
    let mut s = Session::new(600, 400, Some(onepaint_color::Rgba::WHITE));
    let paint = s.active().to_string();
    for r in [4.0, 8.0] {
        let p = Params { radius: Some(r), ..Default::default() };
        s.execute(Command::AddAdjustmentLayer { kind: "gaussian-blur".into(), params: p }).unwrap();
    }
    s.execute(Command::SetActiveLayer { id: paint }).unwrap();
    let keys: Vec<_> = s.doc().bounds().tiles().collect();
    let (progs, cpu) = s.plan(&keys);
    assert!(cpu.is_empty(), "no tile falls back: {cpu:?}");
    let nested = progs.iter().any(|p| {
        op_contents(&p.ops)
            .filter_map(|c| c.spatial.as_ref())
            .any(|sp| sp.backdrop.iter().any(|(_, b)| op_contents(&b.ops).any(|c| c.spatial.is_some())))
    });
    assert!(nested, "the upper blur's backdrop holds the lower blur");
    // Past three, planning would grow ninefold per layer: all render on the CPU.
    for _ in 0..2 {
        let p = Params { radius: Some(2.0), ..Default::default() };
        s.execute(Command::AddAdjustmentLayer { kind: "gaussian-blur".into(), params: p }).unwrap();
    }
    let (progs, _) = s.plan(&keys);
    assert!(!progs.iter().any(|p| op_contents(&p.ops).any(|c| c.spatial.is_some())));
}

/// Spec 53: a lone twirl layer plans on the GPU path and shows a stroke under
/// it during the stroke; with a second spatial layer it renders on the CPU.
#[test]
fn a_lone_twirl_layer_runs_live_on_the_gpu_path() {
    use onepaint_compose::program::{op_contents, run};
    let mut s = Session::new(600, 400, Some(onepaint_color::Rgba::WHITE));
    let paint = s.active().to_string();
    let p = Params { center_x: Some(0.4), center_y: Some(0.3), angle: Some(120.0), extent: Some(0.8), ..Default::default() };
    s.execute(Command::AddAdjustmentLayer { kind: "twirl".into(), params: p }).unwrap();
    s.execute(Command::SetActiveLayer { id: paint.clone() }).unwrap();
    s.take_dirty_tiles();
    s.stroke_begin(onepaint_stroke::StrokePoint::at(250.0, 120.0, 1.0, 0.0));
    s.stroke_extend(&[onepaint_stroke::StrokePoint::at(254.0, 120.0, 1.0, 8.0)]);
    let keys: Vec<_> = s.doc().bounds().tiles().collect();
    let (progs, cpu) = s.plan(&keys);
    assert!(cpu.is_empty(), "no tile falls back: {cpu:?}");
    assert!(progs.iter().any(|p| op_contents(&p.ops).any(|c| c.spatial.as_ref().is_some_and(|s| s.whole.is_some()))));
    // The stroke in progress shows through the twirl somewhere: not all white.
    let dark = progs.iter().any(|p| run(p).chunks_exact(4).any(|px| px[0] < 0.9));
    assert!(dark, "the stroke shows, twirled");
    // A whole-canvas layer is live only on its own.
    s.stroke_end();
    let p = Params { radius: Some(3.0), ..Default::default() };
    s.execute(Command::AddAdjustmentLayer { kind: "gaussian-blur".into(), params: p }).unwrap();
    let (progs, _) = s.plan(&keys);
    assert!(!progs.iter().any(|p| op_contents(&p.ops).any(|c| c.spatial.is_some())));
}
