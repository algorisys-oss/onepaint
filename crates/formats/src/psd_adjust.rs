//! PSD adjustment blocks for Exposure (`expA`), Vibrance (`vibA`), Photo
//! Filter (`phfl`) and Channel Mixer (`mixr`), mapped as happypaint's
//! `psd-map.ts` does (byte layouts as ag-psd reads and writes them). Photo
//! Filter's colour follows happypaint v0.204.7 `readPhotoFilter`: version 3
//! holds D50 XYZ with 1.0 = 32768, version 2 a colour record.

use onepaint_doc::Adjustment;
use onepaint_filters::Params;

fn be16(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn f32be(d: &[u8], at: usize) -> Option<f32> {
    be32(d, at).map(f32::from_bits)
}

fn l2s(c: f64) -> f64 {
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// D50 XYZ to sRGB 0..1 through the Bradford-adapted matrix (happypaint
/// `xyzD50ToSrgb`), clamped in linear light.
pub fn xyz_d50_to_srgb([x, y, z]: [f64; 3]) -> [f32; 3] {
    const MI: [f64; 9] = [3.1338561, -1.6168667, -0.4906146, -0.9787684, 1.9161415, 0.033454, 0.0719453, -0.2289914, 1.4052427];
    std::array::from_fn(|i| l2s((MI[i * 3] * x + MI[i * 3 + 1] * y + MI[i * 3 + 2] * z).clamp(0.0, 1.0)) as f32)
}

/// CIE Lab (L 0..100, D50) to sRGB (happypaint `labToSrgb`).
pub fn lab_to_srgb(l: f64, a: f64, b: f64) -> [f32; 3] {
    const WHITE: [f64; 3] = [0.9642, 1.0, 0.8249];
    let finv = |t: f64| if t.powi(3) > 216.0 / 24389.0 { t.powi(3) } else { (116.0 * t - 16.0) * 27.0 / 24389.0 };
    let fy = (l + 16.0) / 116.0;
    let (fx, fz) = (fy + a / 500.0, fy - b / 200.0);
    xyz_d50_to_srgb([finv(fx) * WHITE[0], finv(fy) * WHITE[1], finv(fz) * WHITE[2]])
}

/// A 10-byte colour record (space, four u16) as sRGB 0..1; `None` for CMYK
/// and spaces happypaint doesn't map (`readColor` + `fromBinaryColor` +
/// `colorFromPsd`).
fn record_color(d: &[u8], at: usize) -> Option<[f32; 3]> {
    let space = be16(d, at)?;
    let v = |i: usize| be16(d, at + 2 + i * 2);
    let s = |i: usize| v(i).map(|u| u as i16 as f64);
    match space {
        0 => Some([v(0)? as f32 / 65535.0, v(1)? as f32 / 65535.0, v(2)? as f32 / 65535.0]),
        1 => {
            let (h, sat, val) = (v(0)? as f64 / 65535.0, v(1)? as f64 / 65535.0, v(2)? as f64 / 65535.0);
            let f = |n: f64| {
                let k = (n + h * 6.0) % 6.0;
                (val - val * sat * k.min(4.0 - k).clamp(0.0, 1.0)) as f32
            };
            Some([f(5.0), f(3.0), f(1.0)])
        }
        7 => Some(lab_to_srgb(s(0)? / 100.0, s(1)? / 100.0, s(2)? / 100.0)),
        8 => {
            let g = (1.0 - v(0)? as f32 / 10000.0).clamp(0.0, 1.0);
            Some([g; 3])
        }
        _ => None,
    }
}

/// Above this an XYZ component isn't a colour (D50 white is 0.96 / 1 / 0.82).
const XYZ_MAX: f64 = 2.0;

/// `phfl` (`readPhotoFilter`).
fn photo_filter(d: &[u8]) -> Option<Params> {
    let (color, at) = match be16(d, 0)? {
        3 => {
            let xyz = [be32(d, 2)?, be32(d, 6)?, be32(d, 10)?].map(|v| v as f64 / 32768.0);
            if xyz.iter().any(|&v| v > XYZ_MAX) {
                return None;
            }
            (xyz_d50_to_srgb(xyz), 14)
        }
        2 => (record_color(d, 2)?, 12),
        _ => return None,
    };
    let density = (be32(d, at)? as f32 / 100.0).clamp(0.0, 1.0);
    let keep = *d.get(at + 4)? != 0;
    Some(Params { filter_color: Some(color), filter_density: Some(density), preserve_lum: Some(keep), ..Default::default() })
}

/// The `long`, `doub` and `bool` (0 or 1) items of an action descriptor
/// (version 16), by key; an object's items are keyed `object.item`.
fn descriptor_numbers(d: &[u8]) -> Option<Vec<(String, f64)>> {
    if be32(d, 0)? != 16 {
        return None;
    }
    let mut out = Vec::new();
    // A stop part-way keeps what was read before it.
    let _ = descriptor_items(d, 4, "", &mut out);
    Some(out)
}

/// One descriptor's name, class id and items from `at`, into `out`; where
/// it ends. An unknown item type ends the read: nothing past it is needed.
fn descriptor_items(d: &[u8], mut at: usize, prefix: &str, out: &mut Vec<(String, f64)>) -> Option<usize> {
    // The name (UTF-16 with a length), then the class id.
    at += 4 + be32(d, at)? as usize * 2;
    let id_len = be32(d, at)? as usize;
    at += 4 + if id_len == 0 { 4 } else { id_len };
    let n = be32(d, at)?;
    at += 4;
    for _ in 0..n {
        let kl = be32(d, at)? as usize;
        let kl = if kl == 0 { 4 } else { kl };
        let key = format!("{prefix}{}", String::from_utf8_lossy(d.get(at + 4..at + 4 + kl)?));
        at += 4 + kl;
        let ty = d.get(at..at + 4)?;
        at += 4;
        match ty {
            b"long" => {
                out.push((key, be32(d, at)? as i32 as f64));
                at += 4;
            }
            b"doub" => {
                out.push((key, f64::from_bits(u64::from(be32(d, at)?) << 32 | u64::from(be32(d, at + 4)?))));
                at += 8;
            }
            b"bool" => {
                out.push((key, f64::from(*d.get(at)?)));
                at += 1;
            }
            b"Objc" => at = descriptor_items(d, at, &format!("{key}."), out)?,
            _ => return None,
        }
    }
    Some(at)
}

/// Black & White (`blwh`): the six weights in percent, the tint flag and
/// colour (happypaint `psd-map.ts`, at full strength: Amount is 1).
fn black_white(d: &[u8]) -> Option<Params> {
    let items = descriptor_numbers(d).unwrap_or_default();
    let get = |k: &str| items.iter().find(|(key, _)| key == k).map(|i| i.1 as f32);
    let [r, y, g, c, b, m] = onepaint_filters::BW_DEFAULT_WEIGHTS;
    let w = |k: &str, d: f32| Some(get(k).map_or(d, |v| v / 100.0));
    let tint = match (get("tintColor.Rd  "), get("tintColor.Grn "), get("tintColor.Bl  ")) {
        (Some(r), Some(g), Some(b)) => [r, g, b].map(|v| (v / 255.0).clamp(0.0, 1.0)),
        _ => onepaint_filters::BW_DEFAULT_TINT,
    };
    Some(Params {
        amount: Some(1.0),
        bw_reds: w("Rd  ", r),
        bw_yellows: w("Yllw", y),
        bw_greens: w("Grn ", g),
        bw_cyans: w("Cyn ", c),
        bw_blues: w("Bl  ", b),
        bw_magentas: w("Mgnt", m),
        bw_tint: Some(get("useTint").is_some_and(|v| v != 0.0)),
        bw_tint_color: Some(tint),
        ..Default::default()
    })
}

fn mixr(d: &[u8]) -> Option<Params> {
    if be16(d, 0)? != 1 {
        return None;
    }
    let mono = be16(d, 2)? != 0;
    // A row: red, green, blue, two spare bytes, constant (percent).
    let row = |i: usize| -> Option<[f32; 4]> {
        let at = 4 + i * 10;
        let s = |o: usize| be16(d, at + o).map(|v| v as i16 as f32 / 100.0);
        Some([s(0)?, s(2)?, s(4)?, s(8)?])
    };
    let mut p = Params { mix_mono: Some(mono), ..Default::default() };
    let set = |p: &mut Params, k: usize, r: [f32; 4]| {
        let f = match k {
            0 => [&mut p.mix_rr, &mut p.mix_rg, &mut p.mix_rb, &mut p.mix_rc],
            1 => [&mut p.mix_gr, &mut p.mix_gg, &mut p.mix_gb, &mut p.mix_gc],
            2 => [&mut p.mix_br, &mut p.mix_bg, &mut p.mix_bb, &mut p.mix_bc],
            _ => [&mut p.mix_kr, &mut p.mix_kg, &mut p.mix_kb, &mut p.mix_kc],
        };
        for (slot, v) in f.into_iter().zip(r) {
            *slot = Some(v);
        }
    };
    if mono {
        set(&mut p, 3, row(0)?);
        let d = onepaint_filters::default_params(onepaint_filters::Kind::ChannelMixer);
        (p.mix_rr, p.mix_gg, p.mix_bb) = (d.mix_rr, d.mix_gg, d.mix_bb);
    } else {
        for k in 0..4 {
            set(&mut p, k, row(k)?);
        }
    }
    Some(p)
}

/// An adjustment layer from one of these blocks.
pub fn read(key: &[u8; 4], d: &[u8]) -> Option<Adjustment> {
    let (kind, params) = match key {
        b"expA" => {
            if be16(d, 0)? != 1 {
                return None;
            }
            let g = f32be(d, 10)?;
            let p = Params {
                exposure: Some(f32be(d, 2)?),
                exp_offset: Some(f32be(d, 6)?),
                exp_gamma: Some(if g == 0.0 || !g.is_finite() { 1.0 } else { g }),
                ..Default::default()
            };
            ("exposure", p)
        }
        b"vibA" => {
            let items = descriptor_numbers(d)?;
            let get = |k: &str| items.iter().find(|(key, _)| key == k).map_or(0.0, |v| v.1 as f32 / 100.0);
            ("vibrance", Params { vibrance: Some(get("vibrance")), vib_saturation: Some(get("Strt")), ..Default::default() })
        }
        b"phfl" => ("photo-filter", photo_filter(d)?),
        b"mixr" => ("channel-mixer", mixr(d)?),
        b"levl" => ("levels", levels(d)?),
        b"curv" => ("curves", curves(d)?),
        b"hue2" => ("hue-saturation", hue_sat(d)?),
        b"blnc" => ("color-balance", color_balance(d)?),
        b"blwh" => ("black-white", black_white(d)?),
        b"selc" => ("cmyk-selective-color", selective_cmyk(d)?),
        b"grdm" => ("gradient-map", gradient_map(d)?),
        b"clrL" => ("color-lookup", color_lookup(d)?),
        _ => return None,
    };
    Some(Adjustment { kind: kind.into(), params })
}

/// Color Lookup (`clrL`: version 1, then a descriptor), as happypaint's
/// `psd-map.ts`: only a 3D LUT file onepaint can read comes in live (an
/// abstract or device-link profile, or a `.look`, keeps its record).
/// Photoshop's own fields go to `extra.lutPsd`.
fn color_lookup(d: &[u8]) -> Option<Params> {
    use crate::psd_desc::DVal;
    if be16(d, 0)? != 1 {
        return None;
    }
    let (desc, _) = crate::psd_desc::read(d, 6)?;
    if desc.enum_val("lookupType").is_some_and(|t| t != "3DLUT") {
        return None;
    }
    let Some(DVal::Data(data)) = desc.get("LUT3DFileData") else { return None };
    let name = desc.text("LUT3DFileName").or(desc.text("Nm  ")).filter(|n| !n.is_empty()).unwrap_or("LUT").to_string();
    let format = match desc.enum_val("LUTFormat") {
        Some("LUTFormatCUBE") => "cube",
        Some("LUTFormat3DL") => "3dl",
        _ => onepaint_filters::lut3d::lut_format_of(&name)?,
    };
    let text = String::from_utf8_lossy(data).into_owned();
    onepaint_filters::lut3d::parse_lut(format, &text)?;
    let mut p = Params { lut: Some(onepaint_filters::LutFile { format: format.into(), name, text }), ..Default::default() };
    let mut psd = serde_json::Map::new();
    if let Some(b) = desc.bool("Dthr") {
        psd.insert("dither".into(), b.into());
    }
    for key in ["dataOrder", "tableOrder"] {
        if let Some(o) = desc.enum_val(key) {
            psd.insert(key.into(), (if o == "bgrOrder" { "bgr" } else { "rgb" }).into());
        }
    }
    if !psd.is_empty() {
        p.extra.insert("lutPsd".into(), psd.into());
    }
    Some(p)
}

/// `clrL` for a Color Lookup layer (ag-psd's key order). A table loaded in
/// onepaint has no companion profile: Photoshop's was compiled from the
/// table it came with, and an untouched layer keeps its original record.
fn color_lookup_block(p: &Params) -> Vec<u8> {
    use crate::psd_desc::{DVal, Desc};
    let mut d = Desc::new("null");
    d.push("lookupType", DVal::Enum("colorLookupType".into(), "3DLUT".into()));
    if let Some(l) = &p.lut {
        let psd = p.extra.get("lutPsd");
        let field = |k: &str| psd.and_then(|v| v.get(k));
        let order = |k: &str| field(k).and_then(|v| v.as_str()).map(|o| if o == "bgr" { "bgrOrder" } else { "rgbOrder" });
        d.push("Nm  ", DVal::Text(l.name.clone()));
        d.push("Dthr", DVal::Bool(field("dither").and_then(serde_json::Value::as_bool).unwrap_or(true)));
        let fmt = if l.format == "3dl" { "LUTFormat3DL" } else { "LUTFormatCUBE" };
        d.push("LUTFormat", DVal::Enum("LUTFormatType".into(), fmt.into()));
        for k in ["dataOrder", "tableOrder"] {
            if let Some(o) = order(k) {
                d.push(k, DVal::Enum("colorLookupOrder".into(), o.into()));
            }
        }
        d.push("LUT3DFileData", DVal::Data(l.text.as_bytes().to_vec()));
        d.push("LUT3DFileName", DVal::Text(l.name.clone()));
    }
    let mut o = Vec::new();
    o.extend(1u16.to_be_bytes());
    o.extend(16u32.to_be_bytes());
    crate::psd_desc::write_into(&mut o, &d);
    o
}

/// The block for an adjustment layer of these kinds.
pub fn write(a: &Adjustment) -> Option<([u8; 4], Vec<u8>)> {
    let p = &a.params;
    let mut o = Vec::new();
    let pct = |v: f32| (v.clamp(-2.0, 2.0) * 100.0).round() as i16;
    let key = match a.kind.as_str() {
        "color-lookup" => return Some((*b"clrL", color_lookup_block(p))),
        "exposure" => {
            o.extend(1u16.to_be_bytes());
            o.extend(p.exposure.unwrap_or(0.0).clamp(-20.0, 20.0).to_be_bytes());
            o.extend(p.exp_offset.unwrap_or(0.0).clamp(-0.5, 0.5).to_be_bytes());
            o.extend(p.exp_gamma.unwrap_or(1.0).clamp(0.01, 9.99).to_be_bytes());
            o.extend([0, 0]);
            *b"expA"
        }
        "vibrance" => {
            o.extend(16u32.to_be_bytes());
            o.extend(1u32.to_be_bytes()); // an empty name: just its terminator
            o.extend([0, 0]);
            o.extend(0u32.to_be_bytes());
            o.extend(*b"null");
            o.extend(2u32.to_be_bytes());
            for (k, v) in [("vibrance", p.vibrance.unwrap_or(0.0)), ("Strt", p.vib_saturation.unwrap_or(0.0))] {
                if k.len() == 4 {
                    o.extend(0u32.to_be_bytes());
                } else {
                    o.extend((k.len() as u32).to_be_bytes());
                }
                o.extend(k.as_bytes());
                o.extend(*b"long");
                o.extend(((v.clamp(-1.0, 1.0) * 100.0).round() as i32).to_be_bytes());
            }
            *b"vibA"
        }
        "photo-filter" => {
            let c = p.filter_color.unwrap_or([236.0 / 255.0, 138.0 / 255.0, 0.0]);
            o.extend(2u16.to_be_bytes());
            o.extend(0u16.to_be_bytes()); // RGB
            for v in c {
                o.extend(((v.clamp(0.0, 1.0) * 255.0).round() as u16 * 257).to_be_bytes());
            }
            o.extend([0, 0]);
            o.extend(((p.filter_density.unwrap_or(0.25).clamp(0.0, 1.0) * 100.0).round() as u32).to_be_bytes());
            o.push(u8::from(p.preserve_lum.unwrap_or(true)));
            o.extend([0, 0, 0]);
            *b"phfl"
        }
        "channel-mixer" => {
            let rows = onepaint_filters::channel_mixer_rows(&Params { mix_mono: Some(false), ..p.clone() });
            let mono = p.mix_mono.unwrap_or(false);
            o.extend(1u16.to_be_bytes());
            o.extend(u16::from(mono).to_be_bytes());
            let mut row = |r: [f32; 4]| {
                for v in &r[..3] {
                    o.extend(pct(*v).to_be_bytes());
                }
                o.extend([0, 0]);
                o.extend(pct(r[3]).to_be_bytes());
            };
            if mono {
                row(rows[3]);
            } else {
                for r in rows {
                    row(r);
                }
            }
            if mono {
                o.extend([0; 30]);
            }
            *b"mixr"
        }
        "levels" => {
            o.extend(2u16.to_be_bytes());
            let b = |v: Option<f32>, d: f32| (v.unwrap_or(d).clamp(0.0, 1.0) * 255.0).round() as i16;
            let gamma = (p.gamma.unwrap_or(1.0).clamp(0.1, 9.99) * 100.0).round() as i16;
            let rgb = [b(p.in_black, 0.0), b(p.in_white, 1.0), b(p.out_black, 0.0), b(p.out_white, 1.0), gamma];
            for v in rgb {
                o.extend(v.to_be_bytes());
            }
            // The red, green and blue records and the rest stay at their defaults.
            for _ in 0..62 {
                for v in [0i16, 255, 0, 255, 100] {
                    o.extend(v.to_be_bytes());
                }
            }
            *b"levl"
        }
        "curves" => {
            let chans: Vec<(u16, Vec<[f64; 2]>)> = [(&p.curve, 1u16), (&p.curve_r, 2), (&p.curve_g, 4), (&p.curve_b, 8)]
                .into_iter()
                .filter_map(|(c, bit)| {
                    let pts = c.as_ref().filter(|c| c.len() >= 2)?;
                    Some((bit, pts.iter().map(|q| [q.x, q.y]).collect()))
                })
                .collect();
            let chans = if chans.iter().any(|c| c.0 == 1) { chans } else { [vec![(1, vec![[0.0, 0.0], [1.0, 1.0]])], chans].concat() };
            let v = |x: f64| ((x.clamp(0.0, 1.0) * 255.0).round() as i16).to_be_bytes();
            o.push(0);
            o.extend(1u16.to_be_bytes());
            o.extend(0u16.to_be_bytes());
            o.extend(chans.iter().map(|c| c.0).fold(0u16, |a, b| a | b).to_be_bytes());
            for (_, pts) in &chans {
                o.extend((pts.len() as u16).to_be_bytes());
                for q in pts {
                    o.extend(v(q[1]));
                    o.extend(v(q[0]));
                }
            }
            // Photoshop's second copy (`Crv `), which newer versions read.
            o.extend(*b"Crv ");
            o.extend(4u16.to_be_bytes());
            o.extend(0u16.to_be_bytes());
            o.extend((chans.len() as u16).to_be_bytes());
            for (bit, pts) in &chans {
                o.extend((bit.trailing_zeros() as u16).to_be_bytes());
                o.extend((pts.len() as u16).to_be_bytes());
                for q in pts {
                    o.extend(v(q[1]));
                    o.extend(v(q[0]));
                }
            }
            *b"curv"
        }
        "hue-saturation" => {
            o.extend(2u16.to_be_bytes());
            let master = [
                0,
                0,
                0,
                0,
                p.hue.unwrap_or(0.0).clamp(-180.0, 180.0).round() as i16,
                (p.saturation.unwrap_or(0.0).clamp(-1.0, 1.0) * 100.0).round() as i16,
                (p.brightness.unwrap_or(0.0).clamp(-1.0, 1.0) * 100.0).round() as i16,
            ];
            for v in master {
                o.extend(v.to_be_bytes());
            }
            o.extend([0u8; 6 * 14]);
            *b"hue2"
        }
        "color-balance" => {
            let pc = |v: Option<f32>| ((v.unwrap_or(0.0).clamp(-1.0, 1.0) * 100.0).round() as i16).to_be_bytes();
            let ranges = [
                p.cb_shadow_cr,
                p.cb_shadow_mg,
                p.cb_shadow_yb,
                p.cyan_red,
                p.magenta_green,
                p.yellow_blue,
                p.cb_high_cr,
                p.cb_high_mg,
                p.cb_high_yb,
            ];
            for v in ranges {
                o.extend(pc(v));
            }
            o.extend([u8::from(p.preserve_lum != Some(false)), 0]);
            *b"blnc"
        }
        "black-white" => {
            o.extend(write_black_white(p));
            *b"blwh"
        }
        "cmyk-selective-color" => {
            // Version 1, the method, then ten (C, M, Y, K) groups in percent:
            // the first unused, then the ranges in `SC_RANGES` order.
            let (t, abs) = onepaint_filters::sc_table_of(p);
            o.extend(1u16.to_be_bytes());
            o.extend(u16::from(abs).to_be_bytes());
            o.extend([0u8; 8]);
            for range in t {
                for v in range {
                    o.extend(((v.clamp(-1.0, 1.0) * 100.0).round() as i16).to_be_bytes());
                }
            }
            *b"selc"
        }
        "gradient-map" => {
            o.extend(write_gradient_map(p));
            *b"grdm"
        }
        _ => return None,
    };
    Some((key, o))
}

fn levels(d: &[u8]) -> Option<Params> {
    if be16(d, 0)? != 2 {
        return None;
    }
    let v = |i: usize| be16(d, 2 + i * 2).map(|x| x as i16 as f32);
    let n = |x: f32| (x / 255.0).clamp(0.0, 1.0);
    let gamma = v(4)? / 100.0;
    Some(Params {
        in_black: Some(n(v(0)?)),
        in_white: Some(n(v(1)?)),
        out_black: Some(n(v(2)?)),
        out_white: Some(n(v(3)?)),
        gamma: Some(if gamma > 0.0 { gamma } else { 1.0 }),
        ..Default::default()
    })
}

fn curves(d: &[u8]) -> Option<Params> {
    use onepaint_stroke::CurvePoint;
    if be16(d, 1)? != 1 {
        return None;
    }
    let mask = be16(d, 5)?;
    let mut at = 7;
    let mut p = Params { curve_mode: Some("smooth".into()), ..Default::default() };
    for bit in [1u16, 2, 4, 8] {
        if mask & bit == 0 {
            continue;
        }
        let n = be16(d, at)? as usize;
        at += 2;
        let mut pts = Vec::with_capacity(n);
        for _ in 0..n {
            let (out, inp) = (be16(d, at)? as i16 as f64, be16(d, at + 2)? as i16 as f64);
            pts.push(CurvePoint::new((inp / 255.0).clamp(0.0, 1.0), (out / 255.0).clamp(0.0, 1.0)));
            at += 4;
        }
        let pts = (pts.len() >= 2).then_some(pts);
        match bit {
            1 => p.curve = pts,
            2 => p.curve_r = pts,
            4 => p.curve_g = pts,
            _ => p.curve_b = pts,
        }
    }
    if p.curve.is_none() {
        p.curve = Some(vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)]);
    }
    Some(p)
}

/// The master hue, saturation and lightness; colorize and the six ranges
/// aren't kept (as happypaint).
fn hue_sat(d: &[u8]) -> Option<Params> {
    if be16(d, 0)? != 2 {
        return None;
    }
    let v = |i: usize| be16(d, 2 + i * 2).map(|x| x as i16 as f32);
    Some(Params { hue: Some(v(4)?), saturation: Some(v(5)? / 100.0), brightness: Some(v(6)? / 100.0), ..Default::default() })
}

/// The midtones; onepaint's Color Balance has no shadow or highlight range yet.
/// Selective Color (`selc`): version 1, relative (0) or absolute (1), then
/// ten (C, M, Y, K) groups in percent, the first unused.
fn selective_cmyk(d: &[u8]) -> Option<Params> {
    if be16(d, 0)? != 1 {
        return None;
    }
    let abs = be16(d, 2)? == 1;
    let sc = (0..36).map(|i| be16(d, 12 + i * 2).map(|v| v as i16 as f32 / 100.0)).collect::<Option<Vec<_>>>()?;
    Some(Params { sc_cmyk: Some(sc), sc_method: Some(if abs { "absolute" } else { "relative" }.into()), ..Default::default() })
}

/// Color Balance (`blnc`): shadows, midtones and highlights, each (cyan–red,
/// magenta–green, yellow–blue) in percent, then Preserve Luminosity.
fn color_balance(d: &[u8]) -> Option<Params> {
    let v = |i: usize| be16(d, i * 2).map(|x| x as i16 as f32 / 100.0);
    Some(Params {
        cb_shadow_cr: Some(v(0)?),
        cb_shadow_mg: Some(v(1)?),
        cb_shadow_yb: Some(v(2)?),
        cyan_red: Some(v(3)?),
        magenta_green: Some(v(4)?),
        yellow_blue: Some(v(5)?),
        cb_high_cr: Some(v(6)?),
        cb_high_mg: Some(v(7)?),
        cb_high_yb: Some(v(8)?),
        preserve_lum: Some(d.get(18).is_none_or(|b| *b != 0)),
        ..Default::default()
    })
}

fn unicode(o: &mut Vec<u8>, s: &str) {
    let u: Vec<u16> = s.encode_utf16().collect();
    o.extend((u.len() as u32 + 1).to_be_bytes());
    for c in u {
        o.extend(c.to_be_bytes());
    }
    o.extend([0, 0]);
}

fn desc_key(o: &mut Vec<u8>, k: &str) {
    if k.len() == 4 {
        o.extend(0u32.to_be_bytes());
    } else {
        o.extend((k.len() as u32).to_be_bytes());
    }
    o.extend(k.as_bytes());
}

/// Black & White: the weights in percent and the tint. Photoshop has no
/// Amount (happypaint writes the layer at full strength; its opacity fades it).
fn write_black_white(p: &Params) -> Vec<u8> {
    let (prim, sec) = ([p.bw_reds, p.bw_greens, p.bw_blues], [p.bw_yellows, p.bw_cyans, p.bw_magentas]);
    let [dr, dy, dg, dc, db, dm] = onepaint_filters::BW_DEFAULT_WEIGHTS;
    let pct = |v: Option<f32>, d: f32| (v.unwrap_or(d).clamp(-2.0, 3.0) * 100.0).round() as i32;
    let weights = [
        ("Rd  ", pct(prim[0], dr)),
        ("Yllw", pct(sec[0], dy)),
        ("Grn ", pct(prim[1], dg)),
        ("Cyn ", pct(sec[1], dc)),
        ("Bl  ", pct(prim[2], db)),
        ("Mgnt", pct(sec[2], dm)),
    ];
    let tint = p.bw_tint_color.unwrap_or(onepaint_filters::BW_DEFAULT_TINT).map(|v| f64::from(v.clamp(0.0, 1.0)) * 255.0);
    let mut o = Vec::new();
    o.extend(16u32.to_be_bytes());
    unicode(&mut o, "");
    desc_key(&mut o, "null");
    o.extend(8u32.to_be_bytes());
    for (k, v) in weights {
        desc_key(&mut o, k);
        o.extend(*b"long");
        o.extend(v.to_be_bytes());
    }
    desc_key(&mut o, "useTint");
    o.extend(*b"bool");
    o.push(u8::from(p.bw_tint.unwrap_or(false)));
    desc_key(&mut o, "tintColor");
    o.extend(*b"Objc");
    unicode(&mut o, "");
    desc_key(&mut o, "RGBC");
    o.extend(3u32.to_be_bytes());
    for (k, v) in [("Rd  ", tint[0]), ("Grn ", tint[1]), ("Bl  ", tint[2])] {
        desc_key(&mut o, k);
        o.extend(*b"doub");
        o.extend(v.to_bits().to_be_bytes());
    }
    o
}

/// Stops' locations are in 1/4096ths (`interpolation`).
fn gradient_map(d: &[u8]) -> Option<Params> {
    let mut at = 2;
    let version = be16(d, 0)?;
    at += 2; // reverse, dither
    if version == 3 && *d.get(at)? != 0 {
        at += 4;
    }
    at += 4 + be32(d, at)? as usize * 2;
    let n = be16(d, at)? as usize;
    at += 2;
    let mut raw = Vec::with_capacity(n);
    for _ in 0..n {
        raw.push((be32(d, at)? as f32, record_color(d, at + 8)));
        at += 20;
    }
    let m = be16(d, at)? as usize;
    at += 2 + m * 10;
    let interpolation = be16(d, at + 2)?.max(1) as f32;
    let mode = be16(d, at + 6)?;
    if mode != 0 || raw.len() < 2 {
        return None;
    }
    let mut stops = raw
        .into_iter()
        .map(|(loc, c)| Some(onepaint_filters::Stop { pos: (loc / interpolation).clamp(0.0, 1.0), color: c? }))
        .collect::<Option<Vec<_>>>()?;
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    Some(Params { gradient_stops: Some(stops), ..Default::default() })
}

fn write_gradient_map(p: &Params) -> Vec<u8> {
    let mut stops = p.gradient_stops.clone().filter(|s| s.len() >= 2).unwrap_or_else(|| {
        vec![
            onepaint_filters::Stop { pos: 0.0, color: p.shadow.unwrap_or([0.0; 3]) },
            onepaint_filters::Stop { pos: 1.0, color: p.highlight.unwrap_or([1.0; 3]) },
        ]
    });
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    let mut o = Vec::new();
    o.extend(1u16.to_be_bytes());
    o.extend([0, 0]);
    unicode(&mut o, "Custom");
    o.extend((stops.len() as u16).to_be_bytes());
    for s in &stops {
        o.extend(((s.pos.clamp(0.0, 1.0) * 4096.0).round() as u32).to_be_bytes());
        o.extend(50u32.to_be_bytes());
        o.extend(0u16.to_be_bytes());
        for v in s.color {
            o.extend(((v.clamp(0.0, 1.0) * 255.0).round() as u16 * 257).to_be_bytes());
        }
        o.extend([0, 0, 0, 0]);
    }
    o.extend(2u16.to_be_bytes());
    for loc in [0u32, 4096] {
        o.extend(loc.to_be_bytes());
        o.extend(50u32.to_be_bytes());
        o.extend(255u16.to_be_bytes());
    }
    o.extend(2u16.to_be_bytes());
    o.extend(4096u16.to_be_bytes());
    o.extend(32u16.to_be_bytes());
    // Solid, no seed, no transparency, unrestricted, smooth, RGB, the full range.
    o.extend([0u8; 2 + 4 + 2 + 2 + 4]);
    o.extend(3u16.to_be_bytes());
    o.extend([0u8; 8]);
    for _ in 0..4 {
        o.extend(0x8000u16.to_be_bytes());
    }
    o.extend([0u8; 4]);
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close3(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() <= eps)
    }

    // happypaint psd-roundtrip.test.ts: Color Lookup comes in live with its
    // table, and a replaced table goes out without a stale profile.
    #[test]
    fn color_lookup_round_trips_live() {
        let cube = "LUT_3D_SIZE 2\n0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n";
        let mut p = Params {
            lut: Some(onepaint_filters::LutFile { format: "cube".into(), name: "Swap.CUBE".into(), text: cube.into() }),
            ..Default::default()
        };
        p.extra.insert("lutPsd".into(), serde_json::json!({ "dither": false, "dataOrder": "bgr" }));
        let a = Adjustment { kind: "color-lookup".into(), params: p };
        let (key, block) = write(&a).unwrap();
        assert_eq!(&key, b"clrL");
        let back = read(&key, &block).unwrap();
        assert_eq!(back, a);
        assert!(!block.windows(7).any(|w| w == b"profile"), "no profile");
        // An abstract profile or an unreadable table stays a record.
        let mut d = crate::psd_desc::Desc::new("null");
        d.push("lookupType", crate::psd_desc::DVal::Enum("colorLookupType".into(), "abstractProfile".into()));
        let mut o = vec![0, 1, 0, 0, 0, 16];
        crate::psd_desc::write_into(&mut o, &d);
        assert!(read(b"clrL", &o).is_none());
        let mut bad = a.clone();
        bad.params.lut.as_mut().unwrap().text = "nonsense".into();
        assert!(read(b"clrL", &write(&bad).unwrap().1).is_none());
    }

    #[test]
    fn photo_filter_colours_like_adjust_blocks_test() {
        // v3: D50 white is white.
        let w = [0.9642, 1.0, 0.8249].map(|v: f64| (v * 32768.0).round() as u32);
        let mut d = vec![0, 3];
        for v in w {
            d.extend(v.to_be_bytes());
        }
        d.extend(25u32.to_be_bytes());
        d.push(1);
        let p = photo_filter(&d).unwrap();
        assert!(close3(p.filter_color.unwrap(), [1.0; 3], 1.5 / 255.0), "{:?}", p.filter_color);
        assert_eq!((p.filter_density, p.preserve_lum), (Some(0.25), Some(true)));
        // The real CS2 block: a warm yellow, 63 %.
        let cs2 = [0, 3, 0, 0, 0x5e, 0x7f, 0, 0, 0x61, 0xaa, 0, 0, 0x0e, 0xf3, 0, 0, 0, 0x3f, 1, 0];
        let p = photo_filter(&cs2).unwrap();
        let c = p.filter_color.unwrap();
        assert!((c[0] - 1.0).abs() < 0.005 && c[1] > 0.8 && c[2] < 0.35, "{c:?}");
        assert_eq!(p.filter_density, Some(0.63));
        // Not a colour.
        let mut bad = vec![0, 3];
        bad.extend((24191u32 * 100).to_be_bytes());
        bad.extend([0; 13]);
        assert!(photo_filter(&bad).is_none());
        // v2 RGB #EC8A00, 40 %, luminosity off.
        let v2 = [0, 2, 0, 0, 0xec, 0xec, 0x8a, 0x8a, 0, 0, 0, 0, 0, 0, 0, 40, 0];
        let p = photo_filter(&v2).unwrap();
        assert!(close3(p.filter_color.unwrap(), [236.0 / 255.0, 138.0 / 255.0, 0.0], 1e-4));
        assert_eq!((p.filter_density, p.preserve_lum), (Some(0.4), Some(false)));
        // v2 grey 0 % ink is white; Lab 100/0/0 is white.
        let grey = [0, 2, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 25, 1];
        assert!(close3(photo_filter(&grey).unwrap().filter_color.unwrap(), [1.0; 3], 1e-6));
        assert!(close3(lab_to_srgb(100.0, 0.0, 0.0), [1.0; 3], 0.01));
    }

    #[test]
    fn the_four_kinds_round_trip_like_psd_map_test() {
        let rt = |kind: &str, p: Params| {
            let a = Adjustment { kind: kind.into(), params: p };
            let (key, data) = write(&a).unwrap();
            read(&key, &data).unwrap().params
        };
        let e = rt("exposure", Params { exposure: Some(1.5), exp_offset: Some(-0.1), exp_gamma: Some(0.8), ..Default::default() });
        assert_eq!((e.exposure, e.exp_offset, e.exp_gamma), (Some(1.5), Some(-0.1), Some(0.8)));
        let v = rt("vibrance", Params { vibrance: Some(0.35), vib_saturation: Some(-0.2), ..Default::default() });
        assert_eq!((v.vibrance, v.vib_saturation), (Some(0.35), Some(-0.2)));
        let f = rt(
            "photo-filter",
            Params { filter_color: Some([1.0, 0.5, 0.0]), filter_density: Some(0.4), preserve_lum: Some(false), ..Default::default() },
        );
        assert!((f.filter_color.unwrap()[1] - 0.5).abs() < 0.003);
        assert_eq!((f.filter_density, f.preserve_lum), (Some(0.4), Some(false)));
        let base = onepaint_filters::default_params(onepaint_filters::Kind::ChannelMixer);
        let m = rt("channel-mixer", Params { mix_rg: Some(0.25), mix_bc: Some(-0.1), mix_kb: Some(0.5), ..base.clone() });
        assert_eq!((m.mix_rg, m.mix_bc, m.mix_kb, m.mix_rr, m.mix_mono), (Some(0.25), Some(-0.1), Some(0.5), Some(1.0), Some(false)));
        let mono = rt("channel-mixer", Params { mix_mono: Some(true), mix_kr: Some(0.3), mix_kg: Some(0.6), mix_kb: Some(0.1), ..base });
        assert_eq!((mono.mix_mono, mono.mix_kr, mono.mix_kg, mono.mix_kb), (Some(true), Some(0.3), Some(0.6), Some(0.1)));
    }

    #[test]
    fn the_older_kinds_map_like_psd_map_test() {
        use onepaint_stroke::CurvePoint;
        let rt = |kind: &str, p: Params| {
            let (key, data) = write(&Adjustment { kind: kind.into(), params: p }).unwrap();
            read(&key, &data).unwrap()
        };
        // happypaint psd-map.test.ts: Selective Color in percent, Absolute kept.
        let mut sc = vec![0.0; 36];
        (sc[0], sc[7], sc[35]) = (-0.25, 0.6, -1.0);
        let s = rt("cmyk-selective-color", Params { sc_cmyk: Some(sc.clone()), sc_method: Some("absolute".into()), ..Default::default() });
        assert_eq!(
            (s.kind.as_str(), s.params.sc_cmyk, s.params.sc_method.as_deref()),
            ("cmyk-selective-color", Some(sc), Some("absolute"))
        );
        let h =
            rt("hue-saturation", Params { hue: Some(40.0), saturation: Some(0.3), brightness: Some(-0.2), ..Default::default() }).params;
        assert_eq!((h.hue, h.saturation, h.brightness), (Some(40.0), Some(0.3), Some(-0.2)));
        let l = Params {
            in_black: Some(0.1),
            in_white: Some(0.9),
            gamma: Some(1.3),
            out_black: Some(0.05),
            out_white: Some(0.95),
            ..Default::default()
        };
        let (_, data) = write(&Adjustment { kind: "levels".into(), params: l.clone() }).unwrap();
        // 26, 230, 13, 242 and gamma 130 (psd-map.test.ts).
        assert_eq!(&data[2..12], &[0, 26, 0, 230, 0, 13, 0, 242, 0, 130]);
        assert_eq!(data.len(), 632);
        let l2 = read(b"levl", &data).unwrap().params;
        assert_eq!(l2.gamma, Some(1.3));
        assert!((l2.in_black.unwrap() - 26.0 / 255.0).abs() < 1e-6);
        let c = Params {
            curve: Some(vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(0.4, 0.6), CurvePoint::new(1.0, 1.0)]),
            curve_r: Some(vec![CurvePoint::new(0.0, 0.1), CurvePoint::new(1.0, 0.9)]),
            ..Default::default()
        };
        let c2 = rt("curves", c).params;
        let pts: Vec<_> = c2.curve.unwrap().iter().map(|q| ((q.x * 255.0).round(), (q.y * 255.0).round())).collect();
        assert_eq!(pts, vec![(0.0, 0.0), (102.0, 153.0), (255.0, 255.0)]);
        assert!(c2.curve_r.is_some() && c2.curve_g.is_none());
        assert_eq!(c2.curve_mode.as_deref(), Some("smooth"));
        let b = rt("color-balance", Params { yellow_blue: Some(0.45), magenta_green: Some(-0.3), ..Default::default() }).params;
        assert_eq!((b.cyan_red, b.magenta_green, b.yellow_blue), (Some(0.0), Some(-0.3), Some(0.45)));
        // happypaint psd-map.test.ts: all three ranges and Preserve Luminosity.
        let cb = Params {
            cb_shadow_cr: Some(-0.3),
            yellow_blue: Some(0.45),
            cb_high_mg: Some(1.0),
            preserve_lum: Some(false),
            ..Default::default()
        };
        let b = rt("color-balance", cb).params;
        assert_eq!((b.cb_shadow_cr, b.yellow_blue, b.cb_high_mg, b.preserve_lum), (Some(-0.3), Some(0.45), Some(1.0), Some(false)));
        assert_eq!(rt("color-balance", Params::default()).params.preserve_lum, Some(true), "on unless off");
        assert_eq!(rt("black-white", Params { amount: Some(0.5), ..Default::default() }).kind, "black-white");
        // happypaint psd-map.test.ts: weights in percent and the tint.
        let bw = Params {
            bw_reds: Some(-0.5),
            bw_magentas: Some(2.5),
            bw_tint: Some(true),
            bw_tint_color: Some([1.0, 0.8, 0.6]),
            ..Default::default()
        };
        let w = rt("black-white", bw).params;
        assert_eq!((w.bw_reds, w.bw_yellows, w.bw_magentas, w.bw_tint), (Some(-0.5), Some(0.6), Some(2.5), Some(true)));
        assert!((w.bw_tint_color.unwrap()[1] - 0.8).abs() < 1e-3);
        assert_eq!(w.amount, Some(1.0), "Photoshop has no amount");
        let stops = vec![
            onepaint_filters::Stop { pos: 0.0, color: [0.0, 0.0, 0.2] },
            onepaint_filters::Stop { pos: 0.6, color: [1.0, 0.5, 0.0] },
            onepaint_filters::Stop { pos: 1.0, color: [1.0; 3] },
        ];
        let g = rt("gradient-map", Params { gradient_stops: Some(stops.clone()), ..Default::default() }).params.gradient_stops.unwrap();
        for (a, b) in g.iter().zip(&stops) {
            assert!((a.pos - b.pos).abs() < 1e-3 && close3(a.color, b.color, 1.0 / 255.0), "{a:?} {b:?}");
        }
        // The black & white descriptor reads back as Photoshop's weights.
        let (_, bw) = write(&Adjustment { kind: "black-white".into(), params: Params::default() }).unwrap();
        let items = descriptor_numbers(&bw).unwrap();
        assert_eq!(items.iter().take(6).map(|i| i.1).collect::<Vec<_>>(), vec![40.0, 60.0, 40.0, 60.0, 20.0, 80.0]);
        let d = black_white(&bw).unwrap();
        let def = onepaint_filters::default_params(onepaint_filters::Kind::BlackWhite);
        assert_eq!((d.bw_reds, d.bw_magentas, d.bw_tint), (def.bw_reds, def.bw_magentas, Some(false)));
    }
}
