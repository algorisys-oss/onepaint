//! Flattened CMYK TIFF for print hand-off (happypaint `utils/tiff-cmyk.ts`):
//! little-endian, uncompressed, chunky Separated/InkSet 1, 8 bits per ink,
//! one strip, document DPI in the resolution tags. 0 = paper, 255 = solid.

/// Encode straight RGBA8 already flattened onto white (separated through the
/// built-in press) as a CMYK TIFF.
pub fn encode_cmyk_tiff(rgba: &[u8], width: u32, height: u32, dpi: f64) -> Vec<u8> {
    encode_separated(&onepaint_color::cmyk::separate_rgba8(rgba), width, height, dpi)
}

/// Encode pre-separated CMYK bytes.
pub fn encode_separated(cmyk: &[u8], width: u32, height: u32, dpi: f64) -> Vec<u8> {
    const SHORT: u16 = 3;
    const LONG: u16 = 4;
    const RATIONAL: u16 = 5;
    const IFD: usize = 8;
    const N: usize = 14;
    let res_off = IFD + 2 + N * 12 + 4;
    let bits_off = res_off + 16;
    let strip_off = bits_off + 8;
    let d = dpi.round().clamp(1.0, 65535.0) as u32;
    let entries: [(u16, u16, u32, u32); N] = [
        (256, LONG, 1, width),
        (257, LONG, 1, height),
        (258, SHORT, 4, bits_off as u32),
        (259, SHORT, 1, 1),
        (262, SHORT, 1, 5),
        (273, LONG, 1, strip_off as u32),
        (277, SHORT, 1, 4),
        (278, LONG, 1, height),
        (279, LONG, 1, cmyk.len() as u32),
        (282, RATIONAL, 1, res_off as u32),
        (283, RATIONAL, 1, res_off as u32 + 8),
        (284, SHORT, 1, 1),
        (296, SHORT, 1, 2),
        (332, SHORT, 1, 1),
    ];
    let mut out = Vec::with_capacity(strip_off + cmyk.len());
    out.extend(b"II");
    out.extend(42u16.to_le_bytes());
    out.extend((IFD as u32).to_le_bytes());
    out.extend((N as u16).to_le_bytes());
    for (tag, ty, count, value) in entries {
        out.extend(tag.to_le_bytes());
        out.extend(ty.to_le_bytes());
        out.extend(count.to_le_bytes());
        if ty == SHORT && count == 1 {
            out.extend((value as u16).to_le_bytes());
            out.extend([0, 0]);
        } else {
            out.extend(value.to_le_bytes());
        }
    }
    out.extend(0u32.to_le_bytes());
    for v in [d, 1, d, 1] {
        out.extend(v.to_le_bytes());
    }
    for _ in 0..4 {
        out.extend(8u16.to_le_bytes());
    }
    out.extend(cmyk);
    out
}
