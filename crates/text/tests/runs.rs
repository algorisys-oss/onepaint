//! Rich-text run edits (spec 28).
#![allow(clippy::unwrap_used)]
use onepaint_text::runs::*;
use onepaint_text::*;

fn spec(text: &str) -> TextSpec {
    TextSpec { text: text.into(), font_size_px: 40.0, color: "#000000".into(), ..Default::default() }
}

fn run(text: &str) -> TextRun {
    TextRun { text: text.into(), ..Default::default() }
}

fn texts(s: &TextSpec) -> Vec<String> {
    s.runs.iter().flatten().map(|r| r.text.clone()).collect()
}

#[test]
fn split_and_merge() {
    let mut r = vec![run("hello"), TextRun { bold: Some(true), ..run(" world") }];
    split_at(&mut r, 2);
    split_at(&mut r, 5); // already a boundary
    split_at(&mut r, 8);
    let t: Vec<&str> = r.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(t, ["he", "llo", " wo", "rld"]);
    split_at(&mut r, 0);
    split_at(&mut r, 99);
    assert_eq!(r.len(), 4, "edges are no-ops");
    let m = merge(r);
    let t: Vec<&str> = m.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(t, ["hello", " world"]);
    // Empty runs vanish; multibyte chars split by char, not byte.
    let mut u = vec![run("añb"), run(""), run("c")];
    split_at(&mut u, 2);
    assert_eq!(u[0].text, "añ");
    assert_eq!(merge(u).len(), 1);
}

#[test]
fn bold_a_range_then_toggle_it_back() {
    let s = spec("Hello world");
    let b = apply_style(&s, 6..11, &StyleEdit::Bold);
    assert_eq!(texts(&b), ["Hello ", "world"]);
    let runs = b.runs.clone().unwrap();
    assert_eq!((runs[0].bold, runs[1].bold), (None, Some(true)));
    assert_eq!(b.text, "Hello world");
    // All of it bold → toggles off, and the runs collapse away.
    let off = apply_style(&b, 6..11, &StyleEdit::Bold);
    assert_eq!(off.runs, None);
    // A mixed range turns on (execCommand semantics).
    let mixed = apply_style(&b, 4..8, &StyleEdit::Bold);
    assert_eq!(texts(&mixed), ["Hell", "o world"]);
    // On a bold object, "bold" for a range turns it off: an override false.
    let bold_obj = TextSpec { bold: true, ..spec("abc") };
    let r = apply_style(&bold_obj, 0..1, &StyleEdit::Bold);
    assert_eq!(r.runs.unwrap()[0].bold, Some(false));
}

#[test]
fn colour_font_size_and_decorations() {
    let s = spec("abcdef");
    let c = apply_style(&s, 1..3, &StyleEdit::Color("#FF0000".into()));
    let r = c.runs.clone().unwrap();
    assert_eq!(texts(&c), ["a", "bc", "def"]);
    assert_eq!(r[1].color.as_deref(), Some("#ff0000"));
    // Colouring it back to the base colour strips the override.
    assert_eq!(apply_style(&c, 0..6, &StyleEdit::Color("#000000".into())).runs, None);
    let f = apply_style(&s, 0..2, &StyleEdit::Font("Georgia, serif".into()));
    assert_eq!(f.runs.unwrap()[0].font_family.as_deref(), Some("Georgia, serif"));
    let u = apply_style(&s, 2..4, &StyleEdit::Underline);
    assert_eq!(u.runs.clone().unwrap()[1].underline, Some(true));
    let st = apply_style(&u, 3..5, &StyleEdit::Strikethrough);
    assert_eq!(texts(&st), ["ab", "c", "d", "e", "f"]);
    // A+ scales each fragment's own size; A− undoes it (2 px floor, 0.01 rounding).
    let big = apply_style(&s, 0..3, &StyleEdit::ScaleSize(1.1));
    assert_eq!(big.runs.clone().unwrap()[0].font_size_px, Some(44.0));
    let mixed = apply_style(&big, 2..5, &StyleEdit::ScaleSize(1.1));
    let r = mixed.runs.unwrap();
    assert_eq!(r.iter().map(|r| r.font_size_px).collect::<Vec<_>>(), [Some(44.0), Some(48.4), Some(44.0), None]);
    let tiny = apply_style(&spec("x"), 0..1, &StyleEdit::ScaleSize(0.01));
    assert_eq!(tiny.runs.unwrap()[0].font_size_px, Some(2.0));
    // A collapsed range is a no-op.
    assert_eq!(apply_style(&s, 2..2, &StyleEdit::Bold), s);
}

#[test]
fn object_underline_becomes_run_underline() {
    let s = TextSpec { underline: Some(true), ..spec("abcd") };
    let r = apply_style(&s, 0..2, &StyleEdit::Underline);
    let runs = r.runs.clone().unwrap();
    assert_eq!((runs[0].underline, runs[1].underline), (None, Some(true)));
    assert_eq!(r.underline, None, "runs decide the underline");
}

#[test]
fn typing_keeps_runs_in_step() {
    let s = apply_style(&spec("Hello world"), 6..11, &StyleEdit::Bold);
    // Typing inside the bold run extends it.
    let t = replace_text(&s, "Hello worldly");
    assert_eq!(texts(&t), ["Hello ", "worldly"]);
    assert_eq!(t.text, "Hello worldly");
    // Typing at a boundary takes the style before the caret.
    let t = replace_text(&s, "Hello, world");
    assert_eq!(texts(&t), ["Hello, ", "world"]);
    // At the very start, the style after it.
    let b = apply_style(&spec("ab"), 0..1, &StyleEdit::Bold);
    let t = replace_text(&b, "Xab");
    assert_eq!(texts(&t), ["Xa", "b"]);
    // Deleting the styled text collapses the runs.
    assert_eq!(replace_text(&s, "Hello ").runs, None);
    // Replacing a selection across runs.
    let t = replace_text(&s, "HeYld");
    assert_eq!(texts(&t), ["HeY", "ld"]);
    // Without runs it is a plain text change.
    assert_eq!(replace_text(&spec("a"), "ab"), spec("ab"));
}

#[test]
fn style_at_reports_the_common_style() {
    let s = apply_style(&spec("Hello world"), 6..11, &StyleEdit::Bold);
    assert!(style_at(&s, 6..11).bold);
    assert!(!style_at(&s, 4..8).bold);
    let c = apply_style(&s, 0..3, &StyleEdit::Color("#ff0000".into()));
    assert_eq!(style_at(&c, 0..3).color.as_deref(), Some("#ff0000"));
    assert_eq!(style_at(&c, 0..5).color, None, "mixed");
    assert_eq!(style_at(&c, 4..5).color.as_deref(), Some("#000000"));
    assert_eq!(style_at(&c, 4..5).size, Some(40.0));
}

#[test]
fn rich_runs_round_trip_json() {
    let s = apply_style(&apply_style(&spec("Hello world"), 0..5, &StyleEdit::Italic), 6..11, &StyleEdit::ScaleSize(1.1));
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["runs"][0]["italic"], true);
    assert_eq!(v["runs"][2]["fontSizePx"], 44.0);
    let back: TextSpec = serde_json::from_value(v).unwrap();
    assert_eq!(back, s);
}

#[test]
fn run_strikethrough_and_underline_draw_per_fragment() {
    let r = Renderer::new();
    let base = TextSpec { text: "aaaa bbbb".into(), x: 10.0, y: 10.0, font_size_px: 40.0, ..Default::default() };
    let sum = |s: &onepaint_raster::Surface| s.read_rect(onepaint_raster::Rect::new(0, 0, 400, 100)).chunks(4).map(|p| p[3]).sum::<f32>();
    let plain = sum(&r.render(std::slice::from_ref(&base), 400, 100));
    let struck = apply_style(&base, 5..9, &StyleEdit::Strikethrough);
    let s = r.render(&[struck], 400, 100);
    assert!(sum(&s) > plain + 40.0, "strike adds ink");
    // Struck only on the second word: the first word's mid-line has no rule.
    let left = r.render(&[apply_style(&base, 0..4, &StyleEdit::Strikethrough)], 400, 100);
    let right_ink =
        |s: &onepaint_raster::Surface| s.read_rect(onepaint_raster::Rect::new(130, 0, 270, 100)).chunks(4).map(|p| p[3]).sum::<f32>();
    assert!(right_ink(&s) > right_ink(&left) + 40.0);
    // A run colour paints the glyphs in that colour.
    let red = apply_style(&base, 0..9, &StyleEdit::Color("#ff0000".into()));
    let red = TextSpec { color: "#00ff00".into(), ..red };
    let px = r.render(&[red], 400, 100);
    let b = px.content_bounds().unwrap();
    let mut seen = false;
    for y in b.y..b.y + b.h {
        for x in b.x..b.x + b.w {
            let p = px.get(x, y);
            if p[3] > 0.99 {
                assert!(p[0] > 0.9 && p[1] < 0.05, "{p:?}");
                seen = true;
            }
        }
    }
    assert!(seen);
}
