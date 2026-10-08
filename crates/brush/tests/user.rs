#![allow(clippy::unwrap_used)]
//! The user brush registry (spec 23): imported presets, tips and grains
//! resolve through the same lookups as the built-ins.

use onepaint_brush::*;
use std::sync::Arc;

fn ring(n: usize) -> TipMask {
    let mut data = vec![0.0; n * n];
    for y in 0..n {
        for x in 0..n {
            let r = ((x as f64 + 0.5 - n as f64 / 2.0).hypot(y as f64 + 0.5 - n as f64 / 2.0)) / (n as f64 / 2.0);
            data[y * n + x] = if (0.6..0.95).contains(&r) { 1.0 } else { 0.0 };
        }
    }
    TipMask { size: n, data }
}

fn imported(id: &str, tip: &str) -> BrushPreset {
    let mut p = preset("hard-round").unwrap();
    p.id = id.into();
    p.name = format!("Brush {id}");
    p.category = IMPORTED_CATEGORY.into();
    p.tip_id = tip.into();
    p
}

#[test]
fn registered_presets_tips_and_grains_resolve() {
    let mut lib = UserLibrary::default();
    lib.presets.push(imported("imported-reg-a", "custom:reg-a"));
    lib.tips.insert("custom:reg-a".into(), Arc::new(ring(32)));
    lib.grains.insert("customgrain:reg-g".into(), UserGrain { name: "G".into(), mask: Arc::new(TipMask::from_u8(2, &[0, 255, 255, 0])) });
    assert!(preset("imported-reg-a").is_none());
    assert!(matches!(tip_for("custom:reg-a"), Tip::Circle), "unknown user tips fall back to round");
    add_user_brushes(lib);

    let p = preset("imported-reg-a").unwrap();
    assert_eq!(p.category, IMPORTED_CATEGORY);
    assert!(user_presets().iter().any(|q| q.id == "imported-reg-a"));
    assert!(!presets().iter().any(|q| q.id == "imported-reg-a"), "built-ins stay built-in");
    match tip_for("custom:reg-a") {
        Tip::Image(m) => assert_eq!(m.size, 32),
        t => panic!("{t:?}"),
    }
    let g = grain_for("customgrain:reg-g").unwrap();
    assert_eq!(g.to_u8(), vec![0, 255, 255, 0]);
    // Built-ins still resolve.
    assert!(matches!(tip_for("chalk"), Tip::Image(_)));
    assert!(grain_for("paper").is_some());

    assert!(remove_user_brush("imported-reg-a"));
    assert!(preset("imported-reg-a").is_none());
    assert!(matches!(tip_for("custom:reg-a"), Tip::Circle), "an unused tip goes with its preset");
    assert!(grain_for("customgrain:reg-g").is_some(), "grains are a library of their own");
    assert!(!remove_user_brush("imported-reg-a"));
}

#[test]
fn merge_replaces_by_id_and_keeps_shared_tips() {
    let mut lib = UserLibrary::default();
    lib.presets.push(imported("a", "custom:t"));
    lib.presets.push(imported("b", "custom:t"));
    lib.tips.insert("custom:t".into(), Arc::new(ring(8)));
    let mut again = UserLibrary::default();
    let mut a2 = imported("a", "custom:t");
    a2.size = 99.0;
    again.presets.push(a2);
    lib.merge(again);
    assert_eq!(lib.presets.len(), 2);
    assert_eq!(lib.presets[0].size, 99.0);
    assert!(lib.remove("a"));
    assert!(lib.tips.contains_key("custom:t"), "b still uses the tip");
    assert!(lib.remove("b"));
    assert!(lib.tips.is_empty());
}

#[test]
fn image_tip_paints_its_shape() {
    let mut lib = UserLibrary::default();
    lib.presets.push(imported("imported-ring", "custom:ring"));
    lib.tips.insert("custom:ring".into(), Arc::new(ring(64)));
    add_user_brushes(lib);
    let mut b = preset("imported-ring").unwrap();
    b.size = 60.0;
    let eng = StrokeEngine::new(b, [1.0; 3], 1);
    let mut s = onepaint_raster::Surface::new();
    let d = Dab { x: 50.0, y: 50.0, radius: 30.0, alpha: 1.0, rotation: 0.0, aspect: 1.0, hardness: 1.0, color: [1.0; 3] };
    eng.rasterize(&mut s, &d, None);
    assert!(s.get(50, 50)[3] < 0.05, "hole in the middle: {:?}", s.get(50, 50));
    assert!(s.get(50 + 23, 50)[3] > 0.5, "ring: {:?}", s.get(73, 50));
}

#[test]
fn custom_brushes_duplicate_rename_and_save_over() {
    // Spec 27: a pure UserLibrary, so no global state is touched.
    let mut lib = UserLibrary::default();
    let mut base = preset("soft-round").unwrap();
    base.size = 42.0;
    let a = lib.add_custom(&base, "  My soft  ");
    assert_eq!((a.id.as_str(), a.name.as_str(), a.category.as_str()), ("b_1", "My soft", CUSTOM_CATEGORY));
    assert_eq!(a.size, 42.0);
    let b = lib.add_custom(&base, "   ");
    assert_eq!((b.id.as_str(), b.name.as_str()), ("b_2", "Brush"));
    lib.remove("b_1");
    assert_eq!(lib.add_custom(&base, "x").id, "b_1", "first free number");

    let e = lib.add_custom(&preset("eraser-hard").unwrap(), "My eraser");
    assert!(e.is_eraser(), "a custom eraser stays an eraser");

    assert!(lib.rename("b_2", "Renamed"));
    assert_eq!(lib.preset("b_2").unwrap().name, "Renamed");
    assert!(!lib.rename("soft-round", "nope"), "built-ins can't be renamed");

    let mut edited = lib.preset("b_2").unwrap().clone();
    edited.hardness = 0.25;
    assert!(lib.update(&edited));
    assert_eq!(lib.preset("b_2").unwrap().hardness, 0.25);
    assert!(!lib.update(&preset("hard-round").unwrap()), "built-ins are read-only");
}
