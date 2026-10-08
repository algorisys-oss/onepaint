//! Hot-path benches (AGENTS.md: a >5% regression needs a justification).
#![allow(clippy::unwrap_used)]

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use onepaint_brush::{Dab, rasterize_dab};
use onepaint_color::Rgba;
use onepaint_engine::{Command, Session};
use onepaint_raster::Surface;
use onepaint_stroke::StrokePoint;

fn dabs(c: &mut Criterion) {
    for r in [6.0, 30.0, 120.0] {
        let d = Dab { x: 300.0, y: 300.0, radius: r, alpha: 0.5, rotation: 0.3, aspect: 0.8, hardness: 0.6, color: [0.2, 0.4, 0.8] };
        c.bench_function(&format!("rasterize_dab r{r}"), |b| {
            let mut s = Surface::new();
            b.iter(|| rasterize_dab(&mut s, black_box(&d), false, None))
        });
    }
}

fn composite(c: &mut Criterion) {
    let mut s = Session::new(2048, 1536, Some(Rgba::WHITE));
    for _ in 0..4 {
        s.execute(Command::AddLayer).unwrap();
        s.stroke_begin(StrokePoint::at(10.0, 10.0, 1.0, 0.0));
        s.stroke_extend(&[StrokePoint::at(2000.0, 1500.0, 1.0, 500.0)]);
        s.stroke_end();
    }
    c.bench_function("composite one tile, 6 layers", |b| b.iter(|| s.composite(black_box(&[(3, 2)]))));
    let all: Vec<_> = s.doc().bounds().tiles().collect();
    c.bench_function("composite 2048x1536, 6 layers", |b| b.iter(|| s.composite(black_box(&all))));
    // A 4×2 manga page: every panel's clip is derived per tile (spec 30).
    s.execute(Command::CreatePanelGrid { rows: 4, cols: 2, gutter: 24.0, margin: 40.0, border: 4.0 }).unwrap();
    c.bench_function("composite 2048x1536, 4x2 panel page", |b| b.iter(|| s.composite(black_box(&all))));
}

fn stroke(c: &mut Criterion) {
    c.bench_function("hard-round 24px stroke, 100 events + end", |b| {
        b.iter(|| {
            let mut s = Session::new(1024, 1024, None);
            s.stroke_begin(StrokePoint::at(10.0, 500.0, 0.5, 0.0));
            for i in 1..=100 {
                s.stroke_extend(&[StrokePoint::at(10.0 + i as f64 * 9.0, 500.0, 0.8, i as f64 * 8.0)]);
            }
            s.stroke_end();
        })
    });
}

criterion_group!(benches, dabs, composite, stroke);
criterion_main!(benches);
