//! Distort kinds (AR:1205-1290): twirl, pinch, ripple and polar coordinates.
//! They only move where they read, so they resample premultiplied RGBA as is.

use crate::params::Params;
use crate::tex::{Src, fract, len2, render};

fn centre(p: &Params, size: [f32; 2]) -> [f32; 2] {
    [p.center_x.unwrap_or(0.5) * size[0], p.center_y.unwrap_or(0.5) * size[1]]
}

/// Twirl and pinch reach: a fraction of half the shorter side.
fn reach(p: &Params, size: [f32; 2]) -> f32 {
    (p.extent.unwrap_or(0.8) * 0.5 * size[0].min(size[1])).max(1.0)
}

/// TWIRL_SRC: rotate by `θ·(1 − r/R)²` inside the circle.
pub(crate) fn twirl(p: &Params, s: Src) -> Vec<f32> {
    let size = s.f.size();
    let (c, rad) = (centre(p, size), reach(p, size));
    let angle = p.angle.unwrap_or(0.0).to_radians();
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let d = [q[0] - c[0], q[1] - c[1]];
        let r = len2(d);
        if r < rad {
            let f = 1.0 - r / rad;
            let a = angle * f * f;
            let (co, sn) = (a.cos(), a.sin());
            s.px([c[0] + co * d[0] - sn * d[1], c[1] + sn * d[0] + co * d[1]])
        } else {
            s.t.get(i, j)
        }
    })
}

/// PINCH_SRC: read at `c + d·sin(π/2·r/R)^−amount`.
pub(crate) fn pinch(p: &Params, s: Src) -> Vec<f32> {
    let size = s.f.size();
    let (c, rad) = (centre(p, size), reach(p, size));
    let amount = p.amount.unwrap_or(0.0).clamp(-1.0, 1.0);
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let d = [q[0] - c[0], q[1] - c[1]];
        let r = len2(d);
        if r < rad && r > 0.0 {
            let k = (std::f32::consts::FRAC_PI_2 * r / rad).sin().powf(-amount);
            s.px([c[0] + d[0] * k, c[1] + d[1] * k])
        } else {
            s.t.get(i, j)
        }
    })
}

/// RIPPLE_SRC: a sine across the direction of travel plus a smaller one along it.
pub(crate) fn ripple(p: &Params, s: Src) -> Vec<f32> {
    let amp = p.amplitude.unwrap_or(0.0).max(0.0);
    let len = p.wavelength.unwrap_or(60.0).max(2.0);
    let a = p.angle.unwrap_or(0.0).to_radians();
    let dir = [a.cos(), -a.sin()];
    let nrm = [-dir[1], dir[0]];
    let k = std::f32::consts::TAU / len.max(1.0);
    render(s.t.w, s.t.h, |i, j| {
        let q = s.centre(i, j);
        let along = (q[0] * dir[0] + q[1] * dir[1]) * k;
        let across = (q[0] * nrm[0] + q[1] * nrm[1]) * k * 1.37;
        let (m, n) = (amp * along.sin(), amp * 0.35 * across.sin());
        s.px([q[0] + nrm[0] * m + dir[0] * n, q[1] + nrm[1] * m + dir[1] * n])
    })
}

/// POLAR_SRC: rectangular ↔ polar over the ellipse that fits the canvas.
pub(crate) fn polar(p: &Params, s: Src) -> Vec<f32> {
    let size = s.f.size();
    let c = centre(p, size);
    let hs = [size[0] * 0.5, size[1] * 0.5];
    let to_rect = p.polar_mode.as_deref() == Some("to-rect");
    render(s.t.w, s.t.h, |i, j| {
        let pq = s.centre(i, j);
        let v_uv = [pq[0] / size[0], pq[1] / size[1]];
        let uv = if !to_rect {
            let q = [(pq[0] - c[0]) / hs[0], (pq[1] - c[1]) / hs[1]];
            let a = q[0].atan2(-q[1]);
            [fract(a / std::f32::consts::TAU + 1.0), len2(q)]
        } else {
            let a = v_uv[0] * std::f32::consts::TAU;
            [(c[0] + a.sin() * v_uv[1] * hs[0]) / size[0], (c[1] - a.cos() * v_uv[1] * hs[1]) / size[1]]
        };
        s.uv([uv[0].clamp(0.0, 1.0), uv[1].clamp(0.0, 1.0)])
    })
}
