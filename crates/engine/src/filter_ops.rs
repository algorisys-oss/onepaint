//! Destructive filters with live preview, and adjustment layers (spec 10).

use super::*;
use onepaint_filters::{Kind, Params, apply_spatial, is_identity, prepare, reach};
use rayon::prelude::*;

pub(crate) struct FilterSession {
    pub layer: NodeId,
    pub kind: Kind,
    pub original: Surface,
    pub params: Params,
}

impl Session {
    pub fn filter_active(&self) -> bool {
        self.filter.is_some()
    }

    /// Kind and current params of the filter being previewed.
    pub fn filter_state(&self) -> Option<(Kind, &Params)> {
        self.filter.as_ref().map(|f| (f.kind, &f.params))
    }

    pub(crate) fn begin_filter(&mut self, kind: &str) -> Result<(), EngineError> {
        self.commit_filter();
        self.commit_transform();
        let kind = Kind::parse(kind).ok_or_else(|| EngineError::UnknownPreset(kind.to_string()))?;
        let Some(l) = self.doc.layer(&self.active).filter(|l| l.visible && !l.locked && l.adjustment.is_none()) else { return Ok(()) };
        self.filter =
            Some(FilterSession { layer: l.id.clone(), kind, original: l.pixels.clone(), params: onepaint_filters::default_params(kind) });
        let p = onepaint_filters::default_params(kind);
        self.update_filter(p);
        Ok(())
    }

    /// Re-run the filter from the original pixels with new params.
    pub(crate) fn update_filter(&mut self, params: Params) {
        let doc = self.doc.bounds();
        let clip = self.clip.clone();
        let Some(f) = self.filter.as_mut() else { return };
        f.params = params;
        let (kind, params) = (f.kind, f.params.clone());
        let mut out = f.original.clone();
        let content = f.original.content_bounds().and_then(|b| b.intersect(&doc));
        if let Some(content) = content
            && !is_identity(kind, &params)
        {
            let keep = |x: i32, y: i32| clip.as_ref().map_or(1.0, |m| m.coverage(x, y));
            if kind.is_color() {
                let prepared = prepare(kind, &params);
                let keys: Vec<_> =
                    f.original.tile_keys().into_iter().filter(|&(tx, ty)| Rect::from_tile(tx, ty).intersect(&doc).is_some()).collect();
                let tiles: Vec<_> = keys
                    .par_iter()
                    .filter_map(|&(tx, ty)| {
                        let t = f.original.tile(tx, ty)?;
                        let mut buf = t.to_f32();
                        for i in (0..buf.len()).step_by(4) {
                            let p = (i / 4) as i32;
                            let (x, y) = (tx * TILE + p % TILE, ty * TILE + p / TILE);
                            let s = [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]];
                            let k = keep(x, y);
                            if k <= 0.0 {
                                continue;
                            }
                            let o = prepared.color_px(s, x, y);
                            for c in 0..4 {
                                buf[i + c] = s[c] + (o[c] - s[c]) * k;
                            }
                        }
                        Some(((tx, ty), Arc::new(Tile::from_f32(&buf))))
                    })
                    .collect();
                for ((tx, ty), t) in tiles {
                    out.set_tile(tx, ty, Some(t));
                }
            } else {
                // Spatial: run over the content plus the kernel's reach, clipped to the canvas.
                let r = reach(kind, &params);
                let region =
                    Rect::new(content.x - r, content.y - r, content.w + 2 * r, content.h + 2 * r).intersect(&doc).unwrap_or(content);
                let src = f.original.read_rect(region);
                let res = apply_spatial(kind, &params, &src, region.w as usize, region.h as usize);
                let mixed: Vec<f32> = if clip.is_some() {
                    let mut m = res;
                    for y in 0..region.h {
                        for x in 0..region.w {
                            let k = keep(region.x + x, region.y + y);
                            let i = ((y * region.w + x) * 4) as usize;
                            for c in 0..4 {
                                m[i + c] = src[i + c] + (m[i + c] - src[i + c]) * k;
                            }
                        }
                    }
                    m
                } else {
                    res
                };
                out.write_rect(region, &mixed);
                for (tx, ty) in region.tiles() {
                    out.prune_tile(tx, ty);
                }
            }
        }
        let id = f.layer.clone();
        if let Some(l) = self.doc.layer_mut(&id) {
            l.pixels = out;
        }
        self.mark_all_dirty();
    }

    pub(crate) fn cancel_filter(&mut self) {
        let Some(f) = self.filter.take() else { return };
        if let Some(l) = self.doc.layer_mut(&f.layer) {
            l.pixels = f.original;
        }
        self.mark_all_dirty();
    }

    pub(crate) fn commit_filter(&mut self) {
        let Some(f) = self.filter.take() else { return };
        let Some(l) = self.doc.layer(&f.layer) else { return };
        let mut keys: BTreeSet<(i32, i32)> = f.original.tile_keys().into_iter().collect();
        keys.extend(l.pixels.tile_keys());
        let mut changes: Vec<TileChange> = Vec::new();
        let mut bytes = 0;
        for (tx, ty) in keys {
            let before = f.original.tile(tx, ty).cloned();
            let after = l.pixels.tile(tx, ty).cloned();
            let same = match (&before, &after) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                (None, None) => true,
                _ => false,
            };
            if !same {
                bytes += tile_bytes(&before) + tile_bytes(&after);
                changes.push(((tx, ty), before, after));
            }
        }
        if !changes.is_empty() {
            self.history.push(Change::Tiles { layer: f.layer, mask: false, tiles: changes }, bytes);
            self.doc.touch();
            self.history_changed();
        }
    }

    /// A new adjustment layer above the active one.
    pub(crate) fn add_adjustment_layer(&mut self, kind: &str, params: Params) -> Result<(), EngineError> {
        let k = Kind::parse(kind).ok_or_else(|| EngineError::UnknownPreset(kind.to_string()))?;
        let mut l = Layer::new(k.label());
        l.adjustment = Some(Box::new(onepaint_doc::Adjustment { kind: kind.to_string(), params }));
        self.insert_above_active(Node::Layer(l))
    }

    /// Change an adjustment layer's params (coalesced per layer like other prop edits).
    pub(crate) fn set_adjustment(&mut self, id: &str, params: Params) -> Result<(), EngineError> {
        if self.doc.layer(id).and_then(|l| l.adjustment.as_ref()).is_none() {
            return Err(EngineError::NoSuchLayer(id.into()));
        }
        let key = Some(format!("{id}:adjustment"));
        let id = id.to_string();
        self.tree_edit(key, move |s| {
            if let Some(a) = s.doc.layer_mut(&id).and_then(|l| l.adjustment.as_mut()) {
                a.params = params;
            }
            Ok(())
        })
    }
}
