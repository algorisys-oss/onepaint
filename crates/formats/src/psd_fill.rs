//! PSD fill layers both ways (happypaint `psd-map.ts` `fillToPsd` /
//! `fillFromPsd`): Solid Color (`SoCo`) and Gradient (`GdFl`) as live fill
//! layers; patterns stay pixels, as in happypaint.

use crate::psd_desc::{DVal, Desc, read, write_into};
use crate::psd_effects::{colour, colour_of, gradient_desc, stops_of};
use onepaint_doc::Adjustment;
use onepaint_filters::{FillStop, Params};

/// Shapes about their start point (Photoshop centres them on the offset).
fn centred(shape: &str) -> bool {
    matches!(shape, "radial" | "angle" | "diamond" | "reflected")
}

/// Half the document's extent along `deg` (Photoshop's angle, y up).
fn half_extent(deg: f64, w: f64, h: f64) -> f64 {
    let t = deg.to_radians();
    ((w * t.cos()).abs() + (h * t.sin()).abs()) / 2.0
}

fn versioned(d: &Desc) -> Vec<u8> {
    let mut o = 16u32.to_be_bytes().to_vec();
    write_into(&mut o, d);
    o
}

/// A fill layer as its PSD block, or `None` (patterns: written as pixels).
pub fn write(a: &Adjustment, w: f64, h: f64) -> Option<([u8; 4], Vec<u8>)> {
    let p = &a.params;
    match a.kind.as_str() {
        "solid-fill" => {
            let mut d = Desc::new("null");
            d.push("Clr ", colour(p.fill_color.unwrap_or([0.0; 3])));
            Some((*b"SoCo", versioned(&d)))
        }
        "gradient-fill" => {
            let shape = p.grad_shape.clone().unwrap_or_else(|| "linear".into());
            let v = |o: Option<f32>, d: f64| o.map_or(d, f64::from);
            let (ax, ay) = (v(p.grad_ax, 0.5) * w, v(p.grad_ay, 0.0) * h);
            let (bx, by) = (v(p.grad_bx, 0.5) * w, v(p.grad_by, 1.0) * h);
            let c = centred(&shape);
            let (cx, cy) = if c { (ax, ay) } else { ((ax + bx) / 2.0, (ay + by) / 2.0) };
            let len = (bx - ax).hypot(by - ay) / if c { 1.0 } else { 2.0 };
            let angle = (-(by - ay)).atan2(bx - ax).to_degrees();
            let he = half_extent(angle, w, h);
            let stops = p
                .fill_stops
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|s| (f64::from(s.pos), s.color, s.opacity, s.mid.map(f64::from)))
                .collect();
            let kind = match shape.as_str() {
                "radial" => "Rdl ",
                "angle" => "Angl",
                "reflected" => "Rflc",
                "diamond" => "Dmnd",
                _ => "Lnr ",
            };
            let mut ofst = Desc::new("Pnt ");
            ofst.push("Hrzn", DVal::Unit(*b"#Prc", (cx / w - 0.5) * 100.0)).push("Vrtc", DVal::Unit(*b"#Prc", (cy / h - 0.5) * 100.0));
            let mut d = Desc::new("null");
            d.push("Grad", gradient_desc(stops))
                .push("Type", DVal::Enum("GrdT".into(), kind.into()))
                .push("Angl", DVal::Unit(*b"#Ang", angle))
                .push("Scl ", DVal::Unit(*b"#Prc", if he > 0.0 { len / he * 100.0 } else { 100.0 }))
                .push("Rvrs", DVal::Bool(p.grad_reverse.unwrap_or(false)))
                .push("Algn", DVal::Bool(false))
                .push("Ofst", DVal::Obj(ofst));
            Some((*b"GdFl", versioned(&d)))
        }
        _ => None,
    }
}

/// A fill block as a live fill layer, or `None` (patterns, colours that
/// can't be read): the record's pixels stay.
pub fn read_block(key: &[u8; 4], data: &[u8], w: f64, h: f64) -> Option<Adjustment> {
    if data.get(..4) != Some(&16u32.to_be_bytes()) {
        return None;
    }
    let (d, _) = read(data, 4)?;
    match key {
        b"SoCo" => Some(Adjustment {
            kind: "solid-fill".into(),
            params: Params { fill_color: Some(colour_of(d.obj("Clr "))?), ..Default::default() },
        }),
        b"GdFl" => {
            let shape = match d.enum_val("Type") {
                Some("Rdl ") => "radial",
                Some("Angl") => "angle",
                Some("Rflc") => "reflected",
                Some("Dmnd") => "diamond",
                _ => "linear",
            };
            let stops: Vec<FillStop> = stops_of(d.obj("Grad")?)?
                .iter()
                .filter_map(|s| {
                    let c = s.get("color")?.as_array()?;
                    Some(FillStop {
                        pos: s.get("pos")?.as_f64()? as f32,
                        color: [c.first()?.as_f64()? as f32, c.get(1)?.as_f64()? as f32, c.get(2)?.as_f64()? as f32],
                        opacity: s.get("opacity").and_then(serde_json::Value::as_f64).unwrap_or(1.0) as f32,
                        mid: s.get("mid").and_then(serde_json::Value::as_f64).map(|m| m as f32),
                    })
                })
                .collect();
            if stops.is_empty() {
                return None;
            }
            let angle = d.f64("Angl").unwrap_or(90.0);
            let len = d.f64("Scl ").map_or(1.0, |s| s / 100.0) * half_extent(angle, w, h);
            let o = d.obj("Ofst");
            let off = |k: &str| o.and_then(|o| o.f64(k)).map_or(0.0, |v| v / 100.0);
            let (cx, cy) = ((off("Hrzn") + 0.5) * w, (off("Vrtc") + 0.5) * h);
            let (ux, uy) = (angle.to_radians().cos(), -angle.to_radians().sin());
            let (a, b) = if centred(shape) {
                ((cx, cy), (cx + ux * len, cy + uy * len))
            } else {
                ((cx - ux * len, cy - uy * len), (cx + ux * len, cy + uy * len))
            };
            let f = |v: f64| Some(v as f32);
            Some(Adjustment {
                kind: "gradient-fill".into(),
                params: Params {
                    grad_shape: Some(shape.into()),
                    fill_stops: Some(stops),
                    grad_reverse: Some(d.bool("Rvrs").unwrap_or(false)),
                    grad_ax: f(a.0 / w),
                    grad_ay: f(a.1 / h),
                    grad_bx: f(b.0 / w),
                    grad_by: f(b.1 / h),
                    ..Default::default()
                },
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Option<f32>, b: f32) -> bool {
        a.is_some_and(|a| (a - b).abs() < 1e-3)
    }

    #[test]
    fn solid_and_gradient_fills_round_trip() {
        let solid = Adjustment { kind: "solid-fill".into(), params: Params { fill_color: Some([1.0, 0.5, 0.0]), ..Default::default() } };
        let (k, b) = write(&solid, 400.0, 200.0).unwrap();
        let back = read_block(&k, &b, 400.0, 200.0).unwrap();
        assert_eq!(back.kind, "solid-fill");
        let c = back.params.fill_color.unwrap();
        assert!((c[1] - 128.0 / 255.0).abs() < 1e-6 && c[0] == 1.0 && c[2] == 0.0);
        for shape in ["linear", "radial", "reflected"] {
            let g = Adjustment {
                kind: "gradient-fill".into(),
                params: Params {
                    grad_shape: Some(shape.into()),
                    fill_stops: Some(vec![
                        FillStop { pos: 0.0, color: [1.0, 0.0, 0.0], opacity: 1.0, mid: Some(0.3) },
                        FillStop { pos: 1.0, color: [0.0, 0.0, 1.0], opacity: 0.5, mid: None },
                    ]),
                    grad_reverse: Some(true),
                    grad_ax: Some(0.1),
                    grad_ay: Some(0.25),
                    grad_bx: Some(0.9),
                    grad_by: Some(0.5),
                    ..Default::default()
                },
            };
            let (k, b) = write(&g, 400.0, 200.0).unwrap();
            assert_eq!(&k, b"GdFl");
            let p = read_block(&k, &b, 400.0, 200.0).unwrap().params;
            assert_eq!((p.grad_shape.as_deref(), p.grad_reverse), (Some(shape), Some(true)));
            assert!(close(p.grad_ax, 0.1) && close(p.grad_ay, 0.25) && close(p.grad_bx, 0.9) && close(p.grad_by, 0.5), "{shape}: {p:?}");
            let s = p.fill_stops.unwrap();
            assert_eq!(s.len(), 2);
            assert!(close(s[0].mid, 0.3) && (s[1].opacity - 0.5).abs() < 1e-2);
        }
        let pattern = Adjustment { kind: "pattern-fill".into(), params: Params::default() };
        assert!(write(&pattern, 10.0, 10.0).is_none());
    }
}
