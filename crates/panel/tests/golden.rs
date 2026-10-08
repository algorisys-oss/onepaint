//! Goldens exported from happypaint with bun: a page split by a slanted cut,
//! each half inset by half an 18 px gutter and rasterised by
//! `rasterizeContoursCoverage`. Fixture: `fixtures/happypaint_split_masks.json`.

use onepaint_panel::coverage::panel_mask_coverage;
use onepaint_panel::geometry::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Golden {
    page: Vec<Pt>,
    left: Vec<Pt>,
    right: Vec<Pt>,
    inset_left: Vec<Pt>,
    inset_right: Vec<Pt>,
    border: Vec<Pt>,
    width: u32,
    height: u32,
    gutter: f64,
    masks: Vec<Vec<[u32; 2]>>,
}

fn golden() -> Golden {
    serde_json::from_str(include_str!("fixtures/happypaint_split_masks.json")).expect("fixture")
}

fn assert_pts(got: &[Pt], want: &[Pt]) {
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g.x - w.x).abs() < 1e-9 && (g.y - w.y).abs() < 1e-9, "{g:?} vs {w:?}");
    }
}

#[test]
fn split_and_inset_match_happypaint() {
    let g = golden();
    let page = rect_outline(Pt::new(13.3, 7.7), Pt::new(287.1, 190.4));
    assert_pts(&page, &g.page);
    let (l, r) = split_polygon_by_line(&page, Pt::new(120.2, -5.0), Pt::new(170.9, 210.0));
    assert_pts(&l, &g.left);
    assert_pts(&r, &g.right);
    assert_pts(&inset_polygon(&l, g.gutter / 2.0), &g.inset_left);
    assert_pts(&inset_polygon(&r, g.gutter / 2.0), &g.inset_right);
    assert_pts(&panel_border_outline(&r, g.gutter).expect("ring"), &g.border);
}

#[test]
fn masks_match_happypaint_byte_for_byte() {
    let g = golden();
    for (outline, rle) in [(&g.left, &g.masks[0]), (&g.right, &g.masks[1])] {
        let want: Vec<u8> = rle.iter().flat_map(|&[v, n]| std::iter::repeat_n(v as u8, n as usize)).collect();
        let got = panel_mask_coverage(outline, g.gutter, g.width, g.height).expect("interior");
        assert_eq!(got.len(), want.len());
        let diffs = got.iter().zip(&want).filter(|(a, b)| a != b).count();
        assert_eq!(diffs, 0, "{diffs} pixels differ");
    }
}
