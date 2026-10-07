//! Layer masks: add (white or from the selection), delete, apply, enable, edit target (spec 11).

use super::*;

impl Session {
    /// True when strokes paint into the active layer's mask.
    pub fn mask_target(&self) -> bool {
        self.mask_target_layer.as_deref() == Some(self.active.as_str()) && self.doc.layer(&self.active).is_some_and(|l| l.mask.is_some())
    }

    pub(crate) fn add_mask(&mut self) -> Result<(), EngineError> {
        let id = self.active.clone();
        let Some(l) = self.doc.layer(&id) else { return Ok(()) };
        if l.mask.is_some() {
            return Ok(());
        }
        let (w, h) = (self.doc.width, self.doc.height);
        let mask = match &self.clip {
            Some(sel) => {
                let mut m = Surface::new();
                let data: Vec<f32> = sel
                    .data()
                    .iter()
                    .flat_map(|&v| {
                        let c = v as f32 / 255.0;
                        [c, c, c, 1.0]
                    })
                    .collect();
                m.write_rect(Rect::new(0, 0, w as i32, h as i32), &data);
                m
            }
            None => Document::filled(w, h, [1.0, 1.0, 1.0, 1.0]),
        };
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(&id) {
                l.mask = Some(mask);
                l.mask_enabled = true;
            }
            Ok(())
        })?;
        self.mask_target_layer = Some(id);
        Ok(())
    }

    pub(crate) fn delete_mask(&mut self, id: &str) -> Result<(), EngineError> {
        if self.doc.layer(id).and_then(|l| l.mask.as_ref()).is_none() {
            return Ok(());
        }
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.mask = None;
            }
            Ok(())
        })
    }

    pub(crate) fn apply_mask(&mut self, id: &str) -> Result<(), EngineError> {
        let Some(l) = self.doc.layer(id) else { return Ok(()) };
        let Some(mask) = l.mask.clone() else { return Ok(()) };
        let mut pixels = l.pixels.clone();
        for (tx, ty) in pixels.tile_keys() {
            let m = mask.tile(tx, ty).map(|t| t.to_f32());
            let mut buf = pixels.tile(tx, ty).map(|t| t.to_f32()).unwrap_or_default();
            for i in (0..buf.len()).step_by(4) {
                let k = m.as_ref().map_or(0.0, |m| m[i]);
                buf[i..i + 4].iter_mut().for_each(|v| *v *= k);
            }
            let t = Tile::from_f32(&buf);
            pixels.set_tile(tx, ty, (!t.is_empty()).then(|| Arc::new(t)));
        }
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.pixels = pixels;
                l.mask = None;
            }
            Ok(())
        })
    }

    pub(crate) fn set_mask_enabled(&mut self, id: &str, enabled: bool) -> Result<(), EngineError> {
        self.doc.layer(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let key = Some(format!("{id}:mask-enabled"));
        self.tree_edit(key, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.mask_enabled = enabled;
            }
            Ok(())
        })
    }
}
