#![allow(clippy::unwrap_used)]
//! Brush import and export (spec 23), checked against goldens from
//! happypaint's own parsers (`scripts/brush-fixtures/make.ts`).

use onepaint_brush::{BrushPreset, IMPORTED_CATEGORY, TipMask, UserLibrary};
use onepaint_formats::brushes::{self, bplist::Plist};
use serde_json::Value;
use std::sync::Arc;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/brushes");

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DIR}/{name}")).unwrap()
}

fn golden() -> Value {
    serde_json::from_slice(&fixture("golden.json")).unwrap()
}

/// Every key in `want` is in `got` with an equal value (numbers within 1e-9).
fn subset(want: &Value, got: &Value, path: &str) {
    match (want, got) {
        (Value::Object(w), Value::Object(g)) => {
            for (k, v) in w {
                subset(v, g.get(k).unwrap_or(&Value::Null), &format!("{path}.{k}"));
            }
        }
        (Value::Array(w), Value::Array(g)) => {
            assert_eq!(w.len(), g.len(), "{path}: {want} vs {got}");
            for (i, (a, b)) in w.iter().zip(g).enumerate() {
                subset(a, b, &format!("{path}[{i}]"));
            }
        }
        (Value::Number(a), Value::Number(b)) => {
            assert!((a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9, "{path}: {a} vs {b}")
        }
        _ => assert_eq!(want, got, "{path}"),
    }
}

fn as_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}

#[test]
fn abr_matches_happypaint_parse() {
    let g = golden();
    for name in ["v1", "v2", "v6"] {
        let f = brushes::parse_abr(&fixture(&format!("{name}.abr"))).unwrap();
        let want = &g[name];
        let tips = want["tips"].as_array().unwrap();
        assert_eq!(f.tips.len(), tips.len(), "{name} tips");
        for (t, w) in f.tips.iter().zip(tips) {
            assert_eq!(t.name.as_deref(), w["name"].as_str(), "{name}");
            assert_eq!(t.width as u64, w["width"].as_u64().unwrap());
            assert_eq!(t.height as u64, w["height"].as_u64().unwrap());
            assert_eq!(t.uuid.as_deref(), w["uuid"].as_str());
            let data: Vec<u8> = w["data"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
            assert_eq!(t.data, data, "{name} tip pixels");
            assert_eq!(t.desc.is_some(), !w["mapped"].is_null());
        }
        let computed = want["computed"].as_array().unwrap();
        assert_eq!(f.computed.len(), computed.len());
        for (c, w) in f.computed.iter().zip(computed) {
            subset(
                w,
                &serde_json::json!({"name": c.name, "diameter": c.diameter, "hardness": c.hardness,
                "spacing": c.spacing, "angle": c.angle, "roundness": c.roundness}),
                name,
            );
        }
        let patterns = want["patterns"].as_array().unwrap();
        assert_eq!(f.patterns.len(), patterns.len());
        for (p, w) in f.patterns.iter().zip(patterns) {
            assert_eq!(p.name, w["name"].as_str().unwrap());
            assert_eq!((p.width as u64, p.height as u64), (w["width"].as_u64().unwrap(), w["height"].as_u64().unwrap()));
            let data: Vec<u8> = w["data"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
            assert_eq!(p.data, data, "{name} pattern {}", p.name);
        }
        let warnings: Vec<&str> = want["warnings"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(f.warnings, warnings);
    }
}

#[test]
fn abr_descriptor_mapping_matches_happypaint() {
    let g = golden();
    let f = brushes::parse_abr(&fixture("v6.abr")).unwrap();
    for (t, w) in f.tips.iter().zip(g["v6"]["tips"].as_array().unwrap()) {
        let want = &w["mapped"];
        let mut p = brushes::import_defaults();
        let m = brushes::map_abr_descriptor(t.desc.as_ref().unwrap(), &mut p);
        subset(&want["settings"], &as_json(&p), "settings");
        match (&m.tip_transform, want.get("tipTransform")) {
            (Some(tt), Some(wt)) => subset(
                wt,
                &serde_json::json!({"angleDeg": tt.angle_deg, "roundnessPct": tt.roundness_pct,
                "flipX": tt.flip_x, "flipY": tt.flip_y}),
                "tipTransform",
            ),
            (None, None) => {}
            (a, b) => panic!("tip transform {a:?} vs {b:?}"),
        }
        assert_eq!(m.dual_tip_uuid.as_deref(), want.get("dualTipUuid").and_then(Value::as_str));
        let unmapped: Vec<&str> = want["unmapped"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(m.unmapped, unmapped);
    }
}

#[test]
fn abr_rejects_unknown_versions_and_skips_bad_records() {
    assert!(brushes::parse_abr(&[0, 3, 0, 1]).is_err());
    assert!(brushes::parse_abr(&[0, 6, 0, 3]).is_err());
    assert!(brushes::parse_abr(&[0, 1]).is_err());
    // A truncated v2 record is skipped, not fatal.
    let f = brushes::parse_abr(&[0, 2, 0, 1, 0, 2, 0, 0, 0, 40, 1, 2]).unwrap();
    assert!(f.tips.is_empty());
    assert_eq!(f.warnings, ["skipped malformed v2 brush #1"]);
}

#[test]
fn packbits_decodes_literals_runs_and_noops() {
    let mut out = [0u8; 7];
    brushes::packbits(&[2, 1, 2, 3, 0x80, 0xfd, 9], &mut out);
    assert_eq!(out, [1, 2, 3, 9, 9, 9, 9]);
}

#[test]
fn bplist_reads_happypaint_params_and_round_trips() {
    let g = golden();
    let parsed = brushes::parse_procreate(&fixture("one.brush"), "one.brush").unwrap();
    let want = g["brush"].as_array().unwrap();
    assert_eq!(parsed.len(), 1);
    let b = &parsed[0];
    assert_eq!(b.name, want[0]["name"].as_str().unwrap());
    assert_eq!(b.shape_png.as_ref().map(Vec::len), want[0]["shape"].as_u64().map(|n| n as usize));
    assert_eq!(b.grain_png.as_ref().map(Vec::len), want[0]["grain"].as_u64().map(|n| n as usize));
    let params = Value::Object(b.params.iter().map(|(k, v)| (k.clone(), v.to_json())).collect());
    subset(&want[0]["params"], &params, "params");
    assert_eq!(params.as_object().unwrap().len(), want[0]["params"].as_object().unwrap().len());

    let p = Plist::Dict(
        [
            ("a".to_string(), Plist::Int(-5)),
            ("b".into(), Plist::Real(0.25)),
            ("c".into(), Plist::String("héllo ✎".into())),
            ("d".into(), Plist::Array(vec![Plist::Bool(true), Plist::Null, Plist::Uid(3), Plist::Data(vec![1, 2])])),
            ("e".into(), Plist::String("x".repeat(40))),
            ("f".into(), Plist::Int(1 << 40)),
        ]
        .into_iter()
        .collect(),
    );
    assert_eq!(brushes::bplist::parse_bplist(&brushes::bplist::write_bplist(&p)).unwrap(), p);
    assert!(brushes::bplist::parse_bplist(b"notaplist").is_err());
}

#[test]
fn brushset_groups_by_folder_like_happypaint() {
    let g = golden();
    let parsed = brushes::parse_procreate(&fixture("set.brushset"), "set.brushset").unwrap();
    let want = g["brushset"].as_array().unwrap();
    let names: Vec<&str> = parsed.iter().map(|b| b.name.as_str()).collect();
    let want_names: Vec<&str> = want.iter().map(|b| b["name"].as_str().unwrap()).collect();
    assert_eq!(names, want_names);
    for (b, w) in parsed.iter().zip(want) {
        let params = Value::Object(b.params.iter().map(|(k, v)| (k.clone(), v.to_json())).collect());
        subset(&w["params"], &params, "params");
        assert_eq!(params.as_object().unwrap().len(), w["params"].as_object().unwrap().len());
        assert_eq!(b.shape_png.as_ref().map(Vec::len), w["shape"].as_u64().map(|n| n as usize));
    }
}

fn tip_of(lib: &UserLibrary, p: &BrushPreset) -> Arc<TipMask> {
    lib.tips.get(&p.tip_id).unwrap_or_else(|| panic!("tip {}", p.tip_id)).clone()
}

#[test]
fn procreate_brush_imports_with_mapped_params_shape_and_grain() {
    let imp = brushes::import_brush_file("one.brush", &fixture("one.brush")).unwrap();
    assert_eq!((imp.brushes, imp.grains), (1, 1));
    let lib = &imp.library;
    let p = &lib.presets[0];
    assert_eq!(p.name, "Soft Chalk ✎");
    assert_eq!(p.category, IMPORTED_CATEGORY);
    assert!(p.id.starts_with("imported-"));
    assert_eq!(p.size, 100.0);
    assert!((p.spacing - 0.15).abs() < 1e-9);
    assert!((p.opacity - 0.8).abs() < 1e-9);
    assert!((p.size_pressure - 0.6).abs() < 1e-9 && (p.flow_pressure - 0.5).abs() < 1e-9);
    let d = p.dynamics.as_ref().unwrap();
    assert!((d.size[0].min.unwrap() - 0.75).abs() < 1e-9);
    assert!((d.flow[0].min.unwrap() - 0.9).abs() < 1e-9);
    assert_eq!(d.scatter, Some(1.4));
    let r = d.rotation.as_ref().unwrap();
    assert_eq!((r.source.as_str(), r.jitter), ("angle", Some(0.3)));
    let gr = p.grain.as_ref().unwrap();
    assert!(gr.id.starts_with("customgrain:"));
    assert_eq!((gr.scale, gr.moves), (115.0, Some(0.0)));
    assert!((gr.strength - 0.7).abs() < 1e-9);
    assert_eq!(lib.grains[&gr.id].name, "Soft Chalk ✎ grain");
    assert_eq!(lib.grains[&gr.id].mask.size, 256);
    // White disc on black: light = paint, contain-fitted to 128².
    assert!(p.tip_id.starts_with("custom:"));
    let tip = tip_of(lib, p);
    assert_eq!(tip.size, 128);
    assert!(tip.sample(0.5, 0.5) > 0.95);
    assert!(tip.sample(0.02, 0.02) < 0.05);
    assert!(tip.sample(0.5, 0.03) > 0.3, "the disc fills the box");
}

#[test]
fn brushset_imports_each_brush_with_auto_polarity() {
    let imp = brushes::import_brush_file("set.brushset", &fixture("set.brushset")).unwrap();
    let lib = &imp.library;
    let names: Vec<&str> = lib.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Ink Pen", "Inky"]);
    let ink = &lib.presets[0];
    assert_eq!(ink.size, 25.0);
    // A dark bar on white: dark = paint, so the bar (not the field) is the tip.
    let tip = tip_of(lib, ink);
    assert!(tip.sample(0.5, 0.5) > 0.85, "1 - 30/255");
    assert!(tip.sample(0.1, 0.5) < 0.05);
    assert!((lib.presets[1].spacing - 0.05).abs() < 1e-9);
    assert_eq!(lib.presets[1].size, 60.0);
    assert!(lib.grains.is_empty());
}

#[test]
fn abr_imports_tips_computed_brushes_patterns_and_dual_tips() {
    let imp = brushes::import_brush_file("v6.abr", &fixture("v6.abr")).unwrap();
    let lib = &imp.library;
    assert_eq!((imp.brushes, imp.grains), (2, 2));
    let names: Vec<&str> = lib.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Chalk Dust", "Leaf"]);
    let (chalk, leaf) = (&lib.presets[0], &lib.presets[1]);
    assert_eq!(chalk.size, 45.0);
    assert_eq!(leaf.size, 20.0);
    assert!((leaf.spacing - 0.05).abs() < 1e-9);
    assert_eq!(chalk.extra["maskTip"], serde_json::json!({"id": leaf.tip_id, "strength": 0.7}));
    assert_ne!(chalk.tip_id, leaf.tip_id);
    let grain_names: Vec<&str> = lib.grains.values().map(|g| g.name.as_str()).collect();
    assert!(grain_names.contains(&"Gradient") && grain_names.contains(&"Colour"));
    assert!(lib.grains.values().all(|g| g.mask.size == 256));

    let v2 = brushes::import_brush_file("v2.abr", &fixture("v2.abr")).unwrap().library;
    let names: Vec<&str> = v2.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Sampled ellipse", "Round 9"]);
    assert_eq!(v2.presets[0].size, 24.0);
    let round = &v2.presets[1];
    assert_eq!((round.tip_id.as_str(), round.size), ("round", 9.0));
    assert!((round.hardness - 0.8).abs() < 1e-9 && (round.spacing - 0.3).abs() < 1e-9);

    let v1 = brushes::import_brush_file("v1.abr", &fixture("v1.abr")).unwrap().library;
    let names: Vec<&str> = v1.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["v1 1", "v1 round 1"]);
    // Importing the same file again gives the same ids.
    let again = brushes::import_brush_file("v1.abr", &fixture("v1.abr")).unwrap().library;
    assert_eq!(again.presets.iter().map(|p| &p.id).collect::<Vec<_>>(), v1.presets.iter().map(|p| &p.id).collect::<Vec<_>>());
}

#[test]
fn happypaint_happybrush_decodes() {
    let g = golden();
    let d = brushes::decode_happybrush(&fixture("chalk.happybrush")).unwrap();
    assert_eq!(d.name, "Chalk (mine)");
    let mut want = g["happybrushSettings"].clone();
    // happypaint's `move: 1` grain flag is onepaint's `moves`, serialised as `move`.
    want.as_object_mut().unwrap().remove("id");
    subset(&want, &as_json(&d.settings), "settings");
    let tip = d.tip.as_ref().unwrap();
    assert_eq!(tip.len(), 128 * 128);
    assert_eq!(tip[64 * 128 + 64], 255);
    assert_eq!(d.grain.as_ref().unwrap().len(), 256 * 256);

    let imp = brushes::import_brush_file("chalk.happybrush", &fixture("chalk.happybrush")).unwrap();
    let p = &imp.library.presets[0];
    assert!(p.tip_id.starts_with("custom:") && p.tip_id != "custom:embedded");
    let gid = &p.grain.as_ref().unwrap().id;
    assert!(gid.starts_with("customgrain:") && gid != "customgrain:embedded");
    assert_eq!(imp.library.tips[&p.tip_id].to_u8(), *tip);
    assert_eq!(imp.library.grains[gid].mask.to_u8(), *d.grain.as_ref().unwrap());
    assert_eq!(p.name, "Chalk (mine)");
}

#[test]
fn happybrush_round_trips_and_degrades_missing_assets() {
    let imp = brushes::import_brush_file("chalk.happybrush", &fixture("chalk.happybrush")).unwrap();
    let lib = imp.library;
    let p = lib.presets[0].clone();
    let tip = lib.tips[&p.tip_id].clone();
    let grain = lib.grains[&p.grain.as_ref().unwrap().id].mask.clone();
    let bytes = brushes::encode_happybrush(&p, Some(&tip), Some(&grain)).unwrap();
    let names: Vec<String> = onepaint_formats::zip::read_zip(&bytes).unwrap().into_iter().map(|e| e.0).collect();
    assert_eq!(names, ["manifest.json", "tip.png", "grain.png"]);
    let manifest: Value = serde_json::from_slice(&onepaint_formats::zip::read_zip(&bytes).unwrap()[0].1).unwrap();
    assert_eq!(manifest["format"], "happybrush");
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["settings"]["tipId"], "custom:embedded");
    assert!(manifest["settings"].get("id").is_none() && manifest["settings"].get("category").is_none());

    let back = brushes::import_brush_file("x.happybrush", &bytes).unwrap().library;
    assert_eq!(back.presets[0], p, "same content, same ids");
    assert_eq!(back.tips[&p.tip_id], tip);

    // Built-in tips stay ids; a custom reference with no bitmap degrades.
    let mut q = brushes::import_defaults();
    q.name = "Loose".into();
    q.tip_id = "custom:gone".into();
    q.grain = Some(onepaint_brush::Grain { id: "customgrain:gone".into(), scale: 50.0, strength: 0.5, ..Default::default() });
    let back = brushes::import_brush_file("l.happybrush", &brushes::encode_happybrush(&q, None, None).unwrap()).unwrap().library;
    assert_eq!(back.presets[0].tip_id, "round");
    assert!(back.presets[0].grain.is_none());
    q.tip_id = "chalk".into();
    q.grain = Some(onepaint_brush::Grain { id: "paper".into(), scale: 50.0, strength: 0.5, ..Default::default() });
    let back = brushes::import_brush_file("l.happybrush", &brushes::encode_happybrush(&q, None, None).unwrap()).unwrap().library;
    assert_eq!(back.presets[0].tip_id, "chalk");
    assert_eq!(back.presets[0].grain.as_ref().unwrap().id, "paper");
}

#[test]
fn happybrush_rejects_bad_manifests() {
    let zip = |m: &str| onepaint_formats::zip::write_zip(&[("manifest.json", m.as_bytes())]);
    assert!(brushes::decode_happybrush(&onepaint_formats::zip::write_zip(&[("x", b"1")])).is_err());
    assert!(brushes::decode_happybrush(&zip(r#"{"format":"other","version":1,"settings":{}}"#)).is_err());
    assert!(brushes::decode_happybrush(&zip(r#"{"format":"happybrush","version":2,"settings":{}}"#)).is_err());
    assert!(brushes::decode_happybrush(&zip(r#"{"format":"happybrush","version":1,"settings":3}"#)).is_err());
    let d = brushes::decode_happybrush(&zip(r#"{"format":"happybrush","version":1,"name":"  ","settings":{"size":12}}"#)).unwrap();
    assert_eq!(d.name, "Imported brush");
    assert_eq!(d.settings.size, 12.0);
    assert_eq!(d.settings.tip_id, "round", "missing settings fall back to the import defaults");
}

#[test]
fn user_library_survives_save_and_load() {
    let dir = std::env::temp_dir().join(format!("onepaint-brushlib-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut lib = brushes::import_brush_file("v6.abr", &fixture("v6.abr")).unwrap().library;
    lib.merge(brushes::import_brush_file("one.brush", &fixture("one.brush")).unwrap().library);
    brushes::save_user_library(&dir, &lib).unwrap();
    let back = brushes::load_user_library(&dir);
    assert_eq!(back, lib);
    // Deleting a brush drops its tip file on the next save.
    let gone = lib.presets[0].tip_id.clone();
    let id = lib.presets[0].id.clone();
    lib.remove(&id);
    brushes::save_user_library(&dir, &lib).unwrap();
    assert_eq!(brushes::load_user_library(&dir), lib);
    let tips = std::fs::read_dir(dir.join("tips")).unwrap().count();
    assert_eq!(tips, lib.tips.len());
    assert!(!lib.tips.contains_key(&gone));
    assert!(brushes::load_user_library(&dir.join("missing")).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_files_error_cleanly() {
    assert!(brushes::import_brush_file("x.brush", b"not a zip").is_err());
    assert!(brushes::import_brush_file("x.abr", b"").is_err());
    assert!(brushes::is_brush_file("Pack.ABR") && brushes::is_brush_file("a.brushset") && !brushes::is_brush_file("a.png"));
}

#[test]
fn tip_transform_rotates_squashes_and_flips() {
    use brushes::{TipTransform, tip_mask::transform_coverage};
    // A 4×2 bar with a marked left end.
    let cov = [255, 100, 100, 100, 255, 100, 100, 100];
    let t = TipTransform { angle_deg: 90.0, roundness_pct: 100.0, flip_x: false, flip_y: false };
    let (out, w, h) = transform_coverage(&cov, 4, 2, &t);
    // cos(90°) is 6e-17, so the box rounds up to 3 wide, as in happypaint.
    assert_eq!((w, h), (3, 4));
    // Photoshop angles are counter-clockwise: the left end ends up at the bottom.
    let row_max = |y: usize| *out[y * w..(y + 1) * w].iter().max().unwrap();
    assert!(row_max(3) > 200 && row_max(0) <= 100, "{out:?}");
    let t = TipTransform { angle_deg: 0.0, roundness_pct: 50.0, flip_x: true, flip_y: false };
    let (out, w, h) = transform_coverage(&cov, 4, 2, &t);
    assert_eq!((w, h), (4, 1));
    assert!(out[3] > out[0], "flipped: {out:?}");
}

#[test]
fn custom_brush_saves_and_reloads() {
    // Spec 27: a studio-made brush (with an imported tip) persists.
    let dir = std::env::temp_dir().join(format!("onepaint-custombrush-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut lib = brushes::import_brush_file("one.brush", &fixture("one.brush")).unwrap().library;
    let mut edited = lib.presets[0].clone();
    edited.size = 77.0;
    edited.spacing = 0.33;
    let custom = lib.add_custom(&edited, "Studio brush");
    brushes::save_user_library(&dir, &lib).unwrap();
    let back = brushes::load_user_library(&dir);
    assert_eq!(back, lib);
    let p = back.preset(&custom.id).unwrap();
    assert_eq!((p.name.as_str(), p.size, p.spacing), ("Studio brush", 77.0, 0.33));
    assert_eq!(p.category, onepaint_brush::CUSTOM_CATEGORY);
    assert!(back.tips.contains_key(&p.tip_id), "the shared tip is kept");
    // Rename and save over, then reload again.
    lib.rename(&custom.id, "Renamed");
    let mut again = lib.preset(&custom.id).unwrap().clone();
    again.hardness = 0.1;
    lib.update(&again);
    brushes::save_user_library(&dir, &lib).unwrap();
    let back = brushes::load_user_library(&dir);
    assert_eq!(back.preset(&custom.id).unwrap().name, "Renamed");
    assert_eq!(back.preset(&custom.id).unwrap().hardness, 0.1);
    let _ = std::fs::remove_dir_all(&dir);
}
