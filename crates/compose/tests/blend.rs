#![allow(clippy::unwrap_used)]
use onepaint_color::{prem_lin_to_srgb, prem_srgb_to_lin};
use onepaint_compose::blend::*;
use onepaint_doc::BlendMode::{self, *};

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-5
}

#[test]
fn separable_golden_values() {
    let vals = [0.0f32, 0.25, 0.5, 0.75, 1.0];
    for &b in &vals {
        for &s in &vals {
            let exp = |m: BlendMode| -> f32 {
                match m {
                    Multiply => b * s,
                    Screen => b + s - b * s,
                    Overlay => {
                        if b <= 0.5 {
                            2.0 * b * s
                        } else {
                            1.0 - 2.0 * (1.0 - b) * (1.0 - s)
                        }
                    }
                    Darken => b.min(s),
                    Lighten => b.max(s),
                    HardLight => {
                        if s <= 0.5 {
                            2.0 * b * s
                        } else {
                            1.0 - 2.0 * (1.0 - b) * (1.0 - s)
                        }
                    }
                    Difference => (b - s).abs(),
                    Exclusion => b + s - 2.0 * b * s,
                    LinearBurn => (b + s - 1.0).max(0.0),
                    Add => b + s,
                    _ => unreachable!(),
                }
            };
            for m in [Multiply, Screen, Overlay, Darken, Lighten, HardLight, Difference, Exclusion, LinearBurn, Add] {
                assert!(close(bc(m, b, s), exp(m)), "{m:?} b={b} s={s}");
            }
        }
    }
}

#[test]
fn dodge_burn_divide_edge_cases() {
    assert_eq!(bc(ColorDodge, 0.0, 1.0), 1.0);
    assert!(close(bc(ColorDodge, 0.25, 0.5), 0.5));
    assert_eq!(bc(ColorBurn, 1.0, 0.0), 0.0);
    assert!(close(bc(ColorBurn, 0.75, 0.5), 0.5));
    assert_eq!(bc(Divide, 0.3, 0.0), 1.0);
    assert!(close(bc(Divide, 0.25, 0.5), 0.5));
    // soft light both branches
    assert!(close(bc(SoftLight, 0.5, 0.25), 0.5 - 0.5 * 0.5 * 0.5));
    let d = 0.5f32.sqrt();
    assert!(close(bc(SoftLight, 0.5, 0.75), 0.5 + 0.5 * (d - 0.5)));
    let b = 0.2f32;
    let d2 = ((16.0 * b - 12.0) * b + 4.0) * b;
    assert!(close(bc(SoftLight, b, 1.0), b + (d2 - b)));
}

#[test]
fn non_separable_red_over_blue() {
    let b = [0.0, 0.0, 1.0];
    let s = [1.0, 0.0, 0.0];
    // color: SetLum(s, lum(b)) with lum(b)=0.11, lum(s)=0.3 -> s + (0.11-0.3) = (0.81,-0.19,-0.19) clipped
    let c = blend_color(Color, b, s);
    let l = 0.11f32;
    let raw = [0.81f32, -0.19, -0.19];
    let lr = 0.3 * raw[0] + 0.59 * raw[1] + 0.11 * raw[2];
    let n = -0.19f32;
    let exp: Vec<f32> = raw.iter().map(|&v| lr + (v - lr) * lr / (lr - n)).collect();
    assert!((lr - l).abs() < 1e-6);
    for i in 0..3 {
        assert!(close(c[i], exp[i]), "{c:?} vs {exp:?}");
    }
    // luminosity of a grey over a color keeps lum = grey
    let lum = blend_color(Luminosity, [0.2, 0.4, 0.6], [0.5, 0.5, 0.5]);
    assert!(close(0.3 * lum[0] + 0.59 * lum[1] + 0.11 * lum[2], 0.5));
    // hue/saturation of greys degrade gracefully
    let h = blend_color(Hue, [0.5, 0.5, 0.5], [1.0, 0.0, 0.0]);
    assert!(close(h[0], 0.5) && close(h[1], 0.5));
    let sat = blend_color(Saturation, [0.2, 0.4, 0.6], [0.5, 0.5, 0.5]);
    assert!(close(sat[0], sat[1]) && close(sat[1], sat[2]));
}

#[test]
fn normal_source_over() {
    let o = blend_px(Normal, [0.0; 4], [1.0, 0.0, 0.0, 1.0], 0.5);
    assert_eq!(o, [0.5, 0.0, 0.0, 0.5]);
    let o = blend_px(Normal, [0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, 1.0], 0.5);
    assert_eq!(o, [0.5, 0.0, 0.5, 1.0]);
}

#[test]
fn add_is_linear_and_unclamped() {
    let o = blend_px(Add, [0.8, 0.0, 0.0, 1.0], [0.6, 0.0, 0.0, 1.0], 1.0);
    assert!(close(o[0], 1.4) && close(o[3], 1.0));
}

#[test]
fn multiply_runs_in_srgb_space() {
    let bp = [0.2, 0.3, 0.4, 0.5];
    let sp = [0.1, 0.05, 0.3, 0.6];
    let got = blend_px(Multiply, bp, sp, 1.0);
    // Reference: W3C over premultiplied sRGB, then back to linear.
    let b = prem_lin_to_srgb(bp);
    let s = prem_lin_to_srgb(sp);
    let (ab, a_s) = (b[3], s[3]);
    let mut o = [0.0; 4];
    for i in 0..3 {
        let cb = b[i] / ab;
        let cs = s[i] / a_s;
        o[i] = a_s * (1.0 - ab) * cs + a_s * ab * cb * cs + (1.0 - a_s) * ab * cb;
    }
    o[3] = a_s + ab * (1.0 - a_s);
    let exp = prem_srgb_to_lin(o);
    for i in 0..4 {
        assert!((got[i] - exp[i]).abs() < 1e-5, "{got:?} vs {exp:?}");
    }
}

#[test]
fn zero_alpha_source_leaves_backdrop() {
    let bp = [0.2, 0.3, 0.4, 0.5];
    for m in BlendMode::ALL {
        assert_eq!(blend_px(m, bp, [0.0; 4], 1.0), bp, "{m:?}");
    }
}

#[test]
fn clip_over_keeps_base_alpha() {
    let o = clip_over([0.25, 0.0, 0.0, 0.5], [0.0, 1.0, 0.0, 1.0], 1.0);
    assert!(close(o[3], 0.5) && close(o[1], 0.5) && close(o[0], 0.0));
}
