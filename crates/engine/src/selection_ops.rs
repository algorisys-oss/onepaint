//! Selection, fill and clear commands (spec 08).

use super::*;
use onepaint_selection::{Mask, Mode, color_match, combine, rasterize_polygons};

impl Session {
    /// The committed selection (unfeathered); `None` = nothing selected.
    pub fn selection(&self) -> Option<&Mask> {
        self.selection.as_deref()
    }

    /// The mask that clips painting and is displayed: the selection, feathered.
    pub fn clip_mask(&self) -> Option<&Mask> {
        self.clip.as_deref()
    }

    /// Bumps whenever the clip mask changes (for GPU uploads).
    pub fn selection_version(&self) -> u64 {
        self.selection_version
    }

    pub fn feather(&self) -> f32 {
        self.feather
    }

    pub(crate) fn set_selection_raw(&mut self, m: Option<Arc<Mask>>) {
        self.selection = m;
        self.refresh_clip();
    }

    fn refresh_clip(&mut self) {
        self.clip = match &self.selection {
            Some(m) if self.feather > 0.0 => Some(Arc::new(m.feathered(self.feather))),
            other => other.clone(),
        };
        self.selection_version += 1;
        self.events.push(Event::SelectionChanged);
    }

    /// Replace the selection as one undo step; empty masks clear it.
    pub(crate) fn commit_selection(&mut self, new: Option<Mask>) {
        let after = new.filter(Mask::has_content).map(Arc::new);
        if after.is_none() && self.selection.is_none() {
            return;
        }
        let before = self.selection.clone();
        let bytes = (self.doc.width * self.doc.height) as usize * 2;
        self.record(Change::Selection { before, after: after.clone() }, bytes);
        self.set_selection_raw(after);
        self.history_changed();
    }

    pub(crate) fn region_combined(&self, region: &Mask, mode: Mode) -> Mask {
        combine(self.selection.as_deref(), region, mode)
    }

    pub(crate) fn execute_selection(&mut self, cmd: Command) -> Result<(), EngineError> {
        let (w, h) = (self.doc.width, self.doc.height);
        match cmd {
            Command::SelectShape { contours, mode } => {
                let region = rasterize_polygons(w, h, &contours);
                let m = self.region_combined(&region, mode);
                self.commit_selection(Some(m));
            }
            Command::SelectByColor { x, y, tolerance, mode, contiguous, sample_all } => {
                let region = if sample_all && contiguous {
                    // Composite only the tiles the region reaches, as `flatten` would.
                    let doc = self.flatten_doc();
                    let ov = self.overlay();
                    let cols = w.div_ceil(TILE as u32);
                    let tiles: Vec<std::sync::OnceLock<Vec<f32>>> =
                        (0..cols * h.div_ceil(TILE as u32)).map(|_| std::sync::OnceLock::new()).collect();
                    let t = TILE as u32;
                    let get = |px: u32, py: u32| {
                        let (tx, ty) = (px / t, py / t);
                        let b = tiles[(ty * cols + tx) as usize]
                            .get_or_init(|| onepaint_compose::composite_tile(&doc, tx as i32, ty as i32, ov.as_ref()));
                        let i = (((py % t) * t + px % t) * 4) as usize;
                        [b[i], b[i + 1], b[i + 2], b[i + 3]]
                    };
                    color_match(w, h, &get, x, y, tolerance, true, None)
                } else if sample_all {
                    let flat = self.flatten();
                    let get = |px: u32, py: u32| {
                        let i = ((py * w + px) * 4) as usize;
                        [flat[i], flat[i + 1], flat[i + 2], flat[i + 3]]
                    };
                    color_match(w, h, &get, x, y, tolerance, contiguous, None)
                } else {
                    let Some(l) = self.doc.layer(&self.active) else {
                        return Ok(());
                    };
                    if !l.visible {
                        return Ok(());
                    }
                    let get = |px: u32, py: u32| l.pixels.get(px as i32, py as i32);
                    color_match(w, h, &get, x, y, tolerance, contiguous, None)
                };
                if let Some(r) = region {
                    let m = self.region_combined(&r, mode);
                    self.commit_selection(Some(m));
                }
            }
            Command::SelectAll => self.commit_selection(Some(Mask::full(w, h))),
            Command::Deselect => self.commit_selection(None),
            Command::InvertSelection => {
                if let Some(m) = self.selection.clone() {
                    self.commit_selection(Some(m.inverted()));
                }
            }
            Command::GrowSelection { delta } => {
                if let Some(m) = self.selection.clone()
                    && delta != 0
                {
                    let r = delta.unsigned_abs().min(200);
                    self.commit_selection(Some(if delta > 0 { m.dilated(r) } else { m.eroded(r) }));
                }
            }
            Command::SmoothSelection { px } => {
                if let Some(m) = self.selection.clone() {
                    self.commit_selection(Some(m.smoothed(px)));
                }
            }
            Command::SetFeather { radius } => {
                self.feather = radius.clamp(0.0, 100.0);
                self.refresh_clip();
            }
            Command::Fill { x, y, tolerance, grow } => self.fill(x, y, tolerance, grow.min(8)),
            Command::WashFill { x, y, options } => {
                self.wash_fill(x, y, options);
            }
            Command::FillSelection => self.fill_selection(),
            Command::ClearSelected => {
                self.ensure_raster();
                if self.clip.is_none() {
                    let id = self.active.clone();
                    if self.doc.layer(&id).is_some() {
                        return self.execute(Command::ClearLayer { id });
                    }
                    return Ok(());
                }
                let clip = self.clip.clone();
                let Some(clip) = clip else { return Ok(()) };
                let Some((bx, by, bw, bh)) = clip.bounds() else {
                    return Ok(());
                };
                let id = self.active.clone();
                self.modify_layer(&id, Rect::new(bx as i32, by as i32, bw as i32, bh as i32), |x, y, p| {
                    let k = 1.0 - clip.coverage(x, y);
                    p.map(|v| v * k)
                });
            }
            _ => {}
        }
        Ok(())
    }

    fn editable_active(&self) -> Option<&Layer> {
        self.doc.layer(&self.active).filter(|l| l.visible && !l.locked)
    }

    /// Paint bucket (happypaint `computeFill`, PE:1559-1691).
    fn fill(&mut self, x: i32, y: i32, tol: f32, grow: u32) {
        self.ensure_raster();
        let id = self.active.clone();
        if let Some(b) = self.bucket(&id, x, y, tol, grow) {
            self.modify_layer(&id, b.rect, |x, y, p| b.paint(x, y, p));
        }
    }

    /// The bucket fill of a layer from a seed. `None` when the layer can't
    /// take it or the seed misses (outside the canvas or the selection).
    pub(crate) fn bucket(&self, id: &str, x: i32, y: i32, tol: f32, grow: u32) -> Option<Bucket> {
        let (w, h) = (self.doc.width, self.doc.height);
        let l = self.doc.layer(id).filter(|l| l.visible && !l.locked)?;
        let base = self.selection.clone();
        let get = |px: u32, py: u32| l.pixels.get(px as i32, py as i32);
        let region = color_match(w, h, &get, x, y, tol, true, base.as_deref())?;
        let region = region.grown4(grow, base.as_deref());
        let (bx, by, bw, bh) = region.bounds()?;
        let rect = Rect::new(bx as i32, by as i32, bw as i32, bh as i32);
        Some(Bucket { rect, region, base, color: self.doc.space().pixel(self.color), alpha_lock: l.alpha_lock })
    }

    /// Source-over the current colour at the selection's coverage.
    fn fill_selection(&mut self) {
        self.ensure_raster();
        // No selection: fill the whole layer (Photoshop's Edit ▸ Fill).
        let (w, h) = (self.doc.width, self.doc.height);
        let clip = self.clip.clone().unwrap_or_else(|| Arc::new(Mask::from_vec(w, h, vec![255; (w * h) as usize])));
        let Some(l) = self.editable_active() else {
            return;
        };
        let alpha_lock = l.alpha_lock;
        let Some((bx, by, bw, bh)) = clip.bounds() else {
            return;
        };
        let c = self.doc.space().pixel(self.color);
        let id = self.active.clone();
        self.modify_layer(&id, Rect::new(bx as i32, by as i32, bw as i32, bh as i32), |x, y, p| {
            let a = clip.coverage(x, y);
            if a <= 0.0 {
                return p;
            }
            let k = 1.0 - a;
            if alpha_lock {
                [c[0] * a * p[3] + p[0] * k, c[1] * a * p[3] + p[1] * k, c[2] * a * p[3] + p[2] * k, p[3]]
            } else {
                [c[0] * a + p[0] * k, c[1] * a + p[1] * k, c[2] * a + p[2] * k, a + p[3] * k]
            }
        });
    }

    /// Apply `f` to every pixel of a layer inside `rect` (clipped to the canvas) as one undo step.
    pub(crate) fn modify_layer(&mut self, id: &str, rect: Rect, f: impl Fn(i32, i32, [f32; 4]) -> [f32; 4] + Sync) {
        let Some(rect) = rect.intersect(&self.doc.bounds()) else {
            return;
        };
        let Some(layer) = self.doc.layer_mut(id) else {
            return;
        };
        let changes = paint_rect(&mut layer.pixels, rect, f);
        if changes.is_empty() {
            return;
        }
        let bytes = changes.iter().map(|(_, b, a)| tile_bytes(b) + tile_bytes(a)).sum();
        self.record(Change::Tiles { layer: id.to_string(), plane: Plane::Pixels, tiles: changes }, bytes);
        self.mark_dirty(rect);
        self.doc.touch();
        self.history_changed();
    }
}

/// A computed bucket fill: the region it covers and how it paints.
pub(crate) struct Bucket {
    pub rect: Rect,
    region: Mask,
    base: Option<Arc<Mask>>,
    color: [f32; 4],
    alpha_lock: bool,
}

impl Bucket {
    pub fn paint(&self, x: i32, y: i32, p: [f32; 4]) -> [f32; 4] {
        if self.region.get(x as u32, y as u32) == 0 {
            return p;
        }
        let c = self.color;
        let after = if self.alpha_lock { [c[0] * p[3], c[1] * p[3], c[2] * p[3], p[3]] } else { [c[0], c[1], c[2], 1.0] };
        let s = self.base.as_ref().map_or(1.0, |m| m.coverage(x, y));
        [0, 1, 2, 3].map(|i| p[i] + (after[i] - p[i]) * s)
    }
}

/// Apply `f` to every pixel of `pixels` inside `rect`; returns the tiles
/// that changed (before, after). Tiles are worked in parallel.
pub(crate) fn paint_rect(pixels: &mut Surface, rect: Rect, f: impl Fn(i32, i32, [f32; 4]) -> [f32; 4] + Sync) -> Vec<TileChange> {
    use rayon::prelude::*;
    let keys: Vec<(i32, i32)> = rect.tiles().collect();
    let src = &*pixels;
    let changes: Vec<TileChange> = keys
        .par_iter()
        .filter_map(|&(tx, ty)| {
            let ix = rect.intersect(&Rect::from_tile(tx, ty))?;
            let before = src.tile(tx, ty).cloned();
            let mut buf = before.as_ref().map(|t| t.to_f32()).unwrap_or_else(|| vec![0.0; TILE_PX * 4]);
            for y in ix.y..ix.bottom() {
                for x in ix.x..ix.right() {
                    let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
                    let o = f(x, y, [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]);
                    buf[i..i + 4].copy_from_slice(&o);
                }
            }
            let t = Tile::from_f32(&buf);
            let after = (!t.is_empty()).then(|| Arc::new(t));
            (before.as_deref() != after.as_deref()).then_some(((tx, ty), before, after))
        })
        .collect();
    for ((tx, ty), _, after) in &changes {
        pixels.set_tile(*tx, *ty, after.clone());
    }
    changes
}
