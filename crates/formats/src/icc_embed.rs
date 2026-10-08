//! Tag an encoded PNG or JPEG with an ICC profile (happypaint
//! `utils/image-icc.ts`): the container is patched, pixels are untouched,
//! and anything that does not parse is returned unchanged.
//!
//! PNG gets an `iCCP` chunk right after IHDR; any `iCCP`/`sRGB` chunk already
//! there is dropped (the spec forbids both). JPEG gets one APP2
//! `ICC_PROFILE` segment after the leading APP0/APP1 segments.

use std::io::Write;

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// `bytes` tagged with `profile` (named `name`, the PNG keyword).
pub fn embed_icc(bytes: &[u8], profile: &[u8], name: &str) -> Vec<u8> {
    let tagged = if bytes.starts_with(&PNG_SIG) {
        png_icc(bytes, profile, name)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        jpeg_icc(bytes, profile)
    } else {
        None
    };
    tagged.unwrap_or_else(|| bytes.to_vec())
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut c = Vec::with_capacity(12 + data.len());
    c.extend((data.len() as u32).to_be_bytes());
    c.extend(kind);
    c.extend(data);
    let mut h = flate2::Crc::new();
    h.update(kind);
    h.update(data);
    c.extend(h.sum().to_be_bytes());
    c
}

fn png_icc(bytes: &[u8], profile: &[u8], name: &str) -> Option<Vec<u8>> {
    // Latin-1 keyword, 1..79 bytes.
    let mut data: Vec<u8> = name.chars().filter(|c| (' '..='~').contains(c)).take(79).map(|c| c as u8).collect();
    if data.is_empty() {
        data.push(b'_');
    }
    data.extend([0, 0]); // separator, compression method 0
    let mut z = flate2::write::ZlibEncoder::new(data, flate2::Compression::default());
    z.write_all(profile).ok()?;
    let iccp = png_chunk(b"iCCP", &z.finish().ok()?);

    let mut out = bytes[..8].to_vec();
    let mut off = 8;
    while off + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[off..off + 4].try_into().ok()?) as usize;
        let end = off.checked_add(12 + len).filter(|&e| e <= bytes.len())?;
        let kind = &bytes[off + 4..off + 8];
        if kind != b"iCCP" && kind != b"sRGB" {
            out.extend(&bytes[off..end]);
            if kind == b"IHDR" {
                out.extend(&iccp);
            }
        }
        off = end;
    }
    (off == bytes.len()).then_some(out)
}

fn jpeg_icc(bytes: &[u8], profile: &[u8]) -> Option<Vec<u8>> {
    let len = 2 + 12 + 2 + profile.len();
    let len = u16::try_from(len).ok()?; // one segment holds < 64 KB
    let mut seg = vec![0xff, 0xe2];
    seg.extend(len.to_be_bytes());
    seg.extend(b"ICC_PROFILE\0");
    seg.extend([1, 1]); // sequence 1 of 1
    seg.extend(profile);
    let mut off = 2;
    while off + 4 <= bytes.len() && bytes[off] == 0xff && matches!(bytes[off + 1], 0xe0 | 0xe1) {
        off += 2 + u16::from_be_bytes([bytes[off + 2], bytes[off + 3]]) as usize;
    }
    let off = off.min(bytes.len());
    let mut out = Vec::with_capacity(bytes.len() + seg.len());
    out.extend(&bytes[..off]);
    out.extend(seg);
    out.extend(&bytes[off..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use onepaint_color::icc::{RgbSpace, build_icc_profile};

    fn rgba() -> Vec<u8> {
        (0..16 * 8).flat_map(|i| [i as u8, 255 - i as u8, 40, 255]).collect()
    }

    #[test]
    fn png_gets_one_iccp_after_ihdr_and_still_decodes() {
        let mut png = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png, 16, 8);
            e.set_color(png::ColorType::Rgba);
            e.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            e.write_header().unwrap().write_image_data(&rgba()).unwrap();
        }
        let prof = build_icc_profile(RgbSpace::DisplayP3);
        let out = embed_icc(&png, &prof, RgbSpace::DisplayP3.name());
        assert_eq!(&out[37..41], b"iCCP", "right after IHDR");
        assert!(!out.windows(4).any(|w| w == b"sRGB"));
        let d = png::Decoder::new(std::io::Cursor::new(out)).read_info().unwrap();
        assert_eq!(d.info().icc_profile.as_deref(), Some(&prof[..]));
        // Garbage is passed through.
        assert_eq!(embed_icc(b"nope", &prof, "x"), b"nope");
    }

    #[test]
    fn jpeg_gets_an_app2_after_jfif() {
        let mut jpg = Vec::new();
        let rgb: Vec<u8> = rgba().chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        image::codecs::jpeg::JpegEncoder::new(&mut jpg).encode(&rgb, 16, 8, image::ExtendedColorType::Rgb8).unwrap();
        let prof = build_icc_profile(RgbSpace::Srgb);
        let out = embed_icc(&jpg, &prof, "");
        assert_eq!(&out[2..4], [0xff, 0xe0], "JFIF stays first");
        use image::ImageDecoder;
        let mut d = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&out)).unwrap();
        assert_eq!(d.icc_profile().unwrap().as_deref(), Some(&prof[..]));
    }
}
