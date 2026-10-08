//! The engine facade: one [`Session`] per open document.
//!
//! The UI talks to it through serialisable [`Command`]s, the stroke API and
//! drained [`Event`]s, mirroring happypaint's store handlers, `PaintSink`
//! and engine callbacks. No UI or GPU code lives here.

pub mod anim;
mod canvas_ops;
mod clipboard;
mod command;
mod filter_ops;
pub mod guides;
pub use command::Command;
mod height_ops;
mod liquify_ops;
mod mask_ops;
mod paint_ops;
mod panel_ops;
pub mod path_ops;
pub mod recording;
mod saved_selection_ops;
mod selection_ops;
pub mod smart_filter_ops;
mod smudge_ops;
mod text_ops;
pub mod transform;
mod wet_ops;
pub use guides::Guides;
pub use onepaint_brush::symmetry::{Symmetry, SymmetryMode};
pub use saved_selection_ops::SavedSelection;
pub use smudge_ops::{MixerSettings, PaintSettings};
pub mod view;
pub mod warp;

pub use clipboard::Region;
pub use onepaint_selection::{Mask, Mode as SelectMode};

use onepaint_brush::{BrushPreset, StrokeEngine};
use onepaint_color::Rgba;
use onepaint_compose::{StrokeMode, StrokeOverlay, apply_stroke_px, composite_tile};
use onepaint_doc::{BlendMode, Document, Group, Layer, Node, NodeId};
use onepaint_history::History;
use onepaint_raster::{Rect, Surface, TILE, TILE_PX, Tile};
use onepaint_stroke::StrokePoint;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Tool {
    #[default]
    Brush,
    Eraser,
    Fill,
    Eyedropper,
    SelectRect,
    SelectEllipse,
    SelectLasso,
    SelectPoly,
    SelectWand,
    Move,
    Gradient,
    Shape,
    Text,
    Smudge,
    Mixer,
    Liquify,
    Pen,
    /// Hand: drags pan the view.
    Hand,
    /// Comic panels: draw, cut, reshape and move frames (spec 30).
    Panel,
}

/// Optional layer property edits; `None` leaves a property unchanged.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LayerProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend_mode: Option<BlendMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clipped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_lock: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
}

impl LayerProps {
    fn key(&self) -> String {
        let mut k = String::new();
        for (set, name) in [
            (self.name.is_some(), "name"),
            (self.visible.is_some(), "visible"),
            (self.opacity.is_some(), "opacity"),
            (self.blend_mode.is_some(), "blend"),
            (self.clipped.is_some(), "clipped"),
            (self.alpha_lock.is_some(), "alpha"),
            (self.locked.is_some(), "locked"),
            (self.collapsed.is_some(), "collapsed"),
        ] {
            if set {
                k.push_str(name);
            }
        }
        k
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Composite tiles changed; call [`Session::take_dirty_tiles`].
    NeedsRender,
    LayersChanged,
    ActiveLayerChanged,
    HistoryChanged {
        can_undo: bool,
        can_redo: bool,
    },
    /// The whole document was replaced (new size, new layers).
    DocumentReplaced,
    SelectionChanged,
    SavedSelectionsChanged,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EngineError {
    #[error("no such layer: {0}")]
    NoSuchLayer(String),
    #[error("can't delete the last layer")]
    LastLayer,
    #[error("invalid move")]
    InvalidMove,
    #[error("unknown brush preset: {0}")]
    UnknownPreset(String),
    #[error("nothing below to merge into")]
    NothingBelow,
    #[error("{0}")]
    Format(String),
    #[error("{0}")]
    Invalid(String),
}

impl From<onepaint_formats::FormatError> for EngineError {
    fn from(e: onepaint_formats::FormatError) -> Self {
        EngineError::Format(e.to_string())
    }
}

/// App version written into saved files.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) type TileChange = ((i32, i32), Option<Arc<Tile>>, Option<Arc<Tile>>);

/// Which surface of a layer a tile change belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Plane {
    Pixels,
    Mask,
    /// The impasto height field (spec 35).
    Height,
}

impl Plane {
    pub(crate) fn of(mask: bool) -> Self {
        if mask { Plane::Mask } else { Plane::Pixels }
    }
}

/// Tile changes between a snapshot and the current surface, over `keys`
/// (unchanged tiles share their `Arc` and are skipped). Returns them and their bytes.
pub(crate) fn tile_diff(before: &Surface, after: &Surface, keys: impl IntoIterator<Item = (i32, i32)>) -> (Vec<TileChange>, usize) {
    let mut out = Vec::new();
    let mut bytes = 0;
    for (tx, ty) in keys {
        let (b, a) = (before.tile(tx, ty).cloned(), after.tile(tx, ty).cloned());
        let same = match (&b, &a) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            (None, None) => true,
            _ => false,
        };
        if !same {
            bytes += tile_bytes(&b) + tile_bytes(&a);
            out.push(((tx, ty), b, a));
        }
    }
    (out, bytes)
}

pub(crate) enum Change {
    Tiles { layer: NodeId, plane: Plane, tiles: Vec<TileChange> },
    Tree { before: Vec<Node>, after: Vec<Node>, active_before: NodeId, active_after: NodeId, key: Option<String> },
    Selection { before: Option<Arc<onepaint_selection::Mask>>, after: Option<Arc<onepaint_selection::Mask>> },
    Document { before: Box<canvas_ops::DocShape>, after: Box<canvas_ops::DocShape> },
    GuideLines { before: Vec<guides::GuideLine>, after: Vec<guides::GuideLine> },
    SavedSelections { before: Vec<SavedSelection>, after: Vec<SavedSelection> },
}

struct ActiveStroke {
    layer: NodeId,
    engine: StrokeEngine,
    buffer: Surface,
    mode: StrokeMode,
    opacity: f32,
    /// Alpha-locked erase: draw nothing, record nothing.
    noop: bool,
    /// The selection (feathered) that clips this stroke.
    clip: Option<Arc<onepaint_selection::Mask>>,
    /// Painting into the layer mask.
    mask: bool,
    /// Relief per unit of dab alpha (`pigmentBody`; 0 = flat paint).
    impasto: f64,
    /// The layer's height field, when this stroke builds or carves it.
    height: Option<height_ops::HeightEdit>,
}

/// App preferences the engine honours (happypaint `settings-store.ts`); not
/// part of the document or its history.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preferences {
    /// Upper limit for brush size, doc px (10..=10000; default 1000).
    pub max_brush_size: f64,
    /// Pen pressure fully drives size / flow for any brush (Photoshop's override).
    pub force_size_pressure: bool,
    pub force_flow_pressure: bool,
    /// Brush size is in screen px: strokes divide it by the zoom (`Some(zoom)`).
    pub screen_zoom: Option<f64>,
    /// Undo steps kept (5..=1000; default 200).
    pub max_history_steps: usize,
}

pub const BRUSH_SIZE_HARD_MAX: f64 = 10000.0;

impl Default for Preferences {
    fn default() -> Self {
        Self {
            max_brush_size: 1000.0,
            force_size_pressure: false,
            force_flow_pressure: false,
            screen_zoom: None,
            max_history_steps: onepaint_history::DEFAULT_MAX_ENTRIES,
        }
    }
}

impl Preferences {
    /// The brush a stroke actually paints with.
    pub fn stroke_brush(&self, b: &BrushPreset) -> BrushPreset {
        let mut out = b.clone();
        if let Some(z) = self.screen_zoom.filter(|z| *z > 0.0) {
            out.size = (out.size / z).max(0.5);
        }
        // Forcing = full pressure response with no dynamics: `radius_for`
        // becomes size·max(0.15, cp)/2 and `flow_for` flow·cp (BE:358-368).
        if self.force_size_pressure {
            out.size_pressure = 1.0;
            if let Some(d) = out.dynamics.as_mut() {
                d.size.clear();
            }
        }
        if self.force_flow_pressure {
            out.flow_pressure = 1.0;
            if let Some(d) = out.dynamics.as_mut() {
                d.flow.clear();
            }
        }
        out
    }
}

pub struct Session {
    prefs: Preferences,
    doc: Document,
    active: NodeId,
    tool: Tool,
    brush: BrushPreset,
    eraser: BrushPreset,
    color: Rgba,
    history: History<Change>,
    stroke: Option<ActiveStroke>,
    events: Vec<Event>,
    dirty: BTreeSet<(i32, i32)>,
    seed: u32,
    selection: Option<Arc<onepaint_selection::Mask>>,
    feather: f32,
    clip: Option<Arc<onepaint_selection::Mask>>,
    selection_version: u64,
    transform: Option<transform::Floating>,
    filter: Option<filter_ops::FilterSession>,
    /// The layer whose mask strokes paint into (only while it is active).
    mask_target_layer: Option<NodeId>,
    paint: PaintSettings,
    guides: Guides,
    /// Guide lines before the edit in progress (one undo step per edit).
    guide_edit_before: Option<Vec<guides::GuideLine>>,
    saved: Vec<SavedSelection>,
    /// The next coalescing edit starts a new undo step (end of a drag).
    history_sealed: bool,
    anim: anim::AnimSettings,
    /// Animation was enabled at some point (settings are saved from then on).
    anim_ever: bool,
    anim_slot: usize,
    anim_playing: bool,
    smart_cache: smart_filter_ops::SmartCache,
    spatial_cache: smart_filter_ops::SpatialCache,
    recorder: Option<recording::Recorder>,
    /// Nesting of `execute`: only top-level calls are recorded.
    rec_depth: u32,
    smudge: Option<smudge_ops::ActiveSmudge>,
    liquify: Option<liquify_ops::ActiveLiquify>,
    /// Live wet media: the drying session and the fluid stroke feeding it.
    wet: Option<wet_ops::WetSession>,
    wet_stroke: Option<wet_ops::WetStroke>,
    wet_tilt: [f32; 2],
    /// The Mixer's kept reservoir, keyed by the colour it was loaded with.
    mixer_held: Option<(String, onepaint_brush::smudge::Reservoir)>,
}

fn tile_bytes(t: &Option<Arc<Tile>>) -> usize {
    t.as_ref().map_or(0, |_| TILE_PX * 8)
}

impl Session {
    pub fn new(width: u32, height: u32, background: Option<Rgba>) -> Self {
        Self::from_document(Document::new(width, height, background))
    }

    pub fn from_document(doc: Document) -> Self {
        let active = doc.layer_ids().last().cloned().or_else(|| doc.root.last().map(|n| n.id().to_string())).unwrap_or_default();
        let mut s = Self {
            doc,
            active,
            tool: Tool::Brush,
            brush: onepaint_brush::preset(onepaint_brush::DEFAULT_BRUSH).unwrap_or_else(|| onepaint_brush::presets().remove(0)),
            eraser: onepaint_brush::preset(onepaint_brush::DEFAULT_ERASER).unwrap_or_else(|| onepaint_brush::presets().remove(0)),
            color: Rgba::BLACK,
            history: History::new(),
            prefs: Preferences::default(),
            stroke: None,
            events: Vec::new(),
            dirty: BTreeSet::new(),
            seed: 1,
            selection: None,
            feather: 0.0,
            clip: None,
            selection_version: 0,
            transform: None,
            filter: None,
            mask_target_layer: None,
            paint: PaintSettings::default(),
            guides: Guides::default(),
            guide_edit_before: None,
            saved: Vec::new(),
            history_sealed: false,
            anim: Default::default(),
            anim_ever: false,
            anim_slot: 0,
            anim_playing: false,
            smart_cache: Default::default(),
            spatial_cache: Default::default(),
            recorder: None,
            rec_depth: 0,
            smudge: None,
            liquify: None,
            wet: None,
            wet_stroke: None,
            wet_tilt: [0.0; 2],
            mixer_held: None,
        };
        s.load_saved_selections();
        s.load_anim();
        s.guides = s.doc.extra.get("guides").and_then(|v| serde_json::from_value::<Guides>(v.clone()).ok()).unwrap_or_default().sanitized();
        s.mark_all_dirty();
        s
    }

    /// Replace the document (open / new), clearing history.
    pub fn replace_document(&mut self, doc: Document) {
        let (tool, brush, eraser, color, paint) = (self.tool, self.brush.clone(), self.eraser.clone(), self.color, self.paint);
        *self = Self::from_document(doc);
        (self.tool, self.brush, self.eraser, self.color, self.paint) = (tool, brush, eraser, color, paint);
        self.events.push(Event::DocumentReplaced);
        self.events.push(Event::LayersChanged);
        self.history_changed();
    }

    /// Take the tool, brushes, colour and painting options of another session
    /// (switching drawings keeps what you were painting with).
    pub fn carry_tools_from(&mut self, other: &Session) {
        (self.tool, self.brush, self.eraser, self.color, self.paint) =
            (other.tool, other.brush.clone(), other.eraser.clone(), other.color, other.paint);
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    pub fn doc_mut_meta(&mut self) -> &mut Document {
        &mut self.doc
    }

    pub fn active(&self) -> &str {
        &self.active
    }

    pub fn tool(&self) -> Tool {
        self.tool
    }

    pub fn preferences(&self) -> Preferences {
        self.prefs
    }

    pub fn set_preferences(&mut self, p: Preferences) {
        let mut p = p;
        p.max_brush_size = p.max_brush_size.round().clamp(10.0, BRUSH_SIZE_HARD_MAX);
        p.max_history_steps = p.max_history_steps.clamp(5, 1000);
        self.history.set_max_entries(p.max_history_steps);
        self.prefs = p;
        for b in [&mut self.brush, &mut self.eraser] {
            b.size = b.size.min(p.max_brush_size);
        }
    }

    pub fn color(&self) -> Rgba {
        self.color
    }

    /// The eraser preset for the eraser tool, otherwise the brush preset.
    pub fn current_brush(&self) -> &BrushPreset {
        match self.tool {
            Tool::Eraser => &self.eraser,
            _ => &self.brush,
        }
    }

    fn current_brush_mut(&mut self) -> &mut BrushPreset {
        match self.tool {
            Tool::Eraser => &mut self.eraser,
            _ => &mut self.brush,
        }
    }

    pub fn history_len(&self) -> usize {
        self.history.undo_len()
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn is_stroking(&self) -> bool {
        self.stroke.is_some() || self.smudge.is_some() || self.liquify.is_some() || self.wet_stroke.is_some()
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    // ── Rendering ──

    fn mark_all_dirty(&mut self) {
        self.dirty.extend(self.doc.bounds().tiles());
        self.events.push(Event::NeedsRender);
    }

    fn mark_dirty(&mut self, r: Rect) {
        if let Some(r) = r.intersect(&self.doc.bounds()) {
            self.dirty.extend(r.tiles());
            self.events.push(Event::NeedsRender);
        }
    }

    /// Composite tiles that changed since the last call.
    pub fn take_dirty_tiles(&mut self) -> Vec<(i32, i32)> {
        std::mem::take(&mut self.dirty).into_iter().collect()
    }

    fn overlay(&self) -> Option<StrokeOverlay<'_>> {
        if let Some(t) = self.transform_overlay() {
            return Some(t);
        }
        self.stroke.as_ref().filter(|s| !s.noop).map(|s| StrokeOverlay {
            mask: s.mask,
            layer_id: &s.layer,
            stroke: &s.buffer,
            opacity: s.opacity,
            mode: s.mode,
        })
    }

    /// Composite the given tiles, including any stroke in progress.
    pub fn composite(&self, keys: &[(i32, i32)]) -> Vec<((i32, i32), Tile)> {
        // Wet paint shows on screen while it dries (its preview has the damp veil, so it stays out of exports).
        let ov = self.overlay().or_else(|| self.wet_overlay());
        onepaint_compose::composite_tiles(&self.display_doc(), keys, ov.as_ref())
    }

    /// Flattened canvas as premultiplied linear f32.
    pub fn flatten(&self) -> Vec<f32> {
        if self.has_smart_filters() || Self::has_spatial_adjustments(&self.doc.root) {
            let mut d = self.doc.clone();
            self.apply_smart_filters(&mut d.root);
            self.render_spatial_adjustments(&mut d.root, true);
            return onepaint_compose::flatten(&d, self.overlay().as_ref());
        }
        onepaint_compose::flatten(&self.doc, self.overlay().as_ref())
    }

    /// Eyedropper: the composite colour at a canvas pixel, or `None` outside the
    /// canvas or where it is fully transparent (happypaint `pickColor`).
    pub fn sample_color(&self, x: i32, y: i32) -> Option<Rgba> {
        if x < 0 || y < 0 || x as u32 >= self.doc.width || y as u32 >= self.doc.height {
            return None;
        }
        let (tx, ty) = onepaint_raster::tile_of(x, y);
        let b = composite_tile(&self.display_doc(), tx, ty, self.overlay().as_ref());
        let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
        if b[i + 3] <= 0.0 {
            return None;
        }
        let mut c = Rgba::from_linear_premul([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        c.a = 1.0;
        Some(c)
    }

    /// Eyedropper on the composite.
    pub fn pick_color(&self, x: i32, y: i32) -> Rgba {
        let (tx, ty) = onepaint_raster::tile_of(x, y);
        let b = composite_tile(&self.doc, tx, ty, self.overlay().as_ref());
        let i = (((y - ty * TILE) * TILE + (x - tx * TILE)) * 4) as usize;
        let mut c = Rgba::from_linear_premul([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        c.a = 1.0;
        c
    }

    // ── Files ──

    /// Open a `.happypaint` file, replacing the current document.
    pub fn open_happypaint(&mut self, bytes: &[u8]) -> Result<(), EngineError> {
        let doc = onepaint_formats::read_happypaint(bytes)?;
        self.replace_document(doc);
        Ok(())
    }

    /// Open a PNG/JPEG/WebP/BMP/GIF/TIFF as a new single-layer document.
    pub fn open_image(&mut self, bytes: &[u8], name: &str) -> Result<(), EngineError> {
        let img = onepaint_formats::decode_image(bytes)?;
        let mut doc = Document::new(img.width, img.height, None);
        doc.name = name.to_string();
        if let Some(l) = doc.root.first_mut().and_then(Node::as_layer_mut) {
            l.name = name.to_string();
            l.pixels = onepaint_formats::image_to_surface(&img, 0, 0);
        }
        self.replace_document(doc);
        Ok(())
    }

    /// Open a PSD/PSB (8/16-bit RGB or grey) as layers, replacing the current document.
    pub fn open_psd(&mut self, bytes: &[u8], name: &str) -> Result<(), EngineError> {
        let mut doc = onepaint_formats::psd::read_psd(bytes)?;
        doc.name = name.to_string();
        self.replace_document(doc);
        Ok(())
    }

    /// The document as an 8-bit RGB PSD with layers, groups, masks and a merged composite.
    pub fn export_psd(&mut self) -> Result<Vec<u8>, EngineError> {
        self.commit_transform();
        self.dry_wet();
        let flat = self.flatten();
        Ok(onepaint_formats::psd::write_psd(&self.doc, &flat)?)
    }

    /// The document's drawing guides.
    pub fn guides(&self) -> &Guides {
        &self.guides
    }

    /// Serialise the document as `.happypaint` v9 with a JPEG thumbnail.
    pub fn save_happypaint(&mut self) -> Result<Vec<u8>, EngineError> {
        self.commit_transform();
        self.dry_wet();
        if self.guides.configured() {
            self.doc.extra.insert("guides".into(), serde_json::to_value(&self.guides).unwrap_or_default());
        } else {
            self.doc.extra.remove("guides");
        }
        self.store_saved_selections();
        self.store_anim();
        self.doc.touch();
        let flat = self.flatten();
        let thumb = onepaint_formats::thumbnail_jpeg(&flat, self.doc.width, self.doc.height, 320, 240).ok();
        Ok(onepaint_formats::write_happypaint(&self.doc, thumb.as_deref(), APP_VERSION)?)
    }

    /// What you see (the current animation frame, without onion ghosts), encoded.
    pub fn export_image(&self, o: &onepaint_formats::export::ExportOptions) -> Result<Vec<u8>, EngineError> {
        let flat = if self.anim.enabled && anim::total_slots(&self.doc.root) > 0 {
            let mut v = anim::view_document(&self.doc, &self.anim, self.anim_slot, false, &self.active);
            self.apply_smart_filters(&mut v.root);
            self.render_spatial_adjustments(&mut v.root, true);
            onepaint_compose::flatten(&v, None)
        } else {
            self.flatten()
        };
        let o = onepaint_formats::export::ExportOptions { dpi: self.doc.dpi, ..*o };
        Ok(onepaint_formats::export::export_flat(&flat, self.doc.width, self.doc.height, &o)?.0)
    }

    /// The flattened canvas as an 8-bit straight-alpha sRGB PNG.
    pub fn export_png(&self) -> Result<Vec<u8>, EngineError> {
        let rgba = onepaint_formats::flatten_to_srgb8(&self.flatten());
        Ok(onepaint_formats::encode_png_rgba8(self.doc.width, self.doc.height, &rgba)?)
    }

    // ── Strokes (happypaint PaintSink) ──

    pub fn stroke_begin(&mut self, p: StrokePoint) {
        self.rec(|t| recording::RecEvent::Begin { t, p });
        self.commit_filter();
        self.commit_transform();
        if !self.mask_target() {
            self.ensure_raster();
        }
        self.stroke_cancel();
        // Fluid brushes feed the live wet simulation; any other stroke dries it first.
        if self.wet_brush() {
            self.wet_begin(p);
            return;
        }
        self.dry_wet();
        // Wet presets paint through the mixer path (paint-canvas.tsx:5706-5758).
        let wet_brush = self.tool == Tool::Brush && !self.mask_target() && self.brush.wet().is_some();
        if matches!(self.tool, Tool::Smudge | Tool::Mixer) || wet_brush {
            self.smudge_begin(p);
            return;
        }
        if self.tool == Tool::Liquify {
            self.liquify_begin(p);
            return;
        }
        let Some(layer) = self.doc.layer(&self.active) else {
            return;
        };
        if layer.locked || !layer.visible {
            return;
        }
        let erase = self.tool == Tool::Eraser;
        let to_mask = self.mask_target();
        let noop = erase && layer.alpha_lock && !to_mask;
        // The brush's own blend mode (add, multiply, pigment…) applies to layer strokes.
        let bm = self.current_brush().blend_mode();
        let mode = match (to_mask, erase, layer.alpha_lock) {
            // Masks keep alpha 1: paint coverage with alpha locked (PE:798-801).
            (true, _, _) => StrokeMode::AlphaLock(BlendMode::Normal),
            (false, true, _) => StrokeMode::Erase,
            (false, false, true) => StrokeMode::AlphaLock(bm),
            (false, false, false) => StrokeMode::Paint(bm),
        };
        let brush = self.prefs.stroke_brush(self.current_brush());
        let lin = if to_mask {
            // Mask coverage = the brush colour's luminance (white for the eraser), no transfer curve.
            let c = self.color;
            let l = if erase { 1.0 } else { 0.299 * c.r + 0.587 * c.g + 0.114 * c.b };
            [l, l, l, 1.0]
        } else {
            self.color.to_linear_premul()
        };
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // Pigment thins a paint stroke to a glaze (`pigmentOpacity`, PE:825).
        let paint = !erase && !to_mask;
        let opacity =
            if paint { brush.opacity * onepaint_brush::height::pigment_opacity(brush.pigment_load()) } else { brush.opacity } as f32;
        let impasto = if paint { brush.impasto_body() } else { 0.0 };
        let carves = erase && !noop && !to_mask && layer.height.is_some();
        let height = if impasto > 0.0 || carves { self.height_begin(&self.active) } else { None };
        let mut engine = StrokeEngine::new(brush, [lin[0], lin[1], lin[2]], self.seed);
        engine.set_symmetry(self.symmetry_transforms());
        let mut st = ActiveStroke {
            layer: self.active.clone(),
            engine,
            buffer: Surface::new(),
            mode,
            opacity,
            noop,
            clip: self.clip.clone(),
            mask: to_mask,
            impasto,
            height,
        };
        let dabs = st.engine.begin(p);
        self.stroke = Some(st);
        self.draw_dabs(dabs);
    }

    pub fn stroke_extend(&mut self, pts: &[StrokePoint]) {
        if self.recorder.is_some() {
            let v = pts.to_vec();
            self.rec(|t| recording::RecEvent::Extend { t, pts: v });
        }
        if self.wet_stroke.is_some() {
            self.wet_extend(pts);
            return;
        }
        if self.smudge.is_some() {
            self.smudge_extend(pts);
            return;
        }
        if self.liquify.is_some() {
            self.liquify_extend(pts);
            return;
        }
        let Some(st) = self.stroke.as_mut() else {
            return;
        };
        let mut dabs = Vec::new();
        for p in pts {
            dabs.extend(st.engine.add(*p));
        }
        self.draw_dabs(dabs);
    }

    fn draw_dabs(&mut self, dabs: Vec<onepaint_brush::Dab>) {
        let Some(st) = self.stroke.as_mut() else {
            return;
        };
        if st.noop || dabs.is_empty() {
            return;
        }
        let mut r = Rect::default();
        for d in &dabs {
            r = r.union(&st.engine.rasterize(&mut st.buffer, d, st.clip.as_deref()));
        }
        let relief = height_ops::dry_height_dabs(st.impasto, &dabs);
        let layer = (!relief.is_empty()).then(|| st.layer.clone());
        self.mark_dirty(r);
        if let Some(layer) = layer {
            self.height_stamp(&layer, &relief);
        }
    }

    /// Advance time-based brush effects (airbrush) while a stroke is held.
    pub fn stroke_tick(&mut self, now_ms: f64) {
        if self.stroke.is_some() {
            self.rec(|t| recording::RecEvent::Tick { t, now: now_ms });
        }
        let Some(st) = self.stroke.as_mut() else { return };
        let dabs = st.engine.airbrush_tick(now_ms);
        self.draw_dabs(dabs);
    }

    /// Whether the current stroke wants frames while the pointer is still (airbrush).
    pub fn stroke_wants_ticks(&self) -> bool {
        self.stroke.as_ref().is_some_and(|s| s.engine.brush().airbrush.as_ref().is_some_and(|a| a.rate > 0.0))
    }

    pub fn stroke_cancel(&mut self) {
        if self.is_stroking() {
            self.rec(|t| recording::RecEvent::Cancel { t });
        }
        // A cancelled fluid stroke keeps what it laid down: the paint is wet.
        self.wet_end();
        self.smudge_cancel();
        self.liquify_cancel();
        if let Some(st) = self.stroke.take() {
            for (tx, ty) in st.buffer.tile_keys() {
                self.mark_dirty(Rect::from_tile(tx, ty));
            }
            if let Some(h) = st.height {
                self.height_cancel(h);
            }
        }
    }

    /// Flatten the stroke into its layer as one undo step.
    pub fn stroke_end(&mut self) {
        if self.is_stroking() {
            self.rec(|t| recording::RecEvent::End { t });
        }
        self.wet_end();
        self.smudge_end();
        self.liquify_end();
        let Some(tail) = self.stroke.as_mut().map(|st| st.engine.end()) else {
            return;
        };
        self.draw_dabs(tail);
        let Some(mut st) = self.stroke.take() else {
            return;
        };
        if st.noop {
            return;
        }
        // Watercolour settle: rim and granulation, on paint going on (PE:977).
        if !st.mask
            && st.mode != StrokeMode::Erase
            && let Some(fx) = st.engine.brush().settle()
        {
            let b = st.engine.brush();
            let grain = b.grain.as_ref().and_then(|g| {
                Some(onepaint_brush::settle::SettleGrain { tex: onepaint_brush::grain_for(&g.id)?, scale: g.scale.max(1.0) })
            });
            let hardness = b.hardness;
            onepaint_brush::settle::apply(&mut st.buffer, self.doc.bounds(), fx, grain.as_ref(), hardness);
            for (tx, ty) in st.buffer.tile_keys() {
                self.mark_dirty(Rect::from_tile(tx, ty));
            }
        }
        if st.mode == StrokeMode::Erase && st.height.is_some() {
            self.height_carve(&st.layer, &st.buffer, st.opacity);
        }
        let relief = st.height.take().and_then(|h| self.height_end(h));
        let Some(layer) = self.doc.layer_mut(&st.layer) else {
            return;
        };
        let target = match (st.mask, layer.mask.as_mut()) {
            (true, Some(m)) => m,
            (true, None) => return,
            (false, _) => &mut layer.pixels,
        };
        let mut changes = Vec::new();
        let mut bytes = 0;
        for (tx, ty) in st.buffer.tile_keys() {
            let Some(src) = st.buffer.tile(tx, ty) else {
                continue;
            };
            let before = target.tile(tx, ty).cloned();
            let mut dst = before.as_ref().map(|t| t.to_f32()).unwrap_or_else(|| vec![0.0; TILE_PX * 4]);
            let s = src.to_f32();
            for i in (0..dst.len()).step_by(4) {
                if s[i + 3] == 0.0 && s[i] == 0.0 && s[i + 1] == 0.0 && s[i + 2] == 0.0 {
                    continue;
                }
                let o = apply_stroke_px(
                    [dst[i], dst[i + 1], dst[i + 2], dst[i + 3]],
                    [s[i], s[i + 1], s[i + 2], s[i + 3]],
                    st.opacity,
                    st.mode,
                );
                dst[i..i + 4].copy_from_slice(&o);
            }
            let t = Tile::from_f32(&dst);
            let after = (!t.is_empty()).then(|| Arc::new(t));
            target.set_tile(tx, ty, after.clone());
            bytes += tile_bytes(&before) + tile_bytes(&after);
            changes.push(((tx, ty), before, after));
        }
        let paint =
            (!changes.is_empty()).then(|| (Change::Tiles { layer: st.layer.clone(), plane: Plane::of(st.mask), tiles: changes }, bytes));
        self.push_with_height(paint, relief);
    }

    fn history_changed(&mut self) {
        self.events.push(Event::HistoryChanged { can_undo: self.history.can_undo(), can_redo: self.history.can_redo() });
    }

    // ── Commands ──

    pub fn execute(&mut self, cmd: Command) -> Result<(), EngineError> {
        if self.recorder.is_some() {
            let c = cmd.clone();
            self.rec(|t| recording::RecEvent::Cmd { t, cmd: Box::new(c) });
        }
        self.rec_depth += 1;
        let r = self.execute_inner(cmd);
        self.rec_depth -= 1;
        r
    }

    fn execute_inner(&mut self, cmd: Command) -> Result<(), EngineError> {
        // Wet paint dries before anything that edits or replaces the document
        // (happypaint's dryWet() call sites); brush and colour choices keep it wet.
        if self.wet.is_some()
            && !matches!(
                cmd,
                Command::SetColor { .. }
                    | Command::SetTool { .. }
                    | Command::SetBrush { .. }
                    | Command::SetBrushPreset { .. }
                    | Command::SetBrushSize { .. }
                    | Command::SetBrushOpacity { .. }
                    | Command::SetBrushFlow { .. }
                    | Command::SetSymmetry { .. }
                    | Command::SetGuides { .. }
                    | Command::SetSmudgeStrength { .. }
                    | Command::SetMixer { .. }
                    | Command::SetLiquify { .. }
                    | Command::SetWetTilt { .. }
            )
        {
            self.dry_wet();
        }
        if self.filter.is_some() {
            match &cmd {
                Command::UpdateFilter { .. } | Command::CommitFilter | Command::CancelFilter => {}
                Command::Undo => {
                    self.cancel_filter();
                    return Ok(());
                }
                _ => self.commit_filter(),
            }
        }
        if self.transform.is_some() {
            match &cmd {
                Command::SetTransform { .. }
                | Command::CommitTransform
                | Command::CancelTransform
                | Command::BeginDistort
                | Command::BeginWarp
                | Command::SetWarp { .. } => {}
                Command::SetColor { .. }
                | Command::SetTool { .. }
                | Command::SetBrush { .. }
                | Command::SetBrushPreset { .. }
                | Command::SetBrushSize { .. }
                | Command::SetBrushOpacity { .. }
                | Command::SetBrushFlow { .. }
                | Command::SetSymmetry { .. }
                | Command::SetGuides { .. }
                | Command::EditGuideLines { .. }
                | Command::CommitGuideLines
                | Command::SetSmudgeStrength { .. }
                | Command::SetMixer { .. }
                | Command::CleanMixer
                | Command::SetLiquify { .. } => {}
                // Undo while transforming just abandons the transform.
                Command::Undo => {
                    self.cancel_transform();
                    return Ok(());
                }
                _ => self.commit_transform(),
            }
        }
        match cmd {
            Command::BeginTransform => self.begin_transform(),
            Command::LayerViaCopy { cut } => self.layer_via_copy(cut),
            Command::ResizeCanvas { width, height, dx, dy } => self.resize_canvas(width, height, dx, dy),
            Command::ScaleImage { width, height } => self.scale_image(width, height),
            Command::BeginFilter { kind } => self.begin_filter(&kind),
            Command::UpdateFilter { params } => {
                self.update_filter(params);
                Ok(())
            }
            Command::CommitFilter => {
                self.commit_filter();
                Ok(())
            }
            Command::CancelFilter => {
                self.cancel_filter();
                Ok(())
            }
            Command::AddAdjustmentLayer { kind, params } => self.add_adjustment_layer(&kind, params),
            Command::SetColorLabel { id, label } => self.set_color_label(&id, label),
            Command::AddText { spec } => self.add_text(spec),
            Command::UpdateText { id, index, spec } => self.update_text(&id, index, Some(spec)),
            Command::DeleteText { id, index } => self.update_text(&id, index, None),
            Command::RasterizeText { id } => self.rasterize_text(&id),
            Command::CreatePanelGrid { rows, cols, gutter, margin, border } => self.create_panel_grid(rows, cols, gutter, margin, border),
            Command::CreatePanel { a, b, gutter, border } => self.create_panel(a, b, gutter, border),
            Command::SplitPanel { id, a, b } => self.split_panel(&id, a, b),
            Command::SetPanelOutline { id, outline } => self.set_panel_outline(&id, outline),
            Command::SetPanelStyle { id, gutter, border } => self.set_panel_style(id.as_deref(), gutter, border),
            Command::AddMask => self.add_mask(),
            Command::DeleteMask { id } => self.delete_mask(&id),
            Command::ApplyMask { id } => self.apply_mask(&id),
            Command::SetMaskEnabled { id, enabled } => self.set_mask_enabled(&id, enabled),
            Command::SetMaskTarget { on } => {
                self.mask_target_layer =
                    (on && self.doc.layer(&self.active).is_some_and(|l| l.mask.is_some())).then(|| self.active.clone());
                Ok(())
            }
            Command::SetAdjustment { id, params } => self.set_adjustment(&id, params),
            Command::SetTransform { matrix } => {
                self.set_transform(matrix);
                Ok(())
            }
            Command::CommitTransform => {
                self.commit_transform();
                Ok(())
            }
            Command::CancelTransform => {
                self.cancel_transform();
                Ok(())
            }
            Command::AddLayer => self.insert_above_active(Node::Layer(Layer::new(&self.next_layer_name()))),
            Command::AddGroup => self.insert_above_active(Node::Group(Group::new(&self.next_group_name()))),
            Command::DeleteLayer { id } => self.delete(&id),
            Command::DuplicateLayer { id } => self.duplicate(&id),
            Command::MoveLayer { id, parent, index } => self.move_layer(&id, parent, index),
            Command::SetActiveLayer { id } => {
                self.doc.find(&id).ok_or_else(|| EngineError::NoSuchLayer(id.clone()))?;
                self.active = id;
                self.events.push(Event::ActiveLayerChanged);
                self.sync_playhead_to_active();
                Ok(())
            }
            Command::SetAnimation { settings } => {
                self.set_anim(*settings);
                Ok(())
            }
            Command::SetAnimSlot { slot } => {
                self.set_anim_slot(slot);
                Ok(())
            }
            Command::SetAnimPlaying { playing } => {
                self.anim_playing = playing;
                self.mark_all_dirty();
                Ok(())
            }
            Command::AddFrame => self.add_frame(),
            Command::SetFrameHold { id, hold } => {
                self.set_frame_meta(&id, "holdFrames", (hold != 1).then(|| serde_json::json!(hold.clamp(1, 99))))
            }
            Command::SetFrameRole { id, role } => self.set_frame_meta(&id, "animRole", role.map(serde_json::Value::String)),
            Command::SetLayerProps { id, props } => self.set_props(&id, props),
            Command::MergeDown { id } => self.merge_down(&id),
            Command::ClearLayer { id } => self.clear_layer(&id),
            Command::BevelFromAlpha { id, depth, softness } => {
                let o = onepaint_brush::height::BevelOptions { depth, softness, ..Default::default() };
                self.bevel_from_alpha(&id, o).map(|_| ())
            }
            Command::ClearBevel { id } => self.clear_height(&id).map(|_| ()),
            Command::DryWet => {
                self.dry_wet();
                Ok(())
            }
            Command::SetWetTilt { x, y } => {
                self.set_wet_tilt(x, y);
                Ok(())
            }
            Command::SetTool { tool } => {
                self.tool = tool;
                Ok(())
            }
            Command::SetSymmetry { symmetry } => {
                self.guides.symmetry = symmetry;
                self.guides = std::mem::take(&mut self.guides).sanitized();
                Ok(())
            }
            Command::SetGuides { guides } => {
                self.guides = guides.sanitized();
                Ok(())
            }
            Command::EditGuideLines { lines } => {
                if self.guide_edit_before.is_none() {
                    self.guide_edit_before = Some(self.guides.lines.clone());
                }
                self.guides.lines = lines;
                self.guides = std::mem::take(&mut self.guides).sanitized();
                Ok(())
            }
            Command::SaveSelection { name } => self.save_selection(name),
            Command::CreatePathLayer { spec } => self.create_path_layer(*spec),
            Command::AddPathObject { id, spec } => self.add_path_object(&id, *spec),
            Command::UpdatePathObject { id, index, spec } => self.update_path_object(&id, index, spec.map(|s| *s)),
            Command::RasterizePath { id } => self.rasterize_path(&id),
            Command::SelectFromPath { mode } => self.select_from_path(mode),
            Command::FillPath => self.fill_path(),
            Command::SetSmartFilters { id, filters } => self.set_smart_filters(&id, filters),
            Command::AddSmartFilter { id, kind, params } => self.add_smart_filter(&id, &kind, params),
            Command::BakeSmartFilters { id } => self.bake_smart_filters(&id),
            Command::FillShape { points, opacity } => self.fill_shape(points, opacity.clamp(0.0, 1.0)),
            Command::BeginDistort => {
                self.begin_warp(false);
                Ok(())
            }
            Command::BeginWarp => {
                self.begin_warp(true);
                Ok(())
            }
            Command::SetWarp { warp } => {
                self.set_warp(warp);
                Ok(())
            }
            Command::SealHistory => {
                self.history_sealed = true;
                Ok(())
            }
            Command::StrokePath { tool, simulate_pressure } => self.stroke_path(tool, simulate_pressure),
            Command::MaskFromPath => self.mask_from_path(),
            Command::DeleteSavedSelection { id } => {
                self.delete_saved_selection(&id);
                Ok(())
            }
            Command::LoadSelection { id, mode } => {
                self.load_selection(&id, mode);
                Ok(())
            }
            Command::CommitGuideLines => {
                if let Some(before) = self.guide_edit_before.take()
                    && before != self.guides.lines
                {
                    let bytes = 64 * (before.len() + self.guides.lines.len()) + 64;
                    self.history.push(Change::GuideLines { before, after: self.guides.lines.clone() }, bytes);
                    self.history_changed();
                }
                Ok(())
            }
            Command::SetSmudgeStrength { strength } => {
                self.paint.smudge_strength = strength.clamp(0.0, 1.0);
                Ok(())
            }
            Command::SetMixer { mixer } => {
                let c = |v: f64| v.clamp(0.0, 1.0);
                self.paint.mixer = MixerSettings { wet: c(mixer.wet), load: c(mixer.load), mix: c(mixer.mix), keep: mixer.keep };
                if !mixer.keep {
                    self.mixer_held = None;
                }
                Ok(())
            }
            Command::CleanMixer => {
                self.clean_mixer();
                Ok(())
            }
            Command::SetLiquify { mode, strength } => {
                self.paint.liquify_mode = mode;
                self.paint.liquify_strength = strength.clamp(0.0, 1.0);
                Ok(())
            }
            Command::SetBrush { preset_id } => {
                let p = onepaint_brush::preset(&preset_id).ok_or(EngineError::UnknownPreset(preset_id))?;
                if p.is_eraser() {
                    self.eraser = p;
                    self.tool = Tool::Eraser;
                } else {
                    self.brush = p;
                    self.tool = Tool::Brush;
                }
                Ok(())
            }
            Command::SetBrushPreset { preset } => {
                if preset.is_eraser() {
                    self.eraser = *preset;
                    self.tool = Tool::Eraser;
                } else {
                    self.brush = *preset;
                    self.tool = Tool::Brush;
                }
                Ok(())
            }
            Command::SetBrushSize { size } => {
                let max = self.prefs.max_brush_size;
                self.current_brush_mut().size = size.clamp(1.0, max);
                Ok(())
            }
            Command::SetBrushOpacity { opacity } => {
                self.current_brush_mut().opacity = opacity.clamp(0.01, 1.0);
                Ok(())
            }
            Command::SetBrushFlow { flow } => {
                self.current_brush_mut().flow = flow.clamp(0.01, 1.0);
                Ok(())
            }
            Command::SetColor { rgba } => {
                self.color = rgba;
                Ok(())
            }
            Command::Undo => {
                self.undo();
                Ok(())
            }
            Command::Redo => {
                self.redo();
                Ok(())
            }
            other => self.execute_selection(other),
        }
    }

    /// Where a layer-panel drop lands (happypaint `layer-panel.tsx:285-407`):
    /// `frac` is the pointer's position within the target row (0 = top).
    /// Returns `(parent, index)` for [`Command::MoveLayer`] (index after removal),
    /// or `None` for an invalid drop.
    pub fn resolve_drop(&self, dragged: &str, target: &str, frac: f32) -> Option<(Option<NodeId>, usize)> {
        if dragged == target && !self.doc.find(target)?.is_group() {
            return None;
        }
        if dragged == target || self.is_descendant(target, dragged) {
            return None;
        }
        let t = self.doc.find(target)?;
        let parent = self.doc.parent_of(target)?;
        let idx = *self.doc.path_of(target)?.last()?;
        let (parent, mut index) = match t {
            Node::Group(g) if frac >= 0.34 => (Some(target.to_string()), g.children.len()),
            Node::Group(_) => (parent, idx + 1),
            _ if frac < 0.5 => (parent, idx + 1),
            _ => (parent, idx),
        };
        // MoveLayer removes first: indices above the dragged layer shift down.
        if self.doc.parent_of(dragged)? == parent {
            let di = *self.doc.path_of(dragged)?.last()?;
            if di < index {
                index -= 1;
            }
        }
        Some((parent, index))
    }

    /// The layer colour labels happypaint offers (`engine/layer.ts:43`).
    pub const COLOR_LABELS: [&'static str; 7] = ["red", "orange", "yellow", "green", "blue", "violet", "gray"];

    pub fn color_label(&self, id: &str) -> Option<String> {
        let extra = match self.doc.find(id)? {
            Node::Layer(l) => &l.extra,
            Node::Group(g) => &g.extra,
        };
        extra.get("colorLabel")?.as_str().map(str::to_string)
    }

    fn set_color_label(&mut self, id: &str, label: Option<String>) -> Result<(), EngineError> {
        self.doc.find(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        if label.as_deref().is_some_and(|l| !Self::COLOR_LABELS.contains(&l)) {
            return Err(EngineError::NoSuchLayer(format!("label {}", label.unwrap_or_default())));
        }
        self.tree_edit(None, |s| {
            let extra = match s.doc.find_mut(id) {
                Some(Node::Layer(l)) => &mut l.extra,
                Some(Node::Group(g)) => &mut g.extra,
                None => return Ok(()),
            };
            match label {
                Some(l) => extra.insert("colorLabel".into(), serde_json::Value::String(l)),
                None => extra.remove("colorLabel"),
            };
            Ok(())
        })
    }

    fn next_layer_name(&self) -> String {
        let n = self
            .doc
            .layer_ids()
            .iter()
            .filter_map(|id| self.doc.find(id)?.name().strip_prefix("Layer ")?.parse::<usize>().ok())
            .max()
            .unwrap_or(0);
        format!("Layer {}", n.max(self.doc.layer_ids().len().saturating_sub(1)) + 1)
    }

    fn next_group_name(&self) -> String {
        fn count(nodes: &[Node]) -> usize {
            nodes.iter().map(|n| if let Node::Group(g) = n { 1 + count(&g.children) } else { 0 }).sum()
        }
        format!("Group {}", count(&self.doc.root) + 1)
    }

    /// Run a tree edit as one undo step (whole-tree snapshot: tiles are shared, so it's cheap).
    fn tree_edit(&mut self, key: Option<String>, f: impl FnOnce(&mut Self) -> Result<(), EngineError>) -> Result<(), EngineError> {
        let before = self.doc.root.clone();
        let active_before = self.active.clone();
        f(self)?;
        let after = self.doc.root.clone();
        let active_after = self.active.clone();
        let sealed = std::mem::take(&mut self.history_sealed);
        let coalesced = !sealed && key.is_some() && matches!(self.history.top_mut(), Some(Change::Tree { key: k, .. }) if *k == key) && {
            if let Some(Change::Tree { after: a, active_after: aa, .. }) = self.history.top_mut() {
                *a = after.clone();
                *aa = active_after.clone();
            }
            true
        };
        if !coalesced {
            self.history.push(Change::Tree { before, after, active_before, active_after, key }, 1024);
        }
        self.doc.touch();
        self.events.push(Event::LayersChanged);
        self.events.push(Event::ActiveLayerChanged);
        self.history_changed();
        self.mark_all_dirty();
        Ok(())
    }

    fn insert_above_active(&mut self, node: Node) -> Result<(), EngineError> {
        let id = node.id().to_string();
        self.tree_edit(None, |s| {
            let (parent, index) = match s.doc.path_of(&s.active) {
                Some(p) => (s.doc.parent_of(&s.active).flatten(), p.last().map_or(0, |i| i + 1)),
                None => (None, s.doc.root.len()),
            };
            s.doc.insert(parent.as_deref(), index, node);
            s.active = id;
            Ok(())
        })
    }

    fn delete(&mut self, id: &str) -> Result<(), EngineError> {
        self.doc.find(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let remaining = self.doc.layer_ids().into_iter().filter(|l| l != id && !self.is_descendant(l, id)).count();
        if remaining == 0 {
            return Err(EngineError::LastLayer);
        }
        self.tree_edit(None, |s| {
            let (_, parent, idx) = s.doc.remove(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
            if s.doc.find(&s.active).is_none() {
                let siblings = s.doc.children(parent.as_deref()).cloned().unwrap_or_default();
                s.active = match siblings.get(idx.saturating_sub(1)).or_else(|| siblings.get(idx)) {
                    Some(n) => n.id().to_string(),
                    None => parent.clone().or_else(|| s.doc.layer_ids().last().cloned()).unwrap_or_default(),
                };
            }
            Ok(())
        })
    }

    fn is_descendant(&self, id: &str, ancestor: &str) -> bool {
        match (self.doc.path_of(id), self.doc.path_of(ancestor)) {
            (Some(p), Some(a)) => p.len() > a.len() && p.starts_with(&a),
            _ => false,
        }
    }

    fn duplicate(&mut self, id: &str) -> Result<(), EngineError> {
        let mut node = self.doc.find(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?.clone();
        node.reassign_ids();
        let name = format!("{} copy", node.name());
        node.set_name(name);
        let new_id = node.id().to_string();
        self.tree_edit(None, |s| {
            let parent = s.doc.parent_of(id).flatten();
            let idx = s.doc.path_of(id).and_then(|p| p.last().copied()).unwrap_or(0);
            s.doc.insert(parent.as_deref(), idx + 1, node);
            s.active = new_id;
            Ok(())
        })
    }

    fn move_layer(&mut self, id: &str, parent: Option<NodeId>, index: usize) -> Result<(), EngineError> {
        self.doc.find(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        if let Some(p) = &parent {
            let g = self.doc.find(p).ok_or_else(|| EngineError::NoSuchLayer(p.clone()))?;
            if !g.is_group() || p == id || self.is_descendant(p, id) {
                return Err(EngineError::InvalidMove);
            }
        }
        self.tree_edit(None, |s| {
            let (node, _, _) = s.doc.remove(id).ok_or(EngineError::InvalidMove)?;
            s.doc.insert(parent.as_deref(), index, node);
            Ok(())
        })
    }

    fn set_props(&mut self, id: &str, p: LayerProps) -> Result<(), EngineError> {
        self.doc.find(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let key = Some(format!("{id}:{}", p.key()));
        self.tree_edit(key, |s| {
            let Some(n) = s.doc.find_mut(id) else {
                return Ok(());
            };
            if let Some(v) = p.name {
                n.set_name(v);
            }
            if let Some(v) = p.visible {
                n.set_visible(v);
            }
            if let Some(v) = p.opacity {
                n.set_opacity(v);
            }
            if let Some(v) = p.blend_mode {
                n.set_blend_mode(v);
            }
            match n {
                Node::Layer(l) => {
                    if let Some(v) = p.clipped {
                        l.clipped = v;
                    }
                    if let Some(v) = p.alpha_lock {
                        l.alpha_lock = v;
                    }
                    if let Some(v) = p.locked {
                        l.locked = v;
                    }
                }
                Node::Group(g) => {
                    if let Some(v) = p.collapsed {
                        g.collapsed = v;
                    }
                }
            }
            Ok(())
        })
    }

    /// Bake a layer into the paint layer directly below it (keeps the lower layer's props).
    fn merge_down(&mut self, id: &str) -> Result<(), EngineError> {
        let path = self.doc.path_of(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let idx = *path.last().ok_or(EngineError::NothingBelow)?;
        if idx == 0 {
            return Err(EngineError::NothingBelow);
        }
        let mut below_path = path.clone();
        *below_path.last_mut().ok_or(EngineError::NothingBelow)? = idx - 1;
        let below = self.doc.node_at(&below_path).and_then(Node::as_layer).ok_or(EngineError::NothingBelow)?.clone();
        let upper = self.doc.node_at(&path).cloned().ok_or(EngineError::NothingBelow)?;
        let mut base = below.clone();
        base.opacity = 1.0;
        base.blend_mode = BlendMode::Normal;
        base.clipped = false;
        let mut upper = upper;
        if let Node::Layer(l) = &mut upper {
            l.clipped = false;
        }
        // Relief stays a height field: merge the paint unlit, then add the
        // upper layer's relief onto the lower one's when it shows (PE:7062).
        let height = match &upper {
            Node::Layer(l) if l.visible && l.opacity > 0.0 => {
                let mut h = below.height.clone().unwrap_or_default();
                if let Some(up) = &l.height {
                    onepaint_brush::height::add_into(&mut h, up);
                }
                (h.tile_count() > 0).then_some(h)
            }
            _ => below.height.clone(),
        };
        fn unlit(n: &mut Node) {
            match n {
                Node::Layer(l) => l.height = None,
                Node::Group(g) => g.children.iter_mut().for_each(unlit),
            }
        }
        base.height = None;
        unlit(&mut upper);
        let mut tmp = Document::new(self.doc.width, self.doc.height, None);
        tmp.root = vec![Node::Layer(base), upper];
        let mut keys: BTreeSet<(i32, i32)> = below.pixels.tile_keys().into_iter().collect();
        fn collect(n: &Node, out: &mut BTreeSet<(i32, i32)>) {
            match n {
                Node::Layer(l) => out.extend(l.pixels.tile_keys()),
                Node::Group(g) => g.children.iter().for_each(|c| collect(c, out)),
            }
        }
        collect(&tmp.root[1], &mut keys);
        let keys: Vec<_> = keys.into_iter().collect();
        let merged = onepaint_compose::composite_tiles(&tmp, &keys, None);
        let below_id = below.id.clone();
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(&below_id) {
                for ((tx, ty), t) in merged {
                    l.pixels.set_tile(tx, ty, (!t.is_empty()).then(|| Arc::new(t)));
                }
                l.height = height;
            }
            s.doc.remove(id);
            s.active = below_id;
            Ok(())
        })
    }

    fn clear_layer(&mut self, id: &str) -> Result<(), EngineError> {
        let l = self.doc.layer(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let tiles: Vec<TileChange> = l.pixels.tiles().map(|(k, t)| (k, Some(t.clone()), None)).collect();
        let bytes = tiles.iter().map(|(_, b, _)| tile_bytes(b)).sum();
        // Clearing paint clears its relief too (PE clearLayer).
        let relief = self.height_begin(id);
        if let Some(l) = self.doc.layer_mut(id) {
            l.pixels.clear();
            l.height = None;
        }
        let relief = relief.and_then(|h| self.height_end(h));
        self.push_with_height(Some((Change::Tiles { layer: id.to_string(), plane: Plane::Pixels, tiles }, bytes)), relief);
        self.mark_all_dirty();
        Ok(())
    }

    fn apply(&mut self, undo: bool, entries: Vec<ChangeRef>) {
        for c in entries {
            match c {
                ChangeRef::Tiles { layer, plane, tiles } => {
                    if let Some(l) = self.doc.layer_mut(&layer) {
                        for (k, before, after) in &tiles {
                            let t = if undo { before.clone() } else { after.clone() };
                            match plane {
                                Plane::Mask => {
                                    if let Some(m) = l.mask.as_mut() {
                                        m.set_tile(k.0, k.1, t);
                                    }
                                }
                                Plane::Pixels => l.pixels.set_tile(k.0, k.1, t),
                                Plane::Height => l.height.get_or_insert_default().set_tile(k.0, k.1, t),
                            }
                        }
                        if l.height.as_ref().is_some_and(|h| h.tile_count() == 0) {
                            l.height = None;
                        }
                    }
                    for (k, _, _) in tiles {
                        self.mark_dirty(Rect::from_tile(k.0, k.1));
                    }
                }
                ChangeRef::Tree { root, active } => {
                    self.doc.root = root;
                    self.active = active;
                    self.events.push(Event::LayersChanged);
                    self.events.push(Event::ActiveLayerChanged);
                    self.mark_all_dirty();
                }
                ChangeRef::Selection(m) => self.set_selection_raw(m),
                ChangeRef::Document(d) => self.apply_shape(*d),
                ChangeRef::GuideLines(lines) => self.guides.lines = lines,
                ChangeRef::SavedSelections(list) => {
                    self.saved = list;
                    self.events.push(Event::SavedSelectionsChanged);
                }
            }
        }
        self.history_changed();
    }

    fn snapshot(c: &Change, undo: bool) -> ChangeRef {
        match c {
            Change::Tiles { layer, plane, tiles } => ChangeRef::Tiles { layer: layer.clone(), plane: *plane, tiles: tiles.clone() },
            Change::Tree { before, after, active_before, active_after, .. } => ChangeRef::Tree {
                root: if undo { before.clone() } else { after.clone() },
                active: if undo { active_before.clone() } else { active_after.clone() },
            },
            Change::Selection { before, after } => ChangeRef::Selection(if undo { before.clone() } else { after.clone() }),
            Change::Document { before, after } => ChangeRef::Document(Box::new(if undo { (**before).clone() } else { (**after).clone() })),
            Change::GuideLines { before, after } => ChangeRef::GuideLines(if undo { before.clone() } else { after.clone() }),
            Change::SavedSelections { before, after } => ChangeRef::SavedSelections(if undo { before.clone() } else { after.clone() }),
        }
    }

    pub fn undo(&mut self) {
        self.stroke_cancel();
        let Some(es) = self.history.undo() else {
            return;
        };
        let refs: Vec<_> = es.into_iter().map(|c| Self::snapshot(c, true)).collect();
        self.apply(true, refs);
    }

    pub fn redo(&mut self) {
        self.stroke_cancel();
        let Some(es) = self.history.redo() else {
            return;
        };
        let refs: Vec<_> = es.into_iter().map(|c| Self::snapshot(c, false)).collect();
        self.apply(false, refs);
    }
}

enum ChangeRef {
    Tiles { layer: NodeId, plane: Plane, tiles: Vec<TileChange> },
    Tree { root: Vec<Node>, active: NodeId },
    Selection(Option<Arc<onepaint_selection::Mask>>),
    Document(Box<canvas_ops::DocShape>),
    GuideLines(Vec<guides::GuideLine>),
    SavedSelections(Vec<SavedSelection>),
}
