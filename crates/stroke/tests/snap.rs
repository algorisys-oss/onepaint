#![allow(clippy::unwrap_used)]
use onepaint_stroke::snap::*;

fn close(a: Pt, b: Pt) -> bool {
    (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
}

#[test]
fn grid_node_rounds_to_the_finest_step() {
    assert_eq!(grid_node([33.0, 95.0], 64.0), [64.0, 64.0]);
    assert_eq!(grid_node([33.0, 95.0], 32.0), [32.0, 96.0]);
    assert_eq!(grid_node([0.4, 0.6], 0.2), [0.0, 1.0], "step floors at 1");
}

#[test]
fn nothing_armed_without_snapping() {
    assert!(Snapper::arm(SnapConfig::default(), [0.0, 0.0]).is_none());
    // Hard perspective alone does not snap freehand strokes.
    assert!(Snapper::arm(SnapConfig { vps: vec![[0.0, 0.0]], ..Default::default() }, [5.0, 5.0]).is_none());
}

#[test]
fn hv_waits_then_snaps_then_locks() {
    let mut s = Snapper::arm(SnapConfig { hv: true, ..Default::default() }, [0.0, 0.0]).unwrap();
    assert_eq!(s.apply([3.0, 2.0]), [3.0, 2.0], "under 6 px nothing is projected");
    assert!(close(s.apply([30.0, 4.0]), [30.0, 0.0]));
    // Locked after 24 px: turning downward keeps the horizontal axis.
    assert!(close(s.apply([35.0, 60.0]), [35.0, 0.0]));
}

#[test]
fn axis_is_rechosen_until_committed() {
    let mut s = Snapper::arm(SnapConfig { hv: true, ..Default::default() }, [0.0, 0.0]).unwrap();
    assert!(close(s.apply([8.0, 1.0]), [8.0, 0.0]));
    assert!(close(s.apply([2.0, 10.0]), [0.0, 10.0]), "still choosing below 24 px");
}

#[test]
fn grid_snap_anchors_the_origin() {
    let s = Snapper::arm(SnapConfig { grid_step: Some(32.0), ..Default::default() }, [30.0, 35.0]).unwrap();
    assert_eq!(s.origin(), [32.0, 32.0]);
}

#[test]
fn soft_perspective_pulls_seventy_percent() {
    let cfg = SnapConfig { vps: vec![[-100.0, 50.0], [300.0, 50.0]], soft: true, ..Default::default() };
    let mut s = Snapper::arm(cfg, [100.0, 50.0]).unwrap();
    let p = s.apply([140.0, 60.0]);
    assert!((p[0] - 140.0).abs() < 1e-9 && (p[1] - 53.0).abs() < 1e-9, "{p:?}");
}

#[test]
fn perspective_adds_an_upright_axis() {
    let cfg = SnapConfig { vps: vec![[-100.0, 50.0], [300.0, 50.0]], soft: true, ..Default::default() };
    let c = cfg.candidates([100.0, 50.0], true);
    assert_eq!(c.len(), 3);
    assert!(c.contains(&[0.0, 1.0]));
    // With H/V on, H/V supplies the vertical instead.
    let c = SnapConfig { hv: true, ..cfg }.candidates([100.0, 50.0], true);
    assert_eq!(c.iter().filter(|d| **d == [0.0, 1.0]).count(), 1);
}

#[test]
fn hard_line_aims_at_the_nearest_vanishing_point() {
    let cfg = SnapConfig { vps: vec![[-100.0, 50.0], [300.0, 50.0]], ..Default::default() };
    assert!(close(hard_line_end(&cfg, [100.0, 50.0], [180.0, 58.0]), [180.0, 50.0]));
    let e = hard_line_end(&cfg, [100.0, 50.0], [102.0, 120.0]);
    assert!(close(e, [100.0, 120.0]), "vertical drag → upright axis: {e:?}");
}
