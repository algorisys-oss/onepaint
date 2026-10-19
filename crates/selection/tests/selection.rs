#![allow(clippy::unwrap_used)]
use onepaint_selection::*;

fn m(w: u32, data: &[u8]) -> Mask {
    Mask::from_vec(w, (data.len() as u32) / w, data.to_vec())
}

#[test]
fn combine_goldens() {
    let a = m(5, &[200, 100, 255, 0, 128]);
    let b = m(5, &[100, 200, 128, 128, 255]);
    assert_eq!(combine(Some(&a), &b, Mode::Add).data(), &[200, 200, 255, 128, 255]);
    assert_eq!(combine(Some(&a), &b, Mode::Intersect).data(), &[100, 100, 128, 0, 128]);
    assert_eq!(combine(Some(&a), &b, Mode::Subtract).data(), &[122, 22, 127, 0, 0]);
    assert_eq!(combine(Some(&a), &b, Mode::Replace).data(), b.data());
    assert_eq!(combine(None, &b, Mode::Add).data(), b.data());
    assert_eq!(combine(None, &b, Mode::Subtract).data(), &[0; 5]);
    assert_eq!(combine(None, &b, Mode::Intersect).data(), &[0; 5]);
}

#[test]
fn mode_from_modifiers() {
    assert_eq!(Mode::from_modifiers(true, true, Mode::Replace), Mode::Intersect);
    assert_eq!(Mode::from_modifiers(false, true, Mode::Replace), Mode::Subtract);
    assert_eq!(Mode::from_modifiers(true, false, Mode::Replace), Mode::Add);
    assert_eq!(Mode::from_modifiers(false, false, Mode::Add), Mode::Add);
}

#[test]
fn invert_is_self_inverse() {
    let a = m(5, &[200, 100, 255, 0, 128]);
    assert_eq!(a.inverted().inverted(), a);
    assert_eq!(a.inverted().data(), &[55, 155, 0, 255, 127]);
}

fn naive(mask: &Mask, r: i32, max: bool) -> Vec<u8> {
    let (w, h) = (mask.width() as i32, mask.height() as i32);
    let mut out = vec![0; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut v = if max { 0 } else { 255 };
            for yy in (y - r).max(0)..=(y + r).min(h - 1) {
                for xx in (x - r).max(0)..=(x + r).min(w - 1) {
                    let s = mask.get(xx as u32, yy as u32);
                    v = if max { v.max(s) } else { v.min(s) };
                }
            }
            out[(y * w + x) as usize] = v;
        }
    }
    out
}

#[test]
fn dilate_erode_match_naive() {
    let mut data = vec![0u8; 13 * 9];
    for (i, v) in data.iter_mut().enumerate() {
        *v = ((i * 37 + i * i * 11) % 256) as u8 & if i % 5 == 0 { 0 } else { 255 };
    }
    let mask = m(13, &data);
    for r in [1, 2, 4] {
        assert_eq!(mask.dilated(r).data(), &naive(&mask, r as i32, true)[..], "dilate r{r}");
        assert_eq!(mask.eroded(r).data(), &naive(&mask, r as i32, false)[..], "erode r{r}");
    }
    let full = m(6, &[255; 36]);
    assert_eq!(full.eroded(4), full);
    assert_eq!(m(9, &[0, 0, 0, 64, 128, 255, 255, 255, 255]).dilated(1).data(), &[0, 0, 64, 128, 255, 255, 255, 255, 255]);
}

#[test]
fn feather_and_smooth_goldens() {
    let mut d = vec![0u8; 15];
    d[5..10].fill(255);
    let f = m(15, &d).feathered(2.0);
    assert_eq!((f.data()[7], f.data()[5], f.data()[2]), (255, 153, 0));
    assert_eq!(m(9, &[102; 9]).feathered(2.0).data(), &[102; 9]);
    let s = m(15, &d).smoothed(3.0);
    assert!(s.data().iter().all(|&v| v == 0 || v == 255));
}

#[test]
fn polygon_rasterisation() {
    let r = rasterize_polygons(8, 8, &[rect_polygon([2.0, 2.0], [6.0, 6.0])]);
    for (x, y) in [(3, 3), (5, 5)] {
        assert_eq!(r.get(x, y), 255, "({x},{y})");
    }
    for (x, y) in [(0, 0), (7, 7), (1, 3), (6, 3)] {
        assert_eq!(r.get(x, y), 0, "({x},{y})");
    }
    let nested = rasterize_polygons(8, 8, &[rect_polygon([1.0, 1.0], [7.0, 7.0]), rect_polygon([3.0, 3.0], [5.0, 5.0])]);
    assert_eq!((nested.get(4, 4), nested.get(2, 2)), (0, 255));
    let e = rasterize_polygons(40, 40, &[ellipse_polygon([0.0, 0.0], [40.0, 40.0])]);
    assert_eq!((e.get(20, 20), e.get(1, 1), e.get(20, 1)), (255, 0, 255));
    assert!(!rasterize_polygons(4, 4, &[vec![[0.0, 0.0], [3.0, 0.0]]]).has_content());
}

#[test]
fn constrain_square_and_45() {
    assert_eq!(constrain_square([10.0, 10.0], [14.0, 30.0]), [30.0, 30.0]);
    assert_eq!(constrain_square([10.0, 10.0], [4.0, 6.0]), [4.0, 4.0]);
    let p = constrain_45([0.0, 0.0], [10.0, 1.0]);
    assert!((p[1]).abs() < 1e-9 && (p[0] - 10.0498).abs() < 1e-3);
}

fn img(w: u32, px: &[[f32; 4]]) -> (u32, u32, Vec<[f32; 4]>) {
    (w, px.len() as u32 / w, px.to_vec())
}

#[test]
fn colour_match_contiguous_global_and_tolerance() {
    const R: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const B: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    const R2: [f32; 4] = [0.9, 0.0, 0.0, 1.0];
    // R R B R
    // R B B R2
    let (w, h, px) = img(4, &[R, R, B, R, R, B, B, R2]);
    let get = |x: u32, y: u32| px[(y * w + x) as usize];
    let c = color_match(w, h, &get, 0, 0, 0.05, true, None).unwrap();
    assert_eq!(c.data(), &[255, 255, 0, 0, 255, 0, 0, 0]);
    let g = color_match(w, h, &get, 0, 0, 0.05, false, None).unwrap();
    assert_eq!(g.data(), &[255, 255, 0, 255, 255, 0, 0, 0]);
    let t = color_match(w, h, &get, 0, 0, 0.15, false, None).unwrap();
    assert_eq!(t.data(), &[255, 255, 0, 255, 255, 0, 0, 255]);
    assert!(color_match(w, h, &get, 9, 0, 0.1, true, None).is_none());
    // Confinement: pixels outside `within` are skipped.
    let within = m(4, &[255, 0, 255, 255, 255, 255, 255, 255]);
    let cw = color_match(w, h, &get, 0, 0, 0.05, true, Some(&within)).unwrap();
    assert_eq!(cw.data(), &[255, 0, 0, 0, 255, 0, 0, 0]);
}

#[test]
fn grow_within_dilates_four_connected() {
    let base = m(5, &[0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0]);
    let g = base.grown4(1, None);
    assert_eq!(g.data(), &[0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 0, 0]);
}

/// The span flood finds the same 4-connected region as a pixel-by-pixel
/// search, on random blobs with holes, spirals and U-turns.
#[test]
fn contiguous_wand_matches_a_plain_flood() {
    let (w, h) = (150u32, 131u32);
    for seed in 0..40u64 {
        let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut rnd = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as u32
        };
        let density = 40 + rnd() % 30;
        let cells: Vec<bool> = (0..w * h).map(|_| rnd() % 100 < density).collect();
        let get = |x: u32, y: u32| if cells[(y * w + x) as usize] { [1.0; 4] } else { [0.0; 4] };
        let (sx, sy) = (rnd() % w, rnd() % h);
        let got = color_match(w, h, &get, sx as i32, sy as i32, 0.1, true, None).unwrap();
        // A plain 4-way flood.
        let want_on = cells[(sy * w + sx) as usize];
        let mut want = vec![false; (w * h) as usize];
        let mut stack = vec![(sx, sy)];
        while let Some((x, y)) = stack.pop() {
            let i = (y * w + x) as usize;
            if want[i] || cells[i] != want_on {
                continue;
            }
            want[i] = true;
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < w {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < h {
                stack.push((x, y + 1));
            }
        }
        for y in 0..h {
            for x in 0..w {
                assert_eq!(got.get(x, y) != 0, want[(y * w + x) as usize], "seed {seed} at {x},{y}");
            }
        }
    }
}

/// The old strided passes, as references for the transposed ones.
fn ref_sliding(m: &Mask, r: usize, max: bool) -> Vec<u8> {
    let (w, h) = (m.width() as usize, m.height() as usize);
    let pass = |src: &[u8], len: usize, stride: usize, count: usize, outer: usize, dst: &mut [u8]| {
        for o in 0..count {
            let base = o * outer;
            for i in 0..len {
                let (lo, hi) = (i.saturating_sub(r), (i + r).min(len - 1));
                let vals = (lo..=hi).map(|k| src[base + k * stride]);
                dst[base + i * stride] = if max { vals.max().unwrap() } else { vals.min().unwrap() };
            }
        }
    };
    let mut tmp = vec![0; w * h];
    pass(m.data(), w, 1, h, w, &mut tmp);
    let mut out = vec![0; w * h];
    pass(&tmp, h, w, w, 1, &mut out);
    out
}

fn ref_feather(m: &Mask, r: usize) -> Vec<u8> {
    let (w, h) = (m.width() as usize, m.height() as usize);
    let win = (2 * r + 1) as f32;
    let mut a: Vec<f32> = m.data().iter().map(|&v| v as f32 / 255.0).collect();
    let mut b = vec![0.0f32; w * h];
    let blur = |src: &[f32], dst: &mut [f32], len: usize, stride: usize, count: usize, outer: usize| {
        for o in 0..count {
            let base = o * outer;
            let at = |i: i64| src[base + (i.clamp(0, len as i64 - 1) as usize) * stride];
            let mut sum: f32 = (-(r as i64)..=r as i64).map(at).sum();
            for i in 0..len {
                dst[base + i * stride] = sum / win;
                sum += at(i as i64 + r as i64 + 1) - at(i as i64 - r as i64);
            }
        }
    };
    blur(&a, &mut b, w, 1, h, w);
    blur(&b, &mut a, h, w, w, 1);
    a.iter().map(|v| (v.min(1.0) * 255.0).round() as u8).collect()
}

#[test]
fn grow_shrink_and_feather_match_the_strided_passes() {
    for (w, h, seed) in [(131u32, 77u32, 1u64), (64, 200, 2), (300, 3, 3)] {
        let mut s = seed;
        let data: Vec<u8> = (0..w * h)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                if (s >> 40) % 5 < 2 { ((s >> 20) % 256) as u8 } else { 0 }
            })
            .collect();
        let m = Mask::from_vec(w, h, data);
        for r in [1u32, 4, 9] {
            assert_eq!(m.dilated(r).data(), ref_sliding(&m, r as usize, true).as_slice(), "dilate {w}x{h} r{r}");
            assert_eq!(m.eroded(r).data(), ref_sliding(&m, r as usize, false).as_slice(), "erode {w}x{h} r{r}");
            assert_eq!(m.feathered(r as f32).data(), ref_feather(&m, r as usize).as_slice(), "feather {w}x{h} r{r}");
        }
    }
}
