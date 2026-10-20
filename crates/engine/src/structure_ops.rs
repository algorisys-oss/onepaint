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
