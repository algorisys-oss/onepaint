//! Liquify strokes (spec 17): the layer warps live from a begin snapshot,
//! which is also the reconstruct target; one Tiles change at end.

use super::*;
use onepaint_filters::liquify::{self, LiquifyMode};

pub(crate) struct ActiveLiquify {
    layer: NodeId,
    before: Surface,
    last: (f64, f64),
    radius: f64,
    strength: f64,
    mode: LiquifyMode,
    dirty: Rect,
}

impl Session {
    pub(crate) fn liquify_begin(&mut self, p: StrokePoint) {
        let Some(layer) = self.doc.layer(&self.active) else { return };
        if layer.locked || !layer.visible || layer.adjustment.is_some() {
            return;
        }
        self.liquify = Some(ActiveLiquify {
            layer: self.active.clone(),
            before: layer.pixels.clone(),
            last: (p.x, p.y),
            radius: (self.brush.size / 2.0).max(2.0),
            strength: self.paint.liquify_strength.clamp(0.0, 1.0),
            mode: self.paint.liquify_mode,
            dirty: Rect::default(),
        });
    }

    pub(crate) fn liquify_extend(&mut self, pts: &[StrokePoint]) {
        let Some(mut lq) = self.liquify.take() else { return };
        let doc = self.doc.bounds();
        if let Some(l) = self.doc.layer_mut(&lq.layer) {
            let mut r = Rect::default();
            for p in pts {
                for (c, prev) in liquify::dab_centres(lq.last, (p.x, p.y), lq.radius) {
                    r = r.union(&liquify::dab(&mut l.pixels, &lq.before, doc, lq.mode, c, prev, lq.radius, lq.strength));
                }
                lq.last = (p.x, p.y);
            }
            lq.dirty = lq.dirty.union(&r);
            self.mark_dirty(r);
        }
        self.liquify = Some(lq);
    }

    pub(crate) fn liquify_end(&mut self) {
        let Some(lq) = self.liquify.take() else { return };
        let Some(l) = self.doc.layer(&lq.layer) else { return };
        let mut changes: Vec<TileChange> = Vec::new();
        let mut bytes = 0;
        for (tx, ty) in lq.dirty.tiles() {
            let before = lq.before.tile(tx, ty).cloned();
            let after = l.pixels.tile(tx, ty).cloned();
            let same = match (&before, &after) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if !same {
                bytes += tile_bytes(&before) + tile_bytes(&after);
                changes.push(((tx, ty), before, after));
            }
        }
        if !changes.is_empty() {
            self.history.push(Change::Tiles { layer: lq.layer, plane: Plane::Pixels, tiles: changes }, bytes);
            self.doc.touch();
            self.history_changed();
        }
    }

    pub(crate) fn liquify_cancel(&mut self) {
        let Some(lq) = self.liquify.take() else { return };
        if let Some(l) = self.doc.layer_mut(&lq.layer) {
            l.pixels = lq.before;
        }
        self.mark_dirty(lq.dirty);
    }
}
