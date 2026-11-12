//! Vector path layers and the path menu (spec 22). Specs live in the layer's
//! `paths` (verbatim JSON); the pixels are always their rendering.

use super::*;
use onepaint_compose::{StrokeMode, apply_stroke_px, apply_stroke_px_at};
use onepaint_selection::{Mask, Mode, rasterize_polygons};
use onepaint_stroke::bezier::{PathSpec, area_contours, flatten_subpath, simulated_pressure};

pub const PATH_DEFAULT_SEED: u32 = 0x5eed;

/// Which tool Stroke Path paints with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StrokePathTool {
    #[default]
    Brush,
    Eraser,
    Smudge,
    Mixer,
}

/// Resample a polyline to ~`step` px so smoothed brushes follow it exactly.
fn densify(poly: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(poly.len());
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let n = ((b[0] - a[0]).hypot(b[1] - a[1]) / step).ceil().max(1.0) as usize;
        out.extend((0..n).map(|k| {
            let t = k as f64 / n as f64;
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        }));
    }
    out.extend(poly.last().copied());
    out
}

pub(crate) fn read_paths(l: &Layer) -> Vec<PathSpec> {
    l.extra.get("paths").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn composite_onto(dst: &mut Surface, src: &Surface, opacity: f32, space: onepaint_color::Space) {
    composite_mode(dst, src, opacity, BlendMode::Normal, space);
}

fn composite_mode(dst: &mut Surface, src: &Surface, opacity: f32, mode: BlendMode, space: onepaint_color::Space) {
    for (tx, ty) in src.tile_keys() {
        let Some(s) = src.tile(tx, ty) else { continue };
        let s = s.to_f32();
        let mut d = dst.tile(tx, ty).map(|t| t.to_f32()).unwrap_or_else(|| vec![0.0; TILE_PX * 4]);
        for i in (0..d.len()).step_by(4) {
            if s[i + 3] == 0.0 {
                continue;
            }
            let p = (i / 4) as i32;
            let o = apply_stroke_px_at(
                space,
                [d[i], d[i + 1], d[i + 2], d[i + 3]],
                [s[i], s[i + 1], s[i + 2], s[i + 3]],
                opacity,
                StrokeMode::Paint(mode),
                tx * TILE + p % TILE,
                ty * TILE + p / TILE,
            );
            d[i..i + 4].copy_from_slice(&o);
        }
        dst.set_tile(tx, ty, Some(Arc::new(Tile::from_f32(&d))));
    }
}

/// Render path objects: each fill (even-odd, under), then its stroke through the brush.
pub fn render_paths(specs: &[PathSpec], w: u32, h: u32, space: onepaint_color::Space) -> Surface {
    let mut out = Surface::new();
    for spec in specs {
        if let Some(f) = &spec.fill {
            let contours: Vec<Vec<[f64; 2]>> = spec
                .subpaths
                .iter()
                .filter(|s| s.closed && s.anchors.len() >= 3)
                .map(|s| flatten_subpath(s, 2.0))
                .filter(|p| p.len() >= 3)
                .collect();
            if !contours.is_empty() {
                let cov = rasterize_polygons(w, h, &contours);
                let c = space.pixel(Rgba::new(f.color[0], f.color[1], f.color[2], 1.0));
                let mut buf = Surface::new();
                let data: Vec<f32> = cov
                    .data()
                    .iter()
                    .flat_map(|&v| {
                        let a = v as f32 / 255.0;
                        [c[0] * a, c[1] * a, c[2] * a, a]
                    })
                    .collect();
                buf.write_rect(Rect::new(0, 0, w as i32, h as i32), &data);
                buf.prune_empty();
                composite_onto(&mut out, &buf, f.opacity.clamp(0.0, 1.0), space);
            }
        }
        let Some(st) = &spec.stroke else { continue };
        let mut brush = onepaint_brush::preset(&st.brush_id).unwrap_or_else(|| onepaint_brush::presets().remove(0));
        brush.size = st.size;
        brush.opacity = 1.0;
        brush.smoothing = 0.0;
        brush.stabilizer = None;
        let lin = space.pixel(Rgba::new(st.color[0], st.color[1], st.color[2], 1.0));
        for sub in &spec.subpaths {
            let poly = flatten_subpath(sub, 2.0);
            if poly.len() < 2 {
                continue;
            }
            let mut eng = StrokeEngine::new(brush.clone(), [lin[0], lin[1], lin[2]], st.seed.unwrap_or(PATH_DEFAULT_SEED));
            eng.set_space(space);
            let pt = |p: [f64; 2]| StrokePoint { x: p[0], y: p[1], pressure: 1.0, ..Default::default() };
            let mut dabs = eng.begin(pt(poly[0]));
            for p in &poly[1..] {
                dabs.extend(eng.add(pt(*p)));
            }
            dabs.extend(eng.end());
            let mut buf = Surface::new();
            for d in &dabs {
                eng.rasterize(&mut buf, d, None);
            }
            composite_mode(&mut out, &buf, st.opacity.clamp(0.0, 1.0), brush.blend_mode(), space);
        }
    }
    out.prune_empty();
    out
}

pub(crate) fn write_paths(l: &mut Layer, specs: &[PathSpec], w: u32, h: u32, space: onepaint_color::Space) {
    if specs.is_empty() {
        l.extra.remove("paths");
    } else {
        l.extra.insert("paths".into(), serde_json::to_value(specs).unwrap_or_default());
    }
    l.pixels = render_paths(specs, w, h, space);
}

impl Session {
    pub fn paths(&self, id: &str) -> Vec<PathSpec> {
        self.doc.layer(id).map(read_paths).unwrap_or_default()
    }

    pub fn is_path_layer(&self, id: &str) -> bool {
        self.doc.layer(id).is_some_and(|l| l.extra.contains_key("paths"))
    }

    pub(crate) fn create_path_layer(&mut self, spec: PathSpec) -> Result<(), EngineError> {
        let (w, h, space) = (self.doc.width, self.doc.height, self.doc.space());
        let mut l = Layer::new("Path");
        write_paths(&mut l, &[spec], w, h, space);
        self.insert_above_active(Node::Layer(l))
    }

    pub(crate) fn add_path_object(&mut self, id: &str, spec: PathSpec) -> Result<(), EngineError> {
        if !self.is_path_layer(id) {
            return Err(EngineError::NoSuchLayer(id.into()));
        }
        let (w, h, space) = (self.doc.width, self.doc.height, self.doc.space());
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                let mut specs = read_paths(l);
                specs.push(spec);
                write_paths(l, &specs, w, h, space);
            }
            Ok(())
        })
    }

    /// Replace (live edits coalesce per object) or remove (`None`) one object.
    pub(crate) fn update_path_object(&mut self, id: &str, index: usize, spec: Option<PathSpec>) -> Result<(), EngineError> {
        if self.paths(id).len() <= index {
            return Err(EngineError::NoSuchLayer(format!("{id} path {index}")));
        }
        let (w, h, space) = (self.doc.width, self.doc.height, self.doc.space());
        let key = spec.is_some().then(|| format!("path:{id}:{index}"));
        self.tree_edit(key, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                let mut specs = read_paths(l);
                match spec {
                    Some(sp) => specs[index] = sp,
                    None => {
                        specs.remove(index);
                    }
                }
                write_paths(l, &specs, w, h, space);
            }
            Ok(())
        })
    }

    pub(crate) fn rasterize_path(&mut self, id: &str) -> Result<(), EngineError> {
        if !self.is_path_layer(id) {
            return Ok(());
        }
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.extra.remove("paths");
            }
            Ok(())
        })
    }

    /// The plain paint layer directly below the active path layer.
    pub fn path_op_target(&self) -> Option<NodeId> {
        let parent = self.doc.parent_of(&self.active)?;
        let siblings = self.doc.children(parent.as_deref())?;
        let i = siblings.iter().position(|n| n.id() == self.active)?;
        let below = siblings.get(i.checked_sub(1)?)?.as_layer()?;
        let plain = below.adjustment.is_none() && !below.extra.contains_key("texts") && !below.extra.contains_key("paths");
        plain.then(|| below.id.clone())
    }

    fn path_coverage(&self) -> Option<Mask> {
        let contours = area_contours(&self.paths(&self.active));
        (!contours.is_empty()).then(|| rasterize_polygons(self.doc.width, self.doc.height, &contours))
    }

    pub(crate) fn select_from_path(&mut self, mode: Mode) -> Result<(), EngineError> {
        let Some(mut cov) = self.path_coverage() else { return Ok(()) };
        cov.data_mut().iter_mut().for_each(|v| *v = if *v >= 128 { 255 } else { 0 });
        let m = self.region_combined(&cov, mode);
        self.commit_selection(Some(m));
        Ok(())
    }

    pub(crate) fn fill_path(&mut self) -> Result<(), EngineError> {
        let target = self.path_op_target().ok_or_else(|| EngineError::Invalid("no paint layer below the path".into()))?;
        let Some(cov) = self.path_coverage() else { return Ok(()) };
        let sel = self.clip.clone();
        let c = self.doc.space().pixel(self.color);
        let (w, h) = (self.doc.width, self.doc.height);
        self.tree_edit(None, |s| {
            let Some(l) = s.doc.layer_mut(&target).filter(|l| !l.locked) else { return Ok(()) };
            let mut buf = Surface::new();
            let data: Vec<f32> = cov
                .data()
                .iter()
                .enumerate()
                .flat_map(|(i, &v)| {
                    let k = sel.as_ref().map_or(1.0, |m| m.data()[i] as f32 / 255.0);
                    let a = v as f32 / 255.0 * k;
                    [c[0] * a, c[1] * a, c[2] * a, a]
                })
                .collect();
            buf.write_rect(Rect::new(0, 0, w as i32, h as i32), &data);
            buf.prune_empty();
            let mode = if l.alpha_lock { StrokeMode::AlphaLock(BlendMode::Normal) } else { StrokeMode::Paint(BlendMode::Normal) };
            for (tx, ty) in buf.tile_keys() {
                let Some(src) = buf.tile(tx, ty) else { continue };
                let s32 = src.to_f32();
                let mut d = l.pixels.tile(tx, ty).map(|t| t.to_f32()).unwrap_or_else(|| vec![0.0; TILE_PX * 4]);
                for i in (0..d.len()).step_by(4) {
                    let o = apply_stroke_px([d[i], d[i + 1], d[i + 2], d[i + 3]], [s32[i], s32[i + 1], s32[i + 2], s32[i + 3]], 1.0, mode);
                    d[i..i + 4].copy_from_slice(&o);
                }
                l.pixels.set_tile(tx, ty, Some(Arc::new(Tile::from_f32(&d))));
            }
            Ok(())
        })
    }

    /// Create Vector Mask (happypaint `maskFromPaths`): the active path
    /// layer's paths become the vector mask of the paint layer below,
    /// replacing any vector mask it had; its pixel mask stays.
    pub(crate) fn mask_from_path(&mut self) -> Result<(), EngineError> {
        let target = self.path_op_target().ok_or_else(|| {
            EngineError::Invalid("Create Vector Mask masks the paint layer directly below the path — there isn’t one".into())
        })?;
        let v =
            vector_mask_from_paths(&self.paths(&self.active)).ok_or_else(|| EngineError::Invalid("That path encloses no area".into()))?;
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(&target) {
                l.set_vector_mask(Some(v));
            }
            Ok(())
        })
    }

    /// Stroke each subpath onto the target layer with a painting tool, as one step.
    pub(crate) fn stroke_path(&mut self, tool: StrokePathTool, simulate_pressure: bool) -> Result<(), EngineError> {
        let target = self.path_op_target().ok_or_else(|| EngineError::Invalid("no paint layer below the path".into()))?;
        let specs = self.paths(&self.active);
        let (active, prev_tool) = (self.active.clone(), self.tool);
        self.active = target;
        self.tool = match tool {
            StrokePathTool::Brush => Tool::Brush,
            StrokePathTool::Eraser => Tool::Eraser,
            StrokePathTool::Smudge => Tool::Smudge,
            StrokePathTool::Mixer => Tool::Mixer,
        };
        self.history.begin_transaction();
        for spec in &specs {
            for sub in &spec.subpaths {
                let poly = densify(&flatten_subpath(sub, 2.0), 2.0);
                if poly.len() < 2 {
                    continue;
                }
                let pressure = if simulate_pressure { simulated_pressure(&poly) } else { vec![1.0; poly.len()] };
                let pts: Vec<StrokePoint> = poly
                    .iter()
                    .zip(&pressure)
                    .enumerate()
                    .map(|(i, (p, &pr))| StrokePoint { x: p[0], y: p[1], pressure: pr, t: Some(i as f64), ..Default::default() })
                    .collect();
                self.stroke_begin(pts[0]);
                self.stroke_extend(&pts[1..]);
                self.stroke_end();
            }
        }
        self.history.end_transaction();
        self.active = active;
        self.tool = prev_tool;
        self.history_changed();
        Ok(())
    }
}

impl Session {
    /// Brush fill mode (PE:1779): the stroke's silhouette filled with the
    /// current colour at `opacity`, replicated through symmetry, confined to
    /// the selection, honouring alpha lock; one undo step.
    pub(crate) fn fill_shape(&mut self, points: Vec<[f64; 2]>, opacity: f32) -> Result<(), EngineError> {
        if points.len() < 3 {
            return Ok(());
        }
        self.ensure_raster();
        let Some(l) = self.doc.layer(&self.active) else { return Ok(()) };
        if !l.visible || l.locked || l.adjustment.is_some() {
            return Ok(());
        }
        let (w, h) = (self.doc.width, self.doc.height);
        let tfs = self.symmetry_transforms();
        let polys: Vec<Vec<[f64; 2]>> = if tfs.is_empty() {
            vec![points]
        } else {
            tfs.iter()
                .map(|t| {
                    points
                        .iter()
                        .map(|p| {
                            let q = t.apply(p[0], p[1]);
                            [q.0, q.1]
                        })
                        .collect()
                })
                .collect()
        };
        let mut cov = vec![0u8; (w * h) as usize];
        for poly in &polys {
            let m = rasterize_polygons(w, h, std::slice::from_ref(poly));
            cov.iter_mut().zip(m.data()).for_each(|(c, &v)| *c = (*c).max(v));
        }
        let sel = self.clip.clone();
        let c = self.doc.space().pixel(self.color);
        let id = self.active.clone();
        self.tree_edit(None, |s| {
            let Some(l) = s.doc.layer_mut(&id) else { return Ok(()) };
            let mode = if l.alpha_lock { StrokeMode::AlphaLock(BlendMode::Normal) } else { StrokeMode::Paint(BlendMode::Normal) };
            let mut buf = Surface::new();
            let data: Vec<f32> = cov
                .iter()
                .enumerate()
                .flat_map(|(i, &v)| {
                    let k = sel.as_ref().map_or(1.0, |m| m.data()[i] as f32 / 255.0);
                    let a = v as f32 / 255.0 * k;
                    [c[0] * a, c[1] * a, c[2] * a, a]
                })
                .collect();
            buf.write_rect(Rect::new(0, 0, w as i32, h as i32), &data);
            buf.prune_empty();
            for (tx, ty) in buf.tile_keys() {
                let Some(src) = buf.tile(tx, ty) else { continue };
                let s32 = src.to_f32();
                let mut d = l.pixels.tile(tx, ty).map(|t| t.to_f32()).unwrap_or_else(|| vec![0.0; TILE_PX * 4]);
                for i in (0..d.len()).step_by(4) {
                    let o =
                        apply_stroke_px([d[i], d[i + 1], d[i + 2], d[i + 3]], [s32[i], s32[i + 1], s32[i + 2], s32[i + 3]], opacity, mode);
                    d[i..i + 4].copy_from_slice(&o);
                }
                l.pixels.set_tile(tx, ty, Some(Arc::new(Tile::from_f32(&d))));
            }
            Ok(())
        })
    }
}

/// Pen paths as a vector mask (`vectorMaskFromPathSpecs`): each subpath of
/// two or more anchors becomes a combined, even-odd path with absolute
/// handles (a missing handle sits on its anchor). Like happypaint, an open
/// subpath is filled as closed through its end handles.
pub fn vector_mask_from_paths(specs: &[PathSpec]) -> Option<onepaint_doc::VectorMask> {
    use onepaint_doc::{VectorMask, VectorPath};
    let paths: Vec<VectorPath> = specs
        .iter()
        .flat_map(|p| &p.subpaths)
        .filter(|sub| sub.anchors.len() >= 2)
        .map(|sub| VectorPath {
            knots: sub
                .anchors
                .iter()
                .map(|a| {
                    [
                        a.x + a.in_x.unwrap_or(0.0),
                        a.y + a.in_y.unwrap_or(0.0),
                        a.x,
                        a.y,
                        a.x + a.out_x.unwrap_or(0.0),
                        a.y + a.out_y.unwrap_or(0.0),
                    ]
                })
                .collect(),
            ..Default::default()
        })
        .collect();
    (!paths.is_empty()).then(|| VectorMask { paths, ..Default::default() })
}
