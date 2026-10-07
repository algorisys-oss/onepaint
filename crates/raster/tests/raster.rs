#![allow(clippy::unwrap_used)]
use onepaint_raster::*;

#[test]
fn key_pack_round_trip() {
    for tx in (0..1000).step_by(37) {
        for ty in (0..1000).step_by(41) {
            let k = pack_key(tx, ty);
            assert_eq!(unpack_key(k), (tx, ty));
        }
    }
    assert_eq!(pack_key(1, 2), (1 << 16) | 2);
}

#[test]
fn tile_coords_floor_negative() {
    assert_eq!(tile_of(0, 0), (0, 0));
    assert_eq!(tile_of(255, 256), (0, 1));
    assert_eq!(tile_of(-1, -257), (-1, -2));
}

#[test]
fn write_read_pixel() {
    let mut s = Surface::new();
    assert_eq!(s.get(10, 10), [0.0; 4]);
    s.set(10, 300, [0.25, 0.5, 0.125, 0.5]);
    let p = s.get(10, 300);
    assert_eq!(p, [0.25, 0.5, 0.125, 0.5]);
    assert_eq!(s.tile_count(), 1);
    assert!(s.tile(0, 1).is_some());
    s.set(-5, -5, [0.1, 0.1, 0.1, 0.1]);
    let q = s.get(-5, -5);
    assert!((q[0] - 0.1).abs() < 1e-3);
}

#[test]
fn snapshot_is_copy_on_write() {
    let mut s = Surface::new();
    s.set(1, 1, [1.0, 0.0, 0.0, 1.0]);
    let snap = s.clone();
    s.set(1, 1, [0.0, 1.0, 0.0, 1.0]);
    assert_eq!(snap.get(1, 1), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(s.get(1, 1), [0.0, 1.0, 0.0, 1.0]);
    // Untouched tiles stay shared.
    let mut a = Surface::new();
    a.set(0, 0, [1.0; 4]);
    a.set(600, 0, [1.0; 4]);
    let b = a.clone();
    a.set(0, 0, [0.5; 4]);
    assert!(std::sync::Arc::ptr_eq(
        a.tile(2, 0).unwrap(),
        b.tile(2, 0).unwrap()
    ));
    assert!(!std::sync::Arc::ptr_eq(
        a.tile(0, 0).unwrap(),
        b.tile(0, 0).unwrap()
    ));
}

#[test]
fn empty_tiles_are_pruned() {
    let mut s = Surface::new();
    s.set(3, 3, [0.0, 0.0, 0.0, 0.5]);
    s.set(3, 3, [0.0; 4]);
    s.prune_empty();
    assert_eq!(s.tile_count(), 0);
}

#[test]
fn rect_ops() {
    let a = Rect::new(0, 0, 10, 10);
    let b = Rect::new(5, 5, 10, 10);
    assert_eq!(a.intersect(&b), Some(Rect::new(5, 5, 5, 5)));
    assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
    assert_eq!(
        Rect::new(0, 0, 1, 1).intersect(&Rect::new(5, 5, 1, 1)),
        None
    );
    let tiles: Vec<_> = Rect::new(250, 0, 10, 300).tiles().collect();
    assert_eq!(tiles, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
}

#[test]
fn read_write_rect_round_trip() {
    let mut s = Surface::new();
    let r = Rect::new(250, 250, 12, 12);
    let data: Vec<f32> = (0..12 * 12 * 4).map(|i| (i % 7) as f32 / 8.0).collect();
    s.write_rect(r, &data);
    assert_eq!(s.tile_count(), 4);
    let back = s.read_rect(r);
    assert_eq!(back, data);
}

#[test]
fn bounds_of_content() {
    let mut s = Surface::new();
    assert_eq!(s.content_bounds(), None);
    s.set(300, 10, [0.0, 0.0, 0.0, 1.0]);
    s.set(20, 600, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(s.content_bounds(), Some(Rect::new(20, 10, 281, 591)));
}
