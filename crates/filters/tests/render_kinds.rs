#![allow(clippy::unwrap_used)]
//! Spec 18 render kinds: the clouds family, starfield, lens flare, light rays
//! and lightning (bun golden for the bolt path).

use onepaint_filters::lightning::{lightning_strokes, render_lightning};
use onepaint_filters::*;

const W: usize = 96;
const H: usize = 64;

fn generate(kind: Kind, p: &Params) -> Vec<f32> {
    apply_spatial(kind, p, &vec![0.0; W * H * 4], W, H)
}

#[test]
fn clouds_are_deterministic_seeded_and_premultiplied() {
    for k in [Kind::Clouds, Kind::Fog, Kind::Smoke, Kind::Nebula] {
        let mut p = default_params(k);
        p.gen_scale = Some(30.0);
        let a = generate(k, &p);
        assert_eq!(a, generate(k, &p), "{k:?} deterministic");
        assert!(a.chunks(4).all(|q| (0.0..=1.0).contains(&q[3]) && q[..3].iter().all(|c| *c >= 0.0 && *c <= q[3] + 1e-6)), "{k:?}");
        let cov: f32 = a.chunks(4).map(|q| q[3]).sum::<f32>() / (W * H) as f32;
        assert!(cov > 0.01 && cov < 0.99, "{k:?} coverage {cov}");
        p.gen_seed = Some(2.0);
        assert_ne!(a, generate(k, &p), "{k:?} re-roll");
    }
}

#[test]
fn clouds_density_drives_coverage() {
    let mut p = default_params(Kind::Clouds);
    p.gen_scale = Some(30.0);
    let cov = |p: &Params| generate(Kind::Clouds, p).chunks(4).map(|q| q[3]).sum::<f32>();
    p.gen_density = Some(0.2);
    let thin = cov(&p);
    p.gen_density = Some(0.8);
    let thick = cov(&p);
    assert!(thick > thin * 1.5, "{thin} {thick}");
}

#[test]
fn starfield_density_zero_is_empty() {
    let mut p = default_params(Kind::Starfield);
    p.gen_density = Some(0.0);
    assert!(generate(Kind::Starfield, &p).iter().all(|v| *v == 0.0));
    p.gen_density = Some(1.0);
    let stars = generate(Kind::Starfield, &p);
    assert!(stars.chunks(4).any(|q| q[3] > 0.3));
    // Star colour is genColorB, premultiplied.
    assert!(stars.chunks(4).all(|q| (q[0] - q[3]).abs() < 1e-6));
}

#[test]
fn lens_flare_is_bright_at_the_light() {
    let p = default_params(Kind::LensFlare);
    let out = generate(Kind::LensFlare, &p);
    let at = |x: usize, y: usize| {
        let i = (y * W + x) * 4;
        [out[i], out[i + 1], out[i + 2], out[i + 3]]
    };
    // The light is at (0.3, 0.3) of the canvas.
    let core = at((0.3 * W as f32) as usize, (0.3 * H as f32) as usize);
    assert!(core[3] > 0.7, "{core:?}");
    assert_eq!(core[3], core[0].max(core[1]).max(core[2]));
    let far = at(W - 2, H - 2);
    assert!(far[3] < core[3]);
    assert!(out.chunks(4).all(|q| q[3] == q[0].max(q[1]).max(q[2])));
}

#[test]
fn light_rays_brighten_from_the_light() {
    // A bright disc near the source on a dark opaque field.
    let src: Vec<f32> = (0..W * H)
        .flat_map(|i| {
            let (x, y) = ((i % W) as f32, (i / W) as f32);
            if (x - 48.0).hypot(y - 20.0) < 6.0 { [1.0, 1.0, 1.0, 1.0] } else { [0.02, 0.02, 0.02, 1.0] }
        })
        .collect();
    let p = default_params(Kind::LightRays);
    let out = apply_spatial(Kind::LightRays, &p, &src, W, H);
    let i = (30 * W + 48) * 4; // below the disc: its ray toward the source reaches the disc
    assert!(out[i] > src[i] * 1.5, "{} vs {}", out[i], src[i]);
    assert!(out.chunks(4).all(|q| (q[3] - 1.0).abs() < 1e-6));
}

#[test]
fn lightning_path_matches_happypaint() {
    let golden: serde_json::Value = serde_json::from_str(include_str!("golden/lightning.json")).unwrap();
    for case in golden.as_array().unwrap() {
        let p: Params = serde_json::from_value(case["params"].clone()).unwrap();
        let strokes = lightning_strokes(320, 240, &p);
        let calls = case["calls"].as_array().unwrap();
        assert_eq!(strokes.len(), calls.len());
        for (s, c) in strokes.iter().zip(calls) {
            let pts: Vec<[f64; 2]> = serde_json::from_value(c["pts"].clone()).unwrap();
            assert_eq!(s.pts.len(), pts.len());
            for (a, b) in s.pts.iter().zip(&pts) {
                assert!((a[0] - b[0]).abs() < 1e-6 && (a[1] - b[1]).abs() < 1e-6, "{a:?} vs {b:?}");
            }
            assert!((s.width - c["width"].as_f64().unwrap()).abs() < 1e-9);
            assert!((s.blur - c["blur"].as_f64().unwrap()).abs() < 1e-9);
        }
    }
}

#[test]
fn lightning_raster_spans_the_canvas() {
    let p = default_params(Kind::Lightning);
    let out = render_lightning(W, H, &p);
    assert_eq!(out, generate(Kind::Lightning, &p));
    let row_max = |y: usize| (0..W).map(|x| out[(y * W + x) * 4 + 3]).fold(0.0, f32::max);
    assert!(row_max(0) > 0.5 && row_max(H - 1) > 0.5);
    assert!(out[3] < 0.05, "top-left corner stays clear");
    assert!(out.chunks(4).all(|q| q[3] <= 1.0 + 1e-6 && q[..3].iter().all(|c| *c <= q[3] + 1e-5)));
}
