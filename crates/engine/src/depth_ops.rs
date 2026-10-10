//! 8-bit documents keep their layers as 8-bit tiles (spec 49): every history
//! entry is quantized as it is recorded, so the live document and the entry
//! share the same 8-bit tiles. Work in progress (a stroke, a filter preview)
//! stays `F16` until it lands.

use crate::{Change, Plane, Session};
use onepaint_doc::{Depth, Node};
use onepaint_raster::{Store, Surface, Tile};
use std::collections::HashMap;
use std::sync::Arc;

/// Tiles already converted, by the address of the tile they replace, so a
/// tile shared by the document and a history entry stays shared.
type Converted = HashMap<usize, Arc<Tile>>;

fn quantize_surface(s: &mut Surface, store: Store, done: &mut Converted) {
    for (tx, ty) in s.tile_keys() {
        let Some(t) = s.tile(tx, ty) else { continue };
        if t.store() == store {
            continue;
        }
        let q = done.entry(Arc::as_ptr(t) as usize).or_insert_with(|| Arc::new(t.to_store(store))).clone();
        s.set_tile(tx, ty, Some(q));
    }
}

/// Every layer's pixels to sRGB bytes and masks to raw bytes; heights stay `F16`.
pub(crate) fn quantize_nodes(nodes: &mut [Node], done: &mut Converted) {
    for n in nodes {
        match n {
            Node::Group(g) => quantize_nodes(&mut g.children, done),
            Node::Layer(l) => {
                quantize_surface(&mut l.pixels, Store::Unorm8, done);
                if let Some(m) = &mut l.mask {
                    quantize_surface(m, Store::Unorm8, done);
                }
            }
        }
    }
}

impl Session {
    pub(crate) fn eight_bit(&self) -> bool {
        self.doc.depth == Depth::Rgba8
    }

    /// Push a history entry; in an 8-bit document its new tiles become 8-bit
    /// first, in the entry and in the document alike.
    pub(crate) fn record(&mut self, mut change: Change, bytes: usize) {
        if self.eight_bit() {
            self.quantize_change(&mut change);
        }
        self.history.push(change, bytes);
    }

    fn quantize_change(&mut self, change: &mut Change) {
        let mut done = Converted::new();
        match change {
            Change::Tiles { layer, plane, tiles } => {
                let store = match plane {
                    Plane::Pixels => Store::Unorm8,
                    Plane::Mask => Store::Unorm8,
                    Plane::Height => return,
                };
                let Some(l) = self.doc.layer_mut(layer) else { return };
                let mut surface = match plane {
                    Plane::Mask => l.mask.as_mut(),
                    _ => Some(&mut l.pixels),
                };
                for ((tx, ty), _, after) in tiles.iter_mut() {
                    let Some(t) = after.as_mut().filter(|t| t.store() != store) else { continue };
                    let old = t.clone();
                    let q = done.entry(Arc::as_ptr(&old) as usize).or_insert_with(|| Arc::new(old.to_store(store))).clone();
                    *t = q.clone();
                    // The document holds the tile this entry recorded: swap in the 8-bit one.
                    if let Some(s) = surface.as_deref_mut().filter(|s| s.tile(*tx, *ty).is_some_and(|cur| Arc::ptr_eq(cur, &old))) {
                        s.set_tile(*tx, *ty, Some(q));
                    }
                }
            }
            Change::Tree { after, .. } => {
                quantize_nodes(&mut self.doc.root, &mut done);
                quantize_nodes(after, &mut done);
            }
            Change::Document { after, .. } => {
                quantize_nodes(&mut self.doc.root, &mut done);
                quantize_nodes(&mut after.root, &mut done);
            }
            _ => {}
        }
    }

    /// Bring a whole document to its storage: 8-bit tiles in 8-bit documents.
    pub(crate) fn quantize_document(&mut self) {
        if self.eight_bit() {
            quantize_nodes(&mut self.doc.root, &mut Converted::new());
        }
    }
}
