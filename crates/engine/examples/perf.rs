//! Rough timings of the interactive hot paths: `cargo run -p onepaint-engine --example perf [--release]`.
#![allow(clippy::expect_used)]
use onepaint_color::Rgba;
use onepaint_engine::{Command, Session};
use onepaint_stroke::StrokePoint;
use std::time::Instant;

fn main() {
    let mut s = Session::new(2048, 1536, Some(Rgba::WHITE));
    let t = Instant::now();
    let keys = s.take_dirty_tiles();
    let tiles = s.composite(&keys);
    println!("full composite {} tiles: {:?}", tiles.len(), t.elapsed());
    for (preset, size) in [("hard-round", 24.0), ("soft-round", 60.0), ("soft-airbrush", 200.0)] {
        s.execute(Command::SetBrush { preset_id: preset.into() }).expect("preset");
        s.execute(Command::SetBrushSize { size }).expect("size");
        let t = Instant::now();
        let mut frame_max = std::time::Duration::ZERO;
        s.stroke_begin(StrokePoint::at(100.0, 700.0, 0.5, 0.0));
        for i in 1..=240 {
            let f = Instant::now();
            s.stroke_extend(&[StrokePoint::at(100.0 + i as f64 * 7.0, 700.0 + (i as f64 / 10.0).sin() * 100.0, 0.8, i as f64 * 8.0)]);
            let k = s.take_dirty_tiles();
            let _ = s.composite(&k);
            frame_max = frame_max.max(f.elapsed());
        }
        let e = Instant::now();
        s.stroke_end();
        let end = e.elapsed();
        println!("{preset} {size}px: 240 events {:?} (worst event+composite {:?}), stroke_end {:?}", t.elapsed(), frame_max, end);
    }
}
