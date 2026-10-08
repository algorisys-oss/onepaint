#![allow(clippy::unwrap_used)]
//! Cases ported from happypaint `engine/quad-homography.test.ts`.
use onepaint_engine::warp::*;

const UNIT: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
fn min_w(m: &[f64; 9]) -> f64 {
    1f64.min(m[2] + 1.0).min(m[5] + 1.0).min(m[2] + m[5] + 1.0)
}
fn close(a: [f64; 2], b: [f64; 2], eps: f64) -> bool {
    (a[0] - b[0]).abs() < eps && (a[1] - b[1]).abs() < eps
}

#[test]
fn rectangles_and_trapezoids_map_corner_for_corner() {
    let rect = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
    let m = quad_homography(&rect);
    for (u, want) in UNIT.iter().zip(rect) {
        assert!(close(apply(&m, u[0], u[1]), want, 1e-9));
    }
    let trap = [[-0.6, -0.5], [0.6, -0.5], [0.3, 0.5], [-0.3, 0.5]];
    let m = quad_homography(&trap);
    assert!(min_w(&m) > 0.0);
    assert!(m[2].abs() + m[5].abs() > 1e-6, "projective");
    for (u, want) in UNIT.iter().zip(trap) {
        assert!(close(apply(&m, u[0], u[1]), want, 1e-9));
    }
    let inv = invert(&m).unwrap();
    let p = apply(&m, 0.3, 0.7);
    assert!(close(apply(&inv, p[0], p[1]), [0.3, 0.7], 1e-9), "inverse round-trips");
}

#[test]
fn the_sliver_cell_stays_finite() {
    let m = quad_homography(&[
        [-0.33433982025054676, -0.2419068844852338],
        [-0.282552500373521, -0.2378537131950943],
        [-0.2699168489877075, -0.2323724373579351],
        [-0.31976897901285195, -0.24243548331966092],
    ]);
    assert!(min_w(&m) > 0.0);
    for u in UNIT {
        let p = apply(&m, u[0], u[1]);
        assert!(p[0].is_finite() && p[1].is_finite() && p[0].abs() < 1.0 && p[1].abs() < 1.0);
    }
}

#[test]
fn every_cell_of_a_pulled_mesh_stays_positive() {
    let n = WARP_N;
    let mut pts: Vec<[f64; 2]> =
        (0..n * n).map(|i| [-0.45 + 0.9 * (i % n) as f64 / (n - 1) as f64, -0.25 + 0.05 * (i / n) as f64 / (n - 1) as f64]).collect();
    pts[n + 2] = [pts[n + 2][0] + 0.04, pts[n + 2][1] + 0.3];
    let g = WARP_GRID;
    for gy in 0..g {
        for gx in 0..g {
            let (u0, u1, v0, v1) = (gx as f64 / g as f64, (gx + 1) as f64 / g as f64, gy as f64 / g as f64, (gy + 1) as f64 / g as f64);
            let c = [sample_warp(&pts, u0, v0), sample_warp(&pts, u1, v0), sample_warp(&pts, u1, v1), sample_warp(&pts, u0, v1)];
            assert!(min_w(&quad_homography(&c)) > 0.0, "cell {gx},{gy}");
        }
    }
}

#[test]
fn the_identity_lattice_samples_to_the_rect() {
    let l = lattice(10.0, 20.0, 300.0, 90.0);
    assert!(close(sample_warp(&l, 0.0, 0.0), [10.0, 20.0], 1e-9));
    assert!(close(sample_warp(&l, 1.0, 1.0), [310.0, 110.0], 1e-9));
    // The middle segment is exactly linear (edge segments bow: clamped Catmull-Rom).
    assert!(close(sample_warp(&l, 0.5, 0.5), [160.0, 65.0], 1e-9));
}
