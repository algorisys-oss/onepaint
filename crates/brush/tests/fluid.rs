#![allow(clippy::unwrap_used)]
//! The live wet simulation (spec 35 B).
use onepaint_brush::fluid::*;
use onepaint_raster::{Rect, Surface};

const DOC: Rect = Rect::new(0, 0, 400, 300);

fn dab(x: f64, y: f64, water: f64, pigment: f64, color: [f32; 3]) -> WetDab {
    WetDab { x, y, radius: 20.0, water, pigment, color, vx: 0.0, vy: 0.0, rewet: 0.0, dry: 0.0, grain_thr: 0.0, halo: 1.0 }
}

fn stroke(sim: &mut FluidSim, y: f64, water: f64, pigment: f64, color: [f32; 3]) {
    let dabs: Vec<_> = (0..40).map(|i| dab(150.0 + i as f64 * 2.5, y, water, pigment, color)).collect();
    sim.deposit(&dabs, None);
}

fn layer_mass(l: &Surface) -> f64 {
    l.read_rect(DOC).chunks_exact(4).map(|p| p[3] as f64).sum()
}

fn steps(sim: &mut FluidSim, layer: &mut Surface, n: usize) {
    for _ in 0..n {
        sim.step(layer, sim.rect(), false);
    }
}

#[test]
fn a_step_moves_exactly_what_settles_into_the_layer() {
    // Damp enough to bloom, too shallow for the slope force to set it flowing
    // (semi-Lagrangian flow isn't conservative: spreading puddles gain mass,
    // as in happypaint). Onto an empty layer, source-over adds alpha exactly.
    let mut sim = FluidSim::new(DOC, Rect::new(140, 130, 140, 40), false);
    stroke(&mut sim, 150.0, 0.09, 0.4, [0.1, 0.2, 0.8]);
    let total = sim.suspended_mass();
    assert!(total > 100.0);
    let mut layer = Surface::new();
    steps(&mut sim, &mut layer, 1);
    let settled = layer_mass(&layer);
    assert!(settled > 1.0, "some has settled");
    let now = sim.suspended_mass() + settled;
    assert!((now - total).abs() / total < 0.005, "deposited {total}, now {now}");
}

#[test]
fn water_blooms_and_evaporates_and_dry_now_settles_everything() {
    let mut sim = FluidSim::new(DOC, Rect::new(140, 130, 140, 40), false);
    stroke(&mut sim, 150.0, 2.0, 0.4, [0.8, 0.1, 0.1]);
    let w0 = sim.peak_water();
    let mut layer = Surface::new();
    let reach = |s: &FluidSim| {
        let mut l = Surface::new();
        s.preview().tiles().for_each(|((tx, ty), t)| l.set_tile(tx, ty, Some(t.clone())));
        (150..300).rev().find(|&y| l.get(200, y)[3] > 0.01).unwrap_or(0)
    };
    let before = reach(&sim);
    steps(&mut sim, &mut layer, 120);
    assert!(reach(&sim) > before + 3, "the wash blooms outward: {before} → {}", reach(&sim));
    assert!(sim.peak_water() < w0, "evaporates");
    let total = sim.suspended_mass() + layer_mass(&layer);
    sim.dry_now(&mut layer, sim.rect());
    assert_eq!(sim.suspended_mass(), 0.0);
    assert!(layer_mass(&layer) >= total * 0.97, "force-dry settles it all");
}

#[test]
fn oils_stay_put_and_never_dry_on_their_own() {
    let mut sim = FluidSim::new(DOC, Rect::new(140, 130, 140, 40), true);
    stroke(&mut sim, 150.0, 2.0, 0.5, [0.9, 0.7, 0.1]);
    let (w0, m0) = (sim.water(), sim.suspended_mass());
    let mut layer = Surface::new();
    steps(&mut sim, &mut layer, 60);
    assert!((sim.water() - w0).abs() / w0 < 0.05, "no evaporation");
    assert_eq!(layer_mass(&layer), 0.0, "nothing bakes while wet");
    assert!((sim.suspended_mass() - m0).abs() / m0 < 0.01);
}

#[test]
fn tilt_runs_wet_paint_downhill() {
    let centroid = |tilt: f32| {
        let mut sim = FluidSim::new(DOC, Rect::new(140, 100, 140, 100), false);
        sim.set_tilt(0.0, tilt);
        stroke(&mut sim, 120.0, 2.5, 0.4, [0.1, 0.5, 0.2]);
        let mut layer = Surface::new();
        steps(&mut sim, &mut layer, 80);
        sim.dry_now(&mut layer, sim.rect());
        let px = layer.read_rect(DOC);
        let (mut m, mut my) = (0.0, 0.0);
        for (i, p) in px.chunks_exact(4).enumerate() {
            m += p[3] as f64;
            my += p[3] as f64 * (i / DOC.w as usize) as f64;
        }
        my / m
    };
    let (flat, down) = (centroid(0.0), centroid(1.0));
    assert!(down > flat + 1.0, "flat {flat}, tilted {down}");
}

#[test]
fn rewet_lifts_dried_paint_back_into_suspension() {
    let mut layer = Surface::new();
    for y in 100..200 {
        for x in 100..300 {
            layer.set(x, y, [0.5, 0.0, 0.0, 0.5]);
        }
    }
    let before = layer_mass(&layer);
    let mut sim = FluidSim::new(DOC, Rect::new(140, 130, 140, 40), false);
    // One dab moves mass exactly (overlapping dabs all lift from the
    // pre-batch layer, as in happypaint).
    let dabs = [WetDab { rewet: 0.9 * 0.8 * 0.08, ..dab(200.0, 150.0, 2.4, 0.0, [0.0; 3]) }];
    sim.deposit(&dabs, Some(&mut layer));
    let lifted = before - layer_mass(&layer);
    assert!(lifted > 1.0, "paint left the layer");
    assert!((sim.suspended_mass() - lifted).abs() < lifted * 0.01, "and went into suspension");
}

#[test]
fn wet_into_wet_mixes_through_pigment() {
    // Yellow into wet blue: KM makes green, not grey.
    let mut sim = FluidSim::new(DOC, Rect::new(140, 130, 140, 40), false);
    stroke(&mut sim, 150.0, 2.0, 0.4, [0.0, 0.05, 0.8]);
    stroke(&mut sim, 150.0, 2.0, 0.4, [0.9, 0.8, 0.0]);
    let mut layer = Surface::new();
    sim.dry_now(&mut layer, sim.rect());
    let p = layer.get(200, 150);
    let (r, g, b) = (p[0] / p[3], p[1] / p[3], p[2] / p[3]);
    assert!(g > r && g > b, "green: {r} {g} {b}");
}

#[test]
fn the_simulation_is_deterministic_and_drags_with_the_brush() {
    let run = || {
        let mut sim = FluidSim::new(DOC, Rect::new(140, 130, 140, 40), false);
        stroke(&mut sim, 150.0, 2.0, 0.4, [0.3, 0.3, 0.9]);
        let push: Vec<_> = (0..20).map(|i| WetDab { vx: 8.0, ..dab(160.0 + i as f64 * 4.0, 150.0, 0.0, 0.0, [0.0; 3]) }).collect();
        sim.deposit(&push, None);
        let mut layer = Surface::new();
        steps(&mut sim, &mut layer, 30);
        (layer.read_rect(DOC), sim.preview().read_rect(sim.rect()))
    };
    assert!(run() == run());
}

/// Spec 58 (happypaint v0.199.2): a halo wets the paper out to
/// `radius · halo`; the pigment stays within the radius.
#[test]
fn a_halo_wets_past_the_pigment() {
    let mut sim = FluidSim::new(DOC, Rect::new(150, 120, 100, 60), false);
    sim.deposit(&[WetDab { halo: 1.5, ..dab(200.0, 150.0, 2.0, 0.6, [0.1, 0.2, 0.8]) }], None);
    // 25 px out: past the 20 px radius, inside the 30 px halo.
    let (w, a) = sim.at(225, 150);
    assert!(w > 0.5 && a == 0.0, "water {w}, pigment {a}");
    let (w, _) = sim.at(233, 150);
    assert_eq!(w, 0.0);
    // No halo: no water past the radius.
    let mut plain = FluidSim::new(DOC, Rect::new(150, 120, 100, 60), false);
    plain.deposit(&[dab(200.0, 150.0, 2.0, 0.6, [0.1, 0.2, 0.8])], None);
    assert_eq!(plain.at(225, 150).0, 0.0);
    // The preset's field: absent = 1, clamped to 1..3.
    let f: Fluid = serde_json::from_str(r#"{"water":1.6,"pigment":0.65}"#).unwrap();
    assert_eq!(f.halo(), 1.0);
    assert_eq!(Fluid { halo: 9.0, ..f }.halo(), 3.0);
    assert_eq!(onepaint_brush::preset("watercolor-live-wet").unwrap().fluid().unwrap().halo(), 1.5);
}

/// Spec 58 (happypaint v0.199.1, paper absorbs): water only moves between
/// wet pixels, so a damp patch doesn't creep into the dry paper around it,
/// and a soaking one spreads into it only slowly.
#[test]
fn damp_paint_stays_put_and_soaking_paint_spreads() {
    // Water 3 px past the dab's 20 px radius after a second.
    let past_edge = |water: f64| {
        let mut sim = FluidSim::new(DOC, Rect::new(150, 120, 100, 60), false);
        sim.deposit(&[dab(200.0, 150.0, water, 0.5, [0.1, 0.2, 0.8])], None);
        let mut layer = Surface::new();
        steps(&mut sim, &mut layer, 60);
        sim.at(223, 150).0
    };
    let (damp, soaking) = (past_edge(0.2), past_edge(2.0));
    assert!(damp < 1e-3, "a damp patch stays inside its dab: {damp}");
    // Dry paper drinks the front: before v0.199 this reached ~1.5.
    assert!(soaking > 0.05 && soaking < 0.5, "a soaking one spreads, slowly: {soaking}");
}
