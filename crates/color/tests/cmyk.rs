#![allow(clippy::unwrap_used)]
use onepaint_color::cmyk::*;

#[derive(serde::Deserialize)]
struct Sep {
    rgb: [f64; 3],
    cmyk: [f64; 4],
    proof: [f64; 3],
}

#[derive(serde::Deserialize)]
struct Golden {
    sep: Vec<Sep>,
    #[serde(rename = "lutCmyk")]
    lut_cmyk: Vec<u8>,
    white: [f64; 3],
    black: [f64; 3],
    probe: Vec<[u8; 4]>,
}

fn golden() -> Golden {
    serde_json::from_str(include_str!("fixtures/cmyk.json")).unwrap()
}

#[test]
fn separation_and_print_match_happypaint() {
    for s in golden().sep {
        let c = separate_rgb(s.rgb[0], s.rgb[1], s.rgb[2], None);
        for i in 0..4 {
            assert!((c[i] - s.cmyk[i]).abs() < 1e-9, "{:?}: {c:?} vs {:?}", s.rgb, s.cmyk);
        }
        let p = cmyk_to_proof_rgb(&c);
        assert!(p.iter().zip(&s.proof).all(|(a, b)| (a - b).abs() < 1e-9));
    }
}

#[test]
fn baked_luts_match() {
    let g = golden();
    let p = builtin_press();
    for i in 0..3 {
        assert!((p.white[i] - g.white[i]).abs() < 1e-6 && (p.black[i] - g.black[i]).abs() < 1e-6);
    }
    for (k, want) in [0usize, 1000, 5000, 20000, 35936].iter().zip(&g.probe) {
        let got = &p.proof_lut[k * 4..k * 4 + 4];
        for ch in 0..4 {
            assert!((got[ch] as i32 - want[ch] as i32).abs() <= 1, "lut {k}: {got:?} vs {want:?}");
        }
    }
    let rgba = [255u8, 0, 0, 255, 0, 128, 255, 255, 30, 200, 90, 255, 250, 250, 250, 255];
    let out = separate_rgba8(&rgba);
    for (a, b) in out.iter().zip(&g.lut_cmyk) {
        assert!((*a as i32 - *b as i32).abs() <= 1, "{out:?} vs {:?}", g.lut_cmyk);
    }
}

#[test]
fn proof_keeps_in_gamut_colours_and_flags_saturated_ones() {
    let (white, de) = proof([1.0, 1.0, 1.0], false, false);
    assert!(white.iter().all(|v| (v - 1.0).abs() < 0.01) && de < 1.0);
    let (_, de_blue) = proof([0.0, 0.0, 1.0], false, false);
    assert!(de_blue > 10.0, "pure RGB blue is out of the press gamut: ΔE {de_blue}");
}

#[test]
fn view_modes_on_premultiplied_linear_pixels() {
    let mut px = [0.0f32, 0.0, 1.0, 1.0, 0.5, 0.5, 0.5, 0.5];
    let before = px;
    apply_view(&mut px, &ViewModes::default());
    assert_eq!(px, before);
    let mut g = before;
    apply_view(&mut g, &ViewModes { grayscale: true, ..Default::default() });
    assert!((g[0] - g[1]).abs() < 1e-6 && (g[1] - g[2]).abs() < 1e-6 && g[3] == 1.0 && g[7] == 0.5);
    let mut w = before;
    apply_view(&mut w, &ViewModes { gamut_warn: 5.0, ..Default::default() });
    assert!(w[0] > 0.5 && w[2] < 0.05, "blue flagged red: {w:?}");
    let mut p = before;
    apply_view(&mut p, &ViewModes { proof: true, ..Default::default() });
    assert_eq!(p[7], 1.0, "the proof is flattened on paper");
    let mut sim = before;
    apply_view(&mut sim, &ViewModes { proof: true, paper_white: true, ..Default::default() });
    assert!(sim[4] < p[4], "simulated paper is darker than display white");
}

#[test]
fn a_cmyk_icc_press_separates_and_proofs() {
    // A real press profile from the system (colord's FOGRA39); skipped without one.
    let Ok(bytes) = std::fs::read("/usr/share/color/icc/colord/FOGRA39L_coated.icc") else {
        eprintln!("skipping: no FOGRA39 profile on this system");
        return;
    };
    let p = onepaint_color::cmyk::press_from_icc(&bytes).unwrap();
    assert!(p.name.to_lowercase().contains("fogra") || !p.name.is_empty(), "{}", p.name);
    assert_eq!(p.icc.as_deref(), Some(&bytes[..]));
    let sep = |rgb: [u8; 3]| onepaint_color::cmyk::separate_rgba8_with(&p, &[rgb[0], rgb[1], rgb[2], 255]);
    // Paper takes no ink; black takes lots; cyan-ish blue is mostly cyan.
    assert!(sep([255, 255, 255]).iter().all(|v| *v <= 3), "white {:?}", sep([255, 255, 255]));
    let k = sep([0, 0, 0]);
    assert!(k.iter().map(|v| *v as u32).sum::<u32>() > 600, "black {k:?}");
    let blue = sep([0, 120, 220]);
    assert!(blue[0] > blue[1] && blue[0] > blue[2], "{blue:?}");
    // In-gamut colours proof close; saturated green moves (out of gamut).
    let (q, de) = onepaint_color::cmyk::proof_with(&p, [0.6, 0.5, 0.45], false, false);
    assert!(de < 6.0, "neutral ΔE {de}, {q:?}");
    let (_, de_green) = onepaint_color::cmyk::proof_with(&p, [0.0, 1.0, 0.0], false, false);
    assert!(de_green > 10.0, "green ΔE {de_green}");
    // Not a CMYK profile: refused with a readable reason.
    let srgb = onepaint_color::icc::build_icc_profile(onepaint_color::icc::RgbSpace::Srgb);
    assert!(onepaint_color::cmyk::press_from_icc(&srgb).is_err());
    assert!(onepaint_color::cmyk::press_from_icc(b"junk").is_err());
}
