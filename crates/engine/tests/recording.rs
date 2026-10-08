#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_engine::path_ops::StrokePathTool;
use onepaint_engine::recording::*;
use onepaint_engine::*;
use onepaint_stroke::StrokePoint;
use onepaint_stroke::bezier::*;

fn pt(x: f64, y: f64, t: f64) -> StrokePoint {
    StrokePoint { x, y, pressure: 0.7, t: Some(t), ..Default::default() }
}

fn stroke(s: &mut Session, y: f64) {
    s.stroke_begin(pt(10.0, y, 0.0));
    s.stroke_extend(&(1..30).map(|i| pt(10.0 + i as f64 * 3.0, y + (i as f64 * 0.4).sin() * 8.0, i as f64 * 16.0)).collect::<Vec<_>>());
    s.stroke_end();
}

fn session_recorded() -> (Session, Recording) {
    let mut s = Session::new(128, 96, Some(Rgba::WHITE));
    stroke(&mut s, 10.0); // before recording: advances the seed
    // A scattered, jittery brush so seeds matter.
    let jitter =
        onepaint_brush::presets().into_iter().find(|p| p.dynamics.as_ref().is_some_and(|d| d.scatter.unwrap_or(0.0) > 0.0)).unwrap();
    s.execute(Command::SelectShape {
        contours: vec![onepaint_selection::rect_polygon([0.0, 0.0], [100.0, 90.0])],
        mode: onepaint_selection::Mode::Replace,
    })
    .unwrap();
    s.start_recording().unwrap();
    s.execute(Command::SetBrush { preset_id: jitter.id }).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.2, 0.4, 0.9, 1.0) }).unwrap();
    stroke(&mut s, 40.0);
    s.execute(Command::AddLayer).unwrap();
    stroke(&mut s, 60.0);
    stroke(&mut s, 70.0);
    s.execute(Command::Undo).unwrap();
    let spec = PathSpec {
        subpaths: vec![Subpath { anchors: vec![PathAnchor::corner(20.0, 80.0), PathAnchor::corner(110.0, 85.0)], closed: false }],
        ..Default::default()
    };
    s.execute(Command::CreatePathLayer { spec: Box::new(spec) }).unwrap();
    s.execute(Command::StrokePath { tool: StrokePathTool::Brush, simulate_pressure: true }).unwrap();
    let rec = s.stop_recording().unwrap();
    (s, rec)
}

#[test]
fn replay_reproduces_the_session_exactly() {
    let (s, rec) = session_recorded();
    assert!(!s.is_recording());
    let begins = rec.events.iter().filter(|e| matches!(e, RecEvent::Begin { .. })).count();
    assert_eq!(begins, 3, "Stroke Path's inner strokes are not logged");
    let mut r = Replay::new(&rec).unwrap();
    r.step(usize::MAX / 2);
    assert!(r.done());
    assert_eq!(r.session.doc().root.len(), s.doc().root.len());
    assert_eq!(r.session.flatten(), s.flatten(), "pixel-identical");
}

#[test]
fn a_video_timelapse_spans_three_seconds_at_least() {
    let (_, rec) = session_recorded();
    let mp4 = Replay::new(&rec).unwrap().timelapse_mp4().unwrap();
    let at = |kind: &[u8; 4], off: usize| {
        let i = mp4.windows(4).position(|w| w == kind).unwrap() + off;
        u32::from_be_bytes(mp4[i..i + 4].try_into().unwrap())
    };
    assert_eq!(at(b"stsz", 12), 90, "3 s at 30 fps (a quick session)");
    assert_eq!((at(b"tkhd", 80) >> 16, at(b"tkhd", 84) >> 16), (128, 96));
}

#[test]
fn recordings_round_trip_and_make_a_timelapse() {
    let (s, rec) = session_recorded();
    let back = Recording::from_bytes(&rec.to_bytes().unwrap()).unwrap();
    assert_eq!(back, rec, "exact, floats included");
    let n = rec.events.len();
    let gif = Replay::new(&back).unwrap().timelapse_gif().unwrap();
    use image::AnimationDecoder;
    let frames: Vec<_> =
        image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&gif)).unwrap().into_frames().collect::<Result<_, _>>().unwrap();
    assert_eq!(frames.len(), n.min(240) + 12);
    let last = frames.last().unwrap().buffer();
    assert_eq!((last.width(), last.height()), (128, 96));
    // The final frame is the final composite (flattened on white).
    let flat = onepaint_formats::flatten_to_srgb8(&s.flatten());
    let i = (50 * 128 + 40) * 4;
    let want = flat[i];
    assert!((last.get_pixel(40, 50)[0] as i32 - want as i32).abs() <= 8);
}
