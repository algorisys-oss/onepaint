//! Smudge and mixer strokes (spec 16): the layer is edited live, snapshotted
//! at begin, and recorded as one Tiles change at end.

use super::*;
use onepaint_brush::smudge::{Reservoir, SMUDGE_STRENGTH, Smudge, SmudgeParams, WetParams};

/// The Mixer tool's options (Photoshop's Mixer Brush), tool-store defaults.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixerSettings {
    pub wet: f64,
    pub load: f64,
    pub mix: f64,
    /// Keep colour: the brush holds what it picked up across strokes.
    pub keep: bool,
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self { wet: 0.5, load: 0.5, mix: 0.5, keep: false }
    }
}

/// Session-wide painting options that outlive a document.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaintSettings {
    pub smudge_strength: f64,
    pub mixer: MixerSettings,
    pub liquify_mode: onepaint_filters::liquify::LiquifyMode,
    pub liquify_strength: f64,
}

impl Default for PaintSettings {
    fn default() -> Self {
        Self { smudge_strength: SMUDGE_STRENGTH, mixer: MixerSettings::default(), liquify_mode: Default::default(), liquify_strength: 0.5 }
    }
}

pub(crate) struct ActiveSmudge {
    layer: NodeId,
    mask: bool,
    engine: Smudge,
    before: Surface,
    dirty: Rect,
    key: Option<String>,
}

impl Session {
    pub fn paint_settings(&self) -> &PaintSettings {
        &self.paint
    }

    pub(crate) fn symmetry_transforms(&self) -> Vec<onepaint_brush::symmetry::SymTransform> {
        let s = self.guides.symmetry;
        if s.is_on() { s.transforms(self.doc.width, self.doc.height) } else { Vec::new() }
    }

    pub(crate) fn smudge_begin(&mut self, p: StrokePoint) {
        let to_mask = self.mask_target();
        let Some(layer) = self.doc.layer(&self.active) else { return };
        if layer.locked || !layer.visible || layer.adjustment.is_some() {
            return;
        }
        let mixer = self.tool == Tool::Mixer;
        let b = &self.brush;
        let m = self.paint.mixer;
        let params = SmudgeParams {
            size: b.size,
            strength: if mixer { b.opacity } else { self.paint.smudge_strength },
            hardness: b.hardness,
            tip: onepaint_brush::tip_for(&b.tip_id),
            size_pressure: b.size_pressure,
            flow_pressure: b.flow_pressure,
            spacing: Some(b.spacing),
            wet: mixer.then_some(WetParams { mix: m.mix, load: m.load, wetness: m.wet, pigment: true }),
        };
        let lin = self.color.to_linear_premul();
        let color = mixer.then_some([lin[0], lin[1], lin[2]]);
        let key = mixer.then(|| self.color.to_hex());
        let held = match (&self.mixer_held, mixer && m.keep) {
            (Some((k, r)), true) if Some(k) == key.as_ref() => Some(*r),
            _ => None,
        };
        let target = if to_mask { layer.mask.clone().unwrap_or_default() } else { layer.pixels.clone() };
        let alpha_lock = layer.alpha_lock && !to_mask;
        let engine = Smudge::new(params, color, held, self.symmetry_transforms(), self.doc.bounds(), alpha_lock, p);
        self.smudge = Some(ActiveSmudge { layer: self.active.clone(), mask: to_mask, engine, before: target, dirty: Rect::default(), key });
    }

    fn smudge_target(&mut self, id: &str, mask: bool) -> Option<&mut Surface> {
        let l = self.doc.layer_mut(id)?;
        if mask { l.mask.as_mut() } else { Some(&mut l.pixels) }
    }

    pub(crate) fn smudge_extend(&mut self, pts: &[StrokePoint]) {
        let Some(mut sm) = self.smudge.take() else { return };
        let clip = self.clip.clone();
        if let Some(target) = self.smudge_target(&sm.layer, sm.mask) {
            let mut r = Rect::default();
            for p in pts {
                r = r.union(&sm.engine.add(target, *p, clip.as_deref()));
            }
            sm.dirty = sm.dirty.union(&r);
            self.mark_dirty(r);
        }
        self.smudge = Some(sm);
    }

    pub(crate) fn smudge_end(&mut self) {
        let Some(sm) = self.smudge.take() else { return };
        if self.tool == Tool::Mixer && self.paint.mixer.keep {
            self.mixer_held = sm.key.clone().map(|k| (k, sm.engine.reservoir()));
        } else {
            self.mixer_held = None;
        }
        let Some(target) = self.smudge_target(&sm.layer, sm.mask) else { return };
        let mut changes: Vec<TileChange> = Vec::new();
        let mut bytes = 0;
        for (tx, ty) in sm.dirty.tiles() {
            let before = sm.before.tile(tx, ty).cloned();
            let after = target.tile(tx, ty).cloned();
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
            self.history.push(Change::Tiles { layer: sm.layer, mask: sm.mask, tiles: changes }, bytes);
            self.doc.touch();
            self.history_changed();
        }
    }

    pub(crate) fn smudge_cancel(&mut self) {
        let Some(sm) = self.smudge.take() else { return };
        if let Some(target) = self.smudge_target(&sm.layer, sm.mask) {
            *target = sm.before;
        }
        self.mark_dirty(sm.dirty);
    }

    /// Mixer "Clean": the next stroke loads fresh paint.
    pub(crate) fn clean_mixer(&mut self) {
        self.mixer_held = None;
    }

    /// What a kept Mixer brush still holds (straight linear colour + amount).
    pub fn mixer_reservoir(&self) -> Option<Reservoir> {
        self.mixer_held.as_ref().map(|h| h.1)
    }
}
