#![allow(clippy::unwrap_used)]
use onepaint_color::Rgba;
use onepaint_doc::{Document, Group, Layer, Node};
use onepaint_engine::anim::*;
use onepaint_engine::*;

fn solid(name: &str, c: [f32; 4]) -> Node {
    let mut l = Layer::new(name);
    l.pixels = Document::filled(16, 16, c);
    Node::Layer(l)
}

fn hold(mut n: Node, h: u32) -> Node {
    if let Node::Layer(l) = &mut n {
        l.extra.insert("holdFrames".into(), serde_json::json!(h));
    }
    n
}

/// A three-frame flipbook: red (hold 2), green, blue.
fn flipbook() -> Session {
    let mut d = Document::new(16, 16, None);
    d.root = vec![hold(solid("r", [1.0, 0.0, 0.0, 1.0]), 2), solid("g", [0.0, 1.0, 0.0, 1.0]), solid("b", [0.0, 0.0, 1.0, 1.0])];
    Session::from_document(d)
}

fn shown(s: &Session) -> [f32; 4] {
    let t = s.composite(&[(0, 0)]);
    t[0].1.get(4, 4)
}

fn enable(s: &mut Session, f: impl FnOnce(&mut AnimSettings)) {
    let mut a = s.anim_settings().clone();
    a.enabled = true;
    f(&mut a);
    s.execute(Command::SetAnimation { settings: Box::new(a) }).unwrap();
}

#[test]
fn frames_holds_and_steps() {
    let s = flipbook();
    let f = flipbook_frames(&s.doc().root);
    assert_eq!(f.iter().map(|f| (f.start, f.hold)).collect::<Vec<_>>(), [(0, 2), (2, 1), (3, 1)]);
    assert_eq!(total_slots(&s.doc().root), 4);
    assert_eq!(frame_at_slot(&f, 1).unwrap().start, 0);
    assert_eq!(frame_at_slot(&f, 99).unwrap().start, 3, "clamped");
    assert_eq!(step_slot(3, 0, 3, LoopMode::Loop, 1), (0, 1, false));
    assert_eq!(step_slot(3, 0, 3, LoopMode::Pingpong, 1), (2, -1, false));
    assert_eq!(step_slot(0, 0, 3, LoopMode::Pingpong, -1), (1, 1, false));
    assert_eq!(step_slot(3, 0, 3, LoopMode::Once, 1), (3, 1, true));
    assert_eq!(step_slot(1, 0, 3, LoopMode::Once, 1), (2, 1, false));
}

#[test]
fn tracks_with_starts_and_gaps() {
    let mut track = Group::new("lines");
    track.extra.insert("animTrack".into(), serde_json::json!(true));
    let mut a = Layer::new("a");
    a.extra.insert("holdFrames".into(), serde_json::json!(2));
    let mut b = Layer::new("b");
    b.extra.insert("animStart".into(), serde_json::json!(4));
    let (aid, bid) = (a.id.clone(), b.id.clone());
    track.children = vec![Node::Layer(a), Node::Layer(b)];
    let bg = Layer::new("bg");
    let root = vec![Node::Layer(bg.clone()), Node::Group(track.clone())];
    assert!(is_tracks_doc(&root));
    assert_eq!(total_slots(&root), 5);
    let v = column_view(&root, 1);
    assert_eq!(v.sheets.get(&track.id), Some(&aid));
    assert!(v.allow.contains(&bg.id));
    let gap = column_view(&root, 3);
    assert!(!gap.allow.contains(&track.id), "a gap shows nothing for the track");
    assert_eq!(column_view(&root, 4).sheets.get(&track.id), Some(&bid));
}

#[test]
fn the_view_solos_the_frame_and_disabling_restores() {
    let mut s = flipbook();
    let normal = shown(&s);
    assert!(normal[2] > 0.99, "blue on top normally");
    enable(&mut s, |a| a.onion_enabled = false);
    s.execute(Command::SetAnimSlot { slot: 1 }).unwrap();
    assert!(shown(&s)[0] > 0.99 && shown(&s)[1] < 0.01, "slot 1 is the held red frame");
    s.execute(Command::SetAnimSlot { slot: 2 }).unwrap();
    assert!(shown(&s)[1] > 0.99);
    let mut a = s.anim_settings().clone();
    a.enabled = false;
    s.execute(Command::SetAnimation { settings: Box::new(a) }).unwrap();
    assert_eq!(shown(&s), normal);
    assert!(s.doc().root.iter().all(|n| n.visible()), "visibility never changes");
}

#[test]
fn onion_ghosts_are_tinted_and_hide_while_playing() {
    let mut d = Document::new(16, 16, None);
    let mut prev = Layer::new("prev");
    prev.pixels.write_rect(onepaint_raster::Rect::new(0, 0, 4, 16), &[0.0, 0.0, 0.0, 1.0].repeat(64));
    let cur = Layer::new("cur");
    let mut next = Layer::new("next");
    next.pixels.write_rect(onepaint_raster::Rect::new(8, 0, 4, 16), &[0.0, 0.0, 0.0, 1.0].repeat(64));
    d.root = vec![Node::Layer(prev), Node::Layer(cur), Node::Layer(next)];
    let mut s = Session::from_document(d);
    enable(&mut s, |_| {});
    s.execute(Command::SetAnimSlot { slot: 1 }).unwrap();
    let t = &s.composite(&[(0, 0)])[0].1;
    let (p, n) = (t.get(1, 1), t.get(9, 1));
    assert!((p[3] - 0.35).abs() < 0.01, "ghost opacity .35: {p:?}");
    assert!(p[0] > p[1] && p[0] > p[2], "previous is red: {p:?}");
    assert!(n[1] > n[0] && n[1] > n[2], "next is green: {n:?}");
    s.execute(Command::SetAnimPlaying { playing: true }).unwrap();
    assert_eq!(s.composite(&[(0, 0)])[0].1.get(1, 1)[3], 0.0);
}

#[test]
fn active_layer_and_playhead_follow_each_other() {
    let mut s = flipbook();
    let ids: Vec<String> = s.doc().root.iter().map(|n| n.id().to_string()).collect();
    enable(&mut s, |_| {});
    s.execute(Command::SetAnimSlot { slot: 2 }).unwrap();
    assert_eq!(s.active(), ids[1]);
    s.execute(Command::SetActiveLayer { id: ids[2].clone() }).unwrap();
    assert_eq!(s.anim_slot(), 3);
    s.execute(Command::SetFrameHold { id: ids[2].clone(), hold: 3 }).unwrap();
    assert_eq!(s.anim_total_slots(), 6);
    s.execute(Command::Undo).unwrap();
    assert_eq!(s.anim_total_slots(), 4);
    s.execute(Command::AddFrame).unwrap();
    assert_eq!(s.doc().root.len(), 4);
    assert_eq!(s.anim_slot(), 4, "the new frame is after the current one");
}

#[test]
fn settings_round_trip_and_export() {
    let mut s = flipbook();
    enable(&mut s, |a| a.fps = 8.0);
    let mut a = s.anim_settings().clone();
    a.extra.insert("markers".into(), serde_json::json!([{"slot": 1, "name": "key", "color": "#e5484d"}]));
    s.execute(Command::SetAnimation { settings: Box::new(a) }).unwrap();
    let bytes = s.save_happypaint().unwrap();
    let mut t = Session::new(4, 4, None);
    t.open_happypaint(&bytes).unwrap();
    assert!(t.anim_settings().enabled);
    assert_eq!(t.anim_settings().fps, 8.0);
    assert_eq!(t.anim_settings().extra["markers"][0]["name"], "key");
    assert_eq!(t.anim_total_slots(), 4, "holds survive");

    let gif = t.export_gif(Some(Rgba::WHITE)).unwrap();
    let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&gif)).unwrap();
    use image::AnimationDecoder;
    let frames: Vec<_> = dec.into_frames().collect::<Result<_, _>>().unwrap();
    assert_eq!(frames.len(), 4, "holds expanded");
    let (n, d) = frames[0].delay().numer_denom_ms();
    assert_eq!(n / d, 130, "round(100/8) cs");
    assert!(frames[0].buffer().get_pixel(2, 2)[0] > 250);
    let zip = t.export_png_sequence(None).unwrap();
    let files = onepaint_formats::zip::read_zip(&zip).unwrap();
    assert_eq!(
        files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
        ["frame_0001.png", "frame_0002.png", "frame_0003.png", "frame_0004.png"]
    );
}
