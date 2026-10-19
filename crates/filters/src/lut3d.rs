//! 3D colour lookup tables for Color Lookup (happypaint v0.201.6,
//! `engine/adjustments/lut3d.ts`): `.cube` and `.3dl` parsers and the
//! trilinear sample the shader takes.

/// The largest table side read (happypaint `LUT_MAX_SIZE`).
pub const LUT_MAX_SIZE: usize = 129;

/// A parsed table: `size³` RGB entries, red fastest (`r + N·(g + N·b)`),
/// over the input domain `domain_min..domain_max`.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut3D {
    pub size: usize,
    pub data: Vec<[f32; 3]>,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
}

/// A table file's format, from its name's extension (case-insensitive).
pub fn lut_format_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "cube" => Some("cube"),
        "3dl" => Some("3dl"),
        _ => None,
    }
}

/// Parse a table of `format` ("cube" or "3dl"); `None` if it can't be read.
pub fn parse_lut(format: &str, text: &str) -> Option<Lut3D> {
    match format {
        "cube" => parse_cube(text),
        "3dl" => parse_3dl(text),
        _ => None,
    }
}

fn numbers(line: &str) -> Option<Vec<f32>> {
    line.split_whitespace().map(|t| t.parse::<f32>().ok().filter(|v| v.is_finite())).collect()
}

/// `.cube`: `LUT_3D_SIZE`, optional `DOMAIN_MIN`/`DOMAIN_MAX`, then N³ rows
/// red fastest. 1D tables are refused; other keywords are skipped.
pub fn parse_cube(text: &str) -> Option<Lut3D> {
    let mut size = 0;
    let mut data = Vec::new();
    let (mut domain_min, mut domain_max) = ([0.0; 3], [1.0; 3]);
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let first = words.next()?.to_ascii_uppercase();
        let triple = |rest: &str| numbers(rest).filter(|v| v.len() == 3).map(|v| [v[0], v[1], v[2]]);
        match first.as_str() {
            "LUT_1D_SIZE" => return None,
            "LUT_3D_SIZE" => {
                let n: usize = words.next()?.parse().ok()?;
                if !(2..=LUT_MAX_SIZE).contains(&n) {
                    return None;
                }
                size = n;
                data.reserve(n * n * n);
            }
            "DOMAIN_MIN" => domain_min = triple(&line[first.len()..])?,
            "DOMAIN_MAX" => domain_max = triple(&line[first.len()..])?,
            k if k.starts_with(|c: char| c.is_ascii_uppercase() || c == '_') => {}
            _ => {
                let v = triple(line)?;
                if size == 0 || data.len() >= size * size * size {
                    return None;
                }
                data.push(v);
            }
        }
    }
    if size == 0 || data.len() != size * size * size || (0..3).any(|i| domain_max[i] <= domain_min[i]) {
        return None;
    }
    Some(Lut3D { size, data, domain_min, domain_max })
}

/// `.3dl`: a mesh line (its length is N), then N³ integer rows, blue
/// fastest, scaled by `2^bits − 1` (from `Mesh <in> <out>`, else the
/// smallest of 10, 12, 14, 16 bits that holds the largest value).
pub fn parse_3dl(text: &str) -> Option<Lut3D> {
    let mut size = 0;
    let mut out_bits = None;
    let mut rows = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with(|c: char| c.is_ascii_alphabetic()) {
            let mut w = line.split_whitespace();
            if w.next().is_some_and(|k| k.eq_ignore_ascii_case("mesh")) {
                out_bits = w.nth(1).and_then(|b| b.parse::<u32>().ok());
            }
            continue;
        }
        let v = numbers(line)?;
        if size == 0 {
            if v.len() < 2 {
                return None;
            }
            size = v.len();
            continue;
        }
        if v.len() != 3 {
            return None;
        }
        rows.push([v[0], v[1], v[2]]);
    }
    if size == 0 || size > LUT_MAX_SIZE || rows.len() != size * size * size {
        return None;
    }
    let top = rows.iter().flatten().fold(0.0f32, |a, v| a.max(*v));
    let bits = out_bits.unwrap_or_else(|| [10, 12, 14, 16].into_iter().find(|b| top <= ((1u32 << b) - 1) as f32).unwrap_or(16));
    let scale = 1.0 / ((1u64 << bits.min(31)) - 1) as f32;
    // Row k is (r, g, b) = (k / N², k / N mod N, k mod N): re-laid red fastest.
    let n = size;
    let mut data = vec![[0.0; 3]; n * n * n];
    for (k, v) in rows.into_iter().enumerate() {
        let (r, g, b) = (k / (n * n), (k / n) % n, k % n);
        data[r + n * (g + n * b)] = v.map(|x| x * scale);
    }
    Some(Lut3D { size, data, domain_min: [0.0; 3], domain_max: [1.0; 3] })
}

impl Lut3D {
    /// The colour at `c` (0..1 work values): rescaled by the domain, clamped,
    /// then trilinear between the table's entries (`sampleLut`, the shader's
    /// texture lookup).
    pub fn sample(&self, c: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let m = (n - 1) as f32;
        let t = [0, 1, 2].map(|i| (((c[i] - self.domain_min[i]) / (self.domain_max[i] - self.domain_min[i])).clamp(0.0, 1.0)) * m);
        let i0 = t.map(|v| (v.floor() as usize).min(n - 1));
        let i1 = i0.map(|v| (v + 1).min(n - 1));
        let f = [0, 1, 2].map(|i| t[i] - i0[i] as f32);
        let at = |r: usize, g: usize, b: usize| self.data[r + n * (g + n * b)];
        let lerp = |a: [f32; 3], b: [f32; 3], w: f32| [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * w);
        let c00 = lerp(at(i0[0], i0[1], i0[2]), at(i1[0], i0[1], i0[2]), f[0]);
        let c10 = lerp(at(i0[0], i1[1], i0[2]), at(i1[0], i1[1], i0[2]), f[0]);
        let c01 = lerp(at(i0[0], i0[1], i1[2]), at(i1[0], i0[1], i1[2]), f[0]);
        let c11 = lerp(at(i0[0], i1[1], i1[2]), at(i1[0], i1[1], i1[2]), f[0]);
        lerp(lerp(c00, c10, f[1]), lerp(c01, c11, f[1]), f[2])
    }
}

/// A parsed table for these params' `lut`, cached by its text (happypaint
/// `parsedLut`): `None` without one, or when it can't be read.
pub fn lut_of(lut: Option<&crate::LutFile>) -> Option<std::sync::Arc<Lut3D>> {
    use std::sync::{Arc, Mutex, OnceLock};
    type Entries = Vec<(u64, Option<Arc<Lut3D>>)>;
    static CACHE: OnceLock<Mutex<Entries>> = OnceLock::new();
    let lut = lut?;
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&lut.format, &lut.text).hash(&mut h);
        h.finish()
    };
    let cache = CACHE.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(c) = cache.lock()
        && let Some((_, t)) = c.iter().find(|(k, _)| *k == key)
    {
        return t.clone();
    }
    let parsed = parse_lut(&lut.format, &lut.text).map(Arc::new);
    if let Ok(mut c) = cache.lock() {
        c.push((key, parsed.clone()));
        if c.len() > 8 {
            c.remove(0);
        }
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(v: [f32; 3], d: i32) -> [f32; 3] {
        let k = 10f32.powi(d);
        v.map(|x| (x * k).round() / k)
    }

    /// A `.cube` of side `n` mapping (r, g, b) through `f`.
    fn cube(n: usize, f: impl Fn([f32; 3]) -> [f32; 3], header: &str) -> String {
        let mut s = format!("TITLE \"t\"\n{header}LUT_3D_SIZE {n}\n");
        let m = (n - 1) as f32;
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let o = f([r as f32 / m, g as f32 / m, b as f32 / m]);
                    s += &format!("{} {} {}\n", o[0], o[1], o[2]);
                }
            }
        }
        s
    }

    // happypaint lut3d.test.ts.
    #[test]
    fn cube_identity_swap_and_domain() {
        let id = parse_cube(&cube(2, |c| c, "")).unwrap();
        assert_eq!(round(id.sample([0.2, 0.5, 0.9]), 5), [0.2, 0.5, 0.9]);
        let sw = parse_cube(&cube(17, |c| [c[2], c[1], c[0]], "# a comment\n")).unwrap();
        assert_eq!(round(sw.sample([0.8, 0.4, 0.2]), 5), [0.2, 0.4, 0.8]);
        let l = parse_cube(&cube(2, |c| c, "DOMAIN_MIN 0 0 0\nDOMAIN_MAX 2 2 2\n")).unwrap();
        assert_eq!(round(l.sample([1.0, 1.0, 1.0]), 5), [0.5, 0.5, 0.5]);
    }

    #[test]
    fn bad_cubes_are_refused() {
        assert!(parse_cube("LUT_1D_SIZE 2\n0 0 0\n1 1 1\n").is_none());
        assert!(parse_cube("LUT_3D_SIZE 2\n0 0 zero\n").is_none());
        assert!(parse_cube("").is_none());
        assert!(parse_cube("LUT_3D_SIZE 2\n0 0 0\n").is_none(), "too few rows");
        assert!(parse_lut("cube", "\u{0}\u{1}garbage").is_none());
    }

    /// A 5³ `.3dl` swapping red and blue, 12-bit, blue fastest.
    fn swap_3dl(header: &str, top: f32) -> String {
        let n = 5;
        let mut s = format!("{header}0 256 512 768 1023\n");
        for r in 0..n {
            for g in 0..n {
                for b in 0..n {
                    let v = |i: usize| (i as f32 / 4.0 * top).round();
                    s += &format!("{} {} {}\n", v(b), v(g), v(r));
                }
            }
        }
        s
    }

    #[test]
    fn three_dl_reorders_and_scales() {
        let l = parse_3dl(&swap_3dl("", 4095.0)).unwrap();
        assert_eq!(round(l.sample([1.0, 0.5, 0.0]), 3), [0.0, 0.5, 1.0]);
        assert_eq!(round(l.sample([1.0, 1.0, 1.0]), 4), [1.0, 1.0, 1.0], "12-bit inferred");
        let m = parse_3dl(&swap_3dl("Mesh 4 16\n", 4095.0)).unwrap();
        assert!((m.sample([1.0, 1.0, 1.0])[0] - 4095.0 / 65535.0).abs() < 1e-5);
        assert!(parse_3dl("0 512 1023\n0 0 0\n").is_none());
    }

    #[test]
    fn formats_come_from_the_extension() {
        assert_eq!([lut_format_of("Film.CUBE"), lut_format_of("a.3dl"), lut_format_of("x.look")], [Some("cube"), Some("3dl"), None]);
    }
}
