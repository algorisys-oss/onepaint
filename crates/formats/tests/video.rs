#![allow(clippy::unwrap_used)]
use onepaint_formats::video::{Mp4Writer, encode_mp4};

/// A colour wipe moving one pixel a frame.
fn frame(w: u32, h: u32, f: u32) -> Vec<u8> {
    (0..h).flat_map(|y| (0..w).flat_map(move |x| if x < f { [200, 40, 30, 255] } else { [20, 90, (y * 8) as u8, 255] })).collect()
}

/// (offset, kind, size) of the top-level boxes.
fn boxes(b: &[u8]) -> Vec<(usize, [u8; 4], usize)> {
    let mut out = Vec::new();
    let mut off = 0;
    while off + 8 <= b.len() {
        let size = u32::from_be_bytes(b[off..off + 4].try_into().unwrap()) as usize;
        out.push((off, b[off + 4..off + 8].try_into().unwrap(), size));
        off += size;
    }
    assert_eq!(off, b.len());
    out
}

fn find(b: &[u8], kind: &[u8; 4]) -> usize {
    b.windows(4).position(|w| w == kind).unwrap() - 4
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes(b[i..i + 4].try_into().unwrap())
}

#[test]
fn an_mp4_is_fast_start_and_decodes_back() {
    let (w, h, n) = (33u32, 17u32, 24u32);
    let frames: Vec<Vec<u8>> = (0..n).map(|f| frame(w, h, f)).collect();
    let mp4 = encode_mp4(w, h, &frames, 12.0).unwrap();
    let top: Vec<[u8; 4]> = boxes(&mp4).iter().map(|b| b.1).collect();
    assert_eq!(top, [*b"ftyp", *b"moov", *b"mdat"]);

    // Sample table: n samples, the first a key frame, and the chunk offset
    // pointing into mdat.
    let stsz = find(&mp4, b"stsz");
    assert_eq!(u32_at(&mp4, stsz + 16), n);
    let sizes: Vec<usize> = (0..n as usize).map(|i| u32_at(&mp4, stsz + 20 + i * 4) as usize).collect();
    let stco = find(&mp4, b"stco");
    let base = u32_at(&mp4, stco + 16) as usize;
    let (mdat_off, _, mdat_size) = boxes(&mp4)[2];
    assert_eq!(base, mdat_off + 8);
    assert_eq!(sizes.iter().sum::<usize>(), mdat_size - 8);
    let tkhd = find(&mp4, b"tkhd");
    assert_eq!((u32_at(&mp4, tkhd + 84) >> 16, u32_at(&mp4, tkhd + 88) >> 16), (34, 18), "padded to even");

    // Decode: SPS/PPS from avcC, then each length-prefixed sample as Annex B.
    let avcc = find(&mp4, b"avcC") + 8;
    let sps_len = u16::from_be_bytes([mp4[avcc + 6], mp4[avcc + 7]]) as usize;
    let sps = &mp4[avcc + 8..avcc + 8 + sps_len];
    let p = avcc + 8 + sps_len + 1;
    let pps = &mp4[p + 2..p + 2 + u16::from_be_bytes([mp4[p], mp4[p + 1]]) as usize];
    let mut dec = openh264::decoder::Decoder::new().unwrap();
    let annexb = |nal: &[u8]| [&[0u8, 0, 0, 1][..], nal].concat();
    dec.decode(&[annexb(sps), annexb(pps)].concat()).unwrap();
    let mut off = base;
    let mut last = None;
    for s in sizes {
        let mut sample = &mp4[off..off + s];
        let mut stream = Vec::new();
        while !sample.is_empty() {
            let l = u32_at(sample, 0) as usize;
            stream.extend(annexb(&sample[4..4 + l]));
            sample = &sample[4 + l..];
        }
        if let Some(yuv) = dec.decode(&stream).unwrap() {
            let mut rgb = vec![0u8; 34 * 18 * 3];
            use openh264::formats::YUVSource;
            assert_eq!(yuv.dimensions(), (34, 18));
            yuv.write_rgb8(&mut rgb);
            last = Some(rgb);
        }
        off += s;
    }
    // The last frame: red left of x = 23, blue-green right of it.
    let rgb = last.unwrap();
    let px = |x: usize, y: usize| &rgb[(y * 34 + x) * 3..][..3];
    assert!(px(5, 8)[0] > 150 && px(5, 8)[1] < 90, "{:?}", px(5, 8));
    assert!(px(30, 8)[0] < 70 && px(30, 8)[1] > 50, "{:?}", px(30, 8));
}

#[test]
fn a_wrong_frame_size_is_an_error() {
    let mut w = Mp4Writer::new(8, 8, 30.0).unwrap();
    assert!(w.push(&[0; 10]).is_err());
    assert!(w.finish().is_err(), "no frames");
}
