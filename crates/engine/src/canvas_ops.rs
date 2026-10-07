//! Canvas size and image size (happypaint PE:5588-5702, 5826-5962).

use super::*;

/// Everything a canvas-level change replaces.
#[derive(Clone)]
pub(crate) struct DocShape {
    pub width: u32,
    pub height: u32,
    pub root: Vec<Node>,
    pub active: NodeId,
}

fn map_layers(nodes: &mut [Node], f: &mut impl FnMut(&mut Layer)) {
    for n in nodes {
        match n {
            Node::Layer(l) => f(l),
            Node::Group(g) => map_layers(&mut g.children, f),
        }
    }
}

impl Session {
    fn shape(&self) -> DocShape {
        DocShape {
            width: self.doc.width,
            height: self.doc.height,
            root: self.doc.root.clone(),
            active: self.active.clone(),
        }
    }

    pub(crate) fn apply_shape(&mut self, s: DocShape) {
        self.doc.width = s.width;
        self.doc.height = s.height;
        self.doc.root = s.root;
        self.active = s.active;
        self.set_selection_raw(None);
        self.events.push(Event::DocumentReplaced);
        self.events.push(Event::LayersChanged);
        self.mark_all_dirty();
    }

    fn record_shape(&mut self, before: DocShape) {
        let after = self.shape();
        self.history.push(
            Change::Document {
                before: Box::new(before),
                after: Box::new(after.clone()),
            },
            4096,
        );
        self.apply_shape(after);
        self.doc.touch();
        self.history_changed();
    }

    /// Crop or extend at 1:1; content moves by (dx, dy). New mask area is white.
    pub(crate) fn resize_canvas(
        &mut self,
        width: u32,
        height: u32,
        dx: i32,
        dy: i32,
    ) -> Result<(), EngineError> {
        let (width, height) = (width.clamp(1, 16384), height.clamp(1, 16384));
        let before = self.shape();
        let old = Rect::new(dx, dy, self.doc.width as i32, self.doc.height as i32);
        let new = Rect::new(0, 0, width as i32, height as i32);
        map_layers(&mut self.doc.root, &mut |l| {
            l.pixels = l.pixels.translated(dx, dy);
            if let Some(m) = &l.mask {
                let mut m2 = m.translated(dx, dy);
                for y in 0..new.h {
                    for x in 0..new.w {
                        if !(x >= old.x && x < old.right() && y >= old.y && y < old.bottom()) {
                            m2.set(x, y, [1.0, 1.0, 1.0, 1.0]);
                        }
                    }
                }
                l.mask = Some(m2);
            }
        });
        self.doc.width = width;
        self.doc.height = height;
        self.record_shape(before);
        Ok(())
    }

    /// Resample every layer and mask (bilinear, clamp-to-edge) to a new size.
    pub(crate) fn scale_image(&mut self, width: u32, height: u32) -> Result<(), EngineError> {
        let (width, height) = (width.clamp(1, 16384), height.clamp(1, 16384));
        let before = self.shape();
        let (sx, sy) = (
            width as f64 / self.doc.width as f64,
            height as f64 / self.doc.height as f64,
        );
        let src_bounds = self.doc.bounds();
        let dst = Rect::new(0, 0, width as i32, height as i32);
        let resample = |s: &Surface| -> Surface {
            let mut f = transform::Floating {
                layer: String::new(),
                original: Surface::new(),
                src: s.read_rect(src_bounds),
                bounds: src_bounds,
                matrix: transform::Affine::scale(sx, sy),
                preview: Surface::new(),
                preview_bounds: src_bounds,
                saved_selection: None,
                clamp_edges: true,
            };
            f.render(dst);
            f.preview
        };
        map_layers(&mut self.doc.root, &mut |l| {
            l.pixels = resample(&l.pixels);
            if let Some(m) = &l.mask {
                l.mask = Some(resample(m));
            }
        });
        self.doc.width = width;
        self.doc.height = height;
        self.record_shape(before);
        Ok(())
    }
}
