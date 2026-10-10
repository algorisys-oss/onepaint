//! Where the toolbar and the panels live (happypaint `state/dock-store.ts`,
//! `state/float-groups.ts`, `utils/float-resize.ts` and the toolbar layout in
//! `state/ui-store.ts`). Pure data and geometry; `dock.rs` and `toolbar.rs`
//! draw it. Persisted as `layout.json` next to `settings.json`.

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DOCK_MIN_W: f32 = 190.0;
pub const DOCK_MAX_W: f32 = 400.0;
pub const DOCK_DEFAULT_W: f32 = 280.0;
/// Section flex weights stay in this range.
pub const WEIGHT_MIN: f32 = 0.15;
pub const WEIGHT_MAX: f32 = 8.0;
/// Dropping within this many px of a window edge docks there…
pub const EDGE_BAND: f32 = 56.0;
/// …as does a dragged element whose own edge comes within this many px.
pub const EDGE_NEAR: f32 = 28.0;
/// Floating windows keep this margin from the window edge.
pub const FLOAT_MARGIN: f32 = 4.0;

/// A panel that can float, dock or sit in its home spot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanelId {
    Layers,
    Properties,
    Brush,
    Color,
    Palettes,
    Align,
    Reference,
    Guides,
    ToolOptions,
}

/// Floating size limits: minimum, and whether height is the user's to set.
pub struct FloatResize {
    pub min: Vec2,
    pub height: bool,
}

impl PanelId {
    pub const ALL: [PanelId; 9] = [
        PanelId::Layers,
        PanelId::Properties,
        PanelId::Brush,
        PanelId::Color,
        PanelId::Palettes,
        PanelId::Align,
        PanelId::Reference,
        PanelId::Guides,
        PanelId::ToolOptions,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelId::Layers => "Layers",
            PanelId::Properties => "Properties",
            PanelId::Brush => "Brushes",
            PanelId::Color => "Colour",
            PanelId::Palettes => "Palettes",
            PanelId::Align => "Align",
            PanelId::Reference => "Reference",
            PanelId::Guides => "Guides",
            PanelId::ToolOptions => "Tool options",
        }
    }

    /// `FLOAT_RESIZE` (dock-store.ts:108-117); onepaint's extra panels follow suit.
    pub fn float_resize(self) -> FloatResize {
        let (w, h, height) = match self {
            PanelId::Layers => (220.0, 220.0, true),
            PanelId::Brush => (240.0, 200.0, true),
            PanelId::Reference => (160.0, 160.0, true),
            PanelId::Color => (210.0, 0.0, false),
            PanelId::Palettes => (220.0, 180.0, true),
            PanelId::Align => (200.0, 160.0, true),
            PanelId::Properties => (220.0, 160.0, true),
            PanelId::Guides => (240.0, 200.0, true),
            PanelId::ToolOptions => (220.0, 120.0, true),
        };
        FloatResize { min: vec2(w, h), height }
    }

    /// Home is a popover under its toolbar button (else a floating window).
    pub fn home_is_popover(self) -> bool {
        matches!(self, PanelId::Layers | PanelId::Brush | PanelId::Color | PanelId::Guides | PanelId::ToolOptions)
    }

    /// Where a floating window opens the first time, in a `view`-sized window.
    pub fn default_float(self, view: Vec2) -> Pos2 {
        let x = (view.x - 560.0).max(16.0);
        match self {
            PanelId::Reference => pos2(x - 40.0, 160.0),
            PanelId::Align => pos2(x, 220.0),
            PanelId::Palettes => pos2(x, 180.0),
            _ => pos2(x, 120.0),
        }
    }

    fn index(self) -> usize {
        PanelId::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    Left,
    Right,
}

impl DockSide {
    pub fn i(self) -> usize {
        match self {
            DockSide::Left => 0,
            DockSide::Right => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Placement {
    #[default]
    Home,
    Dock {
        side: DockSide,
    },
    Float {
        x: f32,
        y: f32,
    },
    Group {
        group: String,
    },
}

/// Floating panels tabbed together in one window.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FloatGroup {
    pub x: f32,
    pub y: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f32>,
    pub members: Vec<PanelId>,
    #[serde(default)]
    pub active: Option<PanelId>,
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    /// onepaint's default: a bar across the top right (happypaint: vertical).
    #[default]
    Horizontal,
    Vertical,
}

/// The tool bar: free-floating position, orientation and width.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ToolbarLayout {
    pub orientation: Orientation,
    /// Top-left in window px; `None` = the default spot.
    pub pos: Option<[f32; 2]>,
    /// User-set width (whole button columns); `None` = automatic.
    pub size: Option<f32>,
}

/// One button cell of the toolbar grid (34 px button + 4 px gap).
pub const TOOL_CELL: f32 = 38.0;
/// The toolbar's frame padding, both sides together.
pub const TOOL_PAD: f32 = 10.0;

/// Columns for a toolbar `width` (`round((w − pad) / cell)`, at least one).
pub fn toolbar_cols(width: f32) -> usize {
    (((width - TOOL_PAD) / TOOL_CELL).round() as usize).max(1)
}

/// The width that holds exactly `cols` columns.
pub fn toolbar_width(cols: usize) -> f32 {
    TOOL_PAD + cols.max(1) as f32 * TOOL_CELL
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Layout {
    #[serde(deserialize_with = "lenient_map")]
    pub placements: BTreeMap<PanelId, Placement>,
    /// Dock widths, left then right.
    pub width: [f32; 2],
    /// Docked panels top to bottom, left then right.
    #[serde(deserialize_with = "lenient_order")]
    pub order: [Vec<PanelId>; 2],
    #[serde(deserialize_with = "lenient_map")]
    pub weights: BTreeMap<PanelId, f32>,
    /// Folded to its header strip in a dock.
    #[serde(deserialize_with = "lenient_map")]
    pub collapsed: BTreeMap<PanelId, bool>,
    /// Folded to its title while floating (independent of `collapsed`).
    #[serde(deserialize_with = "lenient_map")]
    pub float_collapsed: BTreeMap<PanelId, bool>,
    /// Open docked or floating panels (popovers keep their own state).
    #[serde(deserialize_with = "lenient_map")]
    pub open: BTreeMap<PanelId, bool>,
    /// The tool bar docked as a rail on this side.
    pub tools_dock: Option<DockSide>,
    pub tabbed: [bool; 2],
    pub active_tab: [Option<PanelId>; 2],
    /// Floating sizes.
    #[serde(deserialize_with = "lenient_map")]
    pub sizes: BTreeMap<PanelId, [f32; 2]>,
    pub groups: BTreeMap<String, FloatGroup>,
    pub toolbar: ToolbarLayout,
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

impl Default for Layout {
    fn default() -> Self {
        let mut placements = BTreeMap::new();
        // Properties docks right, under Layers (dock-store.ts:119-139).
        placements.insert(PanelId::Properties, Placement::Dock { side: DockSide::Right });
        Self {
            placements,
            width: [DOCK_DEFAULT_W; 2],
            order: [Vec::new(), Vec::new()],
            weights: BTreeMap::new(),
            collapsed: BTreeMap::new(),
            float_collapsed: BTreeMap::new(),
            open: BTreeMap::new(),
            tools_dock: None,
            tabbed: [false; 2],
            active_tab: [None; 2],
            sizes: BTreeMap::new(),
            groups: BTreeMap::new(),
            toolbar: ToolbarLayout::default(),
            path: None,
        }
    }
}

/// A map keyed by panel id, dropping entries a newer or damaged file has
/// that this build can't read.
fn lenient_map<'de, D, T>(d: D) -> Result<BTreeMap<PanelId, T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let raw = BTreeMap::<String, serde_json::Value>::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| {
            let id = serde_json::from_value::<PanelId>(serde_json::Value::String(k)).ok()?;
            Some((id, serde_json::from_value(v).ok()?))
        })
        .collect())
}

fn lenient_order<'de, D>(d: D) -> Result<[Vec<PanelId>; 2], D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = <[Vec<serde_json::Value>; 2]>::deserialize(d)?;
    Ok(raw.map(|side| side.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()))
}

pub fn default_path() -> Option<PathBuf> {
    crate::settings::default_path().map(|p| p.with_file_name("layout.json"))
}

/// The rect a resize edge moves: `n`, `s`, `e`, `w` and the corners.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
}

impl Edge {
    pub const ALL: [Edge; 8] = [Edge::N, Edge::S, Edge::E, Edge::W, Edge::Ne, Edge::Nw, Edge::Se, Edge::Sw];

    fn parts(self) -> (bool, bool, bool, bool) {
        // (north, south, east, west)
        match self {
            Edge::N => (true, false, false, false),
            Edge::S => (false, true, false, false),
            Edge::E => (false, false, true, false),
            Edge::W => (false, false, false, true),
            Edge::Ne => (true, false, true, false),
            Edge::Nw => (true, false, false, true),
            Edge::Se => (false, true, true, false),
            Edge::Sw => (false, true, false, true),
        }
    }

    pub fn horizontal_only(self) -> bool {
        matches!(self, Edge::E | Edge::W)
    }
}

/// Resize `start` by dragging `edge` by `d`: the opposite edge stays put,
/// the size keeps to `min`, the rect to the view (less the margin), and
/// the result is whole px (`resizeRect`, utils/float-resize.ts).
pub fn resize_rect(edge: Edge, start: Rect, d: Vec2, min: Vec2, view: Vec2) -> Rect {
    let (n, s, e, w) = edge.parts();
    let m = FLOAT_MARGIN;
    let (mut x0, mut y0, mut x1, mut y1) = (start.left(), start.top(), start.right(), start.bottom());
    if e {
        x1 = (x1 + d.x).clamp(x0 + min.x, view.x - m);
    }
    if w {
        x0 = (x0 + d.x).clamp(m, x1 - min.x);
    }
    if s {
        y1 = (y1 + d.y).clamp(y0 + min.y, view.y - m);
    }
    if n {
        y0 = (y0 + d.y).clamp(m, y1 - min.y);
    }
    Rect::from_min_max(pos2(x0.round(), y0.round()), pos2(x1.round(), y1.round()))
}

/// A stored floating size, raised to the minimum; `None` if not finite.
pub fn clamp_float_size(size: [f32; 2], min: Vec2) -> Option<[f32; 2]> {
    size.iter().all(|v| v.is_finite()).then(|| [size[0].max(min.x), size[1].max(min.y)])
}

/// Which dock a drop over a header lands in (`tabDropSide`): edges count,
/// zero-width rects don't.
pub fn tab_drop_side(p: Pos2, heads: &[(DockSide, Rect)]) -> Option<DockSide> {
    heads.iter().find(|(_, r)| r.width() > 0.0 && r.height() > 0.0 && r.contains(p)).map(|(s, _)| *s)
}

/// The dock side a drop near a window edge lands in (`dropDockSide`).
pub fn drop_dock_side(p: Pos2, element: Option<Rect>, view: Vec2) -> Option<DockSide> {
    if p.x <= EDGE_BAND {
        return Some(DockSide::Left);
    }
    if p.x >= view.x - EDGE_BAND {
        return Some(DockSide::Right);
    }
    let r = element.filter(|r| r.width() > 0.0)?;
    if r.left() <= EDGE_NEAR {
        Some(DockSide::Left)
    } else if r.right() >= view.x - EDGE_NEAR {
        Some(DockSide::Right)
    } else {
        None
    }
}

/// The insertion slot for a drop at `y` among docked sections whose
/// vertical midpoints are `mids` (`dockInsertIndex`).
pub fn dock_insert_index(mids: &[f32], y: f32) -> usize {
    mids.iter().filter(|m| **m < y).count()
}

/// The last target (drawn on top) containing `p` (`floatTargetAt`).
pub fn float_target_at<K: Clone>(p: Pos2, targets: &[(K, Rect)]) -> Option<K> {
    targets.iter().rev().find(|(_, r)| r.contains(p)).map(|(k, _)| k.clone())
}

impl Layout {
    pub fn load(path: Option<PathBuf>) -> Self {
        let mut l = path.as_ref().and_then(|p| std::fs::read(p).ok()).map(|b| Self::parse(&b)).unwrap_or_default();
        l.path = path;
        l
    }

    /// Read a saved layout, keeping only sound values (`parseLayout`).
    pub fn parse(bytes: &[u8]) -> Self {
        let mut l: Layout = serde_json::from_slice(bytes).unwrap_or_default();
        l.sanitize();
        l
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(self) {
            let tmp = path.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, path);
            }
        }
    }

    fn sanitize(&mut self) {
        for w in &mut self.width {
            *w = if w.is_finite() { w.round().clamp(DOCK_MIN_W, DOCK_MAX_W) } else { DOCK_DEFAULT_W };
        }
        self.weights.retain(|_, w| w.is_finite() && *w > 0.0);
        for w in self.weights.values_mut() {
            *w = w.min(WEIGHT_MAX);
        }
        let sizes = std::mem::take(&mut self.sizes);
        self.sizes = sizes.into_iter().filter_map(|(id, s)| Some((id, clamp_float_size(s, id.float_resize().min)?))).collect();
        for p in self.placements.values_mut() {
            if let Placement::Float { x, y } = p
                && !(x.is_finite() && y.is_finite())
            {
                *p = Placement::Home;
            }
        }
        if let Some(p) = self.toolbar.pos
            && !(p[0].is_finite() && p[1].is_finite())
        {
            self.toolbar.pos = None;
        }
        self.toolbar.size = self.toolbar.size.filter(|s| s.is_finite() && *s >= toolbar_width(1));
        self.groups.retain(|_, g| g.x.is_finite() && g.y.is_finite());
        self.repair_order();
        self.repair_groups();
    }

    pub fn placement(&self, id: PanelId) -> Placement {
        self.placements.get(&id).cloned().unwrap_or_default()
    }

    pub fn dock_side_of(&self, id: PanelId) -> Option<DockSide> {
        match self.placement(id) {
            Placement::Dock { side } => Some(side),
            _ => None,
        }
    }

    pub fn is_open(&self, id: PanelId) -> bool {
        self.open.get(&id).copied().unwrap_or(false)
    }

    pub fn set_open(&mut self, id: PanelId, open: bool) {
        self.open.insert(id, open);
    }

    pub fn weight(&self, id: PanelId) -> f32 {
        self.weights.get(&id).copied().unwrap_or(1.0)
    }

    pub fn is_collapsed(&self, id: PanelId) -> bool {
        self.collapsed.get(&id).copied().unwrap_or(false)
    }

    pub fn is_float_collapsed(&self, id: PanelId) -> bool {
        self.float_collapsed.get(&id).copied().unwrap_or(false)
    }

    /// Panels docked to `side`, top to bottom; unordered ones go last.
    pub fn docked_to(&self, side: DockSide) -> Vec<PanelId> {
        let mut ids: Vec<PanelId> = PanelId::ALL.into_iter().filter(|id| self.dock_side_of(*id) == Some(side)).collect();
        let order = &self.order[side.i()];
        ids.sort_by_key(|id| (order.iter().position(|o| o == id).unwrap_or(99), id.index()));
        ids
    }

    /// Move a panel to `p`; docking inserts it at `index` (default: last).
    pub fn set_placement(&mut self, id: PanelId, p: Placement, index: Option<usize>) {
        if let Some(Placement::Group { group }) = self.placements.get(&id).cloned()
            && !matches!(&p, Placement::Group { group: g } if *g == group)
        {
            self.leave_group(id);
        }
        for o in &mut self.order {
            o.retain(|x| *x != id);
        }
        if let Placement::Dock { side } = p {
            let o = &mut self.order[side.i()];
            let i = index.unwrap_or(o.len()).min(o.len());
            o.insert(i, id);
        }
        self.placements.insert(id, p);
    }

    pub fn set_dock_width(&mut self, side: DockSide, w: f32) {
        self.width[side.i()] = w.round().clamp(DOCK_MIN_W, DOCK_MAX_W);
    }

    pub fn set_weights(&mut self, a: PanelId, b: PanelId, wa: f32, wb: f32) {
        self.weights.insert(a, wa.clamp(WEIGHT_MIN, WEIGHT_MAX));
        self.weights.insert(b, wb.clamp(WEIGHT_MIN, WEIGHT_MAX));
    }

    /// The tab shown in a tabbed dock: the stored one if open, else the first.
    pub fn active_dock_tab(&self, side: DockSide, open: &[PanelId]) -> Option<PanelId> {
        self.active_tab[side.i()].filter(|id| open.contains(id)).or_else(|| open.first().copied())
    }

    pub fn set_active_dock_tab(&mut self, side: DockSide, id: PanelId) {
        self.active_tab[side.i()] = Some(id);
    }

    /// Drop `dragged` onto `target`'s floating window: start or join its
    /// tab group, with `dragged` active (`joinGroup`). `target_rect` is the
    /// target window's rect, which a new group takes.
    pub fn join_group(&mut self, dragged: PanelId, target: PanelId, target_rect: Rect) {
        if dragged == target {
            return;
        }
        let key = match self.placement(target) {
            Placement::Group { group } if self.groups.contains_key(&group) => group,
            _ => {
                let key = format!("g{}", (0..).find(|i| !self.groups.contains_key(&format!("g{i}"))).unwrap_or(0));
                self.groups.insert(
                    key.clone(),
                    FloatGroup {
                        x: target_rect.left(),
                        y: target_rect.top(),
                        w: Some(target_rect.width()),
                        h: Some(target_rect.height()),
                        members: vec![target],
                        active: Some(target),
                        collapsed: false,
                    },
                );
                for o in &mut self.order {
                    o.retain(|x| *x != target);
                }
                self.placements.insert(target, Placement::Group { group: key.clone() });
                key
            }
        };
        if self.placement(dragged) == (Placement::Group { group: key.clone() }) {
            return;
        }
        self.set_placement(dragged, Placement::Group { group: key.clone() }, None);
        if let Some(g) = self.groups.get_mut(&key) {
            g.members.push(dragged);
            g.active = Some(dragged);
        }
    }

    /// Take a panel out of its group (it goes home). A group left with one
    /// member dissolves: the survivor floats where the group was and gets
    /// its size. Returns that survivor.
    pub fn leave_group(&mut self, id: PanelId) -> Option<PanelId> {
        let Placement::Group { group } = self.placement(id) else { return None };
        self.placements.insert(id, Placement::Home);
        let g = self.groups.get_mut(&group)?;
        g.members.retain(|m| *m != id);
        if g.active == Some(id) {
            g.active = g.members.first().copied();
        }
        if g.members.len() >= 2 {
            return None;
        }
        let g = self.groups.remove(&group)?;
        let survivor = *g.members.first()?;
        self.placements.insert(survivor, Placement::Float { x: g.x, y: g.y });
        if let (Some(w), Some(h)) = (g.w, g.h) {
            self.sizes.insert(survivor, [w, h]);
        }
        Some(survivor)
    }

    /// The open member shown in a group (`activeGroupTab`).
    pub fn active_group_tab(&self, key: &str, is_open: impl Fn(PanelId) -> bool) -> Option<PanelId> {
        let g = self.groups.get(key)?;
        g.active.filter(|a| is_open(*a)).or_else(|| g.members.iter().copied().find(|m| is_open(*m)))
    }

    /// Make groups and placements agree (`repairGroups`).
    pub fn repair_groups(&mut self) {
        let keys: Vec<String> = self.groups.keys().cloned().collect();
        for key in keys {
            let mut seen = Vec::new();
            let members: Vec<PanelId> = self.groups[&key].members.clone();
            for m in members {
                if !seen.contains(&m) && self.placement(m) == (Placement::Group { group: key.clone() }) {
                    seen.push(m);
                }
            }
            if seen.len() < 2 {
                for m in &seen {
                    self.placements.insert(*m, Placement::Home);
                }
                self.groups.remove(&key);
                continue;
            }
            let g = self.groups.get_mut(&key).expect("key came from the map");
            g.members = seen;
            if !g.active.is_some_and(|a| g.members.contains(&a)) {
                g.active = g.members.first().copied();
            }
        }
        for p in self.placements.values_mut() {
            if let Placement::Group { group } = p
                && !self.groups.contains_key(group)
            {
                *p = Placement::Home;
            }
        }
    }

    fn repair_order(&mut self) {
        for side in [DockSide::Left, DockSide::Right] {
            let mut seen = Vec::new();
            for id in self.order[side.i()].clone() {
                if !seen.contains(&id) {
                    seen.push(id);
                }
            }
            self.order[side.i()] = seen;
        }
    }

    /// Back to the defaults, keeping where it saves (`resetPanelLayout`).
    pub fn reset(&mut self) {
        let path = self.path.take();
        *self = Layout { path, ..Layout::default() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    #[test]
    fn resize_rect_cases() {
        let start = r(100.0, 100.0, 300.0, 400.0);
        let (min, view) = (vec2(200.0, 150.0), vec2(1200.0, 800.0));
        let rs = |e, dx, dy| resize_rect(e, start, vec2(dx, dy), min, view);
        assert_eq!(rs(Edge::E, 50.0, 0.0), r(100.0, 100.0, 350.0, 400.0));
        assert_eq!(rs(Edge::S, 0.0, 40.0), r(100.0, 100.0, 300.0, 440.0));
        assert_eq!(rs(Edge::W, -60.0, 0.0), r(40.0, 100.0, 360.0, 400.0));
        assert_eq!(rs(Edge::N, 0.0, 30.0), r(100.0, 130.0, 300.0, 370.0));
        assert_eq!(rs(Edge::Se, 20.0, 30.0), r(100.0, 100.0, 320.0, 430.0));
        assert_eq!(rs(Edge::Nw, -20.0, -30.0), r(80.0, 70.0, 320.0, 430.0));
        assert_eq!(rs(Edge::E, -500.0, 0.0).width(), 200.0);
        assert_eq!(rs(Edge::W, 500.0, 0.0), r(200.0, 100.0, 200.0, 400.0));
        assert_eq!(rs(Edge::E, 5000.0, 0.0).width(), 1096.0);
        assert_eq!(rs(Edge::W, -5000.0, 0.0), r(4.0, 100.0, 396.0, 400.0));
        assert_eq!(rs(Edge::N, 0.0, -5000.0), r(100.0, 4.0, 300.0, 496.0));
        assert_eq!(rs(Edge::Se, 10.6, 3.2), r(100.0, 100.0, 311.0, 403.0));
    }

    #[test]
    fn clamp_float_size_cases() {
        let min = vec2(200.0, 150.0);
        assert_eq!(clamp_float_size([320.0, 500.0], min), Some([320.0, 500.0]));
        assert_eq!(clamp_float_size([10.0, 20.0], min), Some([200.0, 150.0]));
        assert_eq!(clamp_float_size([f32::NAN, 20.0], min), None);
    }

    #[test]
    fn tab_drop_side_cases() {
        let heads =
            [(DockSide::Right, Rect::from_min_max(pos2(1160.0, 40.0), pos2(1440.0, 68.0))), (DockSide::Left, r(0.0, 40.0, 280.0, 28.0))];
        assert_eq!(tab_drop_side(pos2(1300.0, 50.0), &heads), Some(DockSide::Right));
        assert_eq!(tab_drop_side(pos2(140.0, 50.0), &heads), Some(DockSide::Left));
        assert_eq!(tab_drop_side(pos2(1300.0, 400.0), &heads), None);
        assert_eq!(tab_drop_side(pos2(1160.0, 40.0), &heads), Some(DockSide::Right), "edges count");
        assert_eq!(tab_drop_side(pos2(1440.0, 68.0), &heads), Some(DockSide::Right));
        assert_eq!(tab_drop_side(pos2(1159.0, 50.0), &heads), None);
        assert_eq!(tab_drop_side(pos2(1300.0, 69.0), &heads), None);
        assert_eq!(tab_drop_side(pos2(1300.0, 50.0), &[]), None);
        assert_eq!(tab_drop_side(pos2(0.0, 50.0), &[(DockSide::Left, r(0.0, 40.0, 0.0, 28.0))]), None, "zero width");
    }

    #[test]
    fn drop_dock_side_bands() {
        let view = vec2(1400.0, 900.0);
        assert_eq!(drop_dock_side(pos2(30.0, 300.0), None, view), Some(DockSide::Left));
        assert_eq!(drop_dock_side(pos2(1360.0, 300.0), None, view), Some(DockSide::Right));
        assert_eq!(drop_dock_side(pos2(700.0, 300.0), None, view), None);
        assert_eq!(
            drop_dock_side(pos2(700.0, 300.0), Some(r(1100.0, 100.0, 280.0, 300.0)), view),
            Some(DockSide::Right),
            "its edge is near"
        );
        assert_eq!(drop_dock_side(pos2(700.0, 300.0), Some(r(20.0, 100.0, 0.0, 300.0)), view), None, "a detached element");
    }

    #[test]
    fn dock_tabs_and_order() {
        let mut l = Layout::default();
        assert_eq!(l.tabbed, [false, false]);
        assert_eq!(l.active_dock_tab(DockSide::Right, &[PanelId::Layers, PanelId::Color]), Some(PanelId::Layers));
        l.set_active_dock_tab(DockSide::Right, PanelId::Color);
        assert_eq!(l.active_dock_tab(DockSide::Right, &[PanelId::Layers, PanelId::Color]), Some(PanelId::Color));
        assert_eq!(l.active_dock_tab(DockSide::Right, &[PanelId::Layers, PanelId::Palettes]), Some(PanelId::Layers));
        assert_eq!(l.active_dock_tab(DockSide::Right, &[]), None);
        l.set_active_dock_tab(DockSide::Left, PanelId::Palettes);
        assert_eq!(l.active_tab, [Some(PanelId::Palettes), Some(PanelId::Color)], "sides keep their own tab");
        // Docking inserts at the slot; re-docking moves it.
        l.set_placement(PanelId::Layers, Placement::Dock { side: DockSide::Right }, Some(0));
        assert_eq!(l.docked_to(DockSide::Right), vec![PanelId::Layers, PanelId::Properties]);
        l.set_placement(PanelId::Align, Placement::Dock { side: DockSide::Right }, Some(1));
        assert_eq!(l.docked_to(DockSide::Right), vec![PanelId::Layers, PanelId::Align, PanelId::Properties]);
        l.set_placement(PanelId::Align, Placement::Dock { side: DockSide::Left }, None);
        assert_eq!(l.docked_to(DockSide::Left), vec![PanelId::Align]);
        assert!(!l.order[1].contains(&PanelId::Align));
        assert_eq!(dock_insert_index(&[100.0, 300.0, 500.0], 320.0), 2);
    }

    #[test]
    fn widths_and_weights_clamp() {
        let mut l = Layout::default();
        l.set_dock_width(DockSide::Left, 1000.0);
        l.set_dock_width(DockSide::Right, 10.4);
        assert_eq!(l.width, [DOCK_MAX_W, DOCK_MIN_W]);
        l.set_weights(PanelId::Layers, PanelId::Color, 0.01, 99.0);
        assert_eq!((l.weight(PanelId::Layers), l.weight(PanelId::Color)), (WEIGHT_MIN, WEIGHT_MAX));
    }

    #[test]
    fn defaults_and_parse() {
        let l = Layout::parse(b"null");
        assert_eq!(l.placement(PanelId::Properties), Placement::Dock { side: DockSide::Right });
        assert!(!l.is_open(PanelId::Properties) && !l.is_collapsed(PanelId::Properties));
        let saved = br#"{
            "placements": {"layers": {"kind":"float","x":900,"y":80}, "align": {"kind":"dock","side":"left"}},
            "order": [["from-the-future", "align"], []],
            "open": {"layers": true, "align": true},
            "floatCollapsed": {"layers": true},
            "tabbed": [true, false],
            "width": [5000, 232],
            "sizes": {"brush": [5, 5]},
            "toolbar": {"orientation": "vertical", "pos": [10, 60], "size": 86}
        }"#;
        let l = Layout::parse(saved);
        assert_eq!(l.placement(PanelId::Layers), Placement::Float { x: 900.0, y: 80.0 });
        assert_eq!(l.placement(PanelId::Align), Placement::Dock { side: DockSide::Left });
        assert!(l.is_open(PanelId::Layers) && l.is_open(PanelId::Align));
        assert_eq!(l.tabbed, [true, false]);
        assert_eq!(l.width, [DOCK_MAX_W, 232.0]);
        assert_eq!(l.sizes.get(&PanelId::Brush), Some(&[240.0, 200.0]), "raised to the minimum");
        assert_eq!(l.toolbar.orientation, Orientation::Vertical);
        assert_eq!(toolbar_cols(86.0), 2);
        // A damaged file loads as the defaults (the order key is unreadable).
        assert_eq!(Layout::parse(b"{\"order\": 3}").placements, Layout::default().placements);
        // Round trip.
        let json = serde_json::to_vec(&l).unwrap();
        assert_eq!(Layout::parse(&json), l);
    }

    #[test]
    fn float_groups_join_leave_and_repair() {
        let mut l = Layout::default();
        let rect = r(400.0, 60.0, 300.0, 400.0);
        l.set_placement(PanelId::Layers, Placement::Float { x: 400.0, y: 60.0 }, None);
        l.join_group(PanelId::Align, PanelId::Layers, rect);
        let key = match l.placement(PanelId::Layers) {
            Placement::Group { group } => group,
            p => panic!("{p:?}"),
        };
        let g = &l.groups[&key];
        assert_eq!(g.members, vec![PanelId::Layers, PanelId::Align]);
        assert_eq!((g.x, g.y, g.active), (400.0, 60.0, Some(PanelId::Align)));
        // Joining a grouped panel's title joins its group.
        l.join_group(PanelId::Color, PanelId::Align, rect);
        assert_eq!(l.groups[&key].members, vec![PanelId::Layers, PanelId::Align, PanelId::Color]);
        assert_eq!(l.groups.len(), 1);
        // Joining yourself or your own group changes nothing.
        let before = l.clone();
        l.join_group(PanelId::Color, PanelId::Color, rect);
        l.join_group(PanelId::Color, PanelId::Layers, rect);
        assert_eq!(l, before);
        // Three minus one keeps the group; active moves on.
        assert_eq!(l.leave_group(PanelId::Color), None);
        assert_eq!(l.groups[&key].active, Some(PanelId::Layers));
        assert_eq!(l.placement(PanelId::Color), Placement::Home);
        // Two minus one dissolves it: the survivor floats where it was.
        assert_eq!(l.leave_group(PanelId::Layers), Some(PanelId::Align));
        assert!(l.groups.is_empty());
        assert_eq!(l.placement(PanelId::Align), Placement::Float { x: 400.0, y: 60.0 });
        assert_eq!(l.sizes.get(&PanelId::Align), Some(&[300.0, 400.0]));
        assert_eq!(l.leave_group(PanelId::Color), None, "not grouped");
        // Docking a grouped panel takes it out of its group.
        l.join_group(PanelId::Layers, PanelId::Align, rect);
        l.set_placement(PanelId::Layers, Placement::Dock { side: DockSide::Left }, None);
        assert!(l.groups.is_empty());
        assert!(matches!(l.placement(PanelId::Align), Placement::Float { .. }));
    }

    #[test]
    fn repair_groups_fixes_damage() {
        let mut l = Layout::default();
        let g = |members: Vec<PanelId>| FloatGroup {
            x: 0.0,
            y: 0.0,
            w: None,
            h: None,
            members,
            active: Some(PanelId::Guides),
            collapsed: false,
        };
        l.groups.insert("a".into(), g(vec![PanelId::Layers, PanelId::Layers, PanelId::Align, PanelId::Color]));
        l.groups.insert("b".into(), g(vec![PanelId::Brush]));
        l.placements.insert(PanelId::Layers, Placement::Group { group: "a".into() });
        l.placements.insert(PanelId::Align, Placement::Group { group: "a".into() });
        l.placements.insert(PanelId::Brush, Placement::Group { group: "b".into() });
        l.placements.insert(PanelId::Palettes, Placement::Group { group: "gone".into() });
        l.repair_groups();
        assert_eq!(l.groups["a"].members, vec![PanelId::Layers, PanelId::Align], "duplicates and strays removed");
        assert_eq!(l.groups["a"].active, Some(PanelId::Layers));
        assert!(!l.groups.contains_key("b"));
        assert_eq!(l.placement(PanelId::Brush), Placement::Home);
        assert_eq!(l.placement(PanelId::Palettes), Placement::Home);
        assert_eq!(l.active_group_tab("a", |p| p == PanelId::Align), Some(PanelId::Align));
        assert_eq!(l.active_group_tab("a", |_| false), None);
    }

    #[test]
    fn float_target_last_wins() {
        let t = [(1, r(0.0, 0.0, 100.0, 30.0)), (2, r(50.0, 0.0, 100.0, 30.0))];
        assert_eq!(float_target_at(pos2(60.0, 10.0), &t), Some(2));
        assert_eq!(float_target_at(pos2(10.0, 10.0), &t), Some(1));
        assert_eq!(float_target_at(pos2(10.0, 50.0), &t), None);
    }
}
