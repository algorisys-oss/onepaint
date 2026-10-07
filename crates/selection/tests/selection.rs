use onepaint_selection::*;

fn m(w: u32, data: &[u8]) -> Mask {
    Mask::from_vec(w, (data.len() as u32) / w, data.to_vec())
}

#[test]
fn combine_goldens() {
    let a = m(5, &[200, 100, 255, 0, 128]);
    let b = m(5, &[100, 200, 128, 128, 255]);
    assert_eq!(
        combine(Some(&a), &b, Mode::Add).data(),
        &[200, 200, 255, 128, 255]
    );
    assert_eq!(
        combine(Some(&a), &b, Mode::Intersect).data(),
        &[100, 100, 128, 0, 128]
    );
    assert_eq!(
        combine(Some(&a), &b, Mode::Subtract).data(),
        &[122, 22, 127, 0, 0]
    );
    assert_eq!(combine(Some(&a), &b, Mode::Replace).data(), b.data());
    assert_eq!(combine(None, &b, Mode::Add).data(), b.data());
    assert_eq!(combine(None, &b, Mode::Subtract).data(), &[0; 5]);
    assert_eq!(combine(None, &b, Mode::Intersect).data(), &[0; 5]);
}

#[test]
fn mode_from_modifiers() {
    assert_eq!(
        Mode::from_modifiers(true, true, Mode::Replace),
        Mode::Intersect
    );
    assert_eq!(
        Mode::from_modifiers(false, true, Mode::Replace),
        Mode::Subtract
    );
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
        assert_eq!(
            mask.dilated(r).data(),
            &naive(&mask, r as i32, true)[..],
            "dilate r{r}"
        );
        assert_eq!(
            mask.eroded(r).data(),
            &naive(&mask, r as i32, false)[..],
            "erode r{r}"
        );
    }
    let full = m(6, &[255; 36]);
    assert_eq!(full.eroded(4), full);
    assert_eq!(
        m(9, &[0, 0, 0, 64, 128, 255, 255, 255, 255])
            .dilated(1)
            .data(),
        &[0, 0, 64, 128, 255, 255, 255, 255, 255]
    );
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
    let nested = rasterize_polygons(
        8,
        8,
        &[
            rect_polygon([1.0, 1.0], [7.0, 7.0]),
            rect_polygon([3.0, 3.0], [5.0, 5.0]),
        ],
    );
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
    assert_eq!(
        g.data(),
        &[0, 0, 255, 0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 0, 0]
    );
}
