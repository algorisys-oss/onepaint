//! Full-recomposite cost on a large, busy document: `cargo run --release -p onepaint-engine --example bigdoc`.
#![allow(clippy::expect_used)]
use onepaint_color::Rgba;
use onepaint_doc::{BlendMode, Document, Layer, Node};
use onepaint_engine::Session;
use std::time::Instant;

fn main() {
    let all_normal = std::env::args().any(|a| a == "--normal");
    for (side, layers) in [(4096u32, 20usize), (8192, 10)] {
        let mut doc = Document::new(side, side, Some(Rgba::WHITE));
        for i in 0..layers {
            let mut l = Layer::new(&format!("L{i}"));
            l.pixels = Document::filled(side, side, [0.1, 0.05 * (i % 10) as f32, 0.2, 0.5]);
            l.opacity = 0.8;
            if i % 3 == 1 && !all_normal {
                l.blend_mode = BlendMode::Multiply;
            }
            doc.root.push(Node::Layer(l));
        }
        let mut s = Session::from_document(doc);
        let keys = s.take_dirty_tiles();
        let t = Instant::now();
        let out = s.composite(&keys);
        println!("{side}x{side}, {layers} full layers: {} tiles composited in {:?}", out.len(), t.elapsed());
    }
}
