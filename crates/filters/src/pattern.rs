//! Built-in fill patterns (happypaint `utils/patterns.ts`): seamless 32 px
//! coverage functions baked into sRGB byte tiles with 4×4 supersampling.

/// Tile size in px at 100 % scale (`PATTERN_TILE`).
pub const PATTERN_TILE: usize = 32;

/// Built-in pattern ids and labels, in menu order.
pub const PATTERNS: [(&str, &str); 8] = [
    ("checker", "Checker"),
    ("stripes", "Stripes"),
    ("dots", "Dots"),
    ("grid", "Grid"),
    ("brick", "Brick"),
    ("crosshatch", "Crosshatch"),
    ("halftone", "Halftone"),
    ("weave", "Canvas weave"),
];

fn wrap(v: f64, m: f64) -> f64 {
    ((v % m) + m) % m
}

/// Coverage of colour A at (x, y), any real coordinates (`patternCoverage`).
pub fn pattern_coverage(id: &str, x: f64, y: f64) -> f64 {
    let s = PATTERN_TILE as f64;
    let u = wrap(x, s);
    let v = wrap(y, s);
    let b = |c: bool| if c { 1.0 } else { 0.0 };
    match id {
        "checker" => b(((u / (s / 2.0)).floor() + (v / (s / 2.0)).floor()) % 2.0 == 0.0),
        "stripes" => b(v < s / 4.0 || (v >= s / 2.0 && v < 3.0 * s / 4.0)),
        "dots" => b((u - s / 2.0).hypot(v - s / 2.0) < s * 0.22),
        "grid" => b(u < 2.0 || v < 2.0),
        "brick" => {
            let course = (v / (s / 2.0)).floor();
            let bu = wrap(u + (course % 2.0) * (s / 2.0), s);
            b(wrap(v, s / 2.0) < 2.0 || bu < 2.0)
        }
        "crosshatch" => b(wrap(u + v, s / 2.0) < 2.0 || wrap(u - v, s / 2.0) < 2.0),
        "halftone" => {
            let corner = u.hypot(v).min((s - u).hypot(v)).min(u.hypot(s - v)).min((s - u).hypot(s - v));
            let centre = (u - s / 2.0).hypot(v - s / 2.0);
            b(corner.min(centre) < s * 0.16)
        }
        "weave" => {
            let bx = (u / (s / 2.0)).floor();
            let by = (v / (s / 2.0)).floor();
            let along = if (bx + by) % 2.0 == 0.0 { v } else { u };
            b(wrap(along, 8.0) < 6.0)
        }
        _ => 0.0,
    }
}

/// One baked tile: straight sRGB bytes, opaque.
#[derive(Clone, Debug, PartialEq)]
pub struct PatternTile {
    pub size: usize,
    pub data: Vec<u8>,
}

impl PatternTile {
    /// Bilinear with REPEAT wrap at uv (one tile per unit), straight 0..1.
    pub fn sample_repeat(&self, u: f32, v: f32) -> [f32; 4] {
        let n = self.size as i64;
        let x = u * self.size as f32 - 0.5;
        let y = v * self.size as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |xx: i64, yy: i64| {
            let i = ((yy.rem_euclid(n) * n + xx.rem_euclid(n)) * 4) as usize;
            [0, 1, 2, 3].map(|k| self.data[i + k] as f32 / 255.0)
        };
        let (xi, yi) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (at(xi, yi), at(xi + 1, yi), at(xi, yi + 1), at(xi + 1, yi + 1));
        [0, 1, 2, 3].map(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bot = c[k] + (d[k] - c[k]) * fx;
            top + (bot - top) * fy
        })
    }
}

/// Bake a tile: A where covered, B elsewhere (`patternPixels`). Unknown ids use checker.
pub fn pattern_pixels(id: &str, a: [f32; 3], b: [f32; 3]) -> PatternTile {
    let id = if PATTERNS.iter().any(|(p, _)| *p == id) { id } else { PATTERNS[0].0 };
    // Colours come from JSON decimals: widen through their shortest decimal form
    // so a tie like 0.1·255 = 25.5 rounds as it does in happypaint's doubles.
    let (ad, bd) = (a.map(crate::tex::widen), b.map(crate::tex::widen));
    let s = PATTERN_TILE;
    let n = 4;
    let mut data = vec![0u8; s * s * 4];
    for y in 0..s {
        for x in 0..s {
            let mut cov = 0.0;
            for j in 0..n {
                for i in 0..n {
                    cov += pattern_coverage(id, x as f64 + (i as f64 + 0.5) / n as f64, y as f64 + (j as f64 + 0.5) / n as f64);
                }
            }
            cov /= (n * n) as f64;
            let k = (y * s + x) * 4;
            for c in 0..3 {
                let v = (bd[c] + (ad[c] - bd[c]) * cov) * 255.0;
                // `Math.round`: halves round up.
                data[k + c] = (v + 0.5).floor().clamp(0.0, 255.0) as u8;
            }
            data[k + 3] = 255;
        }
    }
    PatternTile { size: s, data }
}
