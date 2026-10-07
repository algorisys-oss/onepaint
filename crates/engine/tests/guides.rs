#![allow(clippy::unwrap_used)]
use onepaint_engine::guides::{PerspectiveSnap, perspective_preset};
use onepaint_engine::*;

const STORED: &str = r##"{
  "grid": {"enabled": true, "size": 32, "subdivisions": 4, "snap": true, "color": "#ff0000"},
  "symmetry": {"mode": "radial", "cx": 0.25, "cy": 0.75, "radialCount": 8, "angle": 0.5},
  "perspective": {"enabled": true, "points": [{"x": 0.5, "y": 0.5}], "snap": "soft", "curved": false, "density": 9, "color": "#00ff00"},
  "snapHV": true,
  "visible": false,
  "lines": [{"id": "g1", "x": 10, "y": 20, "angle": 1.5707963}],
  "lineColor": "#123456",
  "rulers": true,
  "units": "cm",
  "snapGuides": false
}"##;

#[test]
fn reads_and_writes_happypaint_stored_guides() {
    let g: Guides = serde_json::from_str(STORED).unwrap();
    assert!(g.grid.enabled && g.grid.snap && g.grid.subdivisions == 4);
    assert_eq!(g.grid.step(), 8.0);
    assert_eq!(g.symmetry.mode, SymmetryMode::Radial);
    assert_eq!(g.symmetry.radial_count, 8);
    assert_eq!(g.perspective.snap, PerspectiveSnap::Soft);
    assert!(g.snap_hv && !g.visible && g.rulers);
    assert_eq!(g.lines[0].id, "g1");
    let v = serde_json::to_value(&g).unwrap();
    let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    for k in ["grid", "symmetry", "perspective", "snapHV", "visible", "lines", "lineColor", "rulers", "units", "snapGuides"] {
        assert!(keys.contains(&k.to_string()), "{k}");
    }
    assert_eq!(v["symmetry"]["radialCount"], 8);
    // Partial objects fall back to defaults.
    let g: Guides = serde_json::from_str(r#"{"grid": {"enabled": true}}"#).unwrap();
    assert_eq!(g.grid.size, 64.0);
    assert_eq!(g.perspective.points, perspective_preset(2));
}

#[test]
fn guides_travel_with_the_document() {
    let mut s = Session::new(64, 64, None);
    let unconfigured = s.save_happypaint().unwrap();
    let doc = onepaint_formats::read_happypaint(&unconfigured).unwrap();
    assert!(!doc.extra.contains_key("guides"), "no guides key until configured");

    let mut g = s.guides().clone();
    g.grid.enabled = true;
    g.grid.size = 1000.0;
    g.perspective.enabled = true;
    s.execute(Command::SetGuides { guides: Box::new(g) }).unwrap();
    assert_eq!(s.guides().grid.size, 512.0, "clamped");
    let bytes = s.save_happypaint().unwrap();

    let mut t = Session::new(8, 8, None);
    t.execute(Command::SetSymmetry { symmetry: Symmetry { mode: SymmetryMode::Vertical, ..Default::default() } }).unwrap();
    t.open_happypaint(&bytes).unwrap();
    assert!(t.guides().grid.enabled && t.guides().perspective.enabled);
    assert_eq!(t.guides().symmetry.mode, SymmetryMode::Off, "the previous document's guides don't leak");
}

#[test]
fn snap_config_follows_the_guides() {
    let mut g = Guides::default();
    assert!(!g.snap_config(100, 100, 1.0, false).active());
    g.perspective.enabled = true;
    assert!(g.hard_perspective(false));
    assert!(!g.hard_perspective(true), "editing points suspends it");
    let c = g.snap_config(100, 100, 1.0, false);
    assert_eq!(c.vps, vec![[-80.0, 50.0], [180.0, 50.0]]);
    assert!(!c.soft);
    g.perspective.snap = PerspectiveSnap::Soft;
    assert!(g.snap_config(100, 100, 1.0, false).active());
    g.grid.enabled = true;
    g.grid.snap = true;
    g.grid.subdivisions = 2;
    assert_eq!(g.snap_config(100, 100, 1.0, false).grid_step, Some(32.0));
}

#[test]
fn guide_line_edits_are_one_undo_step() {
    use onepaint_engine::guides::GuideLine;
    let mut s = Session::new(200, 100, None);
    let h0 = s.history_len();
    for y in [10.0, 20.0, 30.0, 40.0] {
        s.execute(Command::EditGuideLines { lines: vec![GuideLine::new("a", 0.0, y, 0.0)] }).unwrap();
    }
    s.execute(Command::CommitGuideLines).unwrap();
    assert_eq!(s.history_len(), h0 + 1, "a drag of many positions is one step");
    assert_eq!(s.guides().lines[0].y, 40.0);
    s.execute(Command::Undo).unwrap();
    assert!(s.guides().lines.is_empty());
    s.execute(Command::Redo).unwrap();
    assert_eq!(s.guides().lines.len(), 1);
    // Committing with nothing changed records nothing.
    s.execute(Command::CommitGuideLines).unwrap();
    assert_eq!(s.history_len(), h0 + 1);
    // Angles are canonical: θ + π is the same guide.
    s.execute(Command::EditGuideLines {
        lines: vec![GuideLine { angle: std::f64::consts::PI * 1.5, ..GuideLine::new("v", 5.0, 0.0, 0.0) }],
    })
    .unwrap();
    assert!((s.guides().lines[0].angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
}
