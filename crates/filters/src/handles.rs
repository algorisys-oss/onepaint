//! On-canvas handles for the filters that have a place on the picture
//! (happypaint `engine/adjustments/filter-handles.ts`; spec 60): a centre,
//! a reach, a focus band, an ellipse, Field Blur's pins, a gradient fill's
//! ends and a pattern fill's origin. Pure geometry: document px in, params
//! out.

use crate::{BlurPin, Kind, MAX_BLUR_PINS, Params};

/// Which handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleId {
    Center,
    Reach,
    Focus,
    Falloff,
    Rotate,
    Rx,
    Ry,
    Inner,
    Start,
    End,
    Origin,
    Pin(usize),
}

/// A handle in document px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Handle {
    pub id: HandleId,
    pub x: f64,
    pub y: f64,
}

/// A shape to draw over the picture, in document px.
#[derive(Clone, Debug, PartialEq)]
pub enum Guide {
    Poly { pts: Vec<[f64; 2]>, closed: bool, dashed: bool },
    Label { x: f64, y: f64, text: String },
}

/// Screen px from the centre to tilt-shift's rotate handle.
const ROTATE_SCREEN_PX: f64 = 120.0;

fn centred(k: Kind) -> bool {
    matches!(
        k,
        Kind::RadialBlur
            | Kind::Twirl
            | Kind::Pinch
            | Kind::PolarCoordinates
            | Kind::TiltShift
            | Kind::IrisBlur
            | Kind::SpinBlur
            | Kind::LightRays
            | Kind::LensFlare
    )
}

/// Kinds shaped by an ellipse: Spin Blur spins inside the one Iris keeps sharp.
fn elliptic(k: Kind) -> bool {
    matches!(k, Kind::IrisBlur | Kind::SpinBlur)
}

pub fn has_handles(k: Kind) -> bool {
    centred(k) || matches!(k, Kind::FieldBlur | Kind::GradientFill | Kind::PatternFill)
}

type Pt = [f64; 2];

fn add(a: Pt, b: Pt, k: f64) -> Pt {
    [a[0] + b[0] * k, a[1] + b[1] * k]
}

fn dot(a: Pt, b: Pt) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn v(o: Option<f32>, d: f64) -> f64 {
    o.map_or(d, f64::from)
}

/// A gradient fill's ends in doc px (its params are doc fractions).
fn grad_ends(p: &Params, doc: Pt) -> (Pt, Pt) {
    ([v(p.grad_ax, 0.5) * doc[0], v(p.grad_ay, 0.0) * doc[1]], [v(p.grad_bx, 0.5) * doc[0], v(p.grad_by, 1.0) * doc[1]])
}

/// Ramp ends may sit off the canvas, but not absurdly far: −1..2.
fn clamp_end(x: f64) -> f32 {
    x.clamp(-1.0, 2.0) as f32
}

fn clamp01(x: f64) -> f32 {
    x.clamp(0.0, 1.0) as f32
}

/// The centre, the direction of the angle (u) and its normal (n), the shorter side and half it.
struct Frame {
    c: Pt,
    u: Pt,
    n: Pt,
    min_side: f64,
    half: f64,
}

fn frame(p: &Params, doc: Pt) -> Frame {
    let a = v(p.angle, 0.0).to_radians();
    let min_side = doc[0].min(doc[1]);
    Frame {
        c: [v(p.center_x, 0.5) * doc[0], v(p.center_y, 0.5) * doc[1]],
        u: [a.cos(), -a.sin()],
        n: [a.sin(), a.cos()],
        min_side,
        half: min_side / 2.0,
    }
}

/// Iris ellipse radii in doc px.
fn iris_radii(p: &Params, half: f64) -> (f64, f64) {
    let rx = (v(p.extent, 0.6) * half).max(1.0);
    (rx, (rx * v(p.aspect, 0.7)).max(1.0))
}

/// Every handle of the filter, in doc px. `doc_per_screen` keeps a handle
/// that sits a fixed screen distance away (tilt-shift's rotate) there at
/// any zoom.
pub fn handles(kind: Kind, p: &Params, doc: Pt, doc_per_screen: f64) -> Vec<Handle> {
    let h = |id, q: Pt| Handle { id, x: q[0], y: q[1] };
    match kind {
        Kind::FieldBlur => {
            return p
                .pins
                .iter()
                .flatten()
                .enumerate()
                .map(|(i, q)| h(HandleId::Pin(i), [f64::from(q.x) * doc[0], f64::from(q.y) * doc[1]]))
                .collect();
        }
        Kind::GradientFill => {
            let (a, b) = grad_ends(p, doc);
            return vec![h(HandleId::Start, a), h(HandleId::End, b)];
        }
        Kind::PatternFill => {
            // Where the tiling starts: drag it to slide the pattern.
            return vec![h(HandleId::Origin, [v(p.pattern_off_x, 0.0) * doc[0], v(p.pattern_off_y, 0.0) * doc[1]])];
        }
        _ if !centred(kind) => return Vec::new(),
        _ => {}
    }
    let f = frame(p, doc);
    let mut out = vec![h(HandleId::Center, f.c)];
    if matches!(kind, Kind::Twirl | Kind::Pinch) {
        out.push(h(HandleId::Reach, add(f.c, [1.0, 0.0], v(p.extent, 0.8) * f.half)));
    } else if kind == Kind::TiltShift {
        let fo = v(p.focus_size, 0.1) * f.min_side;
        let g = v(p.falloff_size, 0.2) * f.min_side;
        out.extend([
            h(HandleId::Focus, add(f.c, f.n, fo)),
            h(HandleId::Focus, add(f.c, f.n, -fo)),
            h(HandleId::Falloff, add(f.c, f.n, fo + g)),
            h(HandleId::Falloff, add(f.c, f.n, -(fo + g))),
            h(HandleId::Rotate, add(f.c, f.u, ROTATE_SCREEN_PX * doc_per_screen)),
        ]);
    } else if elliptic(kind) {
        let (rx, ry) = iris_radii(p, f.half);
        let k = v(p.focus, 0.5) * std::f64::consts::FRAC_1_SQRT_2;
        out.extend([
            h(HandleId::Rx, add(f.c, f.u, rx)),
            h(HandleId::Ry, add(f.c, f.n, ry)),
            // On the sharp core's ellipse, at 45° between the axes.
            h(HandleId::Inner, add(add(f.c, f.u, rx * k), f.n, ry * k)),
        ]);
    }
    out
}

/// The handle nearest `pt` within `tol` doc px.
pub fn hit(kind: Kind, p: &Params, doc: Pt, pt: Pt, tol: f64, doc_per_screen: f64) -> Option<HandleId> {
    let mut best = None;
    let mut best_d = tol;
    for h in handles(kind, p, doc, doc_per_screen) {
        let d = (h.x - pt[0]).hypot(h.y - pt[1]);
        if d <= best_d {
            (best, best_d) = (Some(h.id), d);
        }
    }
    best
}

/// The params after dragging handle `id` to `pt` (absolute, not a delta).
pub fn drag(p: &Params, doc: Pt, id: HandleId, pt: Pt) -> Params {
    let mut o = p.clone();
    match id {
        HandleId::Pin(i) => {
            let pins: Vec<BlurPin> = p
                .pins
                .iter()
                .flatten()
                .enumerate()
                .map(|(j, q)| if j == i { BlurPin { x: clamp01(pt[0] / doc[0]), y: clamp01(pt[1] / doc[1]), ..*q } } else { *q })
                .collect();
            o.pins = Some(pins);
        }
        HandleId::Center => (o.center_x, o.center_y) = (Some(clamp01(pt[0] / doc[0])), Some(clamp01(pt[1] / doc[1]))),
        HandleId::Start => (o.grad_ax, o.grad_ay) = (Some(clamp_end(pt[0] / doc[0])), Some(clamp_end(pt[1] / doc[1]))),
        HandleId::End => (o.grad_bx, o.grad_by) = (Some(clamp_end(pt[0] / doc[0])), Some(clamp_end(pt[1] / doc[1]))),
        HandleId::Origin => (o.pattern_off_x, o.pattern_off_y) = (Some(clamp01(pt[0] / doc[0])), Some(clamp01(pt[1] / doc[1]))),
        _ => {
            let f = frame(p, doc);
            let d = [pt[0] - f.c[0], pt[1] - f.c[1]];
            match id {
                HandleId::Reach => o.extent = Some((d[0].hypot(d[1]) / f.half).clamp(0.05, 1.5) as f32),
                HandleId::Focus => o.focus_size = Some((dot(d, f.n).abs() / f.min_side) as f32),
                HandleId::Falloff => {
                    let fo = v(p.focus_size, 0.1) * f.min_side;
                    o.falloff_size = Some(((dot(d, f.n).abs() - fo).max(0.0) / f.min_side) as f32);
                }
                HandleId::Rotate => o.angle = Some((-d[1]).atan2(d[0]).to_degrees() as f32),
                HandleId::Rx => {
                    // Resize and turn together, holding the y radius in px.
                    let (_, ry) = iris_radii(p, f.half);
                    let rx = d[0].hypot(d[1]).max(1.0);
                    (o.angle, o.extent, o.aspect) =
                        (Some((-d[1]).atan2(d[0]).to_degrees() as f32), Some((rx / f.half) as f32), Some((ry / rx) as f32));
                }
                HandleId::Ry => {
                    let (rx, _) = iris_radii(p, f.half);
                    o.aspect = Some((dot(d, f.n).abs().max(1.0) / rx) as f32);
                }
                HandleId::Inner => {
                    let (rx, ry) = iris_radii(p, f.half);
                    let e = (dot(d, f.u) / rx).hypot(dot(d, f.n) / ry);
                    o.focus = Some(e.min(0.95) as f32);
                }
                _ => {}
            }
        }
    }
    o
}

/// Field Blur without pin `i` (Alt-click); `None` for no such pin or the last one.
pub fn remove_pin(p: &Params, i: usize) -> Option<Params> {
    let pins = p.pins.clone().unwrap_or_default();
    if i >= pins.len() || pins.len() <= 1 {
        return None;
    }
    Some(Params { pins: Some(pins.into_iter().enumerate().filter(|(j, _)| *j != i).map(|(_, q)| q).collect()), ..p.clone() })
}

/// A press that hit no handle: centred filters move their centre there and
/// drag it; Field Blur drops a pin with the last pin's blur; a gradient
/// fill restarts its ramp there and drags the end. `None`: nothing.
pub fn press_empty(kind: Kind, p: &Params, doc: Pt, pt: Pt) -> Option<(Params, HandleId)> {
    match kind {
        Kind::FieldBlur => {
            let mut pins = p.pins.clone().unwrap_or_default();
            if pins.len() >= MAX_BLUR_PINS {
                return None;
            }
            let blur = pins.last().map_or(20.0, |q| q.blur);
            pins.push(BlurPin { x: clamp01(pt[0] / doc[0]), y: clamp01(pt[1] / doc[1]), blur });
            let n = pins.len() - 1;
            Some((Params { pins: Some(pins), ..p.clone() }, HandleId::Pin(n)))
        }
        Kind::GradientFill => Some((drag(&drag(p, doc, HandleId::Start, pt), doc, HandleId::End, pt), HandleId::End)),
        _ if centred(kind) => Some((drag(p, doc, HandleId::Center, pt), HandleId::Center)),
        _ => None,
    }
}

/// A closed ellipse as a polyline.
fn ellipse(c: Pt, u: Pt, n: Pt, rx: f64, ry: f64) -> Vec<Pt> {
    (0..64)
        .map(|i| {
            let t = f64::from(i) / 64.0 * std::f64::consts::TAU;
            add(add(c, u, t.cos() * rx), n, t.sin() * ry)
        })
        .collect()
}

/// The lines and outlines that show the filter's shape, in doc px.
pub fn guides(kind: Kind, p: &Params, doc: Pt) -> Vec<Guide> {
    let poly = |pts, closed, dashed| Guide::Poly { pts, closed, dashed };
    match kind {
        Kind::GradientFill => {
            let (a, b) = grad_ends(p, doc);
            let mut out = vec![poly(vec![a, b], false, false)];
            // Radial, Angle and Diamond are about the start: show their reach.
            if matches!(p.grad_shape.as_deref(), Some("radial" | "angle" | "diamond")) {
                let r = (b[0] - a[0]).hypot(b[1] - a[1]);
                out.push(poly(ellipse(a, [1.0, 0.0], [0.0, 1.0], r, r), true, true));
            }
            out
        }
        Kind::FieldBlur => p
            .pins
            .iter()
            .flatten()
            .map(|q| Guide::Label { x: f64::from(q.x) * doc[0], y: f64::from(q.y) * doc[1], text: format!("{}", q.blur.round()) })
            .collect(),
        Kind::Twirl | Kind::Pinch => {
            let f = frame(p, doc);
            let r = v(p.extent, 0.8) * f.half;
            vec![poly(ellipse(f.c, [1.0, 0.0], [0.0, 1.0], r, r), true, false)]
        }
        Kind::TiltShift => {
            let f = frame(p, doc);
            // Long enough to cross the whole canvas from anywhere on it.
            let l = doc[0].hypot(doc[1]);
            let line = |off: f64, dashed: bool| {
                let m = add(f.c, f.n, off);
                poly(vec![add(m, f.u, -l), add(m, f.u, l)], false, dashed)
            };
            let fo = v(p.focus_size, 0.1) * f.min_side;
            let g = fo + v(p.falloff_size, 0.2) * f.min_side;
            vec![line(fo, false), line(-fo, false), line(g, true), line(-g, true)]
        }
        _ if elliptic(kind) => {
            let f = frame(p, doc);
            let (rx, ry) = iris_radii(p, f.half);
            let k = v(p.focus, 0.5);
            vec![poly(ellipse(f.c, f.u, f.n, rx, ry), true, false), poly(ellipse(f.c, f.u, f.n, rx * k, ry * k), true, true)]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // happypaint filter-handles.test.ts; the document is 1000 × 800 (half
    // the shorter side is 400).
    const DOC: Pt = [1000.0, 800.0];

    fn near(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-4, "{a} vs {b}");
    }

    fn pr() -> Params {
        Params::default()
    }

    fn handle(kind: Kind, p: &Params, id: HandleId) -> Handle {
        *handles(kind, p, DOC, 1.0).iter().find(|h| h.id == id).unwrap()
    }

    #[test]
    fn which_filters_get_handles() {
        use Kind::*;
        for k in [
            RadialBlur,
            Twirl,
            Pinch,
            PolarCoordinates,
            TiltShift,
            IrisBlur,
            FieldBlur,
            SpinBlur,
            LightRays,
            LensFlare,
            GradientFill,
            PatternFill,
        ] {
            assert!(has_handles(k), "{k:?}");
        }
        for k in [GaussianBlur, Mosaic, Emboss, Curves, Ripple] {
            assert!(!has_handles(k), "{k:?}");
        }
    }

    #[test]
    fn the_centre() {
        let c = handle(Kind::Twirl, &Params { center_x: Some(0.25), center_y: Some(0.5), ..pr() }, HandleId::Center);
        assert_eq!([c.x, c.y], [250.0, 400.0]);
        let d = drag(&pr(), DOC, HandleId::Center, [500.0, 200.0]);
        assert_eq!((d.center_x, d.center_y), (Some(0.5), Some(0.25)));
        let d = drag(&pr(), DOC, HandleId::Center, [-50.0, 900.0]);
        assert_eq!((d.center_x, d.center_y), (Some(0.0), Some(1.0)));
        let (p, h) = press_empty(Kind::LensFlare, &crate::default_params(Kind::LensFlare), DOC, [100.0, 80.0]).unwrap();
        assert_eq!((h, p.center_x, p.center_y), (HandleId::Center, Some(0.1), Some(0.1)));
    }

    #[test]
    fn twirl_and_pinch_reach() {
        let p = Params { center_x: Some(0.5), center_y: Some(0.5), extent: Some(0.5), ..pr() };
        let h = handle(Kind::Pinch, &p, HandleId::Reach);
        near((h.x - 500.0).hypot(h.y - 400.0), 200.0);
        near(f64::from(drag(&p, DOC, HandleId::Reach, [500.0, 100.0]).extent.unwrap()), 0.75);
    }

    #[test]
    fn tilt_shift() {
        let p =
            Params { center_x: Some(0.5), center_y: Some(0.5), angle: Some(0.0), focus_size: Some(0.1), falloff_size: Some(0.2), ..pr() };
        let mut f: Vec<_> = handles(Kind::TiltShift, &p, DOC, 1.0).into_iter().filter(|h| h.id == HandleId::Focus).collect();
        f.sort_by(|a, b| a.y.total_cmp(&b.y));
        near(f[0].y, 320.0);
        near(f[1].y, 480.0);
        assert!(f.iter().all(|h| (h.x - 500.0).abs() < 1e-9));
        near(f64::from(drag(&p, DOC, HandleId::Focus, [700.0, 240.0]).focus_size.unwrap()), 0.2);
        near(f64::from(drag(&p, DOC, HandleId::Focus, [300.0, 560.0]).focus_size.unwrap()), 0.2);
        near(f64::from(drag(&p, DOC, HandleId::Falloff, [500.0, 680.0]).falloff_size.unwrap()), 0.25);
        assert_eq!(drag(&p, DOC, HandleId::Falloff, [500.0, 410.0]).falloff_size, Some(0.0));
        near(f64::from(drag(&p, DOC, HandleId::Rotate, [500.0, 100.0]).angle.unwrap()), 90.0);
        let mut xs: Vec<i64> = handles(Kind::TiltShift, &Params { angle: Some(90.0), ..p.clone() }, DOC, 1.0)
            .iter()
            .filter(|h| h.id == HandleId::Focus)
            .map(|h| h.x.round() as i64)
            .collect();
        xs.sort();
        assert_eq!(xs, [420, 580]);
        let at = |z| handles(Kind::TiltShift, &p, DOC, z).into_iter().find(|h| h.id == HandleId::Rotate).unwrap();
        near((at(4.0).x - 500.0) / (at(1.0).x - 500.0), 4.0);
    }

    #[test]
    fn iris_and_spin() {
        let p = Params {
            center_x: Some(0.5),
            center_y: Some(0.5),
            extent: Some(0.5),
            aspect: Some(0.5),
            angle: Some(0.0),
            focus: Some(0.5),
            ..pr()
        };
        let (rx, ry) = (handle(Kind::IrisBlur, &p, HandleId::Rx), handle(Kind::IrisBlur, &p, HandleId::Ry));
        near(rx.x, 700.0);
        near(rx.y, 400.0);
        near(ry.x, 500.0);
        near(ry.y, 500.0);
        let d = 300.0 / std::f64::consts::SQRT_2;
        let q = drag(&p, DOC, HandleId::Rx, [500.0 + d, 400.0 - d]);
        near(f64::from(q.angle.unwrap()), 45.0);
        near(f64::from(q.extent.unwrap()), 0.75);
        near(f64::from(q.aspect.unwrap()), 100.0 / 300.0);
        let q = drag(&p, DOC, HandleId::Ry, [500.0, 550.0]);
        near(f64::from(q.aspect.unwrap()), 0.75);
        assert_eq!(Params { aspect: p.aspect, ..q.clone() }, p, "only the aspect changes");
        near(f64::from(drag(&p, DOC, HandleId::Inner, [560.0, 400.0]).focus.unwrap()), 0.3);
        assert_eq!(drag(&p, DOC, HandleId::Inner, [900.0, 400.0]).focus, Some(0.95));
        assert_eq!(handles(Kind::SpinBlur, &p, DOC, 1.0), handles(Kind::IrisBlur, &p, DOC, 1.0));
        assert_eq!(guides(Kind::SpinBlur, &p, DOC), guides(Kind::IrisBlur, &p, DOC));
    }

    #[test]
    fn field_blur_pins() {
        let pins = vec![BlurPin { x: 0.5, y: 0.5, blur: 0.0 }, BlurPin { x: 0.1, y: 0.1, blur: 30.0 }];
        let p = Params { pins: Some(pins.clone()), ..pr() };
        let (q, h) = press_empty(Kind::FieldBlur, &p, DOC, [900.0, 400.0]).unwrap();
        assert_eq!(h, HandleId::Pin(2));
        assert_eq!(q.pins.unwrap()[2], BlurPin { x: 0.9, y: 0.5, blur: 30.0 });
        let full = Params { pins: Some((0..8).map(|i| BlurPin { x: i as f32 / 10.0, y: 0.5, blur: 10.0 }).collect()), ..pr() };
        assert!(press_empty(Kind::FieldBlur, &full, DOC, [500.0, 100.0]).is_none());
        assert_eq!(remove_pin(&p, 0).unwrap().pins.unwrap(), vec![pins[1]]);
        assert!(remove_pin(&Params { pins: Some(vec![pins[0]]), ..pr() }, 0).is_none());
        assert!(remove_pin(&p, 5).is_none());
        assert_eq!(drag(&p, DOC, HandleId::Pin(1), [200.0, 400.0]).pins.unwrap(), vec![pins[0], BlurPin { x: 0.2, y: 0.5, blur: 30.0 }]);
    }

    #[test]
    fn hit_testing_and_guides() {
        let p = crate::default_params(Kind::IrisBlur);
        let rx = handle(Kind::IrisBlur, &p, HandleId::Rx);
        assert_eq!(hit(Kind::IrisBlur, &p, DOC, [rx.x + 3.0, rx.y - 2.0], 10.0, 1.0), Some(HandleId::Rx));
        assert_eq!(hit(Kind::IrisBlur, &p, DOC, [5.0, 5.0], 10.0, 1.0), None);
        let g = guides(Kind::TiltShift, &crate::default_params(Kind::TiltShift), DOC);
        let dashed: Vec<bool> = g.iter().map(|g| matches!(g, Guide::Poly { dashed: true, .. })).collect();
        assert_eq!((dashed.iter().filter(|d| !**d).count(), dashed.iter().filter(|d| **d).count()), (2, 2));
        let g = guides(Kind::IrisBlur, &p, DOC);
        assert!(matches!(&g[..], [Guide::Poly { closed: true, dashed: false, .. }, Guide::Poly { closed: true, dashed: true, .. }]));
    }

    #[test]
    fn gradient_and_pattern_fills() {
        let p = Params { grad_ax: Some(0.1), grad_ay: Some(0.25), grad_bx: Some(0.9), grad_by: Some(0.5), ..pr() };
        let hs: Vec<_> = handles(Kind::GradientFill, &p, DOC, 1.0).iter().map(|h| (h.id, h.x.round(), h.y.round())).collect();
        assert_eq!(hs, [(HandleId::Start, 100.0, 200.0), (HandleId::End, 900.0, 400.0)]);
        let e = drag(&p, DOC, HandleId::End, [500.0, 200.0]);
        assert_eq!((e.grad_bx, e.grad_by), (Some(0.5), Some(0.25)));
        let s = drag(&p, DOC, HandleId::Start, [-200.0, 1000.0]);
        assert_eq!((s.grad_ax, s.grad_ay), (Some(-0.2), Some(1.25)));
        let s = drag(&p, DOC, HandleId::Start, [-5000.0, 9000.0]);
        assert_eq!((s.grad_ax, s.grad_ay), (Some(-1.0), Some(2.0)));
        let (q, h) = press_empty(Kind::GradientFill, &p, DOC, [300.0, 400.0]).unwrap();
        assert_eq!((h, q.grad_ax, q.grad_ay, q.grad_bx, q.grad_by), (HandleId::End, Some(0.3), Some(0.5), Some(0.3), Some(0.5)));
        assert_eq!(guides(Kind::GradientFill, &Params { grad_shape: Some("linear".into()), ..p.clone() }, DOC).len(), 1);
        assert_eq!(guides(Kind::GradientFill, &Params { grad_shape: Some("radial".into()), ..p }, DOC).len(), 2);
        let o = handles(Kind::PatternFill, &Params { pattern_off_x: Some(0.25), pattern_off_y: Some(0.5), ..pr() }, DOC, 1.0);
        assert_eq!(o, vec![Handle { id: HandleId::Origin, x: 250.0, y: 400.0 }]);
        let d = drag(&pr(), DOC, HandleId::Origin, [500.0, 200.0]);
        assert_eq!((d.pattern_off_x, d.pattern_off_y), (Some(0.5), Some(0.25)));
        let d = drag(&pr(), DOC, HandleId::Origin, [-50.0, 9000.0]);
        assert_eq!((d.pattern_off_x, d.pattern_off_y), (Some(0.0), Some(1.0)));
    }
}
