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
    height: Option<crate::height_ops::HeightEdit>,
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
        let params = match (mixer, b.wet().filter(|_| self.tool == Tool::Brush)) {
            // A wet preset: the classic 0.35 disc every 0.25 of the size, its
            // tip only with `wet.tip`, opacity as the master coverage
            // (PE:2847-2945); pressure knobs include the force overrides.
            (false, Some(w)) => {
                let fb = self.prefs.stroke_brush(b);
                let tip = if w.tip { onepaint_brush::tip_for(&b.tip_id) } else { onepaint_brush::Tip::Circle };
                SmudgeParams {
                    size: b.size,
                    strength: b.opacity,
                    hardness: 0.35,
                    tip,
                    size_pressure: fb.size_pressure,
                    flow_pressure: fb.flow_pressure,
                    spacing: None,
                    wet: Some(WetParams { mix: w.mix, load: w.load, wetness: w.wetness, pigment: w.pigment }),
                    impasto: onepaint_brush::height::pigment_body(b.impasto_depth(), w.load),
                    blend: w.blend.clamp(0.0, 1.0),
                }
            }
            _ => SmudgeParams {
                size: b.size,
                strength: if mixer { b.opacity } else { self.paint.smudge_strength },
                hardness: b.hardness,
                tip: onepaint_brush::tip_for(&b.tip_id),
                size_pressure: b.size_pressure,
                flow_pressure: b.flow_pressure,
                spacing: Some(b.spacing),
                wet: mixer.then_some(WetParams { mix: m.mix, load: m.load, wetness: m.wet, pigment: true }),
                impasto: if mixer { onepaint_brush::height::pigment_body(b.impasto_depth(), m.load) } else { 0.0 },
                blend: 0.0,
            },
        };
        let paints = params.wet.is_some();
        let lin = self.doc.space().pixel(self.color);
        let color = paints.then_some([lin[0], lin[1], lin[2]]);
        // The brush loads from a patch when one is loaded, else the colour;
        // Keep colour holds the reservoir while that source stays the same.
        let patch = self.mixer_patch.as_ref().filter(|_| mixer);
        let key = mixer.then(|| patch.map_or_else(|| self.color.to_hex(), |(g, _)| format!("patch:{g}")));
        let held = match (&self.mixer_held, mixer && m.keep) {
            (Some((k, r)), true) if Some(k) == key.as_ref() => Some(r.clone()),
            _ => patch.map(|(_, r)| r.clone()),
        };
        let target = if to_mask { layer.mask.clone().unwrap_or_default() } else { layer.pixels.clone() };
        let alpha_lock = layer.alpha_lock && !to_mask;
        // Relief: impasto builds a field; a blender only levels an existing one.
        let relief = !to_mask && (params.impasto > 0.0 || (params.blend > 0.0 && layer.height.is_some()));
        let height = if relief { self.height_begin(&self.active) } else { None };
        let mut engine = Smudge::new(params, color, held, self.symmetry_transforms(), self.doc.bounds(), alpha_lock, p);
        engine.set_space(if to_mask { onepaint_color::Space::Linear } else { self.doc.space() });
        self.smudge =
            Some(ActiveSmudge { layer: self.active.clone(), mask: to_mask, engine, before: target, dirty: Rect::default(), key, height });
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
        let (build, level) = sm.engine.take_height();
        if sm.height.is_some() {
            self.height_stamp(&sm.layer, &build);
            self.height_smooth(&sm.layer, &level, sm.engine.blend_flatten());
        }
        self.smudge = Some(sm);
    }

    pub(crate) fn smudge_end(&mut self) {
        let Some(sm) = self.smudge.take() else { return };
        if self.tool == Tool::Mixer && self.paint.mixer.keep {
            self.mixer_held = sm.key.clone().map(|k| (k, sm.engine.reservoir().clone()));
        } else {
            self.mixer_held = None;
        }
        let mut sm = sm;
        let relief = sm.height.take().and_then(|h| self.height_end(h));
        let Some(target) = self.smudge_target(&sm.layer, sm.mask) else { return };
        let (tiles, bytes) = tile_diff(&sm.before, target, sm.dirty.tiles());
        let paint = (!tiles.is_empty()).then(|| (Change::Tiles { layer: sm.layer.clone(), plane: Plane::of(sm.mask), tiles }, bytes));
        self.push_with_height(paint, relief);
    }

    pub(crate) fn smudge_cancel(&mut self) {
        let Some(sm) = self.smudge.take() else { return };
        if let Some(target) = self.smudge_target(&sm.layer, sm.mask) {
            *target = sm.before;
        }
        self.mark_dirty(sm.dirty);
        if let Some(h) = sm.height {
            self.height_cancel(h);
        }
    }

    /// Mixer "Clean": the next stroke loads fresh paint (and no patch).
    pub(crate) fn clean_mixer(&mut self) {
        self.mixer_held = None;
        self.mixer_patch = None;
    }

    /// What a kept Mixer brush still holds (straight colour in the
    /// document's space, and amount).
    pub fn mixer_reservoir(&self) -> Option<&Reservoir> {
        self.mixer_held.as_ref().map(|h| &h.1)
    }

    /// The patch the Mixer will load from, if one is loaded.
    pub fn mixer_patch(&self) -> Option<&Reservoir> {
        self.mixer_patch.as_ref().map(|p| &p.1)
    }

    /// Load the Mixer from the composite under a brush-wide square
    /// (`loadMixerPatch`, PE:2960-2972).
    pub(crate) fn load_mixer_patch(&mut self, x: f64, y: f64, size: f64) -> Result<(), EngineError> {
        if self.is_stroking() {
            return Err(EngineError::Invalid("can't load the brush mid-stroke".into()));
        }
        let r = (size / 2.0).max(1.0);
        let square = Rect::covering((x - r - 1.0) as f32, (y - r - 1.0) as f32, (x + r + 1.0) as f32, (y + r + 1.0) as f32);
        let none = || EngineError::Invalid("Nothing under the brush to load".into());
        let rect = square.intersect(&self.doc.bounds()).ok_or_else(none)?;
        let doc = self.display_doc();
        let mut buf = vec![0.0f32; (rect.w * rect.h * 4) as usize];
        for (tx, ty) in rect.tiles() {
            let Some(ix) = rect.intersect(&Rect::from_tile(tx, ty)) else { continue };
            let t = composite_tile(&doc, tx, ty, None);
            for yy in ix.y..ix.bottom() {
                let src = (((yy - ty * TILE) * TILE + (ix.x - tx * TILE)) * 4) as usize;
                let dst = (((yy - rect.y) * rect.w + (ix.x - rect.x)) * 4) as usize;
                let n = (ix.w * 4) as usize;
                buf[dst..dst + n].copy_from_slice(&t[src..src + n]);
            }
        }
        let patch = Reservoir::from_canvas(&buf, rect, x, y, r).ok_or_else(none)?;
        self.mixer_patch_gen += 1;
        self.mixer_patch = Some((self.mixer_patch_gen, patch));
        Ok(())
    }
}
