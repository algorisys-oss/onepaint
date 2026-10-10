//! ColorDrop: drag a colour onto the canvas to bucket-fill, with a live
//! preview and an adjustable threshold (happypaint `beginColorDrop`,
//! `updateColorDrop`, `commitColorDrop`, `cancelColorDrop`, PE:1760-1825).
//!
//! The preview is written into the layer without history; the commit
//! records one undo step against the snapshot taken when the drop began.

use super::*;
use crate::selection_ops::paint_rect;

pub(crate) struct ColorDrop {
    layer: NodeId,
    /// The layer before the drop (copy-on-write, so cheap).
    original: Surface,
    /// The rect the current preview touched.
    last: Option<Rect>,
    color: Rgba,
    /// The fill the preview shows: seed pixel, threshold, grow.
    shown: Option<((i32, i32), f32, u32)>,
    /// Fills computed this session (a hold costs none).
    fills: u32,
}

impl Session {
    /// Start a ColorDrop on the active layer at doc (x, y) with `color`.
    /// Returns false (no preview) for a hidden, locked, text, path,
    /// adjustment or group layer, or while something else is in progress.
    pub fn begin_color_drop(&mut self, x: f64, y: f64, color: Rgba, tolerance: f32, grow: u32) -> bool {
        if self.color_drop.is_some() || self.stroke.is_some() || self.transform.is_some() || self.filter.is_some() {
            return false;
        }
        let id = self.active.clone();
        if self.is_text_layer(&id) || self.is_path_layer(&id) {
            return false;
        }
        let Some(l) = self.doc.layer(&id).filter(|l| l.visible && !l.locked && l.adjustment.is_none()) else {
            return false;
        };
        self.color_drop = Some(ColorDrop { layer: id, original: l.pixels.clone(), last: None, color, shown: None, fills: 0 });
        self.update_color_drop(x, y, tolerance, grow);
        true
    }

    /// Move the seed or change the threshold: refill from the snapshot.
    /// Holding still (same seed pixel, threshold and grow) costs nothing.
    pub fn update_color_drop(&mut self, x: f64, y: f64, tolerance: f32, grow: u32) {
        let (tolerance, grow) = (tolerance.clamp(0.0, 1.0), grow.min(8));
        let key = ((x.floor() as i32, y.floor() as i32), tolerance, grow);
        let Some(cd) = self.color_drop.as_mut() else {
            return;
        };
        if cd.shown == Some(key) {
            return;
        }
        cd.shown = Some(key);
        cd.fills += 1;
        let (id, color, last) = (cd.layer.clone(), cd.color, cd.last.take());
        if let Some(r) = last {
            self.restore_color_drop_rect(r);
        }
        let saved = std::mem::replace(&mut self.color, color);
        let fill = self.bucket(&id, key.0.0, key.0.1, tolerance, grow);
        self.color = saved;
        let Some(b) = fill else {
            return;
        };
        let Some(rect) = b.rect.intersect(&self.doc.bounds()) else {
            return;
        };
        if let Some(l) = self.doc.layer_mut(&id) {
            paint_rect(&mut l.pixels, rect, |x, y, p| b.paint(x, y, p));
        }
        if let Some(cd) = self.color_drop.as_mut() {
            cd.last = Some(rect);
        }
        self.mark_dirty(rect);
    }

    /// Keep the previewed fill as one undo step.
    pub fn commit_color_drop(&mut self) {
        let Some(cd) = self.color_drop.take() else {
            return;
        };
        let Some(rect) = cd.last else {
            return;
        };
        let Some(l) = self.doc.layer(&cd.layer) else {
            return;
        };
        let mut changes: Vec<TileChange> = Vec::new();
        for (tx, ty) in rect.tiles() {
            let before = cd.original.tile(tx, ty).cloned();
            let after = l.pixels.tile(tx, ty).cloned();
            if before.as_deref() != after.as_deref() {
                changes.push(((tx, ty), before, after));
            }
        }
        if changes.is_empty() {
            return;
        }
        let bytes = changes.iter().map(|(_, b, a)| tile_bytes(b) + tile_bytes(a)).sum();
        self.history.push(Change::Tiles { layer: cd.layer, plane: Plane::Pixels, tiles: changes }, bytes);
        self.doc.touch();
        self.history_changed();
    }

    /// Drop the preview and put the layer back.
    pub fn cancel_color_drop(&mut self) {
        if let Some(r) = self.color_drop.as_ref().and_then(|cd| cd.last) {
            self.restore_color_drop_rect(r);
        }
        self.color_drop = None;
    }

    /// Fills the current drop has computed (for tests and profiling).
    pub fn color_drop_fills(&self) -> u32 {
        self.color_drop.as_ref().map_or(0, |cd| cd.fills)
    }

    pub fn color_drop_active(&self) -> bool {
        self.color_drop.is_some()
    }

    fn restore_color_drop_rect(&mut self, r: Rect) {
        let Some(cd) = self.color_drop.as_ref() else {
            return;
        };
        let tiles: Vec<_> = r.tiles().map(|(tx, ty)| ((tx, ty), cd.original.tile(tx, ty).cloned())).collect();
        if let Some(l) = self.doc.layer_mut(&cd.layer) {
            for ((tx, ty), t) in tiles {
                l.pixels.set_tile(tx, ty, t);
            }
        }
        self.mark_dirty(r);
    }
}
