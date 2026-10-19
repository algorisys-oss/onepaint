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
        let mut v = Self { view_size, doc_size, scale: 1.0, rotation: 0.0, offset: [0.0, 0.0] };
        v.fit();
        v
    }

    pub fn doc_to_screen(&self, p: [f64; 2]) -> [f64; 2] {
        let (c, s) = (self.rotation.cos(), self.rotation.sin());
        let dx = (p[0] - self.doc_size[0] / 2.0) * self.scale;
        let dy = (p[1] - self.doc_size[1] / 2.0) * self.scale;
        [self.view_size[0] / 2.0 + self.offset[0] + dx * c - dy * s, self.view_size[1] / 2.0 + self.offset[1] + dx * s + dy * c]
    }

    pub fn screen_to_doc(&self, p: [f64; 2]) -> [f64; 2] {
        let (c, s) = (self.rotation.cos(), self.rotation.sin());
        let qx = p[0] - self.view_size[0] / 2.0 - self.offset[0];
        let qy = p[1] - self.view_size[1] / 2.0 - self.offset[1];
        [self.doc_size[0] / 2.0 + (qx * c + qy * s) / self.scale, self.doc_size[1] / 2.0 + (-qx * s + qy * c) / self.scale]
    }

    /// Multiply the scale by `factor`, keeping the screen point `at` fixed.
    pub fn zoom_at(&mut self, at: [f64; 2], factor: f64) {
        let ns = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let f = ns / self.scale;
        let q = [at[0] - self.view_size[0] / 2.0, at[1] - self.view_size[1] / 2.0];
        self.offset = [q[0] - f * (q[0] - self.offset[0]), q[1] - f * (q[1] - self.offset[1])];
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

// ── Scrollbars (happypaint `utils/scrollbars.ts`, v0.192.0) ──

/// Room to scroll past the document, as a share of the view.
pub const SCROLL_MARGIN: f64 = 0.4;
/// The smallest thumb, px.
pub const THUMB_MIN: f64 = 24.0;

/// One scrollbar's geometry along its axis, screen px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisBar {
    pub visible: bool,
    pub range: f64,
    pub view: f64,
    pub track: f64,
    pub thumb_start: f64,
    pub thumb_size: f64,
}

impl View {
    /// The document's screen extent: x0, x1, y0, y1 of its four corners
    /// (rotated too).
    pub fn doc_screen_extent(&self) -> [f64; 4] {
        let [w, h] = self.doc_size;
        let pts = [[0.0, 0.0], [w, 0.0], [0.0, h], [w, h]].map(|p| self.doc_to_screen(p));
        let f = |k: usize, max: bool| {
            pts.iter().map(|p| p[k]).fold(if max { f64::MIN } else { f64::MAX }, if max { f64::max } else { f64::min })
        };
        [f(0, false), f(0, true), f(1, false), f(1, true)]
    }
}

/// A bar for a view `view` px long showing content from `a` to `b` on a
/// `track` px track: hidden while the content fits.
pub fn axis_bar(view: f64, a: f64, b: f64, track: f64) -> AxisBar {
    if a >= 0.0 && b <= view {
        return AxisBar { visible: false, range: view, view, track, thumb_start: 0.0, thumb_size: track };
    }
    let m = SCROLL_MARGIN * view;
    let lo = (a - m).min(0.0);
    let hi = (b + m).max(view);
    let range = view.max(hi - lo);
    let thumb_size = track.min(THUMB_MIN.max(view / range * track));
    let scrollable = range - view;
    let travel = track - thumb_size;
    let thumb_start = if scrollable > 0.0 { -lo / scrollable * travel } else { 0.0 };
    AxisBar { visible: scrollable > 0.0, range, view, track, thumb_start: thumb_start.clamp(0.0, travel.max(0.0)), thumb_size }
}

/// The pan (px) a thumb moved `d_thumb` px stands for.
pub fn thumb_drag_to_pan(bar: &AxisBar, d_thumb: f64) -> f64 {
    let scrollable = bar.range - bar.view;
    let travel = bar.track - bar.thumb_size;
    if scrollable <= 0.0 || travel <= 0.0 {
        return 0.0;
    }
    -(d_thumb * scrollable) / travel
}

/// The pan a click on the track before (−1) or after (1) the thumb takes.
pub fn page_step(view: f64, dir: f64) -> f64 {
    -dir * (view * 0.9).round()
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    // happypaint scrollbars.test.ts.
    #[test]
    fn the_document_extent_on_screen() {
        let v = View { view_size: [1000.0, 800.0], doc_size: [400.0, 300.0], scale: 2.0, rotation: 0.0, offset: [0.0, 0.0] };
        assert_eq!(v.doc_screen_extent(), [100.0, 900.0, 100.0, 700.0]);
        let r = View {
            view_size: [1000.0, 1000.0],
            doc_size: [400.0, 200.0],
            scale: 1.0,
            rotation: std::f64::consts::FRAC_PI_2,
            offset: [0.0, 0.0],
        };
        let e = r.doc_screen_extent();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(close(e[0], 400.0) && close(e[1], 600.0) && close(e[2], 300.0) && close(e[3], 700.0), "{e:?}");
    }

    #[test]
    fn bars_hide_while_it_fits_and_size_their_thumbs() {
        assert!(!axis_bar(1000.0, 100.0, 900.0, 1000.0).visible);
        let b = axis_bar(1000.0, -500.0, 1500.0, 1000.0);
        assert!(b.visible && b.range == 2800.0);
        assert!((b.thumb_size - 1000.0 / 2800.0 * 1000.0).abs() < 1e-9);
        assert!((b.thumb_start - (1000.0 - b.thumb_size) / 2.0).abs() < 1e-9, "centred");
        assert_eq!(axis_bar(1000.0, 400.0, 1500.0, 1000.0).thumb_start, 0.0, "at the margin");
        let far = axis_bar(1000.0, 5000.0, 9000.0, 1000.0);
        assert!(far.thumb_start == 0.0 && far.thumb_start + far.thumb_size <= 1000.0);
        let huge = axis_bar(1000.0, -1e6, 1e6, 600.0);
        assert_eq!(huge.thumb_size, 24.0);
        assert!((thumb_drag_to_pan(&huge, 576.0) + (huge.range - 1000.0)).abs() < 1e-6);
        assert!((thumb_drag_to_pan(&b, b.track - b.thumb_size) + (b.range - b.view)).abs() < 1e-9);
        assert_eq!(thumb_drag_to_pan(&axis_bar(1000.0, 100.0, 900.0, 1000.0), 50.0), 0.0);
        assert_eq!((page_step(1000.0, 1.0), page_step(1000.0, -1.0)), (-900.0, 900.0));
    }
}
