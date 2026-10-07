#![allow(clippy::unwrap_used)]
//! Cases ported from happypaint `engine/selection/saved-selection.test.ts`.
use onepaint_selection::saved::*;

fn ellipse(w: u32, h: u32) -> Vec<u8> {
    let mut m = vec![0u8; (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = ((x as f64 - w as f64 / 2.0) / (w as f64 / 3.0), (y as f64 - h as f64 / 2.0) / (h as f64 / 3.0));
            if dx * dx + dy * dy < 1.0 {
                m[(y * w + x) as usize] = 255;
            }
        }
    }
    m
}

#[test]
fn hard_masks_round_trip_compactly() {
    let m = ellipse(40, 30);
    let s = encode_rle(&m).unwrap();
    assert_eq!(decode_rle(&s, 40, 30).unwrap(), m);
    assert!(s.split(',').count() < 30 * 3, "a few runs per row");
    for fill in [0u8, 255] {
        let m = vec![fill; 64];
        assert_eq!(decode_rle(&encode_rle(&m).unwrap(), 8, 8).unwrap(), m);
    }
}

#[test]
fn the_v0194_format_is_unchanged() {
    let m = [0, 255, 255, 0, 0, 255, 255, 0];
    assert_eq!(encode_rle(&m).unwrap(), "1,2,2,2,1");
    assert_eq!(decode_rle("1,2,2,2,1", 4, 2).unwrap(), m);
}

#[test]
fn soft_masks_keep_every_value() {
    let m: Vec<u8> = (0..300).map(|i| (i * 7 % 256) as u8).collect();
    let s = encode_rle(&m).unwrap();
    assert!(s.starts_with("b:"));
    assert_eq!(decode_rle(&s, 30, 10).unwrap(), m);
    let m: Vec<u8> = [vec![128u8; 500], vec![255u8; 500]].concat();
    assert_eq!(encode_rle(&m).unwrap(), "b:dw.3k,dw.73");
}

#[test]
fn junk_and_wrong_sizes_decode_to_none() {
    let s = encode_rle(&ellipse(10, 10)).unwrap();
    assert!(decode_rle(&s, 10, 11).is_none());
    assert!(decode_rle("zz,!!", 4, 4).is_none());
    assert!(decode_rle("", 4, 4).is_none());
    assert!(decode_rle("b:3.ff,x", 4, 1).is_none());
    assert!(decode_rle("b:4.zz", 4, 1).is_none(), "coverage over 255");
}

#[test]
fn too_detailed_is_refused() {
    let m: Vec<u8> = (0..(MAX_RLE_RUNS + 10) * 2).map(|i| if i % 2 == 0 { 0 } else { 255 }).collect();
    assert!(encode_rle(&m).is_none());
}

#[test]
fn shift_and_resample() {
    let mut m = vec![0u8; 16];
    m[4 + 1] = 255;
    let out = shift_mask(&m, 4, 4, 6, 6, 2, 1);
    assert_eq!(out[(1 + 1) * 6 + (1 + 2)], 255);
    assert_eq!(out.iter().map(|&v| v as u32).sum::<u32>(), 255);
    let full = vec![255u8; 16];
    assert_eq!(shift_mask(&full, 4, 4, 2, 4, -1, 0), vec![255u8; 8]);
    let mut q = vec![0u8; 16];
    for y in 0..2 {
        for x in 0..2 {
            q[y * 4 + x] = 255;
        }
    }
    let up = resample_mask(&q, 4, 4, 8, 8);
    assert_eq!((up[0], up[3 * 8 + 3], up[4 * 8 + 4]), (255, 255, 0));
    assert_eq!(resample_mask(&[100u8; 16], 4, 4, 8, 8), vec![100u8; 64]);
    let r: Vec<u8> = (0..15).collect();
    assert_eq!(resample_mask(&r, 5, 3, 5, 3), r);
}
