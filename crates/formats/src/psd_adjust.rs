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

/// The `long` and `doub` items of a flat action descriptor (version 16), by key.
fn descriptor_numbers(d: &[u8]) -> Option<Vec<(String, f64)>> {
    if be32(d, 0)? != 16 {
        return None;
    }
    let mut at = 4;
    // The name (UTF-16 with a length), then the class id.
    at += 4 + be32(d, at)? as usize * 2;
    let id_len = be32(d, at)? as usize;
    at += 4 + if id_len == 0 { 4 } else { id_len };
    let n = be32(d, at)?;
    at += 4;
    let mut out = Vec::new();
    for _ in 0..n {
        let kl = be32(d, at)? as usize;
        let kl = if kl == 0 { 4 } else { kl };
        let key = String::from_utf8_lossy(d.get(at + 4..at + 4 + kl)?).into_owned();
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
            b"bool" => at += 1,
            // Anything else ends the read: nothing past it is needed here.
            _ => break,
        }
    }
    Some(out)
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
        _ => return None,
    };
    Some(Adjustment { kind: kind.into(), params })
}

/// The block for an adjustment layer of these kinds.
pub fn write(a: &Adjustment) -> Option<([u8; 4], Vec<u8>)> {
    let p = &a.params;
    let mut o = Vec::new();
    let pct = |v: f32| (v.clamp(-2.0, 2.0) * 100.0).round() as i16;
    let key = match a.kind.as_str() {
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
        _ => return None,
    };
    Some((key, o))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close3(a: [f32; 3], b: [f32; 3], eps: f32) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() <= eps)
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
}
