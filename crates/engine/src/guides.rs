//! Drawing guides (spec 19): happypaint's `StoredGuides`, saved in the
//! document's `guides` key. Edit modes are transient and live in the app.

use onepaint_brush::symmetry::Symmetry;
use onepaint_stroke::snap::SnapConfig;
use serde::{Deserialize, Serialize};

pub const DEFAULT_GUIDE_COLOR: &str = "#008cff";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Grid {
    pub enabled: bool,
    /// Cell size in document px (4..512).
    pub size: f64,
    /// Minor subdivisions per cell (1..8).
    pub subdivisions: u32,
    pub snap: bool,
    pub color: String,
}

impl Default for Grid {
    fn default() -> Self {
        Self { enabled: false, size: 64.0, subdivisions: 1, snap: false, color: "#808080".into() }
    }
}

impl Grid {
    /// The finest drawn step, which grid snapping rounds to.
    pub fn step(&self) -> f64 {
        (self.size / self.subdivisions.max(1) as f64).max(1.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PerspectiveSnap {
    Off,
    Soft,
    #[default]
    Hard,
}

/// A vanishing point as a document fraction (off-document is normal).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VanishingPoint {
    pub x: f64,
    pub y: f64,
}

/// happypaint's 1–5-point layouts.
pub fn perspective_preset(n: usize) -> Vec<VanishingPoint> {
    let v = |x, y| VanishingPoint { x, y };
    match n {
        1 => vec![v(0.5, 0.5)],
        3 => vec![v(-0.8, 0.5), v(1.8, 0.5), v(0.5, 2.4)],
        4 => vec![v(-0.8, 0.5), v(1.8, 0.5), v(0.5, -1.4), v(0.5, 2.4)],
        5 => vec![v(-0.8, 0.5), v(1.8, 0.5), v(0.5, -1.4), v(0.5, 2.4), v(0.5, 0.5)],
        _ => vec![v(-0.8, 0.5), v(1.8, 0.5)],
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Perspective {
    pub enabled: bool,
    pub points: Vec<VanishingPoint>,
    pub snap: PerspectiveSnap,
    /// 5-point fish-eye (kept for files; drawn straight for now).
    pub curved: bool,
    pub density: u32,
    pub color: String,
}

impl Default for Perspective {
    fn default() -> Self {
        Self {
            enabled: false,
            points: perspective_preset(2),
            snap: PerspectiveSnap::Hard,
            curved: false,
            density: 6,
            color: "#e87828".into(),
        }
    }
}

/// A user-placed guide: an infinite line through (x, y) at `angle` radians.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GuideLine {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub angle: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Guides {
    pub grid: Grid,
    pub symmetry: Symmetry,
    pub perspective: Perspective,
    #[serde(rename = "snapHV")]
    pub snap_hv: bool,
    pub visible: bool,
    pub lines: Vec<GuideLine>,
    pub line_color: String,
    pub rulers: bool,
    pub units: String,
    pub snap_guides: bool,
}

impl Default for Guides {
    fn default() -> Self {
        Self {
            grid: Grid::default(),
            symmetry: Symmetry::default(),
            perspective: Perspective::default(),
            snap_hv: false,
            visible: true,
            lines: Vec::new(),
            line_color: DEFAULT_GUIDE_COLOR.into(),
            rulers: false,
            units: "px".into(),
            snap_guides: true,
        }
    }
}

impl Guides {
    /// Anything worth saving with the document (`guidesConfigured`).
    pub fn configured(&self) -> bool {
        !self.lines.is_empty()
            || self.rulers
            || self.units != "px"
            || self.grid.enabled
            || self.symmetry.is_on()
            || self.perspective.enabled
            || self.snap_hv
            || !self.visible
            || self.line_color != DEFAULT_GUIDE_COLOR
    }

    /// Clamp values to the ranges the store enforces.
    pub fn sanitized(mut self) -> Self {
        self.grid.size = self.grid.size.clamp(4.0, 512.0).round();
        self.grid.subdivisions = self.grid.subdivisions.clamp(1, 8);
        self.symmetry.radial_count = self.symmetry.radial_count.clamp(2, 24);
        self.symmetry.cx = self.symmetry.cx.clamp(0.0, 1.0);
        self.symmetry.cy = self.symmetry.cy.clamp(0.0, 1.0);
        self.perspective.density = self.perspective.density.clamp(2, 32);
        if self.perspective.points.is_empty() {
            self.perspective.points = perspective_preset(2);
        }
        self
    }

    /// Stroke snapping for a document of this size (`editing` = VP edit mode).
    pub fn snap_config(&self, width: u32, height: u32, editing: bool) -> SnapConfig {
        let (w, h) = (width as f64, height as f64);
        let p = &self.perspective;
        let persp = p.enabled && !editing;
        SnapConfig {
            hv: self.snap_hv,
            grid_step: (self.grid.enabled && self.grid.snap).then(|| self.grid.step()),
            vps: if persp { p.points.iter().map(|v| [v.x * w, v.y * h]).collect() } else { Vec::new() },
            soft: persp && p.snap == PerspectiveSnap::Soft,
            guide_dirs: if self.snap_guides { self.lines.iter().map(|l| [l.angle.cos(), l.angle.sin()]).collect() } else { Vec::new() },
        }
    }

    /// Hard perspective lines replace freehand brush strokes.
    pub fn hard_perspective(&self, editing: bool) -> bool {
        self.perspective.enabled && self.perspective.snap == PerspectiveSnap::Hard && !editing
    }
}
