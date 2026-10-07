use onepaint_input::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 2e-6
}

#[test]
fn g5_velocity_synth_for_mouse() {
    let mut p = PressureProvider::new(PointerKind::Mouse);
    let got: Vec<f64> = [
        (0.0, 0.0),
        (5.0, 10.0),
        (25.0, 20.0),
        (125.0, 30.0),
        (125.0, 40.0),
        (126.0, 60.0),
    ]
    .iter()
    .map(|&(x, t)| p.pressure(x, 0.0, t, None))
    .collect();
    for (g, e) in got
        .iter()
        .zip([0.7, 0.7525, 0.724375, 0.605781, 0.704336, 0.776002])
    {
        assert!(near(*g, e), "{got:?}");
    }
}

#[test]
fn g6_hardware_pen() {
    let mut p = PressureProvider::new(PointerKind::Pen);
    let got: Vec<f64> = [0.01, 0.08, 0.3, 1.0]
        .iter()
        .enumerate()
        .map(|(i, &r)| p.pressure(i as f64, 0.0, i as f64 * 10.0, Some(r)))
        .collect();
    for (g, e) in got.iter().zip([0.064673, 0.170672, 0.430512, 1.0]) {
        assert!(near(*g, e), "{got:?}");
    }
}

#[test]
fn g6_flat_pen_upgrades_to_hardware() {
    let mut p = PressureProvider::new(PointerKind::Pen);
    let got: Vec<f64> = [
        (0.0, 0.0, 0.5),
        (5.0, 10.0, 0.5),
        (10.0, 20.0, 0.51),
        (15.0, 30.0, 0.3),
    ]
    .iter()
    .map(|&(x, t, r)| p.pressure(x, 0.0, t, Some(r)))
    .collect();
    for (g, e) in got.iter().zip([0.7, 0.7525, 0.791875, 0.430512]) {
        assert!(near(*g, e), "{got:?}");
    }
}
