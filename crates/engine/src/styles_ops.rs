//! Layer style operations (spec 56): bake a layer's styles into its pixels
//! (happypaint `bakeLayerStyles`) and apply Global Light across the document
//! (`applyGlobalLight`). Setting styles is a layer property (`LayerProps::styles`).

use crate::{EngineError, Session};
use onepaint_doc::Node;
use onepaint_doc::styles::LayerStyles;
use onepaint_raster::{Rect, TILE};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The tiles `keys` touch, grown by `reach` px, inside the document.
pub(crate) fn grown_keys(keys: impl IntoIterator<Item = (i32, i32)>, reach: i32, doc: Rect) -> Vec<(i32, i32)> {
    let k = (reach + TILE - 1) / TILE;
    let mut out = BTreeSet::new();
    for (x, y) in keys {
        for dy in -k..=k {
            for dx in -k..=k {
                let t = (x + dx, y + dy);
                if Rect::from_tile(t.0, t.1).intersect(&doc).is_some() {
                    out.insert(t);
                }
            }
        }
    }
    out.into_iter().collect()
}

impl Session {
    /// The styled result becomes the layer's pixels and the styles go, as
    /// Photoshop's Rasterize ▸ Layer Style; the effects draw Normal (a bake
    /// has nothing under it) from the unmasked pixels, and the mask stays.
    /// A group has no pixels to bake into. One undo step.
    pub(crate) fn bake_styles(&mut self, id: &str) -> Result<(), EngineError> {
        let l = self.doc.layer(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let Some(styles) = l.styles() else { return Ok(()) };
        if !styles.is_active() {
            // Nothing visible to bake: just drop the recipe.
            return self.tree_edit(None, |s| {
                if let Some(l) = s.doc.layer_mut(id) {
                    l.set_styles(None);
                }
                Ok(())
            });
        }
        let frame = onepaint_compose::doc_frame(&self.doc);
        let pixels = l.pixels.clone();
        let content = |x: i32, y: i32| pixels.tile(x, y).map(|t| t.to_f32());
        let keys = grown_keys(pixels.tile_keys(), styles.reach() + 1, self.doc.bounds());
        let baked: Vec<_> = keys
            .iter()
            .map(|&(x, y)| ((x, y), onepaint_compose::styles::styled_tile(&styles, (x, y), frame, &content, None, None)))
            .collect();
        let eight = self.eight_bit();
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                for ((x, y), buf) in baked {
                    let t = onepaint_raster::Tile::from_f32(&buf);
                    let t = if eight { t.to_store(onepaint_raster::Store::Unorm8) } else { t };
                    l.pixels.set_tile(x, y, (!t.is_empty()).then(|| Arc::new(t)));
                }
                l.set_styles(None);
            }
            Ok(())
        })
    }

    /// Every linked shadow and bevel in the document takes Global Light
    /// `(angle, altitude)` (degrees). One undo step, a drag's edits too
    /// (until history is sealed).
    pub(crate) fn apply_global_light(&mut self, angle: f32, altitude: f32) -> Result<(), EngineError> {
        fn walk(nodes: &mut [Node], light: (f32, f32)) {
            for n in nodes {
                let lit = |s: Option<LayerStyles>| s.and_then(|s| s.with_global_light(light));
                match n {
                    Node::Layer(l) => {
                        if let Some(s) = lit(l.styles()) {
                            l.set_styles(Some(s));
                        }
                    }
                    Node::Group(g) => {
                        if let Some(s) = lit(g.styles()) {
                            g.set_styles(Some(s));
                        }
                        walk(&mut g.children, light);
                    }
                }
            }
        }
        self.tree_edit(Some("global-light".into()), |s| {
            walk(&mut s.doc.root, (angle, altitude));
            Ok(())
        })
    }
}
