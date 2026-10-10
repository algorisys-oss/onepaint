//! `.happypaint` read time on a busy 4096² document (spec 41):
//! `cargo run --release -p onepaint-formats --example read_timing`, and with
//! `RAYON_NUM_THREADS=1` for the serial baseline. A path argument also
//! saves the file there.
#![allow(clippy::unwrap_used)]
use onepaint_doc::{Document, Layer, Node};
use onepaint_formats::{Progress, read_happypaint_with, write_happypaint};
use std::time::Instant;

fn main() {
    let side = 4096u32;
    let mut doc = Document::new(side, side, None);
    for i in 0..8 {
        let mut l = Layer::new(&format!("L{i}"));
        // Smooth gradients with some texture, so the PNGs aren't trivially small.
        for y in 0..side as i32 {
            for x in 0..side as i32 {
                let n = ((x * 7 + y * 13 + i * 31) % 17) as f32 / 170.0;
                let a = 0.5 + 0.4 * ((x + y + i * 100) as f32 * 0.003).sin().abs();
                l.pixels.set(x, y, [(x as f32 / side as f32 + n) * a * 0.5, (y as f32 / side as f32) * a * 0.5, n * a, a]);
            }
        }
        doc.root.push(Node::Layer(l));
    }
    let bytes = write_happypaint(&doc, None, "timing").unwrap();
    println!("{side}² × 8 layers: {:.1} MB file", bytes.len() as f64 / 1e6);
    if let Some(out) = std::env::args().nth(1) {
        std::fs::write(out, &bytes).unwrap();
    }
    let mut times: Vec<f64> = (0..5)
        .map(|_| {
            let t = Instant::now();
            read_happypaint_with(&bytes, &Progress::default()).unwrap();
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(f64::total_cmp);
    println!("read: median {:.0} ms, min {:.0} ms ({} threads)", times[2], times[0], rayon::current_num_threads());
}
