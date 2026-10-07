//! Stroke snapping to H/V, the grid, guide lines and perspective vanishing
//! points (happypaint `paint-canvas.tsx` snapping, spec 19).

use crate::guide_lines::{GuideLine, snap_point_to_guides};

pub type Pt = [f64; 2];

/// Movement (doc px) before a direction is read, and before it locks.
pub const SNAP_READ_MIN: f64 = 6.0;
pub const SNAP_COMMIT: f64 = 24.0;
/// How far soft perspective pulls a point toward the line.
pub const SOFT_SNAP_STRENGTH: f64 = 0.7;
/// A VP within ~20° of vertical already owns "up".
const VERT_TOL: f64 = 0.94;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapConfig {
    pub hv: bool,
    /// Grid snap step (`size / subdivisions`), when grid snapping is on.
    pub grid_step: Option<f64>,
    /// Vanishing points in doc px (perspective enabled and not editing).
    pub vps: Vec<Pt>,
    /// Soft perspective: freehand strokes are pulled toward VP lines.
    pub soft: bool,
    /// Guide lines: strokes run parallel to them, and origins are pulled onto
    /// the nearest one within `guide_tol` doc px.
    pub guides: Vec<GuideLine>,
    pub guide_tol: f64,
}

pub fn grid_node(p: Pt, step: f64) -> Pt {
    let s = step.max(1.0);
    [(p[0] / s).round() * s, (p[1] / s).round() * s]
}

fn unit(x: f64, y: f64) -> Option<Pt> {
    let l = x.hypot(y);
    (l >= 1e-3).then(|| [x / l, y / l])
}

impl SnapConfig {
    /// Whether freehand strokes snap at all (hard perspective uses `hard_line_end`).
    pub fn active(&self) -> bool {
        self.hv || self.grid_step.is_some() || (self.soft && !self.vps.is_empty()) || !self.guides.is_empty()
    }

    pub fn candidates(&self, origin: Pt, include_vp: bool) -> Vec<Pt> {
        let mut c = Vec::new();
        if include_vp && !self.vps.is_empty() {
            c.extend(self.vps.iter().filter_map(|v| unit(v[0] - origin[0], v[1] - origin[1])));
            if !self.hv && !c.iter().any(|d| d[1].abs() > VERT_TOL) {
                c.push([0.0, 1.0]);
            }
        }
        if self.hv || self.grid_step.is_some() {
            c.push([1.0, 0.0]);
            c.push([0.0, 1.0]);
        }
        c.extend(self.guides.iter().map(GuideLine::direction));
        c
    }
}

fn pick_axis(cands: &[Pt], d: Pt) -> Option<Pt> {
    let mut best = None;
    let mut score = -1.0;
    for c in cands {
        let s = (d[0] * c[0] + d[1] * c[1]).abs();
        if s > score {
            score = s;
            best = Some(*c);
        }
    }
    best
}

fn project(o: Pt, dir: Pt, p: Pt) -> Pt {
    let t = (p[0] - o[0]) * dir[0] + (p[1] - o[1]) * dir[1];
    [o[0] + t * dir[0], o[1] + t * dir[1]]
}

/// Hard perspective: the far end of the straight line from `origin`, on the
/// best axis for the drag (re-chosen every move).
pub fn hard_line_end(cfg: &SnapConfig, origin: Pt, cur: Pt) -> Pt {
    let Some(d) = unit(cur[0] - origin[0], cur[1] - origin[1]) else { return cur };
    match pick_axis(&cfg.candidates(origin, true), d) {
        Some(axis) => project(origin, axis, cur),
        None => cur,
    }
}

/// Freehand snapping for one stroke.
#[derive(Clone, Debug)]
pub struct Snapper {
    cfg: SnapConfig,
    origin: Pt,
    /// Still choosing the axis (until the drag is long enough to trust).
    choosing: bool,
    line: Option<(Pt, Pt)>,
}

impl Snapper {
    /// Arm at the stroke origin; `None` when nothing snaps.
    pub fn arm(cfg: SnapConfig, o: Pt) -> Option<Self> {
        if !cfg.active() {
            return None;
        }
        // Grid first, then guides: a guide placed against this drawing wins.
        let origin = snap_point_to_guides(&cfg.guides, cfg.grid_step.map_or(o, |s| grid_node(o, s)), cfg.guide_tol);
        Some(Self { cfg, origin, choosing: true, line: None })
    }

    /// The (grid-snapped) stroke origin.
    pub fn origin(&self) -> Pt {
        self.origin
    }

    pub fn apply(&mut self, p: Pt) -> Pt {
        if self.choosing {
            let (dx, dy) = (p[0] - self.origin[0], p[1] - self.origin[1]);
            let len = dx.hypot(dy);
            if len >= SNAP_READ_MIN {
                let d = [dx / len, dy / len];
                if let Some(axis) = pick_axis(&self.cfg.candidates(self.origin, self.cfg.soft), d) {
                    self.line = Some((self.origin, axis));
                }
                if len >= SNAP_COMMIT {
                    self.choosing = false;
                }
            }
        }
        let Some((o, dir)) = self.line else { return p };
        let proj = project(o, dir, p);
        if self.cfg.soft && !self.cfg.vps.is_empty() {
            return [p[0] + (proj[0] - p[0]) * SOFT_SNAP_STRENGTH, p[1] + (proj[1] - p[1]) * SOFT_SNAP_STRENGTH];
        }
        proj
    }
}
