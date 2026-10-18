//! The colour panel's Mix grid (happypaint `engine/color/mix-grid.ts`,
//! v0.203.0 and v0.204.7): 2–6 colours from six fixed wells form a top and
//! a bottom edge; each edge ramps through its stops and every column blends
//! from the top edge to the bottom one, in OKLab or straight sRGB.

/// The six wells: top-left, top-middle, top-right, bottom-left, -middle, -right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MixSlot {
    Tl,
    Tm,
    Tr,
    Bl,
    Bm,
    Br,
}

impl MixSlot {
    pub const ALL: [MixSlot; 6] = [MixSlot::Tl, MixSlot::Tm, MixSlot::Tr, MixSlot::Bl, MixSlot::Bm, MixSlot::Br];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        ["Top-left", "Top-middle", "Top-right", "Bottom-left", "Bottom-middle", "Bottom-right"][self.index()]
    }
}

pub const MIX_COUNT_MIN: u32 = 2;
pub const MIX_COUNT_MAX: u32 = 6;
pub const MIX_ROWS: (u32, u32) = (1, 15);
pub const MIX_COLS: (u32, u32) = (2, 15);

/// Which wells each colour count uses, top edge then bottom (`MIX_LAYOUT`).
pub fn layout(count: u32) -> (&'static [MixSlot], &'static [MixSlot]) {
    use MixSlot::*;
    match count.clamp(MIX_COUNT_MIN, MIX_COUNT_MAX) {
        2 => (&[Tl, Tr], &[]),
        3 => (&[Tl, Tr], &[Bm]),
        4 => (&[Tl, Tr], &[Bl, Br]),
        5 => (&[Tl, Tm, Tr], &[Bl, Br]),
        _ => (&[Tl, Tm, Tr], &[Bl, Bm, Br]),
    }
}

/// The edges' colours for `count` (`mixEdges`).
pub fn edges(wells: &[String; 6], count: u32) -> (Vec<String>, Vec<String>) {
    let (top, bottom) = layout(count);
    let pick = |s: &MixSlot| wells[s.index()].clone();
    (top.iter().map(pick).collect(), bottom.iter().map(pick).collect())
}

/// How the grid blends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MixSpace {
    /// "Perceptual": even steps in lightness and hue.
    #[default]
    Oklab,
    Rgb,
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f64) -> f64 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// `#rgb` or `#rrggbb` (the `#` optional) as 0..1 channels; black otherwise (`hexToRgb01`).
pub fn hex_to_rgb01(hex: &str) -> [f64; 3] {
    let s = hex.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    let full: String = match s.len() {
        3 => s.chars().flat_map(|c| [c, c]).collect(),
        6 => s.to_string(),
        _ => return [0.0; 3],
    };
    if !full.chars().all(|c| c.is_ascii_hexdigit()) {
        return [0.0; 3];
    }
    let ch = |i: usize| u8::from_str_radix(&full[i..i + 2], 16).map_or(0.0, |v| v as f64 / 255.0);
    [ch(0), ch(2), ch(4)]
}

/// Bytes, rounded and clamped, as lowercase `#rrggbb` (`rgbBytesToHex`).
pub fn bytes_to_hex(r: f64, g: f64, b: f64) -> String {
    let b8 = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", b8(r), b8(g), b8(b))
}

pub fn hex_to_oklab(hex: &str) -> [f64; 3] {
    let [r, g, b] = hex_to_rgb01(hex).map(srgb_to_linear);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// OKLab to hex; out-of-gamut channels clamp in linear light.
pub fn oklab_to_hex([ll, a, bb]: [f64; 3]) -> String {
    let l = (ll + 0.396_337_777_4 * a + 0.215_803_757_3 * bb).powi(3);
    let m = (ll - 0.105_561_345_8 * a - 0.063_854_172_8 * bb).powi(3);
    let s = (ll - 0.089_484_177_5 * a - 1.291_485_548 * bb).powi(3);
    let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s;
    let byte = |c: f64| linear_to_srgb(c.clamp(0.0, 1.0)) * 255.0;
    bytes_to_hex(byte(r), byte(g), byte(b))
}

fn canonical(hex: &str) -> String {
    let [r, g, b] = hex_to_rgb01(hex);
    bytes_to_hex(r * 255.0, g * 255.0, b * 255.0)
}

fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn clamp_int(v: f64, lo: u32, hi: u32) -> u32 {
    if !v.is_finite() { lo } else { (v.round().max(lo as f64).min(hi as f64)) as u32 }
}

struct Edge {
    mix: Vec<[f64; 3]>,
    exact: Vec<Option<String>>,
}

/// The grid's colours, row by row (`mixGrid`). Rows and columns are clamped
/// (columns to at least the stops on either edge); without a bottom edge
/// there is one row, the top edge.
pub fn mix_grid(top: &[String], bottom: &[String], rows: f64, cols: f64, space: MixSpace) -> Vec<Vec<String>> {
    let black = ["#000000".to_string()];
    let top = if top.is_empty() { &black[..] } else { top };
    let nr = if bottom.is_empty() { 1 } else { clamp_int(rows, MIX_ROWS.0, MIX_ROWS.1) } as usize;
    let nc = clamp_int(cols, MIX_COLS.0.max(top.len() as u32).max(bottom.len() as u32), MIX_COLS.1) as usize;
    let enc = |h: &str| match space {
        MixSpace::Oklab => hex_to_oklab(h),
        MixSpace::Rgb => hex_to_rgb01(h),
    };
    let dec = |v: [f64; 3]| match space {
        MixSpace::Oklab => oklab_to_hex(v),
        MixSpace::Rgb => bytes_to_hex(v[0] * 255.0, v[1] * 255.0, v[2] * 255.0),
    };
    let edge = |stops: &[String]| -> Edge {
        let v: Vec<[f64; 3]> = stops.iter().map(|h| enc(h)).collect();
        let m = stops.len();
        if m == 1 {
            return Edge { mix: vec![v[0]; nc], exact: vec![Some(canonical(&stops[0])); nc] };
        }
        let at: Vec<usize> = (0..m).map(|i| (i as f64 * (nc - 1) as f64 / (m - 1) as f64).round() as usize).collect();
        let mut mix = Vec::with_capacity(nc);
        let mut j = 0;
        for c in 0..nc {
            while j < at.len() - 2 && c > at[j + 1] {
                j += 1;
            }
            mix.push(lerp3(v[j], v[j + 1], (c - at[j]) as f64 / (at[j + 1] - at[j]) as f64));
        }
        let mut exact = vec![None; nc];
        for (i, h) in stops.iter().enumerate() {
            exact[at[i]] = Some(canonical(h));
        }
        Edge { mix, exact }
    };
    let upper = edge(top);
    let lower = if bottom.is_empty() { None } else { Some(edge(bottom)) };
    let lower = lower.as_ref().unwrap_or(&upper);
    (0..nr)
        .map(|r| {
            let v = if nr == 1 { 0.0 } else { r as f64 / (nr - 1) as f64 };
            (0..nc)
                .map(|c| {
                    let exact = if r == 0 {
                        upper.exact[c].clone()
                    } else if r == nr - 1 {
                        lower.exact[c].clone()
                    } else {
                        None
                    };
                    exact.unwrap_or_else(|| dec(lerp3(upper.mix[c], lower.mix[c], v)))
                })
                .collect()
        })
        .collect()
}

/// The panel's settings (happypaint `mix-grid-store.ts`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct MixSettings {
    pub wells: [String; 6],
    pub count: u32,
    pub rows: u32,
    pub cols: u32,
    pub space: MixSpace,
}

impl Default for MixSettings {
    fn default() -> Self {
        Self {
            wells: ["#c0392b", "#27ae60", "#f4d03f", "#1f3a93", "#8e44ad", "#f5f5f5"].map(String::from),
            count: 4,
            rows: 5,
            cols: 7,
            space: MixSpace::Oklab,
        }
    }
}

impl MixSettings {
    /// Each field checked on its own (`parseMixSettings`): a bad one takes
    /// its default, numbers clamp.
    pub fn sanitized(mut self) -> Self {
        let d = Self::default();
        for (w, dw) in self.wells.iter_mut().zip(&d.wells) {
            let ok = w.len() == 7 && w.starts_with('#') && w[1..].chars().all(|c| c.is_ascii_hexdigit());
            *w = if ok { w.to_ascii_lowercase() } else { dw.clone() };
        }
        self.count = self.count.clamp(MIX_COUNT_MIN, MIX_COUNT_MAX);
        self.rows = self.rows.clamp(MIX_ROWS.0, MIX_ROWS.1);
        self.cols = self.cols.clamp(MIX_COLS.0, MIX_COLS.1);
        self
    }

    /// The grid as shown: the colours, and its rows × columns.
    pub fn grid(&self) -> Vec<Vec<String>> {
        let (top, bottom) = edges(&self.wells, self.count);
        mix_grid(&top, &bottom, self.rows as f64, self.cols as f64, self.space)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn oklab_matches_the_reference_and_round_trips() {
        let w = hex_to_oklab("#ffffff");
        assert!((w[0] - 1.0).abs() < 1e-4 && w[1].abs() < 1e-4 && w[2].abs() < 1e-4);
        let r = hex_to_oklab("#ff0000");
        assert!((r[0] - 0.62796).abs() < 1e-3 && (r[1] - 0.22486).abs() < 1e-3 && (r[2] - 0.12585).abs() < 1e-3);
        for h in ["#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#1b1b1b", "#c0392b", "#7fd3a2", "#123456"] {
            assert_eq!(oklab_to_hex(hex_to_oklab(h)), h);
        }
        assert_eq!(oklab_to_hex([1.2, 0.4, -0.4]).len(), 7);
    }

    #[test]
    fn grids_match_mix_grid_test() {
        let rgb = MixSpace::Rgb;
        let ok = MixSpace::Oklab;
        let g = mix_grid(&s(&["#C0392B", "#f4d03f"]), &s(&["#1f3a93", "#f5f5f5"]), 5.0, 7.0, ok);
        assert_eq!((g.len(), g[0].len()), (5, 7));
        assert_eq!([&g[0][0], &g[0][6], &g[4][0], &g[4][6]], ["#c0392b", "#f4d03f", "#1f3a93", "#f5f5f5"]);
        assert_eq!(
            mix_grid(&s(&["#000000", "#ffffff"]), &s(&["#ff0000", "#ff0000"]), 1.0, 3.0, rgb),
            vec![s(&["#000000", "#808080", "#ffffff"])]
        );
        assert_eq!(mix_grid(&s(&["#ff0000", "#0000ff"]), &[], 1.0, 3.0, rgb)[0][1], "#800080");
        assert_eq!(mix_grid(&s(&["#000000", "#ffffff"]), &[], 1.0, 3.0, ok)[0][1], "#636363");
        let steps = &mix_grid(&s(&["#000000", "#ffffff"]), &[], 1.0, 9.0, ok)[0];
        for w in steps.windows(2) {
            assert!((hex_to_oklab(&w[1])[0] - hex_to_oklab(&w[0])[0] - 0.125).abs() < 0.01);
        }
        let cols = mix_grid(&s(&["#000000", "#000000"]), &s(&["#ffffff", "#ffffff"]), 3.0, 2.0, rgb);
        assert_eq!(cols, vec![s(&["#000000", "#000000"]), s(&["#808080", "#808080"]), s(&["#ffffff", "#ffffff"])]);
        let bw = (s(&["#000000", "#ffffff"]), s(&["#000000", "#ffffff"]));
        let dims = |r: f64, c: f64| {
            let g = mix_grid(&bw.0, &bw.1, r, c, rgb);
            (g.len(), g[0].len())
        };
        assert_eq!((dims(0.0, 1.0), dims(99.0, 99.0), dims(f64::NAN, f64::NAN).1), ((1, 2), (15, 15), 2));
        let bad = mix_grid(&s(&["nope", "#ffffff"]), &s(&["#000000", "#ffffff"]), 1.0, 3.0, rgb);
        assert_eq!((bad[0][0].as_str(), bad[0][1].as_str()), ("#000000", "#808080"));
        assert_eq!(mix_grid(&s(&["#f00", "#fff"]), &s(&["#000", "#fff"]), 1.0, 2.0, rgb)[0], s(&["#ff0000", "#ffffff"]));
        assert_eq!(mix_grid(&s(&["#000000", "#ffffff"]), &[], 6.0, 3.0, rgb), vec![s(&["#000000", "#808080", "#ffffff"])]);
        for c in [3.0, 4.0, 5.0, 8.0, 15.0] {
            let row = &mix_grid(&s(&["#ff0000", "#00ff00", "#0000ff"]), &[], 1.0, c, ok)[0];
            assert!(row[0] == "#ff0000" && row.last().unwrap() == "#0000ff" && row.contains(&"#00ff00".to_string()), "{c}");
        }
        assert_eq!(
            mix_grid(&s(&["#000000", "#ffffff", "#000000"]), &[], 1.0, 5.0, rgb)[0],
            s(&["#000000", "#808080", "#ffffff", "#808080", "#000000"])
        );
        let one = mix_grid(&s(&["#000000", "#ffffff"]), &s(&["#ff0000"]), 3.0, 3.0, rgb);
        assert!(one[2].iter().all(|h| h == "#ff0000"));
        assert_eq!((one[1][0].as_str(), one[1][2].as_str()), ("#800000", "#ff8080"));
        assert_eq!(
            mix_grid(&s(&["#ff0000", "#00ff00", "#0000ff"]), &s(&["#000000", "#ffffff"]), 2.0, 2.0, rgb)[0],
            s(&["#ff0000", "#00ff00", "#0000ff"])
        );
    }

    #[test]
    fn edges_follow_the_layout() {
        let wells: [String; 6] = std::array::from_fn(|i| format!("#00000{}", i + 1));
        for n in 2..=6 {
            let (t, b) = edges(&wells, n);
            assert_eq!(t.len() + b.len(), n as usize);
        }
        assert_eq!(edges(&wells, 3), (s(&["#000001", "#000003"]), s(&["#000005"])));
        assert_eq!(edges(&wells, 1), edges(&wells, 2));
        assert_eq!(edges(&wells, 9), edges(&wells, 6));
        for n in 4..=6 {
            let (t, b) = edges(&wells, n);
            assert!(t[0] == "#000001" && t.last().unwrap() == "#000003" && b[0] == "#000004" && b.last().unwrap() == "#000006");
        }
    }

    #[test]
    fn settings_fall_back_field_by_field() {
        let d = MixSettings::default();
        assert_eq!(d.count, 4);
        let mut m = d.clone();
        m.wells[0] = "#FFFFFF".into();
        m.wells[1] = "red".into();
        m.rows = 0;
        m.cols = 400;
        m.count = 9;
        let m = m.sanitized();
        assert_eq!((m.wells[0].as_str(), m.wells[1].as_str()), ("#ffffff", d.wells[1].as_str()));
        assert_eq!((m.rows, m.cols, m.count), (1, 15, 6));
        let back: MixSettings = serde_json::from_str(r#"{"count": 2, "space": "rgb"}"#).unwrap();
        assert_eq!((back.count, back.space, back.rows), (2, MixSpace::Rgb, 5));
    }
}
