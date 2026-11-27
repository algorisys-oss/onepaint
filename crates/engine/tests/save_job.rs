#![allow(clippy::unwrap_used)]
//! Saving in two parts: `prepare_save` on the editing thread, `SaveJob::run`
//! anywhere, reusing the last save's chunks.
use onepaint_color::Rgba;
use onepaint_engine::*;

fn painted() -> Session {
    let mut s = Session::new(600, 400, Some(Rgba::WHITE));
    s.execute(Command::AddLayer).unwrap();
    s.execute(Command::SetColor { rgba: Rgba::new(0.9, 0.2, 0.1, 1.0) }).unwrap();
    s.execute(Command::SelectShape {
        contours: vec![onepaint_selection::rect_polygon([20.0, 20.0], [300.0, 200.0])],
        mode: onepaint_selection::Mode::Replace,
        antialias: false,
    })
    .unwrap();
    s.execute(Command::FillSelection).unwrap();
    s.execute(Command::Deselect).unwrap();
    s
}

#[test]
fn a_save_job_runs_on_another_thread_with_the_drawing_as_it_was() {
    let mut s = painted();
    let job = s.prepare_save();
    // Editing goes on while the job runs.
    s.execute(Command::SetColor { rgba: Rgba::new(0.0, 0.0, 1.0, 1.0) }).unwrap();
    s.execute(Command::FillSelection).unwrap();
    let bytes = std::thread::spawn(move || job.run()).join().unwrap().unwrap();
    let doc = onepaint_formats::read_happypaint(&bytes).unwrap();
    let layer = doc.root.last().unwrap().as_layer().unwrap();
    let p = layer.pixels.get(100, 100);
    assert!(p[0] > 0.5 && p[2] < 0.2, "the red from before the edit: {p:?}");
    assert!(onepaint_formats::read_thumbnail(&bytes).unwrap().is_some());
}

#[test]
fn repeated_saves_reuse_chunks_and_match_a_fresh_save() {
    let mut s = painted();
    let first = s.save_happypaint().unwrap();
    let again = s.save_happypaint().unwrap();
    let fresh = Session::from_document(onepaint_formats::read_happypaint(&first).unwrap()).save_happypaint().unwrap();
    let (a, b) = (onepaint_formats::read_happypaint(&again).unwrap(), onepaint_formats::read_happypaint(&fresh).unwrap());
    let (la, lb) = (a.root.last().unwrap().as_layer().unwrap(), b.root.last().unwrap().as_layer().unwrap());
    assert_eq!(la.pixels.get(100, 100), lb.pixels.get(100, 100));
}

#[test]
fn a_save_job_is_send() {
    fn send<T: Send + 'static>() {}
    send::<SaveJob>();
}
