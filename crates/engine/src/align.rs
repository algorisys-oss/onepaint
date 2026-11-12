//! Align and distribute layers (happypaint `utils/layer-align.ts`,
//! `PE.alignLayers` at PE:6245-6269).
//!
//! The maths works on layer content bounds and returns whole-pixel moves;
//! [`Session::align_layers`] applies them as one undo step.

use crate::{EngineError, Session, transform};
use onepaint_doc::{Node, NodeId};
use onepaint_raster::Rect;
use serde::{Deserialize, Serialize};

/// Which edge or centre to line up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlignMode {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

impl AlignMode {
    fn horizontal(self) -> bool {
        matches!(self, AlignMode::Left | AlignMode::HCenter | AlignMode::Right)
    }
}

/// Distribute and even-gap axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// One layer's whole-pixel move.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerMove {
    pub id: NodeId,
    pub dx: i32,
    pub dy: i32,
}

/// A layer and the bounds of its artwork, doc px.
pub type Item = (NodeId, Rect);

/// JS `Math.round`: halves round toward +∞, so −2.5 → −2.
fn js_round(v: f64) -> i32 {
    (v + 0.5).floor() as i32
}

fn mv(id: &NodeId, d: i32, horizontal: bool) -> LayerMove {
    if horizontal { LayerMove { id: id.clone(), dx: d, dy: 0 } } else { LayerMove { id: id.clone(), dx: 0, dy: d } }
}

/// `(start, size)` of a rect along an axis.
fn along(b: &Rect, horizontal: bool) -> (f64, f64) {
    if horizontal { (b.x as f64, b.w as f64) } else { (b.y as f64, b.h as f64) }
}

/// The smallest rect holding every item; `None` for no items.
pub fn union_bounds(items: &[Item]) -> Option<Rect> {
    items.iter().map(|(_, b)| *b).reduce(|a, b| a.union(&b))
}

/// Move each item so the chosen edge or centre lines up with `target`'s.
pub fn align_offsets(items: &[Item], mode: AlignMode, target: Rect) -> Vec<LayerMove> {
    let (tx, ty, tw, th) = (target.x as f64, target.y as f64, target.w as f64, target.h as f64);
    items
        .iter()
        .map(|(id, b)| {
            let (bx, by, bw, bh) = (b.x as f64, b.y as f64, b.w as f64, b.h as f64);
            let d = match mode {
                AlignMode::Left => tx - bx,
                AlignMode::Right => tx + tw - (bx + bw),
                AlignMode::HCenter => tx + tw / 2.0 - (bx + bw / 2.0),
                AlignMode::Top => ty - by,
                AlignMode::Bottom => ty + th - (by + bh),
                AlignMode::VCenter => ty + th / 2.0 - (by + bh / 2.0),
            };
            mv(id, js_round(d), mode.horizontal())
        })
        .collect()
}

/// Space the centres evenly between the outermost two (3+ items).
pub fn distribute_offsets(items: &[Item], axis: Axis) -> Vec<LayerMove> {
    if items.len() < 3 {
        return Vec::new();
    }
    let h = axis == Axis::Horizontal;
    let centre = |b: &Rect| {
        let (a, s) = along(b, h);
        a + s / 2.0
    };
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by(|a, b| centre(&a.1).total_cmp(&centre(&b.1)));
    let first = centre(&sorted[0].1);
    let last = centre(&sorted[sorted.len() - 1].1);
    let step = (last - first) / (sorted.len() - 1) as f64;
    sorted.iter().enumerate().map(|(i, (id, b))| mv(id, js_round(first + step * i as f64 - centre(b)), h)).collect()
}

/// Equal gaps between neighbours, keeping the first start and the last end
/// (3+ items). Gaps may go negative when the items overlap.
pub fn spacing_offsets(items: &[Item], axis: Axis) -> Vec<LayerMove> {
    if items.len() < 3 {
        return Vec::new();
    }
    let h = axis == Axis::Horizontal;
    let mut sorted: Vec<&Item> = items.iter().collect();
    sorted.sort_by(|a, b| along(&a.1, h).0.total_cmp(&along(&b.1, h).0));
    let start = along(&sorted[0].1, h).0;
    let end = sorted.iter().map(|(_, b)| along(b, h)).map(|(a, s)| a + s).fold(f64::NEG_INFINITY, f64::max);
    let total: f64 = sorted.iter().map(|(_, b)| along(b, h).1).sum();
    let gap = (end - start - total) / (sorted.len() - 1) as f64;
    let mut cursor = start;
    sorted
        .iter()
        .map(|(id, b)| {
            let (a, s) = along(b, h);
            let m = mv(id, js_round(cursor - a), h);
            cursor += s + gap;
            m
        })
        .collect()
}

impl Session {
    /// The leaf layers an align acts on: picked layers and every layer inside
    /// a picked group, visible and unlocked (`transformableIds`). A hidden
    /// group hides everything in it, so none of its children move
    /// (happypaint v0.205.3, review N8); a locked group doesn't lock them.
    pub fn transformable_ids(&self, picked: &[NodeId]) -> Vec<NodeId> {
        fn walk(nodes: &[Node], picked: &[NodeId], inside: bool, out: &mut Vec<NodeId>) {
            for n in nodes {
                match n {
                    Node::Group(g) if !g.visible => {}
                    Node::Group(g) => walk(&g.children, picked, inside || picked.contains(&g.id), out),
                    Node::Layer(l) => {
                        if (inside || picked.contains(&l.id)) && l.visible && !l.locked {
                            out.push(l.id.clone());
                        }
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.doc.root, picked, false, &mut out);
        out
    }

    /// Artwork bounds of each layer on the canvas; empty and adjustment
    /// layers are left out (`layerContentBounds`, PE:6218-6231).
    pub fn layer_content_bounds(&self, ids: &[NodeId]) -> Vec<Item> {
        let canvas = self.doc.bounds();
        ids.iter()
            .filter_map(|id| {
                let l = self.doc.layer(id).filter(|l| l.adjustment.is_none())?;
                let b = l.pixels.content_bounds()?.intersect(&canvas)?;
                Some((id.clone(), b))
            })
            .collect()
    }

    /// Bounds of the live selection, if there is one with any coverage.
    pub fn selection_bounds(&self) -> Option<Rect> {
        let (x, y, w, h) = self.selection.as_ref()?.bounds()?;
        Some(Rect::new(x as i32, y as i32, w as i32, h as i32))
    }

    /// Move layers by whole pixels as one undo step. Text and path layers
    /// stay vector; hidden or locked layers don't move. Errors with
    /// `Busy` while a transform is pending.
    pub(crate) fn align_layers(&mut self, moves: Vec<LayerMove>) -> Result<(), EngineError> {
        if self.transform.is_some() {
            return Err(EngineError::Invalid("Finish the move first (Enter), then align".into()));
        }
        let moves: Vec<LayerMove> = moves
            .into_iter()
            .filter(|m| (m.dx, m.dy) != (0, 0))
            .filter(|m| self.doc.layer(&m.id).is_some_and(|l| l.visible && !l.locked))
            .collect();
        if moves.is_empty() {
            return Ok(());
        }
        let (w, h, space) = (self.doc.width, self.doc.height, self.doc.space());
        self.tree_edit(None, move |s| {
            for m in &moves {
                let paths = s.paths(&m.id);
                let texts = s.texts(&m.id);
                let Some(l) = s.doc.layer_mut(&m.id) else { continue };
                let (dx, dy) = (m.dx as f64, m.dy as f64);
                if !paths.is_empty() {
                    let pm = onepaint_stroke::bezier::Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: dx, f: dy };
                    let mapped: Vec<_> = paths.iter().map(|p| onepaint_stroke::bezier::affine(p, &pm)).collect();
                    crate::path_ops::write_paths(l, &mapped, w, h, space);
                } else if !texts.is_empty() {
                    let a = transform::Affine::translate(dx, dy);
                    let mapped: Vec<_> = texts.iter().map(|t| transform::transform_text(t, &a)).collect();
                    crate::text_ops::write_texts(l, &mapped, w, h, space);
                } else {
                    l.pixels = l.pixels.translated(m.dx, m.dy);
                    l.height = l.height.as_ref().map(|h| h.translated(m.dx, m.dy));
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, x: i32, y: i32, w: i32, h: i32) -> Item {
        (id.to_string(), Rect::new(x, y, w, h))
    }

    fn get(ms: &[LayerMove], id: &str) -> (i32, i32) {
        ms.iter().find(|m| m.id == id).map(|m| (m.dx, m.dy)).unwrap_or((i32::MIN, i32::MIN))
    }

    const CANVAS: Rect = Rect { x: 0, y: 0, w: 200, h: 100 };

    #[test]
    fn union_bounds_cases() {
        assert_eq!(union_bounds(&[item("a", 10, 20, 30, 40), item("b", 50, 5, 10, 10)]), Some(Rect::new(10, 5, 50, 55)));
        assert_eq!(union_bounds(&[item("a", 7, 8, 9, 10)]), Some(Rect::new(7, 8, 9, 10)));
        assert_eq!(union_bounds(&[]), None);
    }

    #[test]
    fn align_to_canvas() {
        let items = [item("a", 10, 10, 20, 20), item("b", 100, 60, 40, 30)];
        let o = |m| align_offsets(&items, m, CANVAS);
        let l = o(AlignMode::Left);
        assert_eq!((get(&l, "a"), get(&l, "b")), ((-10, 0), (-100, 0)));
        let r = o(AlignMode::Right);
        assert_eq!((get(&r, "a").0, get(&r, "b").0), (170, 60));
        let c = o(AlignMode::HCenter);
        assert_eq!((get(&c, "a").0, get(&c, "b").0), (80, -20));
        let t = o(AlignMode::Top);
        assert_eq!((get(&t, "a"), get(&t, "b").1), ((0, -10), -60));
        let b = o(AlignMode::Bottom);
        assert_eq!((get(&b, "a").1, get(&b, "b").1), (70, 10));
        let v = o(AlignMode::VCenter);
        assert_eq!((get(&v, "a").1, get(&v, "b").1), (30, -25));
    }

    #[test]
    fn align_edge_cases() {
        let c = align_offsets(&[item("a", 0, 0, 21, 21)], AlignMode::HCenter, Rect::new(0, 0, 100, 100));
        assert_eq!(get(&c, "a").0, 40, "39.5 rounds up like Math.round");
        assert_eq!(get(&align_offsets(&[item("a", 0, 0, 10, 10)], AlignMode::Left, CANVAS), "a"), (0, 0));
        assert!(align_offsets(&[], AlignMode::Left, CANVAS).is_empty());
        assert_eq!(js_round(-2.5), -2);
    }

    #[test]
    fn distribute_centres() {
        let items = [item("a", 0, 0, 10, 10), item("b", 25, 0, 10, 10), item("c", 90, 0, 10, 10)];
        let d = distribute_offsets(&items, Axis::Horizontal);
        assert_eq!((get(&d, "a").0, get(&d, "b").0, get(&d, "c").0), (0, 20, 0));
        let rev: Vec<Item> = items.iter().rev().cloned().collect();
        assert_eq!(get(&distribute_offsets(&rev, Axis::Horizontal), "b").0, 20);
        let vert = [item("a", 0, 0, 10, 10), item("b", 0, 25, 10, 10), item("c", 0, 90, 10, 10)];
        assert_eq!(get(&distribute_offsets(&vert, Axis::Vertical), "b"), (0, 20));
        assert!(distribute_offsets(&[item("a", 0, 0, 10, 10), item("b", 50, 0, 10, 10)], Axis::Horizontal).is_empty());
    }

    #[test]
    fn even_gaps() {
        let items = [item("a", 0, 0, 10, 10), item("b", 20, 0, 30, 10), item("c", 90, 0, 10, 10)];
        let s = spacing_offsets(&items, Axis::Horizontal);
        assert_eq!((get(&s, "a").0, get(&s, "b").0, get(&s, "c").0), (0, 15, 0));

        let items = [item("a", 0, 0, 10, 10), item("b", 30, 0, 5, 10), item("c", 40, 0, 20, 10), item("d", 100, 0, 10, 10)];
        let s = spacing_offsets(&items, Axis::Horizontal);
        let mut moved: Vec<Rect> = items.iter().map(|(id, b)| Rect::new(b.x + get(&s, id).0, b.y, b.w, b.h)).collect();
        moved.sort_by_key(|r| r.x);
        let gaps: Vec<i32> = moved.windows(2).map(|w| w[1].x - (w[0].x + w[0].w)).collect();
        assert!(gaps.iter().all(|g| (g - gaps[0]).abs() <= 1), "{gaps:?}");

        let overlap = [item("a", 0, 0, 30, 10), item("b", 5, 0, 30, 10), item("c", 30, 0, 30, 10)];
        let s = spacing_offsets(&overlap, Axis::Horizontal);
        assert_eq!((get(&s, "a").0, get(&s, "b").0, get(&s, "c").0), (0, 10, 0));
        assert!(spacing_offsets(&[item("a", 0, 0, 10, 10), item("b", 0, 50, 10, 10)], Axis::Vertical).is_empty());
    }
}
