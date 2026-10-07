//! Pen tool (spec 22): build bezier paths by clicking and dragging, edit the
//! active path layer's anchors and handles, and the path menu.

use onepaint_engine::{Command, Session};
use onepaint_stroke::bezier::{AnchorKind, PathAnchor, PathFill, PathSpec, PathStroke, Subpath};

pub type Pt = [f64; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Anchor,
    In,
    Out,
}

/// The path being built, and the last anchor touched while editing.
#[derive(Default)]
pub struct PenState {
    pub building: Option<Vec<PathAnchor>>,
    /// (object, subpath, anchor) for Delete.
    pub last: Option<(usize, usize, usize)>,
    /// Time and position of the last press, for double-click to finish.
    pub last_press: Option<(f64, Pt)>,
}

fn dist(a: Pt, b: Pt) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The handle end or anchor under `p` (handles win, they sit on top).
pub fn hit(specs: &[PathSpec], p: Pt, tol: f64) -> Option<(usize, usize, usize, Part)> {
    let mut best: Option<((usize, usize, usize, Part), f64)> = None;
    let mut consider = |key, at: Pt, bias: f64| {
        let d = dist(at, p) - bias;
        if d <= tol && best.is_none_or(|b| d < b.1) {
            best = Some((key, d));
        }
    };
    for (o, spec) in specs.iter().enumerate() {
        for (s, sub) in spec.subpaths.iter().enumerate() {
            for (a, an) in sub.anchors.iter().enumerate() {
                if an.has_in() {
                    let h = an.in_handle();
                    consider((o, s, a, Part::In), [an.x + h[0], an.y + h[1]], 0.5);
                }
                if an.has_out() {
                    let h = an.out_handle();
                    consider((o, s, a, Part::Out), [an.x + h[0], an.y + h[1]], 0.5);
                }
                consider((o, s, a, Part::Anchor), [an.x, an.y], 0.0);
            }
        }
    }
    best.map(|b| b.0)
}

/// Pull the out-handle of the anchor being placed (smooth, in = −out).
pub fn pull_handle(anchors: &mut [PathAnchor], index: usize, d: Pt, constrain: bool) {
    let Some(a) = anchors.get_mut(index) else { return };
    let mut h = [d[0] - a.x, d[1] - a.y];
    if constrain {
        h = onepaint_stroke::bezier::constrain_handle(h[0], h[1]);
    }
    a.kind = AnchorKind::Smooth;
    (a.out_x, a.out_y, a.in_x, a.in_y) = (Some(h[0]), Some(h[1]), Some(-h[0]), Some(-h[1]));
}

/// Commit the path being built as a new path layer (or an object on the
/// active path layer), stroked with the current brush and colour.
pub fn finish(state: &mut PenState, session: &mut Session, closed: bool, fill: bool, seed: u32) {
    let Some(anchors) = state.building.take() else { return };
    if anchors.len() < 2 {
        return;
    }
    let c = session.color();
    let b = session.current_brush();
    let spec = PathSpec {
        subpaths: vec![Subpath { anchors, closed }],
        stroke: Some(PathStroke { brush_id: b.id.clone(), size: b.size, color: [c.r, c.g, c.b], opacity: 1.0, seed: Some(seed) }),
        fill: (fill && closed).then_some(PathFill { color: [c.r, c.g, c.b], opacity: 1.0 }),
    };
    let active = session.active().to_string();
    let _ = if session.is_path_layer(&active) {
        session.execute(Command::AddPathObject { id: active, spec: Box::new(spec) })
    } else {
        session.execute(Command::CreatePathLayer { spec: Box::new(spec) })
    };
    let _ = session.execute(Command::SealHistory);
}

/// Apply an edit to one path object of the active layer (live; sealed on release).
pub fn edit_object(session: &mut Session, obj: usize, f: impl FnOnce(&mut PathSpec)) {
    let id = session.active().to_string();
    let mut specs = session.paths(&id);
    let Some(spec) = specs.get_mut(obj) else { return };
    f(spec);
    let spec = spec.clone();
    let _ = session.execute(Command::UpdatePathObject { id, index: obj, spec: Some(Box::new(spec)) });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_win_over_anchors() {
        let mut a = PathAnchor::corner(10.0, 10.0);
        (a.out_x, a.out_y) = (Some(1.0), Some(0.0));
        let spec =
            PathSpec { subpaths: vec![Subpath { anchors: vec![a, PathAnchor::corner(50.0, 10.0)], closed: false }], ..Default::default() };
        assert_eq!(hit(std::slice::from_ref(&spec), [11.0, 10.0], 5.0), Some((0, 0, 0, Part::Out)));
        assert_eq!(hit(std::slice::from_ref(&spec), [49.0, 11.0], 5.0), Some((0, 0, 1, Part::Anchor)));
        assert_eq!(hit(&[spec], [30.0, 30.0], 5.0), None);
    }
}
