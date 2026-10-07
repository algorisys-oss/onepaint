//! Median-cut palette extraction (happypaint `engine/color/median-cut.ts`).

fn widest(rgb: &[[u8; 3]], idx: &[u32]) -> (usize, i32) {
    let (mut mn, mut mx) = ([255u8; 3], [0u8; 3]);
    for &i in idx {
        for c in 0..3 {
            let v = rgb[i as usize][c];
            mn[c] = mn[c].min(v);
            mx[c] = mx[c].max(v);
        }
    }
    let mut best = (0, -1);
    for c in 0..3 {
        let r = mx[c] as i32 - mn[c] as i32;
        if r > best.1 {
            best = (c, r);
        }
    }
    best
}

/// Up to `max` hex colours from 8-bit RGBA, largest share first; pixels with
/// alpha < 128 are ignored.
pub fn extract_palette(rgba: &[u8], max: usize) -> Vec<String> {
    if max < 1 {
        return Vec::new();
    }
    let rgb: Vec<[u8; 3]> = rgba.chunks_exact(4).filter(|p| p[3] >= 128).map(|p| [p[0], p[1], p[2]]).collect();
    if rgb.is_empty() {
        return Vec::new();
    }
    let mut boxes: Vec<Vec<u32>> = vec![(0..rgb.len() as u32).collect()];
    while boxes.len() < max {
        let mut best: Option<(usize, i64)> = None;
        for (b, bx) in boxes.iter().enumerate() {
            if bx.len() < 2 {
                continue;
            }
            let (_, range) = widest(&rgb, bx);
            let score = bx.len() as i64 * range as i64;
            if range > 0 && best.is_none_or(|(_, s)| score > s) {
                best = Some((b, score));
            }
        }
        let Some((bi, _)) = best else { break };
        let (ch, _) = widest(&rgb, &boxes[bi]);
        let mut sorted = boxes[bi].clone();
        sorted.sort_by_key(|&i| rgb[i as usize][ch]);
        let val = |i: usize| rgb[sorted[i] as usize][ch];
        let med = sorted.len() / 2;
        let down = (1..=med).rev().find(|&i| val(i - 1) != val(i));
        let up = (med + 1..sorted.len()).find(|&i| val(i - 1) != val(i));
        let mid = match (down, up) {
            (None, Some(u)) => u,
            (Some(d), None) => d,
            (Some(d), Some(u)) => {
                if med - d <= u - med {
                    d
                } else {
                    u
                }
            }
            (None, None) => break,
        };
        if mid == 0 || mid >= sorted.len() {
            break;
        }
        let hi = sorted.split_off(mid);
        boxes.splice(bi..=bi, [sorted, hi]);
    }
    boxes.sort_by_key(|b| std::cmp::Reverse(b.len()));
    let mut out: Vec<String> = Vec::new();
    for b in boxes {
        let n = b.len() as f64;
        let mut s = [0f64; 3];
        for &i in &b {
            for c in 0..3 {
                s[c] += rgb[i as usize][c] as f64;
            }
        }
        let hex = format!("#{:02x}{:02x}{:02x}", (s[0] / n).round() as u8, (s[1] / n).round() as u8, (s[2] / n).round() as u8);
        if !out.contains(&hex) {
            out.push(hex);
        }
    }
    out
}
