//! Text layers (spec 14): specs live in the layer's `texts` (verbatim JSON);
//! the pixels are always their rendering until the layer is rasterised.

use super::*;
use onepaint_text::{Renderer, TextSpec, quad_contains};

fn read_texts(l: &Layer) -> Vec<TextSpec> {
    if let Some(v) = l.extra.get("texts") {
        return serde_json::from_value(v.clone()).unwrap_or_default();
    }
    // v3 files stored a single `text` (lifted on read, like happypaint).
    l.extra.get("text").and_then(|v| serde_json::from_value::<TextSpec>(v.clone()).ok()).into_iter().collect()
}

pub(crate) fn write_texts(l: &mut Layer, specs: &[TextSpec], w: u32, h: u32, space: onepaint_color::Space) {
    l.extra.remove("text");
    if specs.is_empty() {
        l.extra.remove("texts");
    } else {
        l.extra.insert("texts".into(), serde_json::to_value(specs).unwrap_or_default());
    }
    l.pixels = Renderer::in_space(space).render(specs, w, h);
}

impl Session {
    /// The text objects of a layer (empty for non-text layers).
    pub fn texts(&self, id: &str) -> Vec<TextSpec> {
        self.doc.layer(id).map(read_texts).unwrap_or_default()
    }

    pub fn is_text_layer(&self, id: &str) -> bool {
        !self.texts(id).is_empty()
    }

    /// Topmost visible text object at doc (x, y): `(layer id, index)`.
    pub fn text_at(&self, x: f64, y: f64) -> Option<(NodeId, usize)> {
        let r = Renderer::new();
        for id in self.doc.layer_ids().into_iter().rev() {
            let Some(l) = self.doc.layer(&id).filter(|l| l.visible) else { continue };
            let specs = read_texts(l);
            for (i, s) in specs.iter().enumerate().rev() {
                if r.quad(s).is_some_and(|q| quad_contains(&q, [x, y], 6.0)) {
                    return Some((id, i));
                }
            }
        }
        None
    }

    pub(crate) fn add_text(&mut self, spec: TextSpec) -> Result<(), EngineError> {
        let (w, h, space) = (self.doc.width, self.doc.height, self.doc.space());
        let active = self.active.clone();
        if self.is_text_layer(&active) {
            return self.tree_edit(None, |s| {
                if let Some(l) = s.doc.layer_mut(&active) {
                    let mut specs = read_texts(l);
                    specs.push(spec);
                    write_texts(l, &specs, w, h, space);
                }
                Ok(())
            });
        }
        let mut l = Layer::new("Text");
        write_texts(&mut l, &[spec], w, h, space);
        self.insert_above_active(Node::Layer(l))
    }

    pub(crate) fn update_text(&mut self, id: &str, index: usize, spec: Option<TextSpec>) -> Result<(), EngineError> {
        let (w, h, space) = (self.doc.width, self.doc.height, self.doc.space());
        if self.texts(id).len() <= index {
            return Err(EngineError::NoSuchLayer(format!("{id} text {index}")));
        }
        // Live edits of one text object (typing, dragging) coalesce into one undo step.
        let key = spec.is_some().then(|| format!("text:{id}:{index}"));
        self.tree_edit(key, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                let mut specs = read_texts(l);
                match spec {
                    Some(sp) => specs[index] = sp,
                    None => {
                        specs.remove(index);
                    }
                }
                write_texts(l, &specs, w, h, space);
            }
            Ok(())
        })
    }

    pub(crate) fn rasterize_text(&mut self, id: &str) -> Result<(), EngineError> {
        if !self.is_text_layer(id) {
            return Ok(());
        }
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.extra.remove("texts");
                l.extra.remove("text");
            }
            Ok(())
        })
    }

    /// Pixel operations turn a text layer into a plain one first (PE:4620-4655).
    pub(crate) fn ensure_raster(&mut self) {
        let id = self.active.clone();
        if self.is_text_layer(&id) {
            let _ = self.rasterize_text(&id);
        }
        if self.is_path_layer(&id) {
            let _ = self.rasterize_path(&id);
        }
    }
}
