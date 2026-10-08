//! Animation Assist (spec 24): flipbook frames and exposure-sheet tracks over
//! the layer tree, a present-only view with onion skins, and frame export.

use super::*;
use onepaint_doc::Extra;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoopMode {
    #[default]
    Loop,
    Pingpong,
    Once,
}

fn d_fps() -> f64 {
    12.0
}
fn d_one() -> u32 {
    1
}
fn d_true() -> bool {
    true
}
fn d_op() -> f32 {
    0.35
}
fn d_before() -> String {
    "#ff2626".into()
}
fn d_after() -> String {
    "#1ad940".into()
}

/// happypaint's `StoredAnimation`; unknown keys (markers, pegs…) are kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "d_fps")]
    pub fps: f64,
    #[serde(default, rename = "loop")]
    pub loop_mode: LoopMode,
    #[serde(default = "d_true")]
    pub onion_enabled: bool,
    #[serde(default = "d_one")]
    pub onion_before: u32,
    #[serde(default = "d_one")]
    pub onion_after: u32,
    #[serde(default = "d_op")]
    pub onion_opacity: f32,
    #[serde(default = "d_true")]
    pub onion_tint: bool,
    #[serde(default = "d_before")]
    pub onion_color_before: String,
    #[serde(default = "d_after")]
    pub onion_color_after: String,
    #[serde(default)]
    pub onion_front: bool,
    #[serde(default)]
    pub onion_muted: Vec<i32>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for AnimSettings {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap_or_else(|_| unreachable!())
    }
}

/// One flipbook frame or track sheet: a node covering `hold` slots from `start`.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: NodeId,
    pub start: usize,
    pub hold: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: NodeId,
    /// `None` = static (shows on every column).
    pub sheets: Option<Vec<Frame>>,
}

fn extra_of(n: &Node) -> &Extra {
    match n {
        Node::Layer(l) => &l.extra,
        Node::Group(g) => &g.extra,
    }
}

fn extra_mut(n: &mut Node) -> &mut Extra {
    match n {
        Node::Layer(l) => &mut l.extra,
        Node::Group(g) => &mut g.extra,
    }
}

pub fn hold_of(n: &Node) -> usize {
    extra_of(n).get("holdFrames").and_then(|v| v.as_f64()).map_or(1, |h| h.round().max(1.0) as usize)
}

pub fn role_of(n: &Node) -> Option<&str> {
    extra_of(n).get("animRole").and_then(|v| v.as_str())
}

fn is_track(n: &Node) -> bool {
    n.is_group() && extra_of(n).get("animTrack").and_then(|v| v.as_bool()) == Some(true)
}

pub fn is_tracks_doc(root: &[Node]) -> bool {
    root.iter().any(is_track)
}

fn sheets_of(children: &[Node]) -> Vec<Frame> {
    let mut next = 0;
    children
        .iter()
        .map(|c| {
            let hold = hold_of(c);
            let start = extra_of(c).get("animStart").and_then(|v| v.as_f64()).map_or(next, |s| s.round().max(0.0) as usize);
            next = start + hold;
            Frame { id: c.id().to_string(), start, hold }
        })
        .collect()
}

pub fn tracks_of(root: &[Node]) -> Vec<Track> {
    root.iter()
        .map(|n| Track { id: n.id().to_string(), sheets: is_track(n).then(|| sheets_of(n.as_group().map_or(&[][..], |g| &g.children))) })
        .collect()
}

/// Flipbook frames: top-level non-role nodes, bottom-first.
pub fn flipbook_frames(root: &[Node]) -> Vec<Frame> {
    let mut slot = 0;
    root.iter()
        .filter(|n| role_of(n).is_none())
        .map(|n| {
            let hold = hold_of(n);
            let f = Frame { id: n.id().to_string(), start: slot, hold };
            slot += hold;
            f
        })
        .collect()
}

pub fn total_slots(root: &[Node]) -> usize {
    if is_tracks_doc(root) {
        tracks_of(root).iter().filter_map(|t| t.sheets.as_ref()).flatten().map(|s| s.start + s.hold).max().unwrap_or(0)
    } else {
        flipbook_frames(root).iter().map(|f| f.hold).sum()
    }
}

/// The flipbook frame at `slot` (clamped to the last frame).
pub fn frame_at_slot(frames: &[Frame], slot: usize) -> Option<&Frame> {
    let last = frames.last()?;
    let total = last.start + last.hold;
    let t = slot.min(total - 1);
    frames.iter().find(|f| t < f.start + f.hold)
}

/// One playback step over `lo..=hi` (AS:292).
pub fn step_slot(slot: usize, lo: usize, hi: usize, mode: LoopMode, dir: i32) -> (usize, i32, bool) {
    if hi <= lo {
        return (lo, dir, mode == LoopMode::Once);
    }
    let at = slot.clamp(lo, hi) as i64;
    let next = at + dir as i64;
    let (lo, hi) = (lo as i64, hi as i64);
    match mode {
        LoopMode::Loop => {
            let span = hi - lo + 1;
            ((lo + (next - lo).rem_euclid(span)) as usize, dir, false)
        }
        LoopMode::Pingpong => {
            if next > hi {
                ((hi - 1) as usize, -1, false)
            } else if next < lo {
                ((lo + 1) as usize, 1, false)
            } else {
                (next as usize, dir, false)
            }
        }
        LoopMode::Once => {
            if next > hi {
                (hi as usize, dir, true)
            } else {
                (next as usize, dir, false)
            }
        }
    }
}

/// What draws at a column: top-level nodes allowed, and per-track sheets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ColumnView {
    pub allow: Vec<NodeId>,
    pub sheets: HashMap<NodeId, NodeId>,
}

pub fn column_view(root: &[Node], column: usize) -> ColumnView {
    let mut v = ColumnView::default();
    if is_tracks_doc(root) {
        for t in tracks_of(root) {
            match &t.sheets {
                None => v.allow.push(t.id),
                Some(sheets) => {
                    if let Some(s) = sheets.iter().find(|s| column >= s.start && column < s.start + s.hold) {
                        v.sheets.insert(t.id.clone(), s.id.clone());
                        v.allow.push(t.id);
                    }
                }
            }
        }
    } else {
        v.allow.extend(root.iter().filter(|n| role_of(n).is_some()).map(|n| n.id().to_string()));
        if let Some(f) = frame_at_slot(&flipbook_frames(root), column) {
            v.allow.push(f.id.clone());
        }
    }
    v
}

fn only_sheet(g: &mut Group, sheet: &str) {
    for c in &mut g.children {
        let show = c.id() == sheet;
        if !show {
            c.set_visible(false);
        }
    }
}

fn ghost_group(node: &Node, tint: Option<[f32; 3]>, opacity: f32) -> Node {
    let mut g = Group::new("ghost");
    g.id = format!("ghost:{}", node.id());
    let mut inner = node.clone();
    inner.set_blend_mode(BlendMode::Normal);
    g.opacity = node.opacity() * opacity;
    inner.set_opacity(1.0);
    inner.set_visible(true);
    g.children = vec![inner];
    if let Some(t) = tint {
        g.extra.insert(onepaint_compose::GHOST_TINT_KEY.into(), serde_json::json!(t));
    }
    Node::Group(g)
}

fn tint_of(hex: &str) -> [f32; 3] {
    let c = Rgba::from_hex(hex).unwrap_or(Rgba::BLACK).to_linear_premul();
    [c[0], c[1], c[2]]
}

/// The present-only document at `column` (with onion ghosts when asked).
pub fn view_document(doc: &Document, s: &AnimSettings, column: usize, ghosts: bool, active: &str) -> Document {
    let mut view = Document { root: Vec::new(), ..doc.clone_shallow() };
    let cv = column_view(&doc.root, column);
    let tracks = is_tracks_doc(&doc.root);
    let mut ghost_nodes: Vec<(usize, Node)> = Vec::new();
    if ghosts && s.onion_enabled {
        let neighbours = |list: &[Frame], cur: usize| -> Vec<(i32, NodeId)> {
            let mut out = Vec::new();
            for d in 1..=s.onion_before.min(8) as i32 {
                if let Some(f) = cur.checked_sub(d as usize).and_then(|i| list.get(i)) {
                    out.push((-d, f.id.clone()));
                }
            }
            for d in 1..=s.onion_after.min(8) as i32 {
                if let Some(f) = list.get(cur + d as usize) {
                    out.push((d, f.id.clone()));
                }
            }
            out.retain(|(d, _)| !s.onion_muted.contains(d));
            out
        };
        let style = |d: i32| {
            let tint = s.onion_tint.then(|| tint_of(if d < 0 { &s.onion_color_before } else { &s.onion_color_after }));
            (tint, s.onion_opacity / d.unsigned_abs() as f32)
        };
        if tracks {
            let at = tracks_of(&doc.root).into_iter().position(|t| {
                doc.root.iter().any(|n| {
                    n.id() == t.id && (n.id() == active || n.as_group().is_some_and(|g| g.children.iter().any(|c| c.id() == active)))
                })
            });
            if let Some(ti) = at
                && let Some(sheets) = tracks_of(&doc.root)[ti].sheets.clone()
                && let Some(cur) = sheets.iter().position(|f| column >= f.start && column < f.start + f.hold)
                && let Some(Node::Group(track)) = doc.root.get(ti)
            {
                for (d, id) in neighbours(&sheets, cur) {
                    if let Some(sheet) = track.children.iter().find(|c| c.id() == id) {
                        let (tint, op) = style(d);
                        let mut wrapper = track.clone();
                        wrapper.children = vec![sheet.clone()];
                        ghost_nodes.push((ti, ghost_group(&Node::Group(wrapper), tint, op)));
                    }
                }
            }
        } else {
            let frames = flipbook_frames(&doc.root);
            if let Some(cur) = frame_at_slot(&frames, column).and_then(|f| frames.iter().position(|x| x.id == f.id)) {
                let cur_idx = doc.root.iter().position(|n| n.id() == frames[cur].id).unwrap_or(0);
                for (d, id) in neighbours(&frames, cur) {
                    if let Some(n) = doc.root.iter().find(|n| n.id() == id) {
                        let (tint, op) = style(d);
                        ghost_nodes.push((cur_idx, ghost_group(n, tint, op)));
                    }
                }
            }
        }
    }
    for (i, n) in doc.root.iter().enumerate() {
        if !s.onion_front {
            view.root.extend(ghost_nodes.iter().filter(|g| g.0 == i).map(|g| g.1.clone()));
        }
        let mut n = n.clone();
        if !cv.allow.iter().any(|a| a == n.id()) {
            n.set_visible(false);
        } else if let (Some(sheet), Node::Group(g)) = (cv.sheets.get(n.id()), &mut n) {
            only_sheet(g, sheet);
        }
        view.root.push(n);
        if s.onion_front {
            view.root.extend(ghost_nodes.iter().filter(|g| g.0 == i).map(|g| g.1.clone()));
        }
    }
    view
}

impl Session {
    pub fn anim_settings(&self) -> &AnimSettings {
        &self.anim
    }

    pub fn anim_slot(&self) -> usize {
        self.anim_slot
    }

    pub fn anim_frames(&self) -> Vec<Frame> {
        if is_tracks_doc(&self.doc.root) {
            let tr = tracks_of(&self.doc.root);
            return self.track_index_of_active().and_then(|i| tr[i].sheets.clone()).unwrap_or_default();
        }
        flipbook_frames(&self.doc.root)
    }

    pub fn anim_total_slots(&self) -> usize {
        total_slots(&self.doc.root)
    }

    fn track_index_of_active(&self) -> Option<usize> {
        self.doc
            .root
            .iter()
            .position(|n| n.id() == self.active || n.as_group().is_some_and(|g| g.children.iter().any(|c| c.id() == self.active)))
    }

    pub(crate) fn load_anim(&mut self) {
        self.anim = self.doc.extra.get("animation").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        self.anim_ever = self.doc.extra.contains_key("animation");
        self.anim_slot = 0;
    }

    pub(crate) fn store_anim(&mut self) {
        if self.anim_ever {
            self.doc.extra.insert("animation".into(), serde_json::to_value(&self.anim).unwrap_or_default());
        }
    }

    /// The document compositing shows: the animation view when enabled.
    pub(crate) fn display_doc(&self) -> std::borrow::Cow<'_, Document> {
        let anim = self.anim.enabled && total_slots(&self.doc.root) > 0;
        if !anim && !self.has_smart_filters() {
            return std::borrow::Cow::Borrowed(&self.doc);
        }
        let mut view =
            if anim { view_document(&self.doc, &self.anim, self.anim_slot, !self.anim_playing, &self.active) } else { self.doc.clone() };
        self.apply_smart_filters(&mut view.root);
        std::borrow::Cow::Owned(view)
    }

    pub(crate) fn set_anim(&mut self, s: AnimSettings) {
        if s.enabled {
            self.anim_ever = true;
        }
        let was = self.anim.enabled;
        self.anim = AnimSettings {
            fps: s.fps.clamp(1.0, 60.0),
            onion_before: s.onion_before.min(8),
            onion_after: s.onion_after.min(8),
            onion_opacity: s.onion_opacity.clamp(0.0, 1.0),
            ..s
        };
        if self.anim.enabled && !was {
            self.sync_playhead_to_active();
        }
        self.mark_all_dirty();
    }

    /// Move the playhead; the active layer follows the frame under it.
    pub(crate) fn set_anim_slot(&mut self, slot: usize) {
        let total = self.anim_total_slots();
        self.anim_slot = slot.min(total.saturating_sub(1));
        if self.anim.enabled {
            let frames = self.anim_frames();
            let f = if is_tracks_doc(&self.doc.root) {
                frames.iter().find(|f| self.anim_slot >= f.start && self.anim_slot < f.start + f.hold).cloned()
            } else {
                frame_at_slot(&frames, self.anim_slot).cloned()
            };
            if let Some(f) = f {
                let inside = self.doc.find(&f.id).is_some_and(|n| n.id() == self.active || contains(n, &self.active));
                let in_role = self.doc.root.iter().any(|n| role_of(n).is_some() && (n.id() == self.active || contains(n, &self.active)));
                if !inside && !in_role {
                    self.active = f.id;
                    self.events.push(Event::ActiveLayerChanged);
                }
            }
        }
        self.mark_all_dirty();
    }

    /// Selecting a frame's layer moves the playhead to it.
    pub(crate) fn sync_playhead_to_active(&mut self) {
        if !self.anim.enabled {
            return;
        }
        let frames = self.anim_frames();
        let cur = frame_at_slot(&frames, self.anim_slot).map(|f| f.id.clone());
        if let Some(f) = frames.iter().find(|f| self.doc.find(&f.id).is_some_and(|n| n.id() == self.active || contains(n, &self.active)))
            && Some(&f.id) != cur.as_ref()
        {
            self.anim_slot = f.start;
            self.mark_all_dirty();
        }
    }

    pub(crate) fn set_frame_meta(&mut self, id: &str, key: &str, value: Option<serde_json::Value>) -> Result<(), EngineError> {
        self.doc.find(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let (id, key) = (id.to_string(), key.to_string());
        self.tree_edit(None, |s| {
            if let Some(n) = find_node_mut(&mut s.doc.root, &id) {
                let e = extra_mut(n);
                match value {
                    Some(v) => {
                        e.insert(key, v);
                    }
                    None => {
                        e.remove(&key);
                    }
                }
            }
            Ok(())
        })
    }

    /// An empty frame right after the current one, selected.
    pub(crate) fn add_frame(&mut self) -> Result<(), EngineError> {
        let frames = flipbook_frames(&self.doc.root);
        let cur = frame_at_slot(&frames, self.anim_slot).map(|f| f.id.clone());
        let at = cur.and_then(|id| self.doc.root.iter().position(|n| n.id() == id)).map_or(self.doc.root.len(), |i| i + 1);
        let name = format!("Frame {}", frames.len() + 1);
        let l = Layer::new(&name);
        let id = l.id.clone();
        self.tree_edit(None, |s| {
            s.doc.root.insert(at.min(s.doc.root.len()), Node::Layer(l));
            s.active = id.clone();
            Ok(())
        })?;
        if let Some(f) = flipbook_frames(&self.doc.root).into_iter().find(|f| f.id == self.active) {
            self.anim_slot = f.start;
        }
        self.mark_all_dirty();
        Ok(())
    }

    /// Composite one timeline slot without ghosts, for export (straight sRGB 8-bit).
    pub fn render_slot_rgba8(&self, slot: usize, background: Option<Rgba>) -> Vec<u8> {
        let mut view = view_document(&self.doc, &self.anim, slot, false, &self.active);
        self.apply_smart_filters(&mut view.root);
        let mut flat = onepaint_compose::flatten(&view, None);
        if let Some(bg) = background {
            let b = bg.to_linear_premul();
            for p in flat.chunks_exact_mut(4) {
                let k = 1.0 - p[3];
                for c in 0..4 {
                    p[c] += b[c] * k;
                }
            }
        }
        onepaint_formats::flatten_to_srgb8(&flat)
    }

    /// Every timeline slot as RGBA8 (holds expanded).
    pub fn render_all_slots(&self, background: Option<Rgba>) -> Vec<Vec<u8>> {
        (0..self.anim_total_slots().max(1)).map(|s| self.render_slot_rgba8(s, background)).collect()
    }

    pub fn export_gif(&self, background: Option<Rgba>) -> Result<Vec<u8>, EngineError> {
        let frames = self.render_all_slots(background);
        Ok(onepaint_formats::encode_gif(self.doc.width, self.doc.height, &frames, self.anim.fps)?)
    }

    pub fn export_png_sequence(&self, background: Option<Rgba>) -> Result<Vec<u8>, EngineError> {
        let frames = self.render_all_slots(background);
        let mut files = Vec::new();
        for (i, f) in frames.iter().enumerate() {
            files.push((format!("frame_{:04}.png", i + 1), onepaint_formats::encode_png_rgba8(self.doc.width, self.doc.height, f)?));
        }
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        Ok(onepaint_formats::zip::write_zip(&refs))
    }
}

fn contains(n: &Node, id: &str) -> bool {
    n.as_group().is_some_and(|g| g.children.iter().any(|c| c.id() == id || contains(c, id)))
}

fn find_node_mut<'a>(nodes: &'a mut [Node], id: &str) -> Option<&'a mut Node> {
    for n in nodes {
        if n.id() == id {
            return Some(n);
        }
        if let Node::Group(g) = n
            && let Some(f) = find_node_mut(&mut g.children, id)
        {
            return Some(f);
        }
    }
    None
}
