//! Ports of happypaint `engine/panel/panel-geometry.test.ts`, case by case.

use onepaint_panel::coverage::panel_mask_coverage;
use onepaint_panel::geometry::*;

fn p(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

fn r() -> Vec<Pt> {
    rect_outline(p(0.0, 0.0), p(100.0, 80.0))
}

fn area(pts: &[Pt]) -> f64 {
    polygon_area(pts).abs()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() < tol
}

fn rev(pts: &[Pt]) -> Vec<Pt> {
    pts.iter().rev().copied().collect()
}

// ── rectOutline ──

#[test]
fn rect_four_corners_from_any_two_opposite_ones() {
    assert_eq!(r(), vec![p(0.0, 0.0), p(100.0, 0.0), p(100.0, 80.0), p(0.0, 80.0)]);
}

#[test]
fn rect_corner_order_does_not_matter() {
    assert_eq!(rect_outline(p(100.0, 80.0), p(0.0, 0.0)), r());
    assert_eq!(rect_outline(p(0.0, 80.0), p(100.0, 0.0)), r());
}

// ── polygonArea ──

#[test]
fn area_magnitude_is_the_real_area() {
    assert!(near(area(&r()), 8000.0, 1e-6));
}

#[test]
fn area_sign_flips_with_winding() {
    assert_eq!(polygon_area(&r()).signum(), -polygon_area(&rev(&r())).signum());
}

#[test]
fn degenerate_rings_have_no_area() {
    assert_eq!(polygon_area(&[]), 0.0);
    assert_eq!(polygon_area(&[p(0.0, 0.0), p(1.0, 1.0)]), 0.0);
}

// ── pointInPolygon ──

#[test]
fn point_inside_and_outside() {
    assert!(point_in_polygon(p(50.0, 40.0), &r()));
    assert!(!point_in_polygon(p(-5.0, 40.0), &r()));
    assert!(!point_in_polygon(p(50.0, 200.0), &r()));
}

#[test]
fn point_in_non_rectangular_panel() {
    let tri = [p(0.0, 0.0), p(100.0, 0.0), p(0.0, 100.0)];
    assert!(point_in_polygon(p(10.0, 10.0), &tri));
    assert!(!point_in_polygon(p(90.0, 90.0), &tri));
}

// ── insetPolygon ──

#[test]
fn inset_rectangle_to_the_obvious_smaller_rectangle() {
    let got = inset_polygon(&r(), 10.0);
    let mut xs: Vec<f64> = got.iter().map(|q| q.x).collect();
    let mut ys: Vec<f64> = got.iter().map(|q| q.y).collect();
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    assert!(near(xs[0], 10.0, 1e-6));
    assert!(near(xs[3], 90.0, 1e-6));
    assert!(near(ys[0], 10.0, 1e-6));
    assert!(near(ys[3], 70.0, 1e-6));
}

#[test]
fn inset_corners_stay_sharp() {
    assert_eq!(inset_polygon(&r(), 10.0).len(), 4);
}

#[test]
fn inset_is_independent_of_winding() {
    let cw = inset_polygon(&r(), 12.0);
    let ccw = inset_polygon(&rev(&r()), 12.0);
    assert!(near(area(&cw), area(&ccw), 1e-6));
}

#[test]
fn inset_zero_or_negative_leaves_the_polygon_alone() {
    assert_eq!(inset_polygon(&r(), 0.0), r());
    assert_eq!(inset_polygon(&r(), -5.0), r());
}

#[test]
fn inset_that_consumes_the_polygon_returns_nothing() {
    assert!(inset_polygon(&r(), 40.0).is_empty());
    assert!(inset_polygon(&r(), 41.0).is_empty());
    // Over-shooting on both axes folds the ring inside out with its sign intact.
    assert!(inset_polygon(&r(), 60.0).is_empty());
    assert!(inset_polygon(&r(), 500.0).is_empty());
}

#[test]
fn inset_result_always_fits_inside_the_original() {
    for d in [1.0, 5.0, 20.0, 35.0] {
        for q in inset_polygon(&r(), d) {
            assert!(point_in_polygon(q, &r()), "d={d} {q:?}");
        }
    }
}

// ── splitPolygonByLine ──

#[test]
fn vertical_cut_halves_a_rectangle() {
    let (l, rr) = split_polygon_by_line(&r(), p(50.0, -10.0), p(50.0, 90.0));
    assert!(near(area(&l), 4000.0, 1e-6));
    assert!(near(area(&rr), 4000.0, 1e-6));
}

#[test]
fn halves_add_back_up_at_any_angle() {
    for (a, b) in [
        (p(50.0, -10.0), p(50.0, 90.0)),
        (p(-10.0, 40.0), p(110.0, 40.0)),
        (p(-10.0, -10.0), p(110.0, 90.0)),
        (p(0.0, 80.0), p(100.0, 0.0)),
    ] {
        let (l, rr) = split_polygon_by_line(&r(), a, b);
        assert!(near(area(&l) + area(&rr), 8000.0, 1e-4));
    }
}

#[test]
fn cut_edge_is_shared_exactly() {
    let (l, rr) = split_polygon_by_line(&r(), p(30.0, -10.0), p(70.0, 90.0));
    let on_cut = |pts: &[Pt]| -> Vec<Pt> { pts.iter().copied().filter(|q| near(q.y, 0.0, 1e-6) || near(q.y, 80.0, 1e-6)).collect() };
    for q in on_cut(&l) {
        if q.x == 0.0 || q.x == 100.0 {
            continue;
        }
        assert!(on_cut(&rr).iter().any(|s| near(s.x, q.x, 1e-6) && near(s.y, q.y, 1e-6)));
    }
}

#[test]
fn cut_that_misses_returns_the_whole_polygon_on_one_side() {
    let (l, rr) = split_polygon_by_line(&r(), p(500.0, -10.0), p(500.0, 90.0));
    assert!(near(area(&l) + area(&rr), 8000.0, 1e-6));
    assert!(l.is_empty() || rr.is_empty());
}

#[test]
fn degenerate_line_is_a_no_op() {
    let (l, rr) = split_polygon_by_line(&r(), p(10.0, 10.0), p(10.0, 10.0));
    assert!(near(area(&l), 8000.0, 1e-6));
    assert!(rr.is_empty());
}

#[test]
fn splitting_twice_tiles_the_original() {
    let (l1, r1) = split_polygon_by_line(&r(), p(33.0, -10.0), p(33.0, 90.0));
    let (l2, r2) = split_polygon_by_line(&r1, p(66.0, -10.0), p(66.0, 90.0));
    assert!(near(area(&l1) + area(&l2) + area(&r2), 8000.0, 1e-4));
}

#[test]
fn each_half_stays_convex() {
    let (l, rr) = split_polygon_by_line(&r(), p(-10.0, -10.0), p(110.0, 90.0));
    for poly in [l, rr] {
        let mut sign = 0.0;
        for i in 0..poly.len() {
            let (a, b, c) = (poly[i], poly[(i + 1) % poly.len()], poly[(i + 2) % poly.len()]);
            let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
            if cross.abs() < 1e-9 {
                continue;
            }
            if sign == 0.0 {
                sign = cross.signum();
            } else {
                assert_eq!(cross.signum(), sign);
            }
        }
    }
}

// ── panelMaskCoverage ──
// happypaint injects a bounding-box rasteriser here; onepaint's real
// anti-aliased one is exact for these axis-aligned, whole-pixel rectangles.

fn at(cov: &[u8], x: usize, y: usize) -> u8 {
    cov[y * 200 + x]
}

#[test]
fn panel_with_no_gutter_covers_its_whole_outline() {
    let cov = panel_mask_coverage(&rect_outline(p(20.0, 20.0), p(120.0, 100.0)), 0.0, 200, 150).expect("interior");
    assert_eq!(at(&cov, 60, 60), 255);
    assert_eq!(at(&cov, 5, 5), 0);
}

#[test]
fn gutter_is_halved() {
    let cov = panel_mask_coverage(&rect_outline(p(20.0, 20.0), p(120.0, 100.0)), 20.0, 200, 150).expect("interior");
    assert_eq!(at(&cov, 25, 60), 0);
    assert_eq!(at(&cov, 35, 60), 255);
}

#[test]
fn neighbouring_panels_leave_exactly_one_gutter() {
    let whole = rect_outline(p(0.0, 0.0), p(200.0, 100.0));
    let (l, rr) = split_polygon_by_line(&whole, p(100.0, -10.0), p(100.0, 110.0));
    let g = 20.0;
    let li = inset_polygon(&l, g / 2.0);
    let ri = inset_polygon(&rr, g / 2.0);
    let left_edge = li.iter().map(|q| q.x).fold(f64::NEG_INFINITY, f64::max);
    let right_edge = ri.iter().map(|q| q.x).fold(f64::INFINITY, f64::min);
    assert!(near(right_edge - left_edge, g, 1e-6));
}

#[test]
fn frame_thinner_than_its_gutter_has_no_interior() {
    assert!(panel_mask_coverage(&rect_outline(p(0.0, 0.0), p(100.0, 30.0)), 80.0, 200, 150).is_none());
}

#[test]
fn degenerate_coverage_input_returns_none() {
    assert!(panel_mask_coverage(&[], 0.0, 200, 150).is_none());
    assert!(panel_mask_coverage(&r(), 0.0, 0, 150).is_none());
    assert!(panel_mask_coverage(&r(), 0.0, 200, 0).is_none());
}

// ── panelBorderOutline ──

#[test]
fn border_follows_the_clip_edge() {
    assert_eq!(panel_border_outline(&r(), 20.0), Some(inset_polygon(&r(), 10.0)));
}

#[test]
fn no_gutter_border_sits_on_the_outline() {
    assert_eq!(panel_border_outline(&r(), 0.0), Some(r()));
}

#[test]
fn frame_with_no_interior_has_nothing_to_outline() {
    assert_eq!(panel_border_outline(&rect_outline(p(0.0, 0.0), p(100.0, 30.0)), 80.0), None);
    assert_eq!(panel_border_outline(&[p(0.0, 0.0), p(1.0, 1.0)], 0.0), None);
}

// ── nearestVertex ──

fn square() -> Vec<Pt> {
    vec![p(0.0, 0.0), p(100.0, 0.0), p(100.0, 100.0), p(0.0, 100.0)]
}

#[test]
fn nearest_vertex_finds_the_corner_under_the_point() {
    assert_eq!(nearest_vertex(&square(), p(98.0, 3.0), 10.0), Some(1));
}

#[test]
fn nearest_vertex_none_when_out_of_reach() {
    assert_eq!(nearest_vertex(&square(), p(50.0, 50.0), 10.0), None);
}

#[test]
fn nearest_vertex_threshold_is_inclusive() {
    assert_eq!(nearest_vertex(&square(), p(0.0, 10.0), 10.0), Some(0));
}

#[test]
fn nearest_vertex_picks_the_nearest_of_two_in_range() {
    assert_eq!(nearest_vertex(&square(), p(60.0, 0.0), 60.0), Some(1));
}

#[test]
fn empty_outline_has_no_vertex() {
    assert_eq!(nearest_vertex(&[], p(0.0, 0.0), 10.0), None);
}

// ── distanceToSegment ──

#[test]
fn segment_distance_perpendicular_alongside() {
    assert!(near(distance_to_segment(p(50.0, 7.0), p(0.0, 0.0), p(100.0, 0.0)), 7.0, 1e-9));
}

#[test]
fn segment_distance_past_an_end_measures_to_the_endpoint() {
    assert!(near(distance_to_segment(p(200.0, 0.0), p(0.0, 0.0), p(100.0, 0.0)), 100.0, 1e-9));
}

#[test]
fn zero_length_segment_measures_to_the_point() {
    assert!(near(distance_to_segment(p(3.0, 4.0), p(0.0, 0.0), p(0.0, 0.0)), 5.0, 1e-9));
}

// ── distanceToPolygonEdge ──

#[test]
fn edge_distance_near_zero_on_an_edge() {
    assert!(near(distance_to_polygon_edge(&square(), p(50.0, 0.0)), 0.0, 1e-9));
}

#[test]
fn edge_distance_is_unsigned() {
    assert!(near(distance_to_polygon_edge(&square(), p(50.0, 3.0)), 3.0, 1e-9));
    assert!(near(distance_to_polygon_edge(&square(), p(50.0, -3.0)), 3.0, 1e-9));
}

#[test]
fn centre_of_a_frame_is_far_from_every_edge() {
    assert!(near(distance_to_polygon_edge(&square(), p(50.0, 50.0)), 50.0, 1e-9));
}

#[test]
fn edge_distance_closes_the_polygon() {
    assert!(near(distance_to_polygon_edge(&square(), p(0.0, 50.0)), 0.0, 1e-9));
}

// ── translateOutline ──

#[test]
fn translate_slides_every_point() {
    let tri = [p(0.0, 0.0), p(10.0, 0.0), p(0.0, 10.0)];
    assert_eq!(translate_outline(&tri, 5.0, -2.0), vec![p(5.0, -2.0), p(15.0, -2.0), p(5.0, 8.0)]);
}

#[test]
fn translate_keeps_the_area() {
    let tri = [p(0.0, 0.0), p(10.0, 0.0), p(0.0, 10.0)];
    assert!(near(polygon_area(&translate_outline(&tri, 100.0, 100.0)), polygon_area(&tri), 1e-9));
}

#[test]
fn translate_copies_rather_than_aliasing() {
    let tri = [p(0.0, 0.0), p(10.0, 0.0), p(0.0, 10.0)];
    let mut out = translate_outline(&tri, 1.0, 1.0);
    out[0].x = 999.0;
    assert_eq!(tri[0].x, 0.0);
}

// ── Layout grid (createPanelGrid, PE:6621-6638) ──

#[test]
fn grid_cells_tile_the_page_inside_the_margin() {
    let cells = grid_outlines(1000.0, 1400.0, 3, 2, 40.0);
    assert_eq!(cells.len(), 6);
    let total: f64 = cells.iter().map(|c| area(c)).sum();
    assert!(near(total, 920.0 * 1320.0, 1e-6));
    // Row-major from the top-left, like happypaint's "Panel N" numbering.
    assert_eq!(cells[0][0], p(40.0, 40.0));
    assert_eq!(cells[1][0], p(500.0, 40.0));
    assert_eq!(cells[2][0], p(40.0, 480.0));
}

#[test]
fn grid_clamps_rows_and_cols() {
    assert_eq!(grid_outlines(1000.0, 1000.0, 0, 50, 0.0).len(), 12);
}

#[test]
fn grid_with_no_room_is_empty() {
    assert!(grid_outlines(60.0, 60.0, 2, 2, 40.0).is_empty());
}

#[test]
fn layout_presets_match_happypaint() {
    let got: Vec<(u32, u32)> = LAYOUT_PRESETS.iter().map(|l| (l.rows, l.cols)).collect();
    assert_eq!(got, vec![(1, 2), (2, 1), (2, 2), (3, 1), (3, 2), (4, 2)]);
}
