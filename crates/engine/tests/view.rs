use onepaint_engine::view::View;

#[test]
fn doc_screen_round_trip() {
    let mut v = View::new([1200.0, 800.0], [3000.0, 2000.0]);
    for (s, r) in [(0.3, 0.0), (1.0, 0.7), (4.5, -2.1), (12.0, 3.0)] {
        v.scale = s;
        v.rotation = r;
        v.offset = [37.0, -12.5];
        for p in [[0.0, 0.0], [1500.0, 1000.0], [2999.0, 17.0]] {
            let q = v.screen_to_doc(v.doc_to_screen(p));
            assert!(
                (q[0] - p[0]).abs() < 1e-9 && (q[1] - p[1]).abs() < 1e-9,
                "{p:?} -> {q:?}"
            );
        }
    }
}

#[test]
fn identity_view_centres_the_document() {
    let v = View {
        scale: 1.0,
        rotation: 0.0,
        offset: [0.0, 0.0],
        ..View::new([800.0, 600.0], [400.0, 200.0])
    };
    assert_eq!(v.doc_to_screen([200.0, 100.0]), [400.0, 300.0]);
    assert_eq!(v.doc_to_screen([0.0, 0.0]), [200.0, 200.0]);
}

#[test]
fn zoom_at_keeps_anchor_fixed() {
    let mut v = View::new([1000.0, 700.0], [2000.0, 1500.0]);
    v.rotation = 0.4;
    let anchor = [310.0, 222.0];
    let before = v.screen_to_doc(anchor);
    v.zoom_at(anchor, 2.5);
    let after = v.screen_to_doc(anchor);
    assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9);
    v.zoom_at(anchor, 1e9);
    assert_eq!(v.scale, 64.0);
}

#[test]
fn rotate_at_keeps_anchor_and_snaps() {
    let mut v = View::new([1000.0, 700.0], [800.0, 600.0]);
    let anchor = [600.0, 300.0];
    let before = v.screen_to_doc(anchor);
    v.rotate_at(anchor, 0.3);
    let after = v.screen_to_doc(anchor);
    assert!((before[0] - after[0]).abs() < 1e-9 && (before[1] - after[1]).abs() < 1e-9);
    assert_eq!(
        View::snap_rotation(89.0_f64.to_radians()),
        90.0_f64.to_radians()
    );
    assert_eq!(
        View::snap_rotation(80.0_f64.to_radians()),
        80.0_f64.to_radians()
    );
}

#[test]
fn fit_shows_whole_document() {
    let mut v = View::new([1000.0, 700.0], [4000.0, 1000.0]);
    v.fit();
    for c in [[0.0, 0.0], [4000.0, 1000.0], [0.0, 1000.0], [4000.0, 0.0]] {
        let s = v.doc_to_screen(c);
        assert!(
            s[0] >= 0.0 && s[0] <= 1000.0 && s[1] >= 0.0 && s[1] <= 700.0,
            "{s:?}"
        );
    }
}
