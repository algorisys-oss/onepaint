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
        self.history.push(Change::Selection { before, after: after.clone() }, bytes);
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
                let region = if sample_all {
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
        let (w, h) = (self.doc.width, self.doc.height);
        let Some(l) = self.editable_active() else {
            return;
        };
        let base = self.selection.clone();
        let get = |px: u32, py: u32| l.pixels.get(px as i32, py as i32);
        let Some(region) = color_match(w, h, &get, x, y, tol, true, base.as_deref()) else {
            return;
        };
        let region = region.grown4(grow, base.as_deref());
        let Some((bx, by, bw, bh)) = region.bounds() else {
            return;
        };
        let c = self.color.to_linear_premul();
        let alpha_lock = l.alpha_lock;
        let id = self.active.clone();
        self.modify_layer(&id, Rect::new(bx as i32, by as i32, bw as i32, bh as i32), |x, y, p| {
            if region.get(x as u32, y as u32) == 0 {
                return p;
            }
            let after = if alpha_lock { [c[0] * p[3], c[1] * p[3], c[2] * p[3], p[3]] } else { [c[0], c[1], c[2], 1.0] };
            let s = base.as_ref().map_or(1.0, |m| m.coverage(x, y));
            [0, 1, 2, 3].map(|i| p[i] + (after[i] - p[i]) * s)
        });
    }

    /// Source-over the current colour at the selection's coverage.
    fn fill_selection(&mut self) {
        self.ensure_raster();
        let Some(clip) = self.clip.clone() else {
            return;
        };
        let Some(l) = self.editable_active() else {
            return;
        };
        let alpha_lock = l.alpha_lock;
        let Some((bx, by, bw, bh)) = clip.bounds() else {
            return;
        };
        let c = self.color.to_linear_premul();
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
    pub(crate) fn modify_layer(&mut self, id: &str, rect: Rect, f: impl Fn(i32, i32, [f32; 4]) -> [f32; 4]) {
        let Some(rect) = rect.intersect(&self.doc.bounds()) else {
            return;
        };
        let Some(layer) = self.doc.layer_mut(id) else {
            return;
        };
        let mut changes: Vec<TileChange> = Vec::new();
        let mut bytes = 0;
        for (tx, ty) in rect.tiles() {
            let Some(ix) = rect.intersect(&Rect::from_tile(tx, ty)) else {
                continue;
            };
            let before = layer.pixels.tile(tx, ty).cloned();
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
            if before.as_deref() == after.as_deref() {
                continue;
            }
            layer.pixels.set_tile(tx, ty, after.clone());
            bytes += tile_bytes(&before) + tile_bytes(&after);
            changes.push(((tx, ty), before, after));
        }
        if changes.is_empty() {
            return;
        }
        self.history.push(Change::Tiles { layer: id.to_string(), mask: false, tiles: changes }, bytes);
        self.mark_dirty(rect);
        self.doc.touch();
        self.history_changed();
    }
}
