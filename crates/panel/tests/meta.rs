//! Ports of happypaint `engine/panel/panel-meta.test.ts`, plus tile coverage.

use onepaint_doc::Group;
use onepaint_panel::coverage::{TileCoverage, panel_mask_coverage, tile_coverage};
use onepaint_panel::geometry::{Pt, rect_outline};
use onepaint_panel::{PANEL_META_FIELDS, PanelSpec, panel_of, set_panel};

fn spec() -> PanelSpec {
    PanelSpec {
        outline: vec![Pt { x: 0.0, y: 0.0 }, Pt { x: 10.0, y: 0.0 }, Pt { x: 10.0, y: 10.0 }],
        gutter: Some(12.0),
        border: Some(4.0),
        ..Default::default()
    }
}

#[test]
fn carries_every_panel_field_a_node_has() {
    let mut g = Group::new("Panel 1");
    set_panel(&mut g, Some(spec()));
    assert_eq!(panel_of(&g), Some(spec()));
}

#[test]
fn adds_nothing_to_a_group_with_no_panel() {
    let g = Group::new("Group 1");
    assert_eq!(panel_of(&g), None);
    assert!(g.extra.is_empty());
}

#[test]
fn removing_a_panel_removes_the_key() {
    let mut g = Group::new("Panel 1");
    set_panel(&mut g, Some(spec()));
    set_panel(&mut g, None);
    assert!(g.extra.is_empty());
}

#[test]
fn the_declared_field_list_is_exactly_what_gets_stored() {
    let mut g = Group::new("Panel 1");
    set_panel(&mut g, Some(spec()));
    let keys: Vec<&str> = g.extra.keys().map(String::as_str).collect();
    assert_eq!(keys, PANEL_META_FIELDS);
}

#[test]
fn unrelated_group_fields_are_not_dragged_along() {
    let mut g = Group::new("Page 1");
    g.extra.insert("colorLabel".into(), "red".into());
    set_panel(&mut g, Some(spec()));
    assert_eq!(panel_of(&g), Some(spec()));
    assert_eq!(g.extra.get("colorLabel").and_then(|v| v.as_str()), Some("red"));
}

#[test]
fn the_copy_is_deep() {
    let mut g = Group::new("Panel 1");
    set_panel(&mut g, Some(spec()));
    let mut copy = panel_of(&g).expect("panel");
    copy.outline[0].x = 999.0;
    copy.gutter = Some(99.0);
    let orig = panel_of(&g).expect("panel");
    assert_eq!(orig.outline[0].x, 0.0);
    assert_eq!(orig.gutter, Some(12.0));
}

#[test]
fn json_shape_matches_happypaint() {
    let mut g = Group::new("Panel 1");
    set_panel(&mut g, Some(PanelSpec { border_color: Some("#000000".into()), ..spec() }));
    let v = &g.extra["panel"];
    assert_eq!(
        v,
        &serde_json::json!({
            "outline": [{"x": 0.0, "y": 0.0}, {"x": 10.0, "y": 0.0}, {"x": 10.0, "y": 10.0}],
            "gutter": 12.0, "border": 4.0, "borderColor": "#000000"
        })
    );
}

#[test]
fn unknown_panel_keys_survive() {
    let mut g = Group::new("Panel 1");
    g.extra.insert("panel".into(), serde_json::json!({"outline": [{"x": 0, "y": 0}, {"x": 9, "y": 0}, {"x": 9, "y": 9}], "rounded": 6}));
    let s = panel_of(&g).expect("panel");
    set_panel(&mut g, Some(s));
    assert_eq!(g.extra["panel"]["rounded"], serde_json::json!(6));
}

#[test]
fn only_the_recipe_travels() {
    for forbidden in ["mask", "maskTexture", "maskFramebuffer", "texture", "framebuffer"] {
        assert!(!PANEL_META_FIELDS.contains(&forbidden));
    }
    let v = serde_json::to_value(spec()).expect("json");
    for forbidden in ["mask", "texture"] {
        assert!(v.get(forbidden).is_none());
    }
}

// ── Tile coverage ──

#[test]
fn tile_coverage_matches_the_whole_page() {
    let (w, h) = (600u32, 520u32);
    let outline = vec![Pt { x: 30.5, y: 20.25 }, Pt { x: 570.0, y: 60.0 }, Pt { x: 500.0, y: 500.0 }, Pt { x: 10.0, y: 470.0 }];
    let s = PanelSpec { outline: outline.clone(), gutter: Some(24.0), ..Default::default() };
    let page = panel_mask_coverage(&outline, 24.0, w, h).expect("interior");
    for ty in 0..3 {
        for tx in 0..3 {
            let tc = tile_coverage(&s, tx, ty);
            for y in 0..256 {
                for x in 0..256 {
                    let (px, py) = (tx * 256 + x, ty * 256 + y);
                    if px >= w as i32 || py >= h as i32 {
                        continue;
                    }
                    let want = page[(py as u32 * w + px as u32) as usize] as f32 / 255.0;
                    let got = match &tc {
                        TileCoverage::Full => 1.0,
                        TileCoverage::Empty => 0.0,
                        TileCoverage::Partial(c) => c[(y * 256 + x) as usize],
                    };
                    assert!((want - got).abs() < 1e-6, "tile {tx},{ty} px {px},{py}: {want} vs {got}");
                }
            }
        }
    }
}

#[test]
fn tile_coverage_fast_paths() {
    let s =
        PanelSpec { outline: rect_outline(Pt { x: 0.0, y: 0.0 }, Pt { x: 1000.0, y: 1000.0 }), gutter: Some(20.0), ..Default::default() };
    assert!(matches!(tile_coverage(&s, 1, 1), TileCoverage::Full));
    assert!(matches!(tile_coverage(&s, 5, 0), TileCoverage::Empty));
    assert!(matches!(tile_coverage(&s, 0, 0), TileCoverage::Partial(_)));
}

#[test]
fn a_panel_with_no_interior_does_not_clip() {
    let s = PanelSpec { outline: rect_outline(Pt { x: 0.0, y: 0.0 }, Pt { x: 100.0, y: 30.0 }), gutter: Some(80.0), ..Default::default() };
    assert!(matches!(tile_coverage(&s, 0, 0), TileCoverage::Full));
    let s = PanelSpec { outline: vec![Pt { x: 0.0, y: 0.0 }], ..Default::default() };
    assert!(matches!(tile_coverage(&s, 3, 3), TileCoverage::Full));
}
