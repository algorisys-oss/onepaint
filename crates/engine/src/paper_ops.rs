//! The paper's sheet colour on the Background (spec 57; happypaint
//! `fillLayerSolid` and `backgroundLayerId`, `paint-engine.ts:4480-4523`).

use crate::{EngineError, Session};
use onepaint_color::Rgba;
use onepaint_doc::Node;
use onepaint_raster::{Rect, Store, Surface, TILE, Tile};
use std::sync::Arc;

impl Session {
    /// The Background: the bottom layer, when it is a plain paint layer
    /// (not a group, text, path, adjustment or fill layer).
    pub fn background_layer_id(&self) -> Option<String> {
        let Some(Node::Layer(l)) = self.doc.root.first() else { return None };
        (l.adjustment.is_none() && !self.is_text_layer(&l.id) && !self.is_path_layer(&l.id)).then(|| l.id.clone())
    }

    /// Fill a layer with an opaque colour over the whole canvas, as one undo
    /// step: its relief and any pixels past the canvas go too. Locked layers
    /// are left alone.
    pub(crate) fn fill_layer_solid(&mut self, id: &str, rgba: Rgba) -> Result<(), EngineError> {
        self.dry_wet();
        let l = self.doc.layer(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        if l.locked {
            return Ok(());
        }
        let px = self.doc.space().pixel(Rgba { a: 1.0, ..rgba });
        let eight = self.eight_bit();
        let bounds = self.doc.bounds();
        let store = |t: Tile| if eight { t.to_store(Store::Unorm8) } else { t };
        // Whole tiles share one copy.
        let full = Arc::new(store(Tile::filled(px)));
        let mut pixels = Surface::new();
        for (tx, ty) in bounds.tiles() {
            let r = Rect::from_tile(tx, ty);
            if bounds.intersect(&r) == Some(r) {
                pixels.set_tile(tx, ty, Some(full.clone()));
            } else if let Some(part) = bounds.intersect(&r) {
                let mut t = Tile::filled([0.0; 4]);
                for y in part.y..part.bottom() {
                    for x in part.x..part.right() {
                        t.set((x - tx * TILE) as usize, (y - ty * TILE) as usize, px);
                    }
                }
                pixels.set_tile(tx, ty, Some(Arc::new(store(t))));
            }
        }
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.pixels = pixels;
                l.height = None;
            }
            Ok(())
        })
    }
}
