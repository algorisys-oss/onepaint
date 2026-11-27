//! `.happypaint` save and open (AGENTS.md: hot paths get benches).
//!
//! A 2048² 8-bit drawing, four painted layers (256 tiles of varied, partly
//! transparent paint): a full write, an autosave-style write with one tile
//! changed since the last, and a read.
#![allow(clippy::unwrap_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use onepaint_doc::{Depth, Document, Layer, Node};
use onepaint_formats::{ChunkCache, read_happypaint, write_happypaint, write_happypaint_cached};
use onepaint_raster::Surface;
use std::hint::black_box;

const SIDE: i32 = 2048;

/// Soft strokes over noise: not flat (compresses like paint), not random.
fn painted(seed: u32) -> Surface {
    let mut s = Surface::new();
    let mut h = seed.wrapping_mul(2_654_435_761);
    for y in 0..SIDE {
        for x in 0..SIDE {
            h ^= h << 13;
            h ^= h >> 17;
            h ^= h << 5;
            let n = (h & 0xff) as f32 / 255.0 * 0.06;
            let wave = ((x as f32 * 0.011 + seed as f32).sin() * (y as f32 * 0.007).cos() + 1.0) * 0.5;
            let a = (wave * 1.2 - 0.1).clamp(0.0, 1.0);
            if a > 0.0 {
                let c = [(0.8 * wave + n) * a, (0.4 + n) * a, (0.2 + 0.5 * (1.0 - wave)) * a, a];
                s.set(x, y, c);
            }
        }
    }
    s
}

fn doc() -> Document {
    let mut d = Document::new_in(SIDE as u32, SIDE as u32, None, Depth::Rgba8);
    d.root.clear();
    for i in 0..4 {
        let mut l = Layer::new(&format!("Layer {i}"));
        l.pixels = painted(i + 1);
        d.root.push(Node::Layer(l));
    }
    d
}

fn file_io(c: &mut Criterion) {
    let d = doc();
    let mut g = c.benchmark_group("file_io");
    g.sample_size(10);
    g.bench_function("write_full_2048_4_layers", |b| b.iter(|| black_box(write_happypaint(&d, None, "bench").unwrap())));

    // Autosave: the last write's chunks are cached; one tile has changed.
    g.bench_function("write_cached_one_tile_changed", |b| {
        let mut cache = ChunkCache::default();
        write_happypaint_cached(&d, None, "bench", &mut cache).unwrap();
        let mut edited = d.clone();
        let mut flip = 0.0f32;
        b.iter(|| {
            flip = 1.0 - flip;
            if let Node::Layer(l) = &mut edited.root[0] {
                l.pixels.set(10, 10, [flip, 0.0, 0.0, 1.0]);
            }
            black_box(write_happypaint_cached(&edited, None, "bench", &mut cache).unwrap())
        })
    });

    let bytes = write_happypaint(&d, None, "bench").unwrap();
    g.bench_function("read_2048_4_layers", |b| b.iter(|| black_box(read_happypaint(&bytes).unwrap())));
    g.finish();
}

criterion_group!(benches, file_io);
criterion_main!(benches);
