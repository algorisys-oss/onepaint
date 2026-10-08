//! Impasto height fields (spec 35 C): strokes stamp, smooth and carve a
//! layer's relief live; the change lands in the stroke's own undo step.
//! Bevel derives a field from the layer's alpha.

use super::*;
use onepaint_brush::height::{self, BevelOptions, HeightDab};

/// A stroke's hold on its layer's height field.
pub(crate) struct HeightEdit {
    layer: NodeId,
    /// The field at stroke start (empty = flat).
    before: Surface,
}

impl Session {
    /// Snapshot `layer`'s field for a stroke that may change it.
    pub(crate) fn height_begin(&self, layer: &str) -> Option<HeightEdit> {
        let l = self.doc.layer(layer)?;
        Some(HeightEdit { layer: layer.to_string(), before: l.height.clone().unwrap_or_default() })
    }

    /// Add relief under `dabs` (creating the field) and redraw it.
    pub(crate) fn height_stamp(&mut self, layer: &str, dabs: &[HeightDab]) {
        if dabs.is_empty() {
            return;
        }
        let doc = self.doc.bounds();
        let Some(l) = self.doc.layer_mut(layer) else { return };
        let r = height::stamp(l.height.get_or_insert_default(), dabs, doc);
        self.mark_dirty(r.inflate(1));
    }

    /// Level relief under blender dabs; a layer with no field is left flat.
    pub(crate) fn height_smooth(&mut self, layer: &str, dabs: &[HeightDab], flatten: f64) {
        let doc = self.doc.bounds();
        let Some(h) = self.doc.layer_mut(layer).and_then(|l| l.height.as_mut()) else { return };
        if dabs.is_empty() {
            return;
        }
        let r = height::smooth(h, dabs, flatten, doc);
        self.mark_dirty(r.inflate(1));
    }

    /// The eraser's stroke coverage carves the relief under it.
    pub(crate) fn height_carve(&mut self, layer: &str, stroke: &Surface, opacity: f32) {
        let Some(h) = self.doc.layer_mut(layer).and_then(|l| l.height.as_mut()) else { return };
        let r = height::carve(h, stroke, opacity);
        self.mark_dirty(r.inflate(1));
    }

    /// The field's changes since `e` began, as a history entry (pushed by the caller).
    pub(crate) fn height_end(&mut self, e: HeightEdit) -> Option<(Change, usize)> {
        let l = self.doc.layer_mut(&e.layer)?;
        if l.height.as_ref().is_some_and(|h| h.tile_count() == 0) {
            l.height = None;
        }
        let empty = Surface::new();
        let after = l.height.as_ref().unwrap_or(&empty);
        let mut keys: BTreeSet<(i32, i32)> = e.before.tile_keys().into_iter().collect();
        keys.extend(after.tile_keys());
        let (tiles, bytes) = tile_diff(&e.before, after, keys);
        (!tiles.is_empty()).then(|| (Change::Tiles { layer: e.layer, plane: Plane::Height, tiles }, bytes))
    }

    /// Put the field back as it was (stroke cancelled).
    pub(crate) fn height_cancel(&mut self, e: HeightEdit) {
        if let Some(l) = self.doc.layer_mut(&e.layer) {
            l.height = (e.before.tile_count() > 0).then_some(e.before);
        }
        self.mark_all_dirty();
    }

    /// Push a stroke's paint and relief changes as one undo step.
    pub(crate) fn push_with_height(&mut self, paint: Option<(Change, usize)>, relief: Option<(Change, usize)>) {
        if paint.is_none() && relief.is_none() {
            return;
        }
        self.history.begin_transaction();
        for (c, b) in paint.into_iter().chain(relief) {
            self.history.push(c, b);
        }
        self.history.end_transaction();
        self.doc.touch();
        self.history_changed();
    }

    /// Bevel & Emboss: replace the layer's relief with one derived from its
    /// alpha (`bevelFromAlpha`, PE:1161). Nothing painted, or depth 0, leaves
    /// the field alone and returns `false`.
    pub(crate) fn bevel_from_alpha(&mut self, id: &str, o: BevelOptions) -> Result<bool, EngineError> {
        let l = self.doc.layer(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let doc = self.doc.bounds();
        let px = l.pixels.read_rect(doc);
        let alpha: Vec<f32> = px.chunks_exact(4).map(|p| p[3]).collect();
        let Some(field) = height::bevel_field(&alpha, doc.w as usize, doc.h as usize, o) else { return Ok(false) };
        self.replace_height(id, Some(height::field_to_surface(&field, doc)));
        Ok(true)
    }

    /// Drop the layer's relief (`clearBevel`).
    pub(crate) fn clear_height(&mut self, id: &str) -> Result<bool, EngineError> {
        let l = self.doc.layer(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        if l.height.is_none() {
            return Ok(false);
        }
        self.replace_height(id, None);
        Ok(true)
    }

    fn replace_height(&mut self, id: &str, field: Option<Surface>) {
        let Some(e) = self.height_begin(id) else { return };
        if let Some(l) = self.doc.layer_mut(id) {
            l.height = field;
        }
        let relief = self.height_end(e);
        self.push_with_height(None, relief);
        self.mark_all_dirty();
    }
}

/// Relief per dry dab: `body · 0.1 · alpha` (`brush-engine.ts:501`).
pub(crate) fn dry_height_dabs(body: f64, dabs: &[onepaint_brush::Dab]) -> Vec<HeightDab> {
    if body <= 0.0 {
        return Vec::new();
    }
    dabs.iter().map(|d| HeightDab { x: d.x, y: d.y, radius: d.radius, amount: body * height::DRY_RATE * d.alpha }).collect()
}
