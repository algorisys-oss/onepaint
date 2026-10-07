//! Gradient tool and shape strokes (spec 12).

use super::*;
use onepaint_filters::gradient::{GradStop, Ramp, Shape as GShape, shape_t};
use onepaint_stroke::shape::{Shape, outline, sample_path};
use rayon::prelude::*;

impl Session {
    /// Start a gradient on the active layer (a stroke whose buffer is the gradient).
    pub fn gradient_begin(&mut self) {
        self.commit_filter();
        self.commit_transform();
        self.stroke_cancel();
        let Some(l) = self.doc.layer(&self.active).filter(|l| l.visible && !l.locked && l.adjustment.is_none()) else { return };
        let mode = if l.alpha_lock { StrokeMode::AlphaLock(BlendMode::Normal) } else { StrokeMode::Paint(BlendMode::Normal) };
        let brush = self.current_brush().clone();
        let opacity = brush.opacity as f32;
        self.stroke = Some(ActiveStroke {
            layer: self.active.clone(),
            engine: StrokeEngine::new(brush, [0.0; 3], 0),
            buffer: Surface::new(),
            mode,
            opacity,
            wash: false,
            noop: false,
            clip: None,
            mask: false,
        });
    }

    /// Re-render the gradient for a drag from `a` to `b` (doc px).
    pub fn gradient_update(&mut self, a: [f64; 2], b: [f64; 2], shape: GShape, stops: &[GradStop]) {
        let doc = self.doc.bounds();
        let clip = self.clip.clone();
        let Some(st) = self.stroke.as_mut() else { return };
        let ramp = Ramp::new(stops);
        let keys: Vec<_> = doc.tiles().collect();
        let tiles: Vec<_> = keys
            .par_iter()
            .filter_map(|&(tx, ty)| {
                let ix = doc.intersect(&Rect::from_tile(tx, ty))?;
                let mut buf = vec![0.0f32; TILE_PX * 4];
                for y in ix.y..ix.bottom() {
                    for x in ix.x..ix.right() {
                        let c = ramp.sample(shape_t(shape, [x as f64 + 0.5, y as f64 + 0.5], a, b));
                        let k = clip.as_ref().map_or(1.0, |m| m.coverage(x, y));
                        let al = c[3] * k;
                        let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
                        buf[i..i + 4].copy_from_slice(&[c[0] * al, c[1] * al, c[2] * al, al]);
                    }
                }
                let t = Tile::from_f32(&buf);
                (!t.is_empty()).then(|| ((tx, ty), Arc::new(t)))
            })
            .collect();
        st.buffer = Surface::new();
        for ((tx, ty), t) in tiles {
            st.buffer.set_tile(tx, ty, Some(t));
        }
        self.mark_all_dirty();
    }

    /// Flatten the gradient into the layer (one undo step).
    pub fn gradient_end(&mut self) {
        self.stroke_end();
    }

    /// Stroke a shape's outline with the current brush (pressure 1); with `fill`,
    /// fill it with the colour at brush opacity underneath first. One undo step.
    pub fn stroke_shape(&mut self, shape: &Shape, fill: bool) {
        let path = sample_path(shape, 2.0);
        if path.len() < 2 {
            return;
        }
        self.history.begin_transaction();
        if fill
            && self.tool != Tool::Eraser
            && let Some(poly) = outline(shape)
        {
            let (w, h) = (self.doc.width, self.doc.height);
            let region = onepaint_selection::rasterize_polygons(w, h, &[poly]);
            let sel = self.clip.clone();
            let c = self.color.to_linear_premul();
            let op = self.current_brush().opacity as f32;
            let alpha_lock = self.doc.layer(&self.active).is_some_and(|l| l.alpha_lock);
            if let Some((bx, by, bw, bh)) = region.bounds() {
                let id = self.active.clone();
                self.modify_layer(&id, Rect::new(bx as i32, by as i32, bw as i32, bh as i32), |x, y, p| {
                    let a = region.coverage(x, y) * op * sel.as_ref().map_or(1.0, |m| m.coverage(x, y));
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
        }
        let pts: Vec<StrokePoint> = path.iter().enumerate().map(|(i, p)| StrokePoint::at(p[0], p[1], 1.0, i as f64 * 4.0)).collect();
        self.stroke_begin(pts[0]);
        self.stroke_extend(&pts[1..]);
        self.stroke_end();
        self.history.end_transaction();
        self.history_changed();
    }
}
