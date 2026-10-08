//! Comic panels (spec 30, paint-engine.ts:6606-6895).
//!
//! A panel is a top-level group with a `panel` spec; the compositor clips its
//! composite to the frame. Every operation is one undo step and rebuilds the
//! page's border ink, one path layer above the panels, in the same step.

use super::*;
use onepaint_panel::geometry::{
    Pt, grid_outlines, panel_border_outline, point_in_polygon, polygon_area, rect_outline, split_polygon_by_line,
};
use onepaint_panel::{BORDER_BRUSH, BORDER_LAYER, PanelSpec, hex_rgb, panel_of, set_panel};
use onepaint_stroke::bezier::{PathAnchor, PathSpec, PathStroke, Subpath};

/// A drag smaller than this (doc px²) is a tap, not a frame (PE:6749).
const MIN_AREA: f64 = 4.0;

fn pt(p: [f64; 2]) -> Pt {
    Pt::new(p[0], p[1])
}

/// The border path objects for `panels`: one closed subpath per bordered
/// panel, one object per distinct width and colour (PE:6705-6740).
fn border_specs(panels: &[PanelSpec]) -> Vec<PathSpec> {
    let mut out: Vec<PathSpec> = Vec::new();
    for p in panels {
        let size = p.border();
        if size <= 0.0 {
            continue;
        }
        let Some(ring) = panel_border_outline(&p.outline, p.gutter()) else { continue };
        let sub = Subpath { anchors: ring.iter().map(|q| PathAnchor::corner(q.x, q.y)).collect(), closed: true };
        let color = hex_rgb(p.border_color());
        let same = |s: &&mut PathSpec| s.stroke.as_ref().is_some_and(|st| st.size == size && st.color == color);
        match out.iter_mut().find(same) {
            Some(spec) => spec.subpaths.push(sub),
            None => out.push(PathSpec {
                subpaths: vec![sub],
                stroke: Some(PathStroke { brush_id: BORDER_BRUSH.into(), size, color, opacity: 1.0, seed: None }),
                fill: None,
            }),
        }
    }
    out
}

impl Session {
    /// Every top-level panel's id and frame, bottom to top (PE:6880-6886).
    pub fn panel_frames(&self) -> Vec<(NodeId, Vec<Pt>)> {
        self.doc
            .root
            .iter()
            .filter_map(|n| {
                let g = n.as_group()?;
                Some((g.id.clone(), panel_of(g)?.outline))
            })
            .collect()
    }

    /// The topmost panel whose frame contains `p` (PE:6888-6895).
    pub fn panel_at(&self, p: [f64; 2]) -> Option<NodeId> {
        self.panel_frames().into_iter().rev().find(|(_, o)| point_in_polygon(pt(p), o)).map(|(id, _)| id)
    }

    /// The panel spec of group `id`, if it is a panel.
    pub fn panel(&self, id: &str) -> Option<PanelSpec> {
        self.doc.find(id)?.as_group().and_then(panel_of)
    }

    fn panel_count(&self) -> usize {
        self.panel_frames().len()
    }

    /// A panel group holding one empty "Art" layer; returns it and the art id.
    fn new_panel(n: usize, spec: PanelSpec) -> (Group, NodeId) {
        let mut g = Group::new(&format!("Panel {n}"));
        set_panel(&mut g, Some(spec));
        let art = Layer::new("Art");
        let art_id = art.id.clone();
        g.children.push(Node::Layer(art));
        (g, art_id)
    }

    /// Regenerate the "Panel borders" path layer from the panels that exist now.
    /// Called inside a tree edit, so it shares that undo step.
    fn rebuild_panel_borders(&mut self) {
        let panels: Vec<PanelSpec> = self.doc.root.iter().filter_map(|n| n.as_group().and_then(panel_of)).collect();
        let specs = border_specs(&panels);
        let existing =
            self.doc.root.iter().position(|n| n.as_layer().is_some_and(|l| l.name == BORDER_LAYER && l.extra.contains_key("paths")));
        let (w, h) = (self.doc.width, self.doc.height);
        match (existing, specs.is_empty()) {
            (Some(i), true) => {
                let id = self.doc.root.remove(i).id().to_string();
                if self.active == id {
                    self.active = self.doc.layer_ids().last().cloned().unwrap_or_default();
                }
            }
            (Some(i), false) => {
                if let Some(Node::Layer(l)) = self.doc.root.get_mut(i) {
                    path_ops::write_paths(l, &specs, w, h);
                }
            }
            (None, false) => {
                let mut l = Layer::new(BORDER_LAYER);
                path_ops::write_paths(&mut l, &specs, w, h);
                self.doc.root.push(Node::Layer(l));
            }
            (None, true) => {}
        }
    }

    /// Lay out a `rows × cols` page of panels (PE:6621-6685).
    pub(crate) fn create_panel_grid(&mut self, rows: u32, cols: u32, gutter: f64, margin: f64, border: f64) -> Result<(), EngineError> {
        let cells = grid_outlines(self.doc.width as f64, self.doc.height as f64, rows, cols, margin.max(0.0));
        if cells.is_empty() {
            return Err(EngineError::Invalid("the margin leaves no room for panels".into()));
        }
        let (gutter, border) = (gutter.max(0.0), border.max(0.0));
        self.tree_edit(None, |s| {
            let first = s.panel_count() + 1;
            let mut active = None;
            for (i, outline) in cells.into_iter().enumerate() {
                let (g, art) = Self::new_panel(first + i, PanelSpec::new(outline, gutter, border));
                active.get_or_insert(art);
                s.doc.root.push(Node::Group(g));
            }
            s.rebuild_panel_borders();
            if let Some(a) = active {
                s.active = a;
            }
            Ok(())
        })
    }

    /// One frame from a drag rectangle (PE:6746-6770).
    pub(crate) fn create_panel(&mut self, a: [f64; 2], b: [f64; 2], gutter: f64, border: f64) -> Result<(), EngineError> {
        let outline = rect_outline(pt(a), pt(b));
        if polygon_area(&outline).abs() < MIN_AREA {
            return Err(EngineError::Invalid("a panel needs an area".into()));
        }
        self.tree_edit(None, |s| {
            let (g, art) = Self::new_panel(s.panel_count() + 1, PanelSpec::new(outline, gutter.max(0.0), border.max(0.0)));
            s.doc.root.push(Node::Group(g));
            s.rebuild_panel_borders();
            s.active = art;
            Ok(())
        })
    }

    /// Cut panel `id` along the line a→b. The group keeps its art and the left
    /// half; a new empty panel above it takes the right (PE:6780-6820).
    pub(crate) fn split_panel(&mut self, id: &str, a: [f64; 2], b: [f64; 2]) -> Result<(), EngineError> {
        let base = self.panel(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let (left, right) = split_polygon_by_line(&base.outline, pt(a), pt(b));
        if left.len() < 3 || right.len() < 3 {
            return Err(EngineError::Invalid("the cut misses the panel".into()));
        }
        let index = self.doc.root.iter().position(|n| n.id() == id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        self.tree_edit(None, |s| {
            if let Some(Node::Group(g)) = s.doc.root.get_mut(index) {
                set_panel(g, Some(PanelSpec { outline: left, ..base.clone() }));
            }
            let (other, _) = Self::new_panel(s.panel_count() + 1, PanelSpec { outline: right, ..base });
            s.doc.root.insert(index + 1, Node::Group(other));
            s.rebuild_panel_borders();
            Ok(())
        })
    }

    /// Replace one panel's outline. Live edits coalesce into one undo step
    /// until `SealHistory` (PE:6835-6880). The art never moves.
    pub(crate) fn set_panel_outline(&mut self, id: &str, outline: Vec<[f64; 2]>) -> Result<(), EngineError> {
        let base = self.panel(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let outline: Vec<Pt> = outline.into_iter().map(pt).collect();
        if outline.len() < 3 || polygon_area(&outline).abs() < MIN_AREA {
            return Ok(());
        }
        let id = id.to_string();
        self.tree_edit(Some(format!("panel:{id}")), |s| {
            if let Some(Node::Group(g)) = s.doc.find_mut(&id) {
                set_panel(g, Some(PanelSpec { outline, ..base }));
            }
            s.rebuild_panel_borders();
            Ok(())
        })
    }

    /// Change the gutter and/or border width of one panel, or of every panel
    /// with `id = None`. Live edits coalesce until `SealHistory`.
    pub(crate) fn set_panel_style(&mut self, id: Option<&str>, gutter: Option<f64>, border: Option<f64>) -> Result<(), EngineError> {
        let ids: Vec<NodeId> = match id {
            Some(i) => {
                self.panel(i).ok_or_else(|| EngineError::NoSuchLayer(i.into()))?;
                vec![i.to_string()]
            }
            None => self.panel_frames().into_iter().map(|(i, _)| i).collect(),
        };
        if ids.is_empty() {
            return Ok(());
        }
        let key = format!("panel-style:{}", id.unwrap_or("*"));
        self.tree_edit(Some(key), |s| {
            for i in &ids {
                if let Some(Node::Group(g)) = s.doc.find_mut(i)
                    && let Some(mut p) = panel_of(g)
                {
                    if let Some(v) = gutter {
                        p.gutter = Some(v.max(0.0));
                    }
                    if let Some(v) = border {
                        p.border = Some(v.max(0.0));
                    }
                    set_panel(g, Some(p));
                }
            }
            s.rebuild_panel_borders();
            Ok(())
        })
    }
}
