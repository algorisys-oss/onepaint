//! Destructive filters with live preview, generator layers, and adjustment
//! layers (specs 10 and 18).

use super::*;
use onepaint_filters::{Frame, GLOBAL_REACH, Kind, Params, apply_spatial_in, is_identity, prepare_in, reach};
use rayon::prelude::*;

/// A generator's fresh layer (PE `NEW_LAYER_KINDS`): the tree before it was
/// inserted, so cancel restores it and commit records one tree step.
pub(crate) struct GenLayer {
    root_before: Vec<Node>,
    active_before: NodeId,
    /// The layer the generator was started from (line art reads it).
    source: Surface,
}

pub(crate) struct FilterSession {
    pub layer: NodeId,
    pub kind: Kind,
    pub original: Surface,
    pub params: Params,
    pub generated: Option<GenLayer>,
}

impl Session {
    pub fn filter_active(&self) -> bool {
        self.filter.is_some()
    }

    /// Kind and current params of the filter being previewed.
    pub fn filter_state(&self) -> Option<(Kind, &Params)> {
        self.filter.as_ref().map(|f| (f.kind, &f.params))
    }

    fn doc_frame(&self) -> Frame {
        Frame::region(0, 0, self.doc.width, self.doc.height, self.doc.dpi as f32)
    }

    pub(crate) fn begin_filter(&mut self, kind: &str) -> Result<(), EngineError> {
        self.commit_filter();
        self.commit_transform();
        self.ensure_raster();
        let kind = Kind::parse(kind).ok_or_else(|| EngineError::UnknownPreset(kind.to_string()))?;
        let Some(l) = self.doc.layer(&self.active).filter(|l| l.visible && !l.locked && l.adjustment.is_none()) else { return Ok(()) };
        let params = onepaint_filters::default_params(kind);
        if kind.is_generator() {
            let source = l.pixels.clone();
            let fresh = GenLayer { root_before: self.doc.root.clone(), active_before: self.active.clone(), source };
            let mut nl = Layer::new(if kind == Kind::LineArt { "Line art" } else { kind.label() });
            if kind == Kind::LensFlare {
                // Light adds: a flare on a Screen layer can only brighten what is below.
                nl.blend_mode = BlendMode::Screen;
            }
            let id = nl.id.clone();
            let (parent, index) = match self.doc.path_of(&self.active) {
                Some(p) => (self.doc.parent_of(&self.active).flatten(), p.last().map_or(0, |i| i + 1)),
                None => (None, self.doc.root.len()),
            };
            self.doc.insert(parent.as_deref(), index, Node::Layer(nl));
            self.active = id.clone();
            self.events.push(Event::LayersChanged);
            self.events.push(Event::ActiveLayerChanged);
            self.filter = Some(FilterSession { layer: id, kind, original: Surface::new(), params: params.clone(), generated: Some(fresh) });
        } else {
            self.filter =
                Some(FilterSession { layer: l.id.clone(), kind, original: l.pixels.clone(), params: params.clone(), generated: None });
        }
        self.update_filter(params);
        Ok(())
    }

    /// Render a generator into a fresh surface covering the canvas.
    fn render_generator(&self, kind: Kind, params: &Params, source: &Surface) -> Surface {
        let doc = self.doc.bounds();
        let frame = self.doc_frame();
        let mut out = Surface::new();
        if kind.is_pixelwise() {
            // Selection-masked like happypaint's generator shaders (`* mask`).
            let clip = self.clip.clone();
            let prepared = prepare_in(kind, params, frame);
            let keys: Vec<(i32, i32)> = doc.tiles().collect();
            let tiles: Vec<_> = keys
                .par_iter()
                .filter_map(|&(tx, ty)| {
                    let mut buf = vec![0.0f32; TILE_PX * 4];
                    let mut any = false;
                    for (p, px) in buf.chunks_mut(4).enumerate() {
                        let (x, y) = (tx * TILE + p as i32 % TILE, ty * TILE + p as i32 / TILE);
                        if x >= doc.w || y >= doc.h {
                            continue;
                        }
                        let k = clip.as_ref().map_or(1.0, |m| m.coverage(x, y));
                        if k <= 0.0 {
                            continue;
                        }
                        let o = prepared.color_px([0.0; 4], x, y);
                        if o[3] > 0.0 {
                            any = true;
                            for c in 0..4 {
                                px[c] = o[c] * k;
                            }
                        }
                    }
                    any.then(|| ((tx, ty), Arc::new(Tile::from_f32(&buf))))
                })
                .collect();
            for ((tx, ty), t) in tiles {
                out.set_tile(tx, ty, Some(t));
            }
        } else {
            // Line art reads the source; lightning draws on its own. Neither is mask-aware.
            let src = source.read_rect(doc);
            let res = apply_spatial_in(kind, params, &src, doc.w as usize, doc.h as usize, frame);
            out.write_rect(doc, &res);
            for (tx, ty) in doc.tiles() {
                out.prune_tile(tx, ty);
            }
        }
        out
    }

    /// Re-run the filter from the original pixels with new params.
    pub(crate) fn update_filter(&mut self, params: Params) {
        let doc = self.doc.bounds();
        let frame = self.doc_frame();
        let clip = self.clip.clone();
        let Some(f) = self.filter.as_mut() else { return };
        f.params = params;
        let (kind, params) = (f.kind, f.params.clone());
        if let Some(fresh) = &f.generated {
            let source = fresh.source.clone();
            let id = f.layer.clone();
            let out = self.render_generator(kind, &params, &source);
            if let Some(l) = self.doc.layer_mut(&id) {
                l.pixels = out;
            }
            self.mark_all_dirty();
            return;
        }
        let out = apply_filter(&f.original, doc, frame, kind, &params, clip.as_deref());
        let id = f.layer.clone();
        if let Some(l) = self.doc.layer_mut(&id) {
            l.pixels = out;
        }
        self.mark_all_dirty();
    }

    pub(crate) fn cancel_filter(&mut self) {
        let Some(f) = self.filter.take() else { return };
        if let Some(fresh) = f.generated {
            self.doc.root = fresh.root_before;
            self.active = fresh.active_before;
            self.events.push(Event::LayersChanged);
            self.events.push(Event::ActiveLayerChanged);
        } else if let Some(l) = self.doc.layer_mut(&f.layer) {
            l.pixels = f.original;
        }
        self.mark_all_dirty();
    }

    pub(crate) fn commit_filter(&mut self) {
        let Some(f) = self.filter.take() else { return };
        if let Some(fresh) = f.generated {
            // One undo step: the tree with the new layer replaces the tree without it.
            let bytes = self.doc.layer(&f.layer).map_or(0, |l| l.pixels.tile_keys().len() * TILE_PX * 8);
            let change = Change::Tree {
                before: fresh.root_before,
                after: self.doc.root.clone(),
                active_before: fresh.active_before,
                active_after: self.active.clone(),
                key: None,
            };
            self.history.push(change, bytes.max(1024));
            self.doc.touch();
            self.events.push(Event::LayersChanged);
            self.history_changed();
            return;
        }
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
            self.history.push(Change::Tiles { layer: f.layer, plane: Plane::Pixels, tiles: changes }, bytes);
            self.doc.touch();
            self.history_changed();
        }
    }

    /// A new adjustment (or fill) layer above the active one.
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

/// Apply one filter to a layer's pixels (selection-weighted when `clip` is
/// given); spatial kinds read their reach around the content.
pub(crate) fn apply_filter(
    src: &Surface,
    doc: Rect,
    frame: Frame,
    kind: Kind,
    params: &Params,
    clip: Option<&onepaint_selection::Mask>,
) -> Surface {
    let mut out = src.clone();
    let content = src.content_bounds().and_then(|b| b.intersect(&doc));
    if let Some(content) = content
        && !is_identity(kind, params)
    {
        let keep = |x: i32, y: i32| clip.map_or(1.0, |m| m.coverage(x, y));
        if kind.is_pixelwise() {
            let prepared = prepare_in(kind, params, frame);
            let keys: Vec<_> = src.tile_keys().into_iter().filter(|&(tx, ty)| Rect::from_tile(tx, ty).intersect(&doc).is_some()).collect();
            let tiles: Vec<_> = keys
                .par_iter()
                .filter_map(|&(tx, ty)| {
                    let t = src.tile(tx, ty)?;
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
            // Spatial: run over the content plus the kernel's reach, clipped to the
            // canvas; kinds that depend on the whole canvas run over all of it.
            let r = reach(kind, params);
            let region = if r >= GLOBAL_REACH {
                doc
            } else {
                Rect::new(content.x - r, content.y - r, content.w + 2 * r, content.h + 2 * r).intersect(&doc).unwrap_or(content)
            };
            let src = src.read_rect(region);
            let rf = Frame::region(region.x, region.y, frame.doc_w, frame.doc_h, frame.dpi);
            let res = apply_spatial_in(kind, params, &src, region.w as usize, region.h as usize, rf);
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
    out
}
