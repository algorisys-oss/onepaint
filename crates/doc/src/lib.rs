//! Pure-data document model: layer tree, layer properties, blend modes.
//!
//! `root[0]` is the bottom of the stack (happypaint `engine/document.ts:16`).
//! Fields onepaint does not model yet are kept in `extra` so files round-trip.

use onepaint_color::Rgba;
use onepaint_raster::{Rect, Surface};
use serde::{Deserialize, Serialize};

pub mod look;
pub mod paper;
pub mod styles;
pub mod vector_mask;
pub use look::{BlendIf, BlendRange, ChannelRanges, FULL_RANGE, mask_k};
pub use paper::{PAPER_PRESETS, Paper, PaperPreset, paper_preset_id};
pub use vector_mask::{FillRule, VectorKnot, VectorMask, VectorOp, VectorPath};

pub type NodeId = String;
pub type Extra = serde_json::Map<String, serde_json::Value>;

pub fn new_id() -> NodeId {
    uuid::Uuid::new_v4().to_string()
}

/// happypaint `engine/layer.ts:14-50`. The order is the GPU code (`as u32`)
/// and matches happypaint's `BLEND_MODE_INDEX` through `Dissolve` (27);
/// menus use [`BlendMode::LAYER_MODES`] and friends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Add,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    LinearBurn,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
    /// Kubelka–Munk glaze; brush-only.
    Pigment,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Subtract,
    DarkerColor,
    LighterColor,
    /// Normal, with the coverage turned into a per-pixel random dither.
    Dissolve,
    /// Groups only: the children composite straight onto what is below.
    /// Anywhere else it reads as Normal.
    PassThrough,
}

/// A blend mode a file names that onepaint doesn't know is kept under this
/// key in the node's `extra`, so the file saves it back; it composites as
/// Normal meanwhile. Setting a mode clears it.
pub const RAW_BLEND_KEY: &str = "blendMode";

impl BlendMode {
    /// Every mode, in code order.
    pub const ALL: [BlendMode; 29] = {
        use BlendMode::*;
        [
            Normal,
            Multiply,
            Screen,
            Overlay,
            Add,
            Darken,
            Lighten,
            ColorDodge,
            ColorBurn,
            HardLight,
            SoftLight,
            Difference,
            Exclusion,
            LinearBurn,
            Divide,
            Hue,
            Saturation,
            Color,
            Luminosity,
            Pigment,
            VividLight,
            LinearLight,
            PinLight,
            HardMix,
            Subtract,
            DarkerColor,
            LighterColor,
            Dissolve,
            PassThrough,
        ]
    };

    /// Modes offered for layers, in Photoshop's menu order
    /// (`layer-panel/blend-modes.ts` BLEND_MODES).
    pub const LAYER_MODES: [BlendMode; 27] = {
        use BlendMode::*;
        [
            Normal,
            Dissolve,
            Darken,
            Multiply,
            ColorBurn,
            LinearBurn,
            DarkerColor,
            Lighten,
            Screen,
            ColorDodge,
            Add,
            LighterColor,
            Overlay,
            SoftLight,
            HardLight,
            VividLight,
            LinearLight,
            PinLight,
            HardMix,
            Difference,
            Exclusion,
            Subtract,
            Divide,
            Hue,
            Saturation,
            Color,
            Luminosity,
        ]
    };

    /// Modes offered for groups: Pass Through first (GROUP_BLEND_MODES).
    pub const GROUP_MODES: [BlendMode; 28] = {
        let mut out = [BlendMode::PassThrough; 28];
        let mut i = 0;
        while i < 27 {
            out[i + 1] = Self::LAYER_MODES[i];
            i += 1;
        }
        out
    };

    /// Modes offered for brushes: the layer modes plus Pigment Glaze
    /// (BRUSH_BLEND_MODES).
    pub const BRUSH_MODES: [BlendMode; 28] = {
        let mut out = [BlendMode::Pigment; 28];
        let mut i = 0;
        while i < 27 {
            out[i] = Self::LAYER_MODES[i];
            i += 1;
        }
        out
    };

    /// The mode a blend uses: Pass Through outside a passing-through group
    /// is Normal.
    #[inline]
    pub fn effective(self) -> Self {
        if self == BlendMode::PassThrough { BlendMode::Normal } else { self }
    }

    pub fn as_str(self) -> &'static str {
        use BlendMode::*;
        match self {
            Normal => "normal",
            Multiply => "multiply",
            Screen => "screen",
            Overlay => "overlay",
            Add => "add",
            Darken => "darken",
            Lighten => "lighten",
            ColorDodge => "color-dodge",
            ColorBurn => "color-burn",
            HardLight => "hard-light",
            SoftLight => "soft-light",
            Difference => "difference",
            Exclusion => "exclusion",
            LinearBurn => "linear-burn",
            Divide => "divide",
            Hue => "hue",
            Saturation => "saturation",
            Color => "color",
            Luminosity => "luminosity",
            Pigment => "pigment",
            VividLight => "vivid-light",
            LinearLight => "linear-light",
            PinLight => "pin-light",
            HardMix => "hard-mix",
            Subtract => "subtract",
            DarkerColor => "darker-color",
            LighterColor => "lighter-color",
            Dissolve => "dissolve",
            PassThrough => "pass-through",
        }
    }

    /// The menu label (`blend-modes.ts`).
    pub fn label(self) -> &'static str {
        use BlendMode::*;
        match self {
            Normal => "Normal",
            Multiply => "Multiply",
            Screen => "Screen",
            Overlay => "Overlay",
            Add => "Add (Linear Dodge)",
            Darken => "Darken",
            Lighten => "Lighten",
            ColorDodge => "Color Dodge",
            ColorBurn => "Color Burn",
            HardLight => "Hard Light",
            SoftLight => "Soft Light",
            Difference => "Difference",
            Exclusion => "Exclusion",
            LinearBurn => "Linear Burn",
            Divide => "Divide",
            Hue => "Hue",
            Saturation => "Saturation",
            Color => "Color",
            Luminosity => "Luminosity",
            Pigment => "Pigment Glaze",
            VividLight => "Vivid Light",
            LinearLight => "Linear Light",
            PinLight => "Pin Light",
            HardMix => "Hard Mix",
            Subtract => "Subtract",
            DarkerColor => "Darker Color",
            LighterColor => "Lighter Color",
            Dissolve => "Dissolve",
            PassThrough => "Pass Through",
        }
    }

    /// Unknown strings read as `normal` (see [`RAW_BLEND_KEY`]).
    pub fn parse(s: &str) -> Self {
        Self::ALL.into_iter().find(|m| m.as_str() == s).unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Depth {
    #[serde(rename = "8-bit")]
    Rgba8,
    #[default]
    #[serde(rename = "16-bit-float")]
    Rgba16F,
    #[serde(rename = "32-bit-float")]
    Rgba32F,
}

impl Depth {
    pub fn as_str(self) -> &'static str {
        match self {
            Depth::Rgba8 => "8-bit",
            Depth::Rgba16F => "16-bit-float",
            Depth::Rgba32F => "32-bit-float",
        }
    }

    pub fn bit_depth(self) -> u32 {
        match self {
            Depth::Rgba8 => 8,
            Depth::Rgba16F => 16,
            Depth::Rgba32F => 32,
        }
    }
}

/// An adjustment layer's effect (`adjustmentKind` + `adjustmentParams` in files).
/// The kind stays a string so kinds onepaint can't render yet are preserved.
#[derive(Clone, Debug, PartialEq)]
pub struct Adjustment {
    pub kind: String,
    pub params: onepaint_filters::Params,
}

/// A paintable layer.
#[derive(Clone, Debug)]
pub struct Layer {
    pub id: NodeId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub clipped: bool,
    pub locked: bool,
    pub alpha_lock: bool,
    pub inherit_alpha: bool,
    /// Coverage in R; missing tiles mean coverage 0 (masks are written in full).
    pub mask: Option<Surface>,
    pub mask_enabled: bool,
    pub pixels: Surface,
    /// Impasto paint height (spec 35), in every channel; `None` = flat.
    pub height: Option<Surface>,
    pub extra: Extra,
    /// Opaque file chunks onepaint doesn't interpret yet (`overflow`), re-emitted on save.
    pub chunks: std::collections::BTreeMap<String, Vec<u8>>,
    /// Set for adjustment and fill layers (they have no pixels of their own).
    pub adjustment: Option<Box<Adjustment>>,
}

impl Layer {
    pub fn new(name: &str) -> Self {
        Self {
            id: new_id(),
            name: name.to_string(),
            visible: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            clipped: false,
            locked: false,
            alpha_lock: false,
            inherit_alpha: false,
            mask: None,
            mask_enabled: true,
            pixels: Surface::new(),
            height: None,
            extra: Extra::new(),
            chunks: Default::default(),
            adjustment: None,
        }
    }

    /// The layer's knockout (happypaint `look-meta.ts`; groups have none).
    pub fn knockout(&self) -> Option<Knockout> {
        match self.extra.get(KNOCKOUT_KEY).and_then(|v| v.as_str()) {
            Some("shallow") => Some(Knockout::Shallow),
            Some("deep") => Some(Knockout::Deep),
            _ => None,
        }
    }

    /// Set or clear the knockout; `None` drops the key, as happypaint omits it.
    pub fn set_knockout(&mut self, k: Option<Knockout>) {
        match k {
            Some(k) => {
                self.extra.insert(KNOCKOUT_KEY.into(), serde_json::Value::String(k.as_str().into()));
            }
            None => {
                self.extra.remove(KNOCKOUT_KEY);
            }
        }
    }
}

/// Where a knocked-out layer's shape punches to: the bottom of its group, or
/// (at the top level) the Background. happypaint draws the two alike inside a
/// group: Deep stops at the innermost group rather than reaching the
/// Background (`compositor.ts` `knockoutPunch`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Knockout {
    Shallow,
    Deep,
}

impl Knockout {
    pub fn as_str(self) -> &'static str {
        match self {
            Knockout::Shallow => "shallow",
            Knockout::Deep => "deep",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Knockout::Shallow => "Shallow",
            Knockout::Deep => "Deep",
        }
    }
}

/// The `.happypaint` key a layer's knockout lives under, kept in `extra` so
/// the file's key order survives a round trip.
pub const KNOCKOUT_KEY: &str = "knockout";

#[derive(Clone, Debug)]
pub struct Group {
    pub id: NodeId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    pub collapsed: bool,
    pub children: Vec<Node>,
    pub extra: Extra,
}

impl Group {
    pub fn new(name: &str) -> Self {
        Self {
            id: new_id(),
            name: name.to_string(),
            visible: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            collapsed: false,
            children: Vec::new(),
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Node {
    Layer(Layer),
    Group(Group),
}

macro_rules! both {
    ($self:ident, $n:ident => $e:expr) => {
        match $self {
            Node::Layer($n) => $e,
            Node::Group($n) => $e,
        }
    };
}

impl Node {
    pub fn id(&self) -> &str {
        both!(self, n => &n.id)
    }
    pub fn name(&self) -> &str {
        both!(self, n => &n.name)
    }
    pub fn visible(&self) -> bool {
        both!(self, n => n.visible)
    }
    pub fn opacity(&self) -> f32 {
        both!(self, n => n.opacity)
    }
    pub fn blend_mode(&self) -> BlendMode {
        both!(self, n => n.blend_mode)
    }
    pub fn set_name(&mut self, v: String) {
        both!(self, n => n.name = v)
    }
    pub fn set_visible(&mut self, v: bool) {
        both!(self, n => n.visible = v)
    }
    pub fn set_opacity(&mut self, v: f32) {
        both!(self, n => n.opacity = v.clamp(0.0, 1.0))
    }
    pub fn set_blend_mode(&mut self, v: BlendMode) {
        both!(self, n => {
            n.blend_mode = v;
            n.extra.remove(RAW_BLEND_KEY);
        })
    }
    pub fn as_layer(&self) -> Option<&Layer> {
        match self {
            Node::Layer(l) => Some(l),
            Node::Group(_) => None,
        }
    }
    pub fn as_layer_mut(&mut self) -> Option<&mut Layer> {
        match self {
            Node::Layer(l) => Some(l),
            Node::Group(_) => None,
        }
    }
    pub fn as_group(&self) -> Option<&Group> {
        match self {
            Node::Group(g) => Some(g),
            Node::Layer(_) => None,
        }
    }
    pub fn is_group(&self) -> bool {
        matches!(self, Node::Group(_))
    }
    /// Fresh ids for this node and all descendants (for duplicate).
    pub fn reassign_ids(&mut self) {
        match self {
            Node::Layer(l) => l.id = new_id(),
            Node::Group(g) => {
                g.id = new_id();
                g.children.iter_mut().for_each(Node::reassign_ids);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub dpi: f64,
    pub color_space: String,
    pub depth: Depth,
    pub created_at: f64,
    pub updated_at: f64,
    pub root: Vec<Node>,
    /// Unmodelled `doc` keys (paper, animation, guides, savedSelections…).
    pub extra: Extra,
    /// The file the document was opened from, as its reader keeps it (a PSD's
    /// original records, so untouched layers save back as they were). Never
    /// saved; a format's writer downcasts it.
    pub source: Option<std::sync::Arc<dyn std::any::Any + Send + Sync>>,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
}

impl Document {
    /// A Background layer (filled, clipped to the canvas) plus "Layer 1";
    /// only "Layer 1" when `background` is `None` (`document.ts:28-46`).
    pub fn new(width: u32, height: u32, background: Option<Rgba>) -> Self {
        Self::new_in(width, height, background, Depth::Rgba16F)
    }

    /// [`Document::new`] at `depth`: an 8-bit document's background holds
    /// sRGB values (spec 50).
    pub fn new_in(width: u32, height: u32, background: Option<Rgba>, depth: Depth) -> Self {
        let space = if depth == Depth::Rgba8 { onepaint_color::Space::Gamma } else { onepaint_color::Space::Linear };
        let mut root = Vec::new();
        if let Some(bg) = background {
            let mut l = Layer::new("Background");
            l.pixels = Self::filled(width, height, space.pixel(bg));
            root.push(Node::Layer(l));
        }
        root.push(Node::Layer(Layer::new("Layer 1")));
        let now = now_ms();
        Self {
            id: new_id(),
            name: "Untitled".into(),
            width,
            height,
            dpi: 300.0,
            color_space: "srgb".into(),
            depth,
            created_at: now,
            updated_at: now,
            root,
            extra: Extra::new(),
            source: None,
        }
    }

    /// A copy of everything but the layer tree (which is left empty).
    pub fn clone_shallow(&self) -> Self {
        Self {
            id: self.id.clone(),
            name: self.name.clone(),
            width: self.width,
            height: self.height,
            dpi: self.dpi,
            color_space: self.color_space.clone(),
            depth: self.depth,
            created_at: self.created_at,
            updated_at: self.updated_at,
            root: Vec::new(),
            extra: Extra::new(),
            source: self.source.clone(),
        }
    }

    /// What the pixel values mean: 8-bit documents hold sRGB values and
    /// blend them as they are (spec 50); float documents hold linear light.
    pub fn space(&self) -> onepaint_color::Space {
        if self.depth == Depth::Rgba8 { onepaint_color::Space::Gamma } else { onepaint_color::Space::Linear }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// A surface filled with `p` inside the canvas rect only.
    pub fn filled(width: u32, height: u32, p: [f32; 4]) -> Surface {
        let mut s = Surface::new();
        let r = Rect::new(0, 0, width as i32, height as i32);
        for (tx, ty) in r.tiles() {
            let tr = Rect::from_tile(tx, ty);
            let Some(ix) = r.intersect(&tr) else { continue };
            if ix == tr {
                s.set_tile(tx, ty, Some(std::sync::Arc::new(onepaint_raster::Tile::filled(p))));
            } else {
                let t = s.tile_mut(tx, ty);
                for y in ix.y..ix.bottom() {
                    for x in ix.x..ix.right() {
                        t.set((x - tr.x) as usize, (y - tr.y) as usize, p);
                    }
                }
            }
        }
        s
    }

    /// Index path from the root to the node with `id`.
    pub fn path_of(&self, id: &str) -> Option<Vec<usize>> {
        fn walk(nodes: &[Node], id: &str, path: &mut Vec<usize>) -> bool {
            for (i, n) in nodes.iter().enumerate() {
                path.push(i);
                if n.id() == id {
                    return true;
                }
                if let Node::Group(g) = n
                    && walk(&g.children, id, path)
                {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut p = Vec::new();
        walk(&self.root, id, &mut p).then_some(p)
    }

    pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
        let (first, rest) = path.split_first()?;
        let mut n = self.root.get(*first)?;
        for &i in rest {
            n = n.as_group()?.children.get(i)?;
        }
        Some(n)
    }

    pub fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let (first, rest) = path.split_first()?;
        let mut n = self.root.get_mut(*first)?;
        for &i in rest {
            n = match n {
                Node::Group(g) => g.children.get_mut(i)?,
                Node::Layer(_) => return None,
            };
        }
        Some(n)
    }

    pub fn find(&self, id: &str) -> Option<&Node> {
        self.node_at(&self.path_of(id)?)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut Node> {
        let p = self.path_of(id)?;
        self.node_at_mut(&p)
    }

    pub fn layer(&self, id: &str) -> Option<&Layer> {
        self.find(id)?.as_layer()
    }

    pub fn layer_mut(&mut self, id: &str) -> Option<&mut Layer> {
        self.find_mut(id)?.as_layer_mut()
    }

    /// `Some(None)` = at root, `Some(Some(gid))` = inside group, `None` = not found.
    pub fn parent_of(&self, id: &str) -> Option<Option<NodeId>> {
        let p = self.path_of(id)?;
        if p.len() == 1 {
            return Some(None);
        }
        Some(Some(self.node_at(&p[..p.len() - 1])?.id().to_string()))
    }

    /// The child list of a parent (`None` = root).
    pub fn children_mut(&mut self, parent: Option<&str>) -> Option<&mut Vec<Node>> {
        match parent {
            None => Some(&mut self.root),
            Some(pid) => match self.find_mut(pid)? {
                Node::Group(g) => Some(&mut g.children),
                Node::Layer(_) => None,
            },
        }
    }

    pub fn children(&self, parent: Option<&str>) -> Option<&Vec<Node>> {
        match parent {
            None => Some(&self.root),
            Some(pid) => self.find(pid)?.as_group().map(|g| &g.children),
        }
    }

    /// Insert at `index` (clamped) in `parent`. Returns false if the parent is missing.
    pub fn insert(&mut self, parent: Option<&str>, index: usize, node: Node) -> bool {
        match self.children_mut(parent) {
            Some(c) => {
                let i = index.min(c.len());
                c.insert(i, node);
                true
            }
            None => false,
        }
    }

    /// Remove a node; returns it with its former parent and index.
    pub fn remove(&mut self, id: &str) -> Option<(Node, Option<NodeId>, usize)> {
        let path = self.path_of(id)?;
        let parent = self.parent_of(id)?;
        let idx = *path.last()?;
        let list = self.children_mut(parent.as_deref())?;
        Some((list.remove(idx), parent, idx))
    }

    /// All layer ids, depth-first, bottom to top.
    pub fn layer_ids(&self) -> Vec<NodeId> {
        fn walk(nodes: &[Node], out: &mut Vec<NodeId>) {
            for n in nodes {
                match n {
                    Node::Layer(l) => out.push(l.id.clone()),
                    Node::Group(g) => walk(&g.children, out),
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &mut out);
        out
    }

    pub fn touch(&mut self) {
        self.updated_at = now_ms();
    }
}
