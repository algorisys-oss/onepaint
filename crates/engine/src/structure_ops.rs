//! Layer structure from the Layer and Image menus (happypaint
//! `menu-model.ts`): group and ungroup a node, merge visible to a new
//! layer, flatten the image, invert a mask. Each is one undo step.

use super::*;

impl Session {
    /// Put the node in a new group in its place; the node stays active.
    pub(crate) fn group_node(&mut self, id: &str) -> Result<(), EngineError> {
        let path = self.doc.path_of(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        let index = *path.last().unwrap_or(&0);
        let name = self.next_group_name();
        self.tree_edit(None, |s| {
            let (node, parent, _) = s.doc.remove(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
            let mut g = Group::new(&name);
            g.children = vec![node];
            s.doc.insert(parent.as_deref(), index, Node::Group(g));
            Ok(())
        })
    }

    /// Every node id in the Layers panel's order: the top of the stack
    /// first, a group just before its children.
    pub fn panel_order(&self) -> Vec<NodeId> {
        fn walk(nodes: &[Node], out: &mut Vec<NodeId>) {
            for n in nodes.iter().rev() {
                out.push(n.id().to_string());
                if let Node::Group(g) = n {
                    walk(&g.children, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.doc.root, &mut out);
        out
    }

    /// `ids` as a mergeable run, top-first: two or more paint layers, every
    /// one below the top able to take pixels (not an adjustment layer).
    pub fn mergeable_run(&self, ids: &[NodeId]) -> Option<Vec<NodeId>> {
        let run: Vec<NodeId> = self.panel_order().into_iter().filter(|id| ids.contains(id)).collect();
        let layer = |id: &NodeId| self.doc.layer(id);
        let ok = run.len() >= 2
            && run.iter().all(|id| layer(id).is_some())
            && run[1..].iter().all(|id| layer(id).is_some_and(|l| l.adjustment.is_none()));
        ok.then_some(run)
    }

    /// Group `ids` that share the first one's parent into a new group, at the
    /// topmost one's place, keeping their stacking order; the group becomes
    /// active (PE `groupLayers`).
    pub(crate) fn group_nodes(&mut self, ids: &[NodeId]) -> Result<(), EngineError> {
        let found: Vec<&NodeId> = ids.iter().filter(|id| self.doc.find(id).is_some()).collect();
        let Some(first) = found.first() else { return Ok(()) };
        let parent = self.doc.parent_of(first).flatten();
        let mut picks: Vec<(usize, NodeId)> = found
            .iter()
            .filter(|id| self.doc.parent_of(id).flatten() == parent)
            .filter_map(|id| Some((*self.doc.path_of(id)?.last()?, (*id).clone())))
            .collect();
        picks.sort();
        picks.dedup();
        if picks.len() < 2 {
            return Ok(());
        }
        let top = picks.iter().map(|p| p.0).max().unwrap_or(0);
        let at = top + 1 - picks.len();
        let name = self.next_group_name();
        self.tree_edit(None, |s| {
            let mut g = Group::new(&name);
            // Bottom to top, as they stacked.
            for (_, id) in &picks {
                let (node, _, _) = s.doc.remove(id).ok_or_else(|| EngineError::NoSuchLayer(id.clone()))?;
                g.children.push(node);
            }
            s.active = g.id.clone();
            s.doc.insert(parent.as_deref(), at, Node::Group(g));
            Ok(())
        })
    }

    /// Merge picked paint layers (see [`Command::MergeLayers`]).
    pub(crate) fn merge_layers(&mut self, ids: &[NodeId]) -> Result<(), EngineError> {
        let run = self.mergeable_run(ids).ok_or_else(|| EngineError::Invalid("Merge needs two or more paint layers".into()))?;
        self.history.begin_transaction();
        let r = self.merge_run(&run);
        self.history.end_transaction();
        r
    }

    fn merge_run(&mut self, run: &[NodeId]) -> Result<(), EngineError> {
        let parent = self.doc.parent_of(&run[0]).flatten();
        let at = |s: &Self, id: &str| (s.doc.parent_of(id).flatten(), s.doc.path_of(id).and_then(|p| p.last().copied()).unwrap_or(0));
        // Gather: each pick goes directly under the one above it.
        for w in run.windows(2) {
            let ((pp, pi), (cp, ci)) = (at(self, &w[0]), at(self, &w[1]));
            if cp == pp && ci + 1 == pi {
                continue;
            }
            // MoveLayer's index counts after the move's own removal.
            let index = if cp == pp && ci < pi { pi - 1 } else { pi };
            self.move_layer(&w[1], parent.clone(), index)?;
        }
        for id in &run[..run.len() - 1] {
            self.merge_down(id)?;
        }
        Ok(())
    }

    /// Run `commands` as one undo step; stops at the first error.
    pub(crate) fn batch(&mut self, commands: Vec<Command>) -> Result<(), EngineError> {
        if commands.iter().any(|c| matches!(c, Command::Undo | Command::Redo)) {
            return Err(EngineError::Invalid("undo inside a batch".into()));
        }
        self.history.begin_transaction();
        let r = commands.into_iter().try_for_each(|c| self.execute(c).map(|_| ()));
        self.history.end_transaction();
        r
    }

    /// Lift a group's children into its place (the group's own opacity,
    /// mode and styles go with it); its top child becomes active.
    pub(crate) fn ungroup(&mut self, id: &str) -> Result<(), EngineError> {
        let path = self.doc.path_of(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
        if !self.doc.node_at(&path).is_some_and(Node::is_group) {
            return Err(EngineError::Invalid("not a group".into()));
        }
        let index = *path.last().unwrap_or(&0);
        self.tree_edit(None, |s| {
            let (node, parent, _) = s.doc.remove(id).ok_or_else(|| EngineError::NoSuchLayer(id.into()))?;
            let Node::Group(g) = node else { return Err(EngineError::Invalid("not a group".into())) };
            if let Some(top) = g.children.last() {
                s.active = top.id().to_string();
            }
            for (k, child) in g.children.into_iter().enumerate() {
                s.doc.insert(parent.as_deref(), index + k, child);
            }
            Ok(())
        })
    }

    /// What shows, as one surface: the composite without the paper's tooth
    /// (the paper stays the document's, so it isn't applied twice).
    fn visible_pixels(&self) -> Surface {
        let mut d = self.flatten_doc().into_owned();
        d.set_paper(None);
        let flat = onepaint_compose::flatten(&d, None);
        let mut s = Surface::new();
        s.write_rect(self.doc.bounds(), &flat);
        s.prune_empty();
        s
    }

    /// Ctrl+Shift+Alt+E: the visible image on a new layer above the active one.
    pub(crate) fn merge_visible(&mut self) -> Result<(), EngineError> {
        let mut l = Layer::new("Merged");
        l.pixels = self.visible_pixels();
        self.insert_above_active(Node::Layer(l))
    }

    /// Every layer replaced by one layer of what shows.
    pub(crate) fn flatten_image(&mut self) -> Result<(), EngineError> {
        let mut l = Layer::new("Background");
        l.pixels = self.visible_pixels();
        let id = l.id.clone();
        self.tree_edit(None, |s| {
            s.doc.root = vec![Node::Layer(l)];
            s.active = id;
            Ok(())
        })
    }

    /// Hidden becomes shown and shown hidden, over the whole document.
    pub(crate) fn invert_mask(&mut self, id: &str) -> Result<(), EngineError> {
        let Some(mask) = self.doc.layer(id).and_then(|l| l.mask.clone()) else { return Ok(()) };
        let mut out = Surface::new();
        for (tx, ty) in self.doc.bounds().tiles() {
            let mut buf = mask.tile(tx, ty).map(|t| t.to_f32()).unwrap_or_else(|| vec![0.0; onepaint_raster::TILE_PX * 4]);
            for p in buf.chunks_exact_mut(4) {
                let v = 1.0 - p[0];
                p.copy_from_slice(&[v, v, v, 1.0]);
            }
            out.set_tile(tx, ty, Some(Arc::new(Tile::from_f32(&buf))));
        }
        self.tree_edit(None, |s| {
            if let Some(l) = s.doc.layer_mut(id) {
                l.mask = Some(out);
            }
            Ok(())
        })
    }
}
