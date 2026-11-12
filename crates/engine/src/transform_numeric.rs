//! The Transform panel's typed values (happypaint v0.209.0,
//! `components/canvas/transform-numeric.ts`): X / Y / W / H in px and an
//! angle in degrees, read from and written back to the Move tool's live box.
//!
//! The fields describe the same [`TransformParams`] the handles edit, so
//! typing and dragging drive one transform:
//! - **W / H**: the content's own size (|scale| × bounds) before rotation, so
//!   the aspect link means what it says on a rotated box.
//! - **Angle**: degrees, normalised to (−180, 180].
//! - **X / Y**: the top-left of what the box covers on the canvas (its
//!   axis-aligned bounds); with no rotation, simply the box corner.
//!
//! Editing W/H keeps X/Y where they were; editing the angle turns the box
//! about its centre, as the rotate handle does; editing X/Y only moves it.
//! Mirroring (a negative scale) is Flip's job and survives a resize.

use crate::transform::TransformParams;
use onepaint_raster::Rect;

/// The panel's numbers for one box.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TransformFields {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Degrees.
    pub angle: f64,
}

/// Any subset of the fields; `None` (or a non-finite value) leaves one as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FieldsPatch {
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub w: Option<f64>,
    pub h: Option<f64>,
    pub angle: Option<f64>,
}

impl From<TransformFields> for FieldsPatch {
    fn from(f: TransformFields) -> Self {
        Self { x: Some(f.x), y: Some(f.y), w: Some(f.w), h: Some(f.h), angle: Some(f.angle) }
    }
}

fn corners(b: Rect) -> [[f64; 2]; 4] {
    let (x0, y0, x1, y1) = (f64::from(b.x), f64::from(b.y), f64::from(b.right()), f64::from(b.bottom()));
    [[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

/// The axis-aligned top-left of the transformed box.
fn aabb_min(p: &TransformParams, doc: [f64; 2], b: Rect) -> [f64; 2] {
    let m = p.to_affine(doc);
    corners(b).iter().map(|&c| m.apply(c)).fold([f64::INFINITY; 2], |a, q| [a[0].min(q[0]), a[1].min(q[1])])
}

/// Radians → degrees in (−180, 180].
fn to_degrees(rad: f64) -> f64 {
    let d = rad.to_degrees() % 360.0;
    if d <= -180.0 {
        d + 360.0
    } else if d > 180.0 {
        d - 360.0
    } else {
        d
    }
}

impl TransformParams {
    /// The panel's values for this transform of the box `b`.
    pub fn fields(&self, doc: [f64; 2], b: Rect) -> TransformFields {
        let [x, y] = aabb_min(self, doc, b);
        TransformFields { x, y, w: self.sx.abs() * f64::from(b.w), h: self.sy.abs() * f64::from(b.h), angle: to_degrees(self.rotation) }
    }

    /// The transform that shows `patch`. `lock_aspect` makes a lone W or H
    /// edit scale the other side by the same factor. Sizes are floored at 1 px.
    pub fn with_fields(&self, doc: [f64; 2], b: Rect, patch: FieldsPatch, lock_aspect: bool) -> TransformParams {
        if b.w <= 0 || b.h <= 0 {
            return *self;
        }
        let given = |v: Option<f64>| v.filter(|v| v.is_finite());
        let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
        let (pw, ph) = (given(patch.w), given(patch.h));
        let mut sx = self.sx;
        let mut sy = self.sy;
        if let Some(w) = pw {
            sx = sign(sx) * w.max(1.0) / f64::from(b.w);
        }
        if let Some(h) = ph {
            sy = sign(sy) * h.max(1.0) / f64::from(b.h);
        }
        if lock_aspect && pw.is_some() != ph.is_some() {
            if pw.is_some() {
                sy = self.sy * (sx / self.sx).abs();
            } else {
                sx = self.sx * (sy / self.sy).abs();
            }
        }
        let rotation = given(patch.angle).map_or(self.rotation, f64::to_radians);

        // Scale and turn about the box centre, which stays put…
        let c = [doc[0] / 2.0, doc[1] / 2.0];
        let cb = [f64::from(b.x) + f64::from(b.w) / 2.0, f64::from(b.y) + f64::from(b.h) / 2.0];
        let q = self.to_affine(doc).apply(cb);
        let (s, co) = rotation.sin_cos();
        let (vx, vy) = (cb[0] - c[0], cb[1] - c[1]);
        let m = [co * sx * vx - s * sy * vy, s * sx * vx + co * sy * vy];
        let mut out = TransformParams { sx, sy, rotation, dx: q[0] - c[0] - m[0], dy: q[1] - c[1] - m[1] };

        // …then a resize puts the top-left back where it was, and X/Y go where typed.
        let now = aabb_min(&out, doc, b);
        let before = aabb_min(self, doc, b);
        let resized = pw.is_some() || ph.is_some();
        let want = |typed: Option<f64>, i: usize| typed.unwrap_or(if resized { before[i] } else { now[i] });
        out.dx += want(given(patch.x), 0) - now[0];
        out.dy += want(given(patch.y), 1) - now[1];
        out
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    //! Ported from `transform-numeric.test.ts`.
    use super::*;

    const DOC: [f64; 2] = [1000.0, 800.0];
    const B: Rect = Rect::new(100, 200, 300, 150);
    const I: TransformParams = TransformParams { dx: 0.0, dy: 0.0, sx: 1.0, sy: 1.0, rotation: 0.0 };

    fn close(a: f64, e: f64) {
        assert!((a - e).abs() < 1e-6, "{a} != {e}");
    }

    fn pts(t: &TransformParams) -> Vec<[f64; 2]> {
        let m = t.to_affine(DOC);
        corners(B).iter().map(|&c| m.apply(c)).collect()
    }

    fn centre(t: &TransformParams) -> [f64; 2] {
        t.to_affine(DOC).apply([250.0, 275.0])
    }

    fn patch(f: impl FnOnce(&mut FieldsPatch)) -> FieldsPatch {
        let mut p = FieldsPatch::default();
        f(&mut p);
        p
    }

    #[test]
    fn an_untouched_box_reads_as_its_bounds() {
        assert_eq!(I.fields(DOC, B), TransformFields { x: 100.0, y: 200.0, w: 300.0, h: 150.0, angle: 0.0 });
    }

    #[test]
    fn w_and_h_are_positive_even_when_mirrored() {
        let f = TransformParams { sx: -2.0, sy: 0.5, ..I }.fields(DOC, B);
        close(f.w, 600.0);
        close(f.h, 75.0);
    }

    #[test]
    fn the_angle_is_in_degrees_normalised() {
        close(TransformParams { rotation: 1.5 * std::f64::consts::PI, ..I }.fields(DOC, B).angle, -90.0);
        close(TransformParams { rotation: std::f64::consts::PI, ..I }.fields(DOC, B).angle, 180.0);
    }

    #[test]
    fn x_and_y_are_the_top_left_of_what_the_box_covers() {
        let t = TransformParams { rotation: std::f64::consts::FRAC_PI_4, ..I };
        let f = t.fields(DOC, B);
        close(f.x, pts(&t).iter().map(|p| p[0]).fold(f64::INFINITY, f64::min));
        close(f.y, pts(&t).iter().map(|p| p[1]).fold(f64::INFINITY, f64::min));
    }

    #[test]
    fn x_and_y_move_without_resizing_or_rotating() {
        let t = I.with_fields(DOC, B, patch(|p| (p.x, p.y) = (Some(10.0), Some(20.0))), false);
        assert_eq!(t.fields(DOC, B), TransformFields { x: 10.0, y: 20.0, w: 300.0, h: 150.0, angle: 0.0 });
    }

    #[test]
    fn w_alone_stretches_one_axis_and_keeps_x_and_y() {
        let f = I.with_fields(DOC, B, patch(|p| p.w = Some(450.0)), false).fields(DOC, B);
        close(f.w, 450.0);
        close(f.h, 150.0);
        close(f.x, 100.0);
        close(f.y, 200.0);
    }

    #[test]
    fn linked_w_or_h_scales_the_other() {
        let f = I.with_fields(DOC, B, patch(|p| p.w = Some(600.0)), true).fields(DOC, B);
        close(f.w, 600.0);
        close(f.h, 300.0);
        close(f.x, 100.0);
        let f = I.with_fields(DOC, B, patch(|p| p.h = Some(50.0)), true).fields(DOC, B);
        close(f.w, 100.0);
        close(f.h, 50.0);
    }

    #[test]
    fn the_angle_turns_about_the_box_centre_and_keeps_the_size() {
        let t = I.with_fields(DOC, B, patch(|p| p.angle = Some(30.0)), false);
        let (c0, c1) = (centre(&I), centre(&t));
        close(c1[0], c0[0]);
        close(c1[1], c0[1]);
        let f = t.fields(DOC, B);
        close(f.angle, 30.0);
        close(f.w, 300.0);
        close(f.h, 150.0);
    }

    #[test]
    fn resizing_a_rotated_box_keeps_its_top_left() {
        let t0 = I.with_fields(DOC, B, patch(|p| p.angle = Some(40.0)), false);
        let f0 = t0.fields(DOC, B);
        let f1 = t0.with_fields(DOC, B, patch(|p| p.w = Some(120.0)), false).fields(DOC, B);
        close(f1.x, f0.x);
        close(f1.y, f0.y);
        close(f1.w, 120.0);
        close(f1.angle, 40.0);
    }

    #[test]
    fn a_mirrored_box_stays_mirrored_when_resized() {
        let t = TransformParams { sx: -1.0, ..I }.with_fields(DOC, B, patch(|p| p.w = Some(150.0)), false);
        assert!(t.sx < 0.0);
        close(t.fields(DOC, B).w, 150.0);
    }

    #[test]
    fn sizes_are_floored_at_1_px() {
        let f = I.with_fields(DOC, B, patch(|p| (p.w, p.h) = (Some(0.0), Some(-5.0))), false).fields(DOC, B);
        close(f.w, 1.0);
        close(f.h, 1.0);
    }

    #[test]
    fn non_finite_input_is_ignored() {
        let t = I.with_fields(DOC, B, patch(|p| (p.w, p.x) = (Some(f64::NAN), Some(f64::INFINITY))), true);
        assert_eq!(t.fields(DOC, B), I.fields(DOC, B));
    }

    #[test]
    fn writing_back_what_was_read_reproduces_the_box() {
        let t0 = TransformParams { dx: 37.0, dy: -12.0, sx: 1.7, sy: -0.6, rotation: 2.2 };
        let t1 = I.with_fields(DOC, B, t0.fields(DOC, B).into(), false);
        // Mirroring is Flip's job, not a field: the same four points, unmirrored.
        let sorted = |mut v: Vec<[f64; 2]>| {
            v.sort_by(|p, q| p[0].total_cmp(&q[0]).then(p[1].total_cmp(&q[1])));
            v
        };
        for (p, q) in sorted(pts(&t1)).iter().zip(sorted(pts(&t0))) {
            close(p[0], q[0]);
            close(p[1], q[1]);
        }
    }

    #[test]
    fn an_empty_box_is_left_alone() {
        let z = Rect::new(5, 5, 0, 10);
        assert_eq!(I.with_fields(DOC, z, patch(|p| p.w = Some(50.0)), false), I);
    }
}
