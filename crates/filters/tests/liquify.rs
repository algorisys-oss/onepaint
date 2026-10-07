#![allow(clippy::unwrap_used)]
use onepaint_filters::liquify::{LiquifyMode as M, dab, dab_centres};
use onepaint_raster::{Rect, Surface};

const DOC: Rect = Rect { x: 0, y: 0, w: 120, h: 120 };

/// Left half opaque red, right half clear: a vertical edge at x = 60.
fn half() -> Surface {
    let mut s = Surface::new();
    let r = Rect::new(0, 0, 60, 120);
    s.write_rect(r, &[1.0, 0.0, 0.0, 1.0].repeat((r.w * r.h) as usize));
    s
}

fn stroke(s: &mut Surface, orig: &Surface, mode: M, from: (f64, f64), to: (f64, f64), r: f64, strength: f64) {
    for (c, p) in dab_centres(from, to, r) {
        dab(s, orig, DOC, mode, c, p, r, strength);
    }
}

#[test]
fn push_moves_the_edge_along_the_drag() {
    let mut s = half();
    let orig = s.clone();
    stroke(&mut s, &orig, M::Push, (50.0, 60.0), (75.0, 60.0), 25.0, 1.0);
    assert!(s.get(64, 60)[3] > 0.9, "red pushed right past the edge: {:?}", s.get(64, 60));
    assert_eq!(s.get(64, 5)[3], 0.0, "outside the brush untouched");
}

#[test]
fn pinch_pulls_in_and_expand_pushes_out() {
    let orig = half();
    let at = |mode, cx: f64, x: i32| {
        let mut s = half();
        dab(&mut s, &orig, DOC, mode, (cx, 60.0), (cx, 60.0), 30.0, 1.0);
        s.get(x, 60)[3]
    };
    // Centre in the clear half: pinch draws the red edge in toward it.
    assert!(at(M::Pinch, 80.0, 62) > 0.5);
    // Centre in the red half: expand pushes the red edge out, pinch pulls the clear in.
    assert!(at(M::Expand, 40.0, 61) > 0.5);
    assert!(at(M::Pinch, 40.0, 58) < 0.5);
}

#[test]
fn twirl_rotates_a_line_through_the_centre() {
    let mut s = Surface::new();
    let r = Rect::new(0, 58, 120, 4);
    s.write_rect(r, &[0.0, 0.0, 1.0, 1.0].repeat((r.w * r.h) as usize));
    let orig = s.clone();
    dab(&mut s, &orig, DOC, M::TwirlLeft, (60.0, 60.0), (60.0, 60.0), 40.0, 1.0);
    assert!(s.get(75, 60)[3] < 0.5, "the line left its row near the centre");
    let off_row = (40..56).chain(64..80).any(|y| s.get(75, y)[3] > 0.5);
    assert!(off_row, "and appears rotated");
}

#[test]
fn reconstruct_returns_to_the_original() {
    let mut s = half();
    let orig = s.clone();
    stroke(&mut s, &orig, M::Push, (40.0, 60.0), (80.0, 60.0), 30.0, 1.0);
    assert!(s.get(66, 60)[3] > 0.5);
    for _ in 0..12 {
        dab(&mut s, &orig, DOC, M::Reconstruct, (60.0, 60.0), (60.0, 60.0), 50.0, 1.0);
    }
    for x in 50..70 {
        let (a, b) = (s.get(x, 60), orig.get(x, 60));
        assert!((a[3] - b[3]).abs() < 1e-3, "x={x}: {a:?} vs {b:?}");
    }
}

#[test]
fn zero_strength_is_identity() {
    let mut s = half();
    let orig = s.clone();
    stroke(&mut s, &orig, M::TwirlRight, (40.0, 60.0), (80.0, 70.0), 30.0, 0.0);
    assert_eq!(s.read_rect(DOC), orig.read_rect(DOC));
}

#[test]
fn dabs_are_spaced_and_capped() {
    assert_eq!(dab_centres((0.0, 0.0), (30.0, 0.0), 10.0).len(), 10);
    assert_eq!(dab_centres((0.0, 0.0), (10000.0, 0.0), 10.0).len(), 64);
    let d = dab_centres((0.0, 0.0), (0.0, 0.0), 10.0);
    assert_eq!(d.len(), 1);
}
