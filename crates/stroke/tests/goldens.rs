//! Golden values produced by running happypaint's own TS modules.
use onepaint_stroke::*;

fn pt(x: f64, y: f64, p: f64, t: f64) -> StrokePoint {
    StrokePoint {
        x,
        y,
        pressure: p,
        t: Some(t),
        ..Default::default()
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 2e-6
}

#[test]
fn g1_sampler_fixed_gap() {
    let mut s = Sampler::new();
    let mut gap = |_: &StrokePoint, _: Option<(f64, f64)>| 2.4;
    let a = s.add(pt(0.0, 0.0, 1.0, 0.0), &mut gap);
    assert_eq!(a.len(), 1);
    let b = s.add(pt(10.0, 0.0, 1.0, 10.0), &mut gap);
    let xs: Vec<f64> = b.iter().map(|p| p.x).collect();
    for (x, e) in xs.iter().zip([2.4, 4.8, 7.2, 9.6]) {
        assert!(near(*x, e), "{xs:?}");
    }
    assert_eq!(b.len(), 4);
    assert!(near(b[3].t.unwrap(), 9.6));
    let c = s.add(pt(10.0, 5.0, 0.5, 20.0), &mut gap);
    assert_eq!(c.len(), 2);
    assert!(near(c[0].y, 2.0) && near(c[0].pressure, 0.8) && near(c[0].t.unwrap(), 14.0));
    assert!(near(c[1].y, 4.4) && near(c[1].pressure, 0.56) && near(c[1].t.unwrap(), 18.8));
    // Zero-length segment emits nothing.
    assert!(s.add(pt(10.0, 5.0, 0.5, 21.0), &mut gap).is_empty());
}

#[test]
fn g3_smoother() {
    let mut s = Smoother::new(0.25);
    let out: Vec<_> = [
        pt(0.0, 0.0, 0.2, 0.0),
        pt(10.0, 0.0, 1.0, 8.0),
        pt(20.0, 0.0, 1.0, 16.0),
        pt(20.0, 10.0, 0.5, 16.0),
    ]
    .into_iter()
    .map(|p| s.next(p))
    .collect();
    let exp = [
        (0.0, 0.0, 0.2),
        (4.133538, 0.0, 0.861366),
        (10.692, 0.0, 0.975976),
        (12.870739, 2.340717, 0.753151),
    ];
    for (o, e) in out.iter().zip(exp) {
        assert!(
            near(o.x, e.0) && near(o.y, e.1) && near(o.pressure, e.2),
            "{o:?} vs {e:?}"
        );
    }
}

#[test]
fn g4_stabilizer() {
    let mut s = Stabilizer::new(0.5);
    let a = s.next(pt(0.0, 0.0, 0.5, 0.0));
    assert_eq!(a.map(|p| (p.x, p.y, p.pressure)), Some((0.0, 0.0, 0.5)));
    assert!(s.next(pt(30.0, 0.0, 0.6, 1.0)).is_none());
    let c = s.next(pt(100.0, 0.0, 0.7, 2.0)).unwrap();
    assert!(near(c.x, 40.0) && near(c.y, 0.0) && near(c.pressure, 0.7));
    let d = s.next(pt(100.0, 50.0, 0.8, 3.0)).unwrap();
    assert!(
        near(d.x, 53.906723) && near(d.y, 11.588936) && near(d.pressure, 0.8),
        "{d:?}"
    );
    let f = s.finish().unwrap();
    assert_eq!((f.x, f.y, f.pressure), (100.0, 50.0, 0.8));
    assert!(s.finish().is_none());
}

#[test]
fn g7_curves() {
    let c = [
        CurvePoint::new(0.0, 0.0),
        CurvePoint::new(0.5, 0.8),
        CurvePoint::new(1.0, 1.0),
    ];
    let xs = [0.0, 0.25, 0.5, 0.75, 1.0];
    let lin = [0.0, 0.4, 0.8, 0.9, 1.0];
    let smooth = [0.0, 0.4375, 0.8, 0.9375, 1.0];
    for i in 0..5 {
        assert!(near(eval_curve(&c, xs[i], Interp::Linear), lin[i]));
        assert!(
            near(eval_curve(&c, xs[i], Interp::Smooth), smooth[i]),
            "{}",
            eval_curve(&c, xs[i], Interp::Smooth)
        );
    }
    assert_eq!(eval_curve(&[], 0.3, Interp::Linear), 0.3);
    let b = Binding {
        sensor: Sensor::Pressure,
        curve: vec![CurvePoint::new(0.0, 0.5), CurvePoint::new(1.0, 1.0)],
        ..Default::default()
    };
    let st = SensorState {
        pressure: 0.3,
        ..Default::default()
    };
    assert!(near(
        eval_bindings(std::slice::from_ref(&b), &st, Combine::Multiply),
        0.65
    ));
    let p = Binding {
        sensor: Sensor::Pressure,
        curve: vec![],
        ..Default::default()
    };
    let r = Binding {
        sensor: Sensor::Random,
        curve: vec![],
        min: Some(0.7),
        max: Some(1.0),
        ..Default::default()
    };
    let st = SensorState {
        pressure: 0.5,
        random: 0.5,
        ..Default::default()
    };
    assert!(near(eval_bindings(&[p, r], &st, Combine::Multiply), 0.425));
}

#[test]
fn g8_mulberry32() {
    let mut r = Mulberry32::new(12345);
    for e in [0.979728, 0.306752, 0.484205, 0.817934] {
        assert!(near(r.next_f64(), e));
    }
    let mut r = Mulberry32::new(1);
    assert_eq!(r.next_f64(), 0.6270739405881613);
    assert_eq!(r.next_f64(), 0.002735721180215478);
    assert_eq!(r.next_f64(), 0.5274470399599522);
    let mut r = Mulberry32::new(42);
    assert!(near(gaussian(&mut r), -0.956162));
}
