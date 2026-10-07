//! Move / free transform: lift the layer (or the selected part), preview it
//! through an affine matrix, then commit as one undo step.

use super::*;
use rayon::prelude::*;

/// 2-D affine map: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(x: f64, y: f64) -> Self {
        Affine {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    pub fn scale(sx: f64, sy: f64) -> Self {
        Affine {
            a: sx,
            d: sy,
            ..Self::IDENTITY
        }
    }

    pub fn rotate(r: f64) -> Self {
        let (s, c) = r.sin_cos();
        Affine {
            a: c,
            b: s,
            c: -s,
            d: c,
            e: 0.0,
            f: 0.0,
        }
    }

    pub fn scale_about(sx: f64, sy: f64, p: [f64; 2]) -> Self {
        Self::translate(-p[0], -p[1])
            .then(&Self::scale(sx, sy))
            .then(&Self::translate(p[0], p[1]))
    }

    pub fn rotate_about(r: f64, p: [f64; 2]) -> Self {
        Self::translate(-p[0], -p[1])
            .then(&Self::rotate(r))
            .then(&Self::translate(p[0], p[1]))
    }

    /// `self` followed by `next`.
    pub fn then(&self, n: &Affine) -> Affine {
        Affine {
            a: n.a * self.a + n.c * self.b,
            b: n.b * self.a + n.d * self.b,
            c: n.a * self.c + n.c * self.d,
            d: n.b * self.c + n.d * self.d,
            e: n.a * self.e + n.c * self.f + n.e,
            f: n.b * self.e + n.d * self.f + n.f,
        }
    }

    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        [
            self.a * p[0] + self.c * p[1] + self.e,
            self.b * p[0] + self.d * p[1] + self.f,
        ]
    }

    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.d - self.b * self.c;
        if det.abs() < 1e-12 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Affine {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }

    /// Bounding box of `r` after the map.
    pub fn map_rect(&self, r: Rect) -> Rect {
        let (x0, y0, x1, y1) = (r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64);
        let pts = [
            self.apply([x0, y0]),
            self.apply([x1, y0]),
            self.apply([x0, y1]),
            self.apply([x1, y1]),
        ];
        let (mut mx, mut my, mut nx, mut ny) = (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        );
        for p in pts {
            mx = mx.min(p[0]);
            my = my.min(p[1]);
            nx = nx.max(p[0]);
            ny = ny.max(p[1]);
        }
        Rect::covering(mx as f32, my as f32, nx as f32, ny as f32)
    }
}

/// Lifted content being transformed.
pub(crate) struct Floating {
    pub layer: NodeId,
    /// Layer pixels before the transform (for cancel and the undo entry).
    pub original: Surface,
    /// Dense premultiplied copy of the lifted pixels over `bounds`.
    pub src: Vec<f32>,
    pub bounds: Rect,
    pub matrix: Affine,
    pub preview: Surface,
    pub preview_bounds: Rect,
    /// The selection that was lifted (cleared while transforming, restored on cancel).
    pub saved_selection: Option<Arc<onepaint_selection::Mask>>,
}

impl Floating {
    fn sample(&self, x: f64, y: f64) -> [f32; 4] {
        // Bilinear on texel centres, transparent outside.
        let (fx, fy) = (
            x - 0.5 - self.bounds.x as f64,
            y - 0.5 - self.bounds.y as f64,
        );
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = ((fx - x0) as f32, (fy - y0) as f32);
        let (w, h) = (self.bounds.w as i64, self.bounds.h as i64);
        let at = |xi: i64, yi: i64| -> [f32; 4] {
            if xi < 0 || yi < 0 || xi >= w || yi >= h {
                return [0.0; 4];
            }
            let i = ((yi * w + xi) * 4) as usize;
            [
                self.src[i],
                self.src[i + 1],
                self.src[i + 2],
                self.src[i + 3],
            ]
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (p00, p10, p01, p11) = (
            at(x0, y0),
            at(x0 + 1, y0),
            at(x0, y0 + 1),
            at(x0 + 1, y0 + 1),
        );
        let mut o = [0.0; 4];
        for c in 0..4 {
            let top = p00[c] + (p10[c] - p00[c]) * tx;
            let bot = p01[c] + (p11[c] - p01[c]) * tx;
            o[c] = top + (bot - top) * ty;
        }
        o
    }

    /// Resample the lifted pixels through `matrix` into `preview`, clipped to `clip`.
    fn render(&mut self, clip: Rect) {
        let inv = self.matrix.inverse();
        let target = self.matrix.map_rect(self.bounds).intersect(&clip);
        self.preview = Surface::new();
        self.preview_bounds = target.unwrap_or_default();
        let (Some(inv), Some(target)) = (inv, target) else {
            return;
        };
        let keys: Vec<_> = target.tiles().collect();
        let tiles: Vec<_> = keys
            .par_iter()
            .filter_map(|&(tx, ty)| {
                let ix = target.intersect(&Rect::from_tile(tx, ty))?;
                let mut buf = vec![0.0f32; TILE_PX * 4];
                let mut any = false;
                for y in ix.y..ix.bottom() {
                    for x in ix.x..ix.right() {
                        let s = inv.apply([x as f64 + 0.5, y as f64 + 0.5]);
                        let p = self.sample(s[0], s[1]);
                        if p[3] > 0.0 {
                            any = true;
                            let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
                            buf[i..i + 4].copy_from_slice(&p);
                        }
                    }
                }
                any.then(|| ((tx, ty), Arc::new(Tile::from_f32(&buf))))
            })
            .collect();
        for ((tx, ty), t) in tiles {
            self.preview.set_tile(tx, ty, Some(t));
        }
    }
}

impl Session {
    pub fn transform_active(&self) -> bool {
        self.transform.is_some()
    }

    /// Current matrix and the lifted content's bounds (doc px), while transforming.
    pub fn transform_state(&self) -> Option<(Affine, Rect)> {
        self.transform.as_ref().map(|f| (f.matrix, f.bounds))
    }

    pub(crate) fn transform_overlay(&self) -> Option<StrokeOverlay<'_>> {
        self.transform.as_ref().map(|f| StrokeOverlay {
            layer_id: &f.layer,
            stroke: &f.preview,
            opacity: 1.0,
            mode: StrokeMode::Paint(BlendMode::Normal),
        })
    }

    pub(crate) fn begin_transform(&mut self) -> Result<(), EngineError> {
        self.commit_transform();
        self.stroke_cancel();
        let id = self.active.clone();
        let clip = self.clip.clone();
        let Some(layer) = self.doc.layer_mut(&id).filter(|l| !l.locked) else {
            return Ok(());
        };
        let original = layer.pixels.clone();
        let Some(content) = original.content_bounds() else {
            return Ok(());
        };
        let bounds = match clip.as_ref().and_then(|m| m.bounds()) {
            Some((x, y, w, h)) => {
                match content.intersect(&Rect::new(x as i32, y as i32, w as i32, h as i32)) {
                    Some(b) => b,
                    None => return Ok(()),
                }
            }
            None => content,
        };
        let mut src = original.read_rect(bounds);
        if let Some(m) = &clip {
            // Lift layer × mask; leave layer × (1 − mask).
            for y in bounds.y..bounds.bottom() {
                for x in bounds.x..bounds.right() {
                    let cov = m.coverage(x, y);
                    let i = (((y - bounds.y) * bounds.w + (x - bounds.x)) * 4) as usize;
                    for c in 0..4 {
                        src[i + c] *= cov;
                    }
                }
            }
            let rest: Vec<f32> = original
                .read_rect(bounds)
                .iter()
                .zip(&src)
                .map(|(o, l)| o - l)
                .collect();
            layer.pixels.write_rect(bounds, &rest);
            for (tx, ty) in bounds.tiles() {
                layer.pixels.prune_tile(tx, ty);
            }
        } else {
            layer.pixels.clear();
        }
        let saved_selection = self.selection.clone();
        let mut f = Floating {
            layer: id,
            original,
            src,
            bounds,
            matrix: Affine::IDENTITY,
            preview: Surface::new(),
            preview_bounds: bounds,
            saved_selection,
        };
        f.render(self.doc.bounds());
        let lifted_selection = f.saved_selection.is_some();
        self.transform = Some(f);
        if lifted_selection {
            // happypaint clears the marquee while transforming (PE:2016).
            self.set_selection_raw(None);
        }
        self.mark_dirty(bounds);
        Ok(())
    }

    pub(crate) fn set_transform(&mut self, m: Affine) {
        let doc = self.doc.bounds();
        let Some(f) = self.transform.as_mut() else {
            return;
        };
        let old = f.preview_bounds;
        f.matrix = m;
        f.render(doc);
        let new = f.preview_bounds;
        self.mark_dirty(old.union(&new));
    }

    pub(crate) fn cancel_transform(&mut self) {
        let Some(f) = self.transform.take() else {
            return;
        };
        if let Some(l) = self.doc.layer_mut(&f.layer) {
            l.pixels = f.original;
        }
        if f.saved_selection.is_some() {
            self.set_selection_raw(f.saved_selection.clone());
        }
        self.mark_dirty(f.bounds.union(&f.preview_bounds));
    }

    pub(crate) fn commit_transform(&mut self) {
        let Some(f) = self.transform.take() else {
            return;
        };
        let Some(layer) = self.doc.layer_mut(&f.layer) else {
            return;
        };
        let mut keys: BTreeSet<(i32, i32)> = f.preview.tile_keys().into_iter().collect();
        keys.extend(f.original.tile_keys());
        let mut changes: Vec<TileChange> = Vec::new();
        let mut bytes = 0;
        for (tx, ty) in keys {
            let mut buf = layer
                .pixels
                .tile(tx, ty)
                .map(|t| t.to_f32())
                .unwrap_or_else(|| vec![0.0; TILE_PX * 4]);
            if let Some(p) = f.preview.tile(tx, ty) {
                let s = p.to_f32();
                for i in (0..buf.len()).step_by(4) {
                    let k = 1.0 - s[i + 3];
                    for c in 0..4 {
                        buf[i + c] = s[i + c] + buf[i + c] * k;
                    }
                }
            }
            let t = Tile::from_f32(&buf);
            let after = (!t.is_empty()).then(|| Arc::new(t));
            layer.pixels.set_tile(tx, ty, after.clone());
            let before = f.original.tile(tx, ty).cloned();
            if before.as_deref() != after.as_deref() {
                bytes += tile_bytes(&before) + tile_bytes(&after);
                changes.push(((tx, ty), before, after));
            }
        }
        self.mark_dirty(f.bounds.union(&f.preview_bounds));
        if !changes.is_empty() {
            self.history.push(
                Change::Tiles {
                    layer: f.layer,
                    tiles: changes,
                },
                bytes,
            );
            self.doc.touch();
            self.history_changed();
        }
    }
}
