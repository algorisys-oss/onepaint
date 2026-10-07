//! Canvas view transform (happypaint `utils/coords.ts`, `state/viewport-store.ts`).
//!
//! `screen = view/2 + offset + R(θ)·s·(doc − docSize/2)`, in logical screen px, y down.

pub const MIN_SCALE: f64 = 0.05;
pub const MAX_SCALE: f64 = 64.0;
const SNAP: f64 = 5.0 * std::f64::consts::PI / 180.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub view_size: [f64; 2],
    pub doc_size: [f64; 2],
    pub scale: f64,
    pub rotation: f64,
    pub offset: [f64; 2],
}

impl View {
    pub fn new(view_size: [f64; 2], doc_size: [f64; 2]) -> Self {
        let mut v = Self {
            view_size,
            doc_size,
            scale: 1.0,
            rotation: 0.0,
            offset: [0.0, 0.0],
        };
        v.fit();
        v
    }

    pub fn doc_to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        let (c, s) = (self.rotation.cos(), self.rotation.sin());
        let dx = (p[0] - self.doc_size[0] / 2.0) * self.scale;
        let dy = (p[1] - self.doc_size[1] / 2.0) * self.scale;
        [
            self.view_size[0] / 2.0 + self.offset[0] + dx * c - dy * s,
            self.view_size[1] / 2.0 + self.offset[1] + dx * s + dy * c,
        ]
    }

    pub fn screen_to_doc(&self, p: [f64; 2]) -> [f64; 2] {
        let (c, s) = (self.rotation.cos(), self.rotation.sin());
        let qx = p[0] - self.view_size[0] / 2.0 - self.offset[0];
        let qy = p[1] - self.view_size[1] / 2.0 - self.offset[1];
        [
            self.doc_size[0] / 2.0 + (qx * c + qy * s) / self.scale,
            self.doc_size[1] / 2.0 + (-qx * s + qy * c) / self.scale,
        ]
    }

    /// Multiply the scale by `factor`, keeping the screen point `at` fixed.
    pub fn zoom_at(&mut self, at: [f64; 2], factor: f64) {
        let ns = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let f = ns / self.scale;
        let q = [
            at[0] - self.view_size[0] / 2.0,
            at[1] - self.view_size[1] / 2.0,
        ];
        self.offset = [
            q[0] - f * (q[0] - self.offset[0]),
            q[1] - f * (q[1] - self.offset[1]),
        ];
        self.scale = ns;
    }

    pub fn set_scale_at(&mut self, at: [f64; 2], scale: f64) {
        self.zoom_at(at, scale / self.scale);
    }

    /// Rotate by `delta` radians about the screen point `at`.
    pub fn rotate_at(&mut self, at: [f64; 2], delta: f64) {
        let doc = self.screen_to_doc(at);
        self.rotation += delta;
        let moved = self.doc_to_screen(doc);
        self.offset[0] += at[0] - moved[0];
        self.offset[1] += at[1] - moved[1];
    }

    /// Snap to a cardinal angle when within 5°.
    pub fn snap_rotation(r: f64) -> f64 {
        let q = std::f64::consts::FRAC_PI_2;
        let n = (r / q).round() * q;
        if (r - n).abs() <= SNAP { n } else { r }
    }

    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.offset[0] += dx;
        self.offset[1] += dy;
    }

    /// Fit the whole document in view with a margin; resets rotation.
    pub fn fit(&mut self) {
        let m = 48.0;
        let sx = (self.view_size[0] - 2.0 * m).max(1.0) / self.doc_size[0].max(1.0);
        let sy = (self.view_size[1] - 2.0 * m).max(1.0) / self.doc_size[1].max(1.0);
        self.scale = sx.min(sy).clamp(MIN_SCALE, MAX_SCALE);
        self.rotation = 0.0;
        self.offset = [0.0, 0.0];
    }

    /// 100% zoom about the view centre.
    pub fn actual_pixels(&mut self) {
        let c = [self.view_size[0] / 2.0, self.view_size[1] / 2.0];
        self.set_scale_at(c, 1.0);
    }
}
