//! Saved-selection storage (happypaint `engine/selection/saved-selection.ts`, spec 20).

/// Past this many runs a selection is too detailed to store.
pub const MAX_RLE_RUNS: usize = 400_000;
const SOFT_TAG: &str = "b:";

fn b36(mut v: usize) -> String {
    if v == 0 {
        return "0".into();
    }
    let mut s = Vec::new();
    while v > 0 {
        s.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[v % 36]);
        v /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn parse_b36(s: &str) -> Option<usize> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase()) {
        return None;
    }
    usize::from_str_radix(s, 36).ok()
}

/// Run-length encode a coverage mask, or `None` when it is too detailed.
pub fn encode_rle(mask: &[u8]) -> Option<String> {
    if mask.iter().any(|&v| v != 0 && v != 255) {
        return encode_soft(mask);
    }
    let mut runs = Vec::new();
    let (mut cur, mut len) = (false, 0usize);
    for &v in mask {
        let on = v != 0;
        if on == cur {
            len += 1;
            continue;
        }
        runs.push(len);
        if runs.len() > MAX_RLE_RUNS {
            return None;
        }
        cur = on;
        len = 1;
    }
    runs.push(len);
    Some(runs.into_iter().map(b36).collect::<Vec<_>>().join(","))
}

fn encode_soft(mask: &[u8]) -> Option<String> {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < mask.len() {
        let v = mask[i];
        let mut j = i + 1;
        while j < mask.len() && mask[j] == v {
            j += 1;
        }
        parts.push(format!("{}.{}", b36(j - i), b36(v as usize)));
        if parts.len() > MAX_RLE_RUNS {
            return None;
        }
        i = j;
    }
    Some(format!("{SOFT_TAG}{}", parts.join(",")))
}

/// Decode for a w×h canvas; `None` unless it describes exactly w·h pixels.
pub fn decode_rle(rle: &str, w: u32, h: u32) -> Option<Vec<u8>> {
    if rle.is_empty() {
        return None;
    }
    let n = w as usize * h as usize;
    let mut out = vec![0u8; n];
    let mut i = 0;
    if let Some(body) = rle.strip_prefix(SOFT_TAG) {
        for part in body.split(',') {
            let (l, v) = part.split_once('.')?;
            let (len, v) = (parse_b36(l)?, parse_b36(v)?);
            if v > 255 || i + len > n {
                return None;
            }
            out[i..i + len].fill(v as u8);
            i += len;
        }
    } else {
        let mut on = false;
        for part in rle.split(',') {
            let len = parse_b36(part)?;
            if i + len > n {
                return None;
            }
            if on {
                out[i..i + len].fill(255);
            }
            i += len;
            on = !on;
        }
    }
    (i == n).then_some(out)
}

/// Canvas Size: the old origin lands at (off_x, off_y) in the new canvas.
pub fn shift_mask(mask: &[u8], w: u32, h: u32, nw: u32, nh: u32, off_x: i32, off_y: i32) -> Vec<u8> {
    let (w, h, nw, nh) = (w as i64, h as i64, nw as i64, nh as i64);
    let (ox, oy) = (off_x as i64, off_y as i64);
    let mut out = vec![0u8; (nw * nh) as usize];
    let (y0, y1) = (oy.max(0), nh.min(h + oy));
    let (x0, x1) = (ox.max(0), nw.min(w + ox));
    if x1 <= x0 {
        return out;
    }
    for y in y0..y1 {
        let src = ((y - oy) * w + (x0 - ox)) as usize;
        let dst = (y * nw + x0) as usize;
        let len = (x1 - x0) as usize;
        out[dst..dst + len].copy_from_slice(&mask[src..src + len]);
    }
    out
}

/// Image Size: nearest-neighbour resample at pixel centres (keeps hard masks hard).
pub fn resample_mask(mask: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    if nw == w && nh == h {
        return mask.to_vec();
    }
    let mut out = vec![0u8; nw as usize * nh as usize];
    for y in 0..nh as usize {
        let sy = (((y as f64 + 0.5) * h as f64 / nh as f64).floor() as usize).min(h as usize - 1);
        for x in 0..nw as usize {
            let sx = (((x as f64 + 0.5) * w as f64 / nw as f64).floor() as usize).min(w as usize - 1);
            out[y * nw as usize + x] = mask[sy * w as usize + sx];
        }
    }
    out
}
