//! Smart filters (happypaint round 21): a per-layer stack of filters applied
//! non-destructively at display time, editable, toggleable and bakeable.
//! Stored verbatim in the layer's `filters` key.

use super::*;
use onepaint_filters::{Frame, Kind, Params};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SmartFilter {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub params: Params,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl SmartFilter {
    pub fn is_on(&self) -> bool {
        self.enabled != Some(false)
    }
}

/// Render generators, line art and fills are layers, not filters (layer.ts:69).
pub fn is_smart_kind(k: Kind) -> bool {
    !k.is_generator() && !k.is_fill()
}

pub fn smart_filters_of(l: &Layer) -> Vec<SmartFilter> {
    l.extra.get("filters").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

fn active_filters(l: &Layer) -> Vec<(Kind, Params)> {
    smart_filters_of(l)
        .into_iter()
        .filter(|f| f.is_on())
        .filter_map(|f| Some((Kind::parse(&f.kind).filter(|k| is_smart_kind(*k))?, f.params)))
        .collect()
}

/// Filtered pixels, cached by the filter stack and the layer's tile identity.
#[derive(Default)]
pub(crate) struct SmartCache(std::sync::Mutex<HashMap<NodeId, (u64, Surface)>>);

fn fingerprint(l: &Layer, filters: &[(Kind, Params)], w: u32, h: u32) -> u64 {
    let mut s = std::collections::hash_map::DefaultHasher::new();
    (w, h).hash(&mut s);
    serde_json::to_string(&filters.iter().map(|(k, p)| (k.as_str(), p)).collect::<Vec<_>>()).unwrap_or_default().hash(&mut s);
    for (k, t) in l.pixels.tiles() {
        k.hash(&mut s);
        (Arc::as_ptr(t) as usize).hash(&mut s);
    }
    s.finish()
}

fn filtered(l: &Layer, filters: &[(Kind, Params)], doc: Rect, frame: Frame) -> Surface {
    let mut px = l.pixels.clone();
    for (k, p) in filters {
        px = crate::filter_ops::apply_filter(&px, doc, frame, *k, p, None);
    }
    px
}

fn any_filters(nodes: &[Node]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Layer(l) => l.extra.contains_key("filters") && !active_filters(l).is_empty(),
        Node::Group(g) => any_filters(&g.children),
    })
}

impl Session {
    pub fn smart_filters(&self, id: &str) -> Vec<SmartFilter> {
        self.doc.layer(id).map(smart_filters_of).unwrap_or_default()
    }

    /// Replace layer pixels with their filtered versions throughout `nodes`.
    pub(crate) fn apply_smart_filters(&self, nodes: &mut [Node]) {
        let (doc, frame) = (self.doc.bounds(), Frame::region(0, 0, self.doc.width, self.doc.height, self.doc.dpi as f32));
        for n in nodes {
            match n {
                Node::Group(g) => self.apply_smart_filters(&mut g.children),
                Node::Layer(l) => {
                    let f = active_filters(l);
                    if f.is_empty() {
                        continue;
                    }
                    let key = fingerprint(l, &f, self.doc.width, self.doc.height);
                    let mut cache = self.smart_cache.0.lock().unwrap_or_else(|e| e.into_inner());
                    let out = match cache.get(&l.id) {
                        Some((k, s)) if *k == key => s.clone(),
                        _ => {
                            let s = filtered(l, &f, doc, frame);
                            cache.insert(l.id.clone(), (key, s.clone()));
                            s
                        }
                    };
                    l.pixels = out;
                }
            }
        }
    }

    pub(crate) fn has_smart_filters(&self) -> bool {
        any_filters(&self.doc.root)
    }

    /// Replace a layer's stack (live edits of one layer coalesce into one step).
    pub(crate) fn set_smart_filters(&mut self, id: &str, filters: Vec<SmartFilter>) -> Result<(), EngineError> {
        if self.doc.layer(id).is_none_or(|l| l.adjustment.is_some()) {
            return Err(EngineError::NoSuchLayer(id.into()));
        }
        let key = Some(format!("smart:{id}"));
        self.tree_edit(key, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                if filters.is_empty() {
                    l.extra.remove("filters");
                } else {
                    l.extra.insert("filters".into(), serde_json::to_value(&filters).unwrap_or_default());
                }
            }
            Ok(())
        })
    }

    pub(crate) fn add_smart_filter(&mut self, id: &str, kind: &str, params: Params) -> Result<(), EngineError> {
        let k = Kind::parse(kind)
            .filter(|k| is_smart_kind(*k))
            .ok_or_else(|| EngineError::Invalid(format!("{kind} can't be a smart filter")))?;
        let mut list = self.smart_filters(id);
        list.push(SmartFilter { id: format!("sf-{}", onepaint_doc::new_id()), kind: k.as_str().into(), params, enabled: None });
        self.history_sealed = true;
        self.set_smart_filters(id, list)?;
        self.history_sealed = true;
        Ok(())
    }

    /// Bake: the filtered result becomes the pixels and the stack (and any
    /// text/path specs, which would re-render unfiltered) is dropped.
    pub(crate) fn bake_smart_filters(&mut self, id: &str) -> Result<(), EngineError> {
        let Some(l) = self.doc.layer(id) else { return Err(EngineError::NoSuchLayer(id.into())) };
        let f = active_filters(l);
        let out = filtered(l, &f, self.doc.bounds(), Frame::region(0, 0, self.doc.width, self.doc.height, self.doc.dpi as f32));
        self.history_sealed = true;
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.pixels = out;
                for k in ["filters", "texts", "text", "paths"] {
                    l.extra.remove(k);
                }
            }
            Ok(())
        })
    }
}

/// Each stacked spatial layer plans the one below over its 3 × 3 tiles in
/// reach, so planning grows ninefold per layer: a full refresh of 64 tiles
/// plans in 0.8 ms with one, 7.7 ms with two, 63 ms with three (spec 52).
const MAX_LIVE_SPATIAL: usize = 3;

/// The largest document a whole-canvas layer runs live in (spec 53): its
/// backdrop tiles must fit half the segment cache, and its filtered image is
/// 8 bytes a pixel (128 MiB at 4096²).
const MAX_WHOLE_PX: u64 = 4096 * 4096;

/// Live spatial adjustment layers (blur, sharpen…): rendered as the filtered
/// backdrop of their container, cached by a fingerprint of everything below.
#[derive(Default)]
pub(crate) struct SpatialCache(std::sync::Mutex<HashMap<NodeId, (u64, Surface)>>);

fn hash_nodes(nodes: &[Node], s: &mut std::collections::hash_map::DefaultHasher) {
    for n in nodes {
        n.id().hash(s);
        n.visible().hash(s);
        n.opacity().to_bits().hash(s);
        n.blend_mode().as_str().hash(s);
        match n {
            Node::Group(g) => hash_nodes(&g.children, s),
            Node::Layer(l) => {
                (l.clipped, l.inherit_alpha, l.mask_enabled).hash(s);
                for (k, t) in l.pixels.tiles() {
                    k.hash(s);
                    (Arc::as_ptr(t) as usize).hash(s);
                }
                if let Some(m) = &l.mask {
                    for (k, t) in m.tiles() {
                        k.hash(s);
                        (Arc::as_ptr(t) as usize).hash(s);
                    }
                }
                if let Some(a) = &l.adjustment {
                    a.kind.hash(s);
                    serde_json::to_string(&a.params).unwrap_or_default().hash(s);
                }
            }
        }
    }
}

impl Session {
    pub(crate) fn has_spatial_adjustments(nodes: &[Node]) -> bool {
        nodes.iter().any(|n| match n {
            Node::Layer(l) => {
                l.visible && !l.clipped && l.adjustment.as_ref().and_then(|a| Kind::parse(&a.kind)).is_some_and(|k| !k.is_color())
            }
            Node::Group(g) => Self::has_spatial_adjustments(&g.children),
        })
    }

    /// The spatial layers the GPU filters live (spec 52): every spatial
    /// adjustment layer, when each is at the top level or in isolated groups,
    /// of a kind with a GPU pass and a reach within a tile, and at most
    /// [`MAX_LIVE_SPATIAL`] of them; or a whole-canvas layer (spec 53) that
    /// is the document's only spatial layer, in a document of at most
    /// [`MAX_WHOLE_PX`]. Otherwise none, so a stroke never shows a live layer
    /// over a stale one.
    pub(crate) fn live_spatial(doc: &onepaint_doc::Document) -> Vec<NodeId> {
        let nodes = &doc.root[..];
        // Each spatial layer, and whether every group around it is isolated.
        fn all<'a>(nodes: &'a [Node], isolated: bool, out: &mut Vec<(&'a onepaint_doc::Layer, bool)>) {
            for n in nodes {
                match n {
                    Node::Layer(l) if onepaint_compose::spatial::spatial_kind(l).is_some() => out.push((l, isolated)),
                    Node::Layer(_) => {}
                    Node::Group(g) => all(&g.children, isolated && g.blend_mode != onepaint_doc::BlendMode::PassThrough, out),
                }
            }
        }
        let mut found = Vec::new();
        all(nodes, true, &mut found);
        let whole_ok = found.len() == 1 && u64::from(doc.width) * u64::from(doc.height) <= MAX_WHOLE_PX;
        let live = |(l, isolated): &(&onepaint_doc::Layer, bool)| {
            let Some(kind) = onepaint_compose::spatial::spatial_kind(l) else { return false };
            let params = l.adjustment.as_ref().map(|a| a.params.clone()).unwrap_or_default();
            let r = onepaint_filters::reach(kind, &params);
            if r >= onepaint_filters::GLOBAL_REACH {
                return *isolated && whole_ok && onepaint_compose::spatial::whole_supported(kind, &params);
            }
            *isolated && onepaint_compose::spatial::gpu_supported(kind, &params) && r <= onepaint_raster::TILE
        };
        if found.len() > MAX_LIVE_SPATIAL || !found.iter().all(live) {
            return Vec::new();
        }
        found.into_iter().map(|(l, _)| l.id.clone()).collect()
    }

    /// Replace spatial adjustment layers in `nodes` (and nested groups) with
    /// their rendered result. `fresh` = false reuses stale cache entries.
    pub(crate) fn render_spatial_adjustments(&self, nodes: &mut [Node], fresh: bool) {
        self.render_spatial_except(nodes, fresh, &[])
    }

    /// [`Self::render_spatial_adjustments`], leaving `live` for the GPU.
    pub(crate) fn render_spatial_except(&self, nodes: &mut [Node], fresh: bool, live: &[NodeId]) {
        let (w, h) = (self.doc.width, self.doc.height);
        for i in 0..nodes.len() {
            if let Node::Group(g) = &mut nodes[i] {
                self.render_spatial_except(&mut g.children, fresh, live);
                continue;
            }
            let Node::Layer(l) = &nodes[i] else { continue };
            if live.contains(&l.id) {
                continue;
            }
            let Some(kind) = l.adjustment.as_ref().and_then(|a| Kind::parse(&a.kind)) else { continue };
            if kind.is_color() || !l.visible || l.clipped {
                continue;
            }
            let params = l.adjustment.as_ref().map(|a| a.params.clone()).unwrap_or_default();
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            hash_nodes(&nodes[..=i], &mut hs);
            (w, h).hash(&mut hs);
            let key = hs.finish();
            let id = l.id.clone();
            let mut cache = self.spatial_cache.0.lock().unwrap_or_else(|e| e.into_inner());
            let out = match cache.get(&id) {
                Some((k, s)) if *k == key || !fresh => s.clone(),
                _ => {
                    // The backdrop is everything below inside this (isolated) container.
                    let s = onepaint_compose::spatial::render_layer(&self.doc, &nodes[..i], kind, &params);
                    cache.insert(id.clone(), (key, s.clone()));
                    s
                }
            };
            drop(cache);
            if let Node::Layer(l) = &mut nodes[i] {
                l.adjustment = None;
                l.pixels = out;
            }
        }
    }
}
