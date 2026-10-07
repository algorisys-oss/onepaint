#![allow(clippy::unwrap_used)]
use onepaint_color::median_cut::extract_palette;

fn img(parts: &[([u8; 4], usize)]) -> Vec<u8> {
    parts.iter().flat_map(|(c, n)| std::iter::repeat_n(*c, *n).flatten()).collect()
}

#[test]
fn dominant_colours_largest_first() {
    let px = img(&[([255, 0, 0, 255], 400), ([0, 0, 255, 255], 100)]);
    assert_eq!(extract_palette(&px, 4), ["#ff0000", "#0000ff"]);
}

#[test]
fn transparent_and_limits() {
    assert!(extract_palette(&img(&[([10, 20, 30, 0], 50)]), 5).is_empty());
    assert!(extract_palette(&[], 5).is_empty());
    let px = img(&[([255, 0, 0, 255], 10), ([0, 255, 0, 255], 20), ([0, 0, 255, 255], 30), ([9, 9, 9, 100], 99)]);
    let p = extract_palette(&px, 2);
    assert_eq!(p.len(), 2);
    let all = extract_palette(&px, 8);
    assert_eq!(all, ["#0000ff", "#00ff00", "#ff0000"], "by share; translucent pixels ignored");
    assert!(extract_palette(&px, 0).is_empty());
}

#[test]
fn a_gradient_splits_into_averaged_bands() {
    let px: Vec<u8> = (0..=255u8).flat_map(|v| [v, v, v, 255]).collect();
    let p = extract_palette(&px, 4);
    assert_eq!(p.len(), 4);
    assert!(p.iter().all(|h| h.len() == 7));
}
