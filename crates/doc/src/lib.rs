//! Pure-data document model: layer tree, layer properties, blend modes.
//!
//! `root[0]` is the bottom of the stack (happypaint `engine/document.ts:16`).
//! Fields onepaint does not model yet are kept in `extra` so files round-trip.

use onepaint_color::Rgba;
use onepaint_raster::{Rect, Surface};
use serde::{Deserialize, Serialize};

pub type NodeId = String;
pub type Extra = serde_json::Map<String, serde_json::Value>;

pub fn new_id() -> NodeId {
    uuid::Uuid::new_v4().to_string()
}

/// happypaint `engine/layer.ts:14-35`, in the same order.
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
}

impl BlendMode {
    pub const ALL: [BlendMode; 20] = {
        use BlendMode::*;
        [
            Normal, Multiply, Screen, Overlay, Add, Darken, Lighten, ColorDodge, ColorBurn,
            HardLight, SoftLight, Difference, Exclusion, LinearBurn, Divide, Hue, Saturation,
            Color, Luminosity, Pigment,
        ]
    };

    /// Modes offered for layers (everything but the brush-only `pigment`).
    pub const LAYER_MODES: [BlendMode; 19] = {
        let mut out = [BlendMode::Normal; 19];
        let mut i = 0;
        while i < 19 {
            out[i] = Self::ALL[i];
            i += 1;
        }
        out
    };

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
        }
    }

    pub fn label(self) -> &'static str {
        use BlendMode::*;
        match self {
            Normal => "Normal",
            Multiply => "Multiply",
            Screen => "Screen",
            Overlay => "Overlay",
            Add => "Add",
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
            Pigment => "Pigment",
        }
    }

    /// Unknown strings read as `normal`.
    pub fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|m| m.as_str() == s)
            .unwrap_or_default()
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
    pub extra: Extra,
    /// Opaque file chunks onepaint doesn't interpret yet (`height`, `overflow`), re-emitted on save.
    pub chunks: std::collections::BTreeMap<String, Vec<u8>>,
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
            extra: Extra::new(),
            chunks: Default::default(),
        }
    }
}

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
        both!(self, n => n.blend_mode = v)
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
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

impl Document {
    /// A Background layer (filled, clipped to the canvas) plus "Layer 1";
    /// only "Layer 1" when `background` is `None` (`document.ts:28-46`).
    pub fn new(width: u32, height: u32, background: Option<Rgba>) -> Self {
        let mut root = Vec::new();
        if let Some(bg) = background {
            let mut l = Layer::new("Background");
            l.pixels = Self::filled(width, height, bg.to_linear_premul());
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
            depth: Depth::Rgba16F,
            created_at: now,
            updated_at: now,
            root,
            extra: Extra::new(),
        }
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
                s.set_tile(
                    tx,
                    ty,
                    Some(std::sync::Arc::new(onepaint_raster::Tile::filled(p))),
                );
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
