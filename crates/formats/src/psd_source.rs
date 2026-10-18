//! A PSD as it was opened, so saving keeps what onepaint doesn't model
//! (happypaint `storage/psd/export.ts`: untouched layers export as they
//! were, and an untouched file byte for byte). The reader keeps each
//! record's bytes and a fingerprint of the layer or group made from it; the
//! writer reuses a record whose node still matches, and the whole file when
//! nothing changed.
//!
//! Tiles are copy-on-write, so an edit always gives a layer new tiles: the
//! fingerprint holds `Weak`s to the tiles it was read with (which also keep
//! their addresses from being reused).

use onepaint_doc::{Document, Group, Layer, Node, NodeId};
use onepaint_raster::{Surface, Tile};
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Weak};

/// One original record: its bytes, and its channels' image data.
#[derive(Debug, Clone)]
pub(crate) struct RawRecord {
    pub record: Range<usize>,
    pub channels: Vec<Range<usize>>,
}

#[derive(Debug)]
struct SurfaceFp(Vec<((i32, i32), Weak<Tile>)>);

impl SurfaceFp {
    fn of(s: &Surface) -> Self {
        Self(s.tile_keys().into_iter().filter_map(|k| s.tile(k.0, k.1).map(|t| (k, Arc::downgrade(t)))).collect())
    }

    fn matches(&self, s: &Surface) -> bool {
        let keys = s.tile_keys();
        keys.len() == self.0.len()
            && self
                .0
                .iter()
                .zip(keys)
                .all(|((k, w), k2)| *k == k2 && s.tile(k.0, k.1).is_some_and(|t| std::ptr::eq(w.as_ptr(), Arc::as_ptr(t))))
    }
}

#[derive(Debug)]
struct LayerFp {
    props: String,
    pixels: SurfaceFp,
    mask: Option<SurfaceFp>,
}

fn layer_props(l: &Layer) -> String {
    format!(
        "{:?}",
        (
            &l.name,
            l.visible,
            l.opacity.to_bits(),
            l.blend_mode,
            l.clipped,
            l.locked,
            l.alpha_lock,
            l.inherit_alpha,
            l.mask_enabled,
            l.height.is_some(),
            &l.adjustment,
            serde_json::to_string(&l.extra).unwrap_or_default(),
        )
    )
}

fn group_props(g: &Group) -> String {
    format!(
        "{:?}",
        (&g.name, g.visible, g.opacity.to_bits(), g.blend_mode, g.collapsed, serde_json::to_string(&g.extra).unwrap_or_default())
    )
}

impl LayerFp {
    fn of(l: &Layer) -> Self {
        Self { props: layer_props(l), pixels: SurfaceFp::of(&l.pixels), mask: l.mask.as_ref().map(SurfaceFp::of) }
    }

    fn matches(&self, l: &Layer) -> bool {
        self.props == layer_props(l)
            && self.pixels.matches(&l.pixels)
            && match (&self.mask, &l.mask) {
                (None, None) => true,
                (Some(f), Some(m)) => f.matches(m),
                _ => false,
            }
    }
}

#[derive(Debug)]
enum Fp {
    Layer(LayerFp),
    Group(String),
}

/// The records' order: a group's divider, or a layer or group by id.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Divider,
    Node(NodeId),
}

/// An opened 8-bit RGB PSD (not PSB), kept with its document.
#[derive(Debug)]
pub struct PsdSource {
    pub(crate) bytes: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    dpi: f64,
    /// The image resources section's bytes, without its length.
    pub(crate) resources: Range<usize>,
    /// What follows the layer info in the layer and mask section (the
    /// global layer mask and file-wide additional info: patterns, linked
    /// smart objects…).
    pub(crate) tail: Range<usize>,
    records: HashMap<NodeId, (RawRecord, Fp)>,
    order: Vec<Entry>,
}

impl PsdSource {
    pub(crate) fn new(bytes: Arc<Vec<u8>>, doc: &Document, resources: Range<usize>, tail: Range<usize>) -> Self {
        Self { bytes, width: doc.width, height: doc.height, dpi: doc.dpi, resources, tail, records: HashMap::new(), order: Vec::new() }
    }

    pub(crate) fn has_records(&self) -> bool {
        !self.records.is_empty()
    }

    /// Note a divider record (records go bottom to top, as in the file).
    pub(crate) fn divider(&mut self) {
        self.order.push(Entry::Divider);
    }

    /// Note the record a node came from; `fingerprint` once the tree is built.
    pub(crate) fn node(&mut self, id: &str, raw: RawRecord) {
        self.order.push(Entry::Node(id.to_string()));
        self.records.insert(id.to_string(), (raw, Fp::Group(String::new())));
    }

    /// Fingerprint every recorded node as the reader left it.
    pub(crate) fn fingerprint(&mut self, nodes: &[Node]) {
        for n in nodes {
            let fp = match n {
                Node::Layer(l) => Fp::Layer(LayerFp::of(l)),
                Node::Group(g) => {
                    self.fingerprint(&g.children);
                    Fp::Group(group_props(g))
                }
            };
            if let Some(e) = self.records.get_mut(n.id()) {
                e.1 = fp;
            }
        }
    }

    /// The source a document was opened from, if it can be written back.
    pub fn of(doc: &Document) -> Option<&PsdSource> {
        let s = doc.source.as_ref()?.downcast_ref::<PsdSource>()?;
        (s.width == doc.width && s.height == doc.height).then_some(s)
    }

    pub(crate) fn same_dpi(&self, doc: &Document) -> bool {
        self.dpi == doc.dpi
    }

    /// The original record of a node that still matches it.
    pub(crate) fn record(&self, n: &Node) -> Option<&RawRecord> {
        let (raw, fp) = self.records.get(n.id())?;
        let same = match (fp, n) {
            (Fp::Layer(f), Node::Layer(l)) => f.matches(l),
            (Fp::Group(f), Node::Group(g)) => *f == group_props(g),
            _ => false,
        };
        same.then_some(raw)
    }

    /// The original record bytes of a node that still matches it.
    pub fn record_bytes(&self, n: &Node) -> Option<&[u8]> {
        self.record(n).map(|r| &self.bytes[r.record.clone()])
    }

    /// Whether every node matches and they are in the original order.
    pub(crate) fn untouched(&self, doc: &Document) -> bool {
        fn walk(nodes: &[Node], src: &PsdSource, out: &mut Vec<Entry>) -> bool {
            for n in nodes {
                if let Node::Group(g) = n {
                    out.push(Entry::Divider);
                    if !walk(&g.children, src, out) {
                        return false;
                    }
                }
                if src.record(n).is_none() {
                    return false;
                }
                out.push(Entry::Node(n.id().to_string()));
            }
            true
        }
        let mut order = Vec::new();
        self.same_dpi(doc) && walk(&doc.root, self, &mut order) && order == self.order
    }
}
