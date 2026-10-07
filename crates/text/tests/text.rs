#![allow(clippy::unwrap_used)]
use onepaint_text::*;

fn spec(text: &str) -> TextSpec {
    TextSpec { text: text.into(), x: 20.0, y: 30.0, font_size_px: 32.0, ..Default::default() }
}

fn ink(s: &onepaint_raster::Surface) -> Option<onepaint_raster::Rect> {
    s.content_bounds()
}

#[test]
fn renders_at_the_block_top_left() {
    let r = Renderer::new();
    let s = r.render(&[spec("Hello")], 400, 200);
    let b = ink(&s).unwrap();
    assert!(b.x >= 18 && b.x <= 26 && b.y >= 30 && b.y <= 44, "{b:?}");
    assert!(b.w > 50 && b.h > 15 && b.h < 40, "{b:?}");
    assert!(ink(&r.render(&[spec("")], 400, 200)).is_none());
}

#[test]
fn wraps_at_box_width_and_aligns() {
    let r = Renderer::new();
    let long = "the quick brown fox jumps over the lazy dog";
    let one = ink(&r.render(&[spec(long)], 2000, 400)).unwrap();
    let wrapped = ink(&r.render(&[TextSpec { box_width: Some(200.0), ..spec(long) }], 2000, 400)).unwrap();
    assert!(wrapped.h > one.h * 2, "{wrapped:?} vs {one:?}");
    assert!(wrapped.w <= 215, "{wrapped:?}");
    let left = ink(&r.render(&[TextSpec { box_width: Some(300.0), ..spec("Hi") }], 600, 200)).unwrap();
    let right = ink(&r.render(&[TextSpec { box_width: Some(300.0), align: Align::Right, ..spec("Hi") }], 600, 200)).unwrap();
    assert!(right.x > left.x + 200, "{left:?} {right:?}");
}

#[test]
fn rotation_outline_and_bold() {
    let r = Renderer::new();
    let flat = ink(&r.render(&[spec("Rotate me")], 600, 600)).unwrap();
    let rot =
        ink(&r.render(&[TextSpec { x: 300.0, y: 100.0, rotation: std::f64::consts::FRAC_PI_2, ..spec("Rotate me") }], 600, 600)).unwrap();
    assert!(rot.h > rot.w && flat.w > flat.h, "{flat:?} {rot:?}");
    let plain = r.render(&[spec("Outline")], 400, 200);
    let outlined = r.render(&[TextSpec { outline_width: Some(4.0), outline_color: Some("#ff0000".into()), ..spec("Outline") }], 400, 200);
    assert!(ink(&outlined).unwrap().w > ink(&plain).unwrap().w);
    let sum = |s: &onepaint_raster::Surface| s.read_rect(onepaint_raster::Rect::new(0, 0, 400, 200)).chunks(4).map(|p| p[3]).sum::<f32>();
    assert!(sum(&r.render(&[TextSpec { bold: true, ..spec("Weight") }], 400, 200)) > sum(&r.render(&[spec("Weight")], 400, 200)));
}

#[test]
fn json_round_trip_keeps_unknown_fields() {
    let j = serde_json::json!({"text": "a", "fontFamily": "Georgia, serif", "fontSizePx": 20, "color": "#123456", "align": "center",
        "lineHeight": 1.5, "bold": false, "italic": true, "letterSpacing": 2, "x": 1, "y": 2, "rotation": 0,
        "balloon": {"shape": "oval", "fill": "#fff"}});
    let s: TextSpec = serde_json::from_value(j).unwrap();
    assert_eq!((s.align, s.italic, s.font_size_px), (Align::Center, true, 20.0));
    let back = serde_json::to_value(&s).unwrap();
    assert_eq!(back["balloon"]["shape"], "oval");
}
