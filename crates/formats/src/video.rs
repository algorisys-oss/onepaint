//! MP4 video (happypaint `storage/video-export.ts` `renderToMp4`): H.264
//! through OpenH264, 12 Mbit/s, a key frame every second, muxed in memory
//! with the `moov` box first ("fast start"), so it plays while downloading.

use crate::FormatError;
use openh264::OpenH264API;
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode};
use openh264::formats::{RgbaSliceU8, YUVBuffer};

/// Streams opaque RGBA8 frames into an MP4.
pub struct Mp4Writer {
    enc: Encoder,
    width: u32,
    height: u32,
    fps: f64,
    sps: Vec<u8>,
    pps: Vec<u8>,
    /// Length-prefixed sample data, sample sizes and key-frame flags.
    mdat: Vec<u8>,
    sizes: Vec<u32>,
    sync: Vec<bool>,
    scratch: Vec<u8>,
}

fn err(e: impl std::fmt::Display) -> FormatError {
    FormatError::Image(format!("H.264: {e}"))
}

/// H.264 4:2:0 wants even sizes; odd ones grow by one (the edge repeats).
pub fn even_size(w: u32, h: u32) -> (u32, u32) {
    (w.max(2).next_multiple_of(2), h.max(2).next_multiple_of(2))
}

impl Mp4Writer {
    /// Frames passed to `push` are `width`×`height` (any size; odd edges are
    /// padded to even).
    pub fn new(width: u32, height: u32, fps: f64) -> Result<Self, FormatError> {
        let fps = if fps.is_finite() { fps.clamp(1.0, 120.0) } else { 30.0 };
        let cfg = EncoderConfig::new()
            .bitrate(BitRate::from_bps(12_000_000))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps.round() as u32))
            .rate_control_mode(RateControlMode::Bufferbased)
            .skip_frames(false);
        let enc = Encoder::with_api_config(OpenH264API::from_source(), cfg).map_err(err)?;
        Ok(Self {
            enc,
            width,
            height,
            fps,
            sps: Vec::new(),
            pps: Vec::new(),
            mdat: Vec::new(),
            sizes: Vec::new(),
            sync: Vec::new(),
            scratch: Vec::new(),
        })
    }

    pub fn frames(&self) -> usize {
        self.sizes.len()
    }

    /// Encode one straight RGBA8 frame (alpha is ignored: flatten first).
    pub fn push(&mut self, rgba: &[u8]) -> Result<(), FormatError> {
        let (w, h) = (self.width as usize, self.height as usize);
        if rgba.len() != w * h * 4 {
            return Err(FormatError::Image("video frame size".into()));
        }
        let (ew, eh) = even_size(self.width, self.height);
        let (ew, eh) = (ew as usize, eh as usize);
        let src: &[u8] = if (ew, eh) == (w, h) {
            rgba
        } else {
            self.scratch.clear();
            for y in 0..eh {
                let row = &rgba[y.min(h - 1) * w * 4..][..w * 4];
                self.scratch.extend_from_slice(row);
                for _ in w..ew {
                    self.scratch.extend_from_slice(&row[(w - 1) * 4..]);
                }
            }
            &self.scratch
        };
        let yuv = YUVBuffer::from_rgb_source(RgbaSliceU8::new(src, (ew, eh)));
        let bs = self.enc.encode(&yuv).map_err(err)?;
        let start = self.mdat.len();
        let mut key = false;
        for l in 0..bs.num_layers() {
            let Some(layer) = bs.layer(l) else { continue };
            for i in 0..layer.nal_count() {
                let Some(nal) = layer.nal_unit(i) else { continue };
                let nal = strip_start_code(nal);
                let Some(&head) = nal.first() else { continue };
                match head & 0x1f {
                    7 => self.sps = nal.to_vec(),
                    8 => self.pps = nal.to_vec(),
                    t => {
                        key |= t == 5;
                        self.mdat.extend((nal.len() as u32).to_be_bytes());
                        self.mdat.extend(nal);
                    }
                }
            }
        }
        let size = (self.mdat.len() - start) as u32;
        if size > 0 {
            self.sizes.push(size);
            self.sync.push(key);
        }
        Ok(())
    }

    /// The finished file.
    pub fn finish(self) -> Result<Vec<u8>, FormatError> {
        if self.sizes.is_empty() || self.sps.len() < 4 || self.pps.is_empty() {
            return Err(FormatError::Image("no video frames".into()));
        }
        let (ew, eh) = even_size(self.width, self.height);
        let mux = Mux {
            width: ew,
            height: eh,
            timescale: (self.fps * 1000.0).round() as u32,
            delta: 1000,
            sps: &self.sps,
            pps: &self.pps,
            sizes: &self.sizes,
            sync: &self.sync,
        };
        Ok(mux.write(&self.mdat))
    }
}

fn strip_start_code(nal: &[u8]) -> &[u8] {
    if nal.starts_with(&[0, 0, 0, 1]) {
        &nal[4..]
    } else if nal.starts_with(&[0, 0, 1]) {
        &nal[3..]
    } else {
        nal
    }
}

/// Encode `frames` (opaque RGBA8 at `width`×`height`) at `fps`.
pub fn encode_mp4(width: u32, height: u32, frames: &[Vec<u8>], fps: f64) -> Result<Vec<u8>, FormatError> {
    let mut w = Mp4Writer::new(width, height, fps)?;
    for f in frames {
        w.push(f)?;
    }
    w.finish()
}

/// The MP4 boxes around an already-encoded AVC stream.
struct Mux<'a> {
    width: u32,
    height: u32,
    timescale: u32,
    delta: u32,
    sps: &'a [u8],
    pps: &'a [u8],
    sizes: &'a [u32],
    sync: &'a [bool],
}

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(8 + body.len());
    b.extend((8 + body.len() as u32).to_be_bytes());
    b.extend(kind);
    b.extend(body);
    b
}

/// A full box: version 0 and `flags`.
fn fbx(kind: &[u8; 4], flags: u32, body: &[u8]) -> Vec<u8> {
    let mut v = flags.to_be_bytes().to_vec();
    v[0] = 0;
    v.extend(body);
    bx(kind, &v)
}

fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// The unity transform matrix (16.16 / 2.30).
const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

impl Mux<'_> {
    fn write(&self, mdat: &[u8]) -> Vec<u8> {
        let ftyp = bx(b"ftyp", &[b"isom".as_slice(), &be32(0x200), b"isom", b"iso2", b"avc1", b"mp41"].concat());
        // The chunk offset depends on the moov size, which does not depend on
        // the offset's value: write once to measure, once for real.
        let moov_len = self.moov(0).len();
        let large = mdat.len() as u64 + 8 > u32::MAX as u64;
        let header = if large { 16 } else { 8 };
        let offset = (ftyp.len() + moov_len + header) as u32;
        let mut out = ftyp;
        out.extend(self.moov(offset));
        if large {
            out.extend(1u32.to_be_bytes());
            out.extend(b"mdat");
            out.extend((mdat.len() as u64 + 16).to_be_bytes());
        } else {
            out.extend((mdat.len() as u32 + 8).to_be_bytes());
            out.extend(b"mdat");
        }
        out.extend(mdat);
        out
    }

    fn moov(&self, offset: u32) -> Vec<u8> {
        let n = self.sizes.len() as u32;
        let dur = n * self.delta;
        let movie_dur = (dur as u64 * 1000 / self.timescale.max(1) as u64) as u32;
        let mut mvhd = Vec::new();
        mvhd.extend([be32(0), be32(0), be32(1000), be32(movie_dur)].concat());
        mvhd.extend(be32(0x0001_0000)); // rate 1.0
        mvhd.extend([1, 0]); // volume 1.0
        mvhd.extend([0u8; 10]);
        MATRIX.iter().for_each(|m| mvhd.extend(be32(*m)));
        mvhd.extend([0u8; 24]);
        mvhd.extend(be32(2)); // next track id

        let mut tkhd = Vec::new();
        tkhd.extend([be32(0), be32(0), be32(1), be32(0), be32(movie_dur)].concat());
        tkhd.extend([0u8; 8]);
        tkhd.extend([0u8; 8]); // layer, group, volume, reserved
        MATRIX.iter().for_each(|m| tkhd.extend(be32(*m)));
        tkhd.extend(be32(self.width << 16));
        tkhd.extend(be32(self.height << 16));

        let mut mdhd = Vec::new();
        mdhd.extend([be32(0), be32(0), be32(self.timescale), be32(dur)].concat());
        mdhd.extend([0x55, 0xc4, 0, 0]); // "und"

        let hdlr = fbx(b"hdlr", 0, &[be32(0).as_slice(), b"vide", &[0u8; 12], b"VideoHandler\0"].concat());
        let vmhd = fbx(b"vmhd", 1, &[0u8; 8]);
        let dinf = bx(b"dinf", &fbx(b"dref", 0, &[be32(1).as_slice(), &fbx(b"url ", 1, &[])].concat()));

        let mut avcc = vec![1, self.sps[1], self.sps[2], self.sps[3], 0xff, 0xe1];
        avcc.extend((self.sps.len() as u16).to_be_bytes());
        avcc.extend(self.sps);
        avcc.push(1);
        avcc.extend((self.pps.len() as u16).to_be_bytes());
        avcc.extend(self.pps);
        let mut avc1 = vec![0u8; 6];
        avc1.extend([0, 1]); // data reference index
        avc1.extend([0u8; 16]);
        avc1.extend((self.width as u16).to_be_bytes());
        avc1.extend((self.height as u16).to_be_bytes());
        avc1.extend([be32(0x0048_0000), be32(0x0048_0000), be32(0)].concat()); // 72 dpi
        avc1.extend([0, 1]); // frame count
        avc1.extend([0u8; 32]); // compressor name
        avc1.extend([0, 0x18, 0xff, 0xff]); // depth 24, pre-defined −1
        avc1.extend(bx(b"avcC", &avcc));
        let stsd = fbx(b"stsd", 0, &[be32(1).as_slice(), &bx(b"avc1", &avc1)].concat());

        let stts = fbx(b"stts", 0, &[be32(1), be32(n), be32(self.delta)].concat());
        let keys: Vec<u32> = self.sync.iter().enumerate().filter(|(_, k)| **k).map(|(i, _)| i as u32 + 1).collect();
        let mut stss = be32(keys.len() as u32).to_vec();
        keys.iter().for_each(|k| stss.extend(be32(*k)));
        let stsc = fbx(b"stsc", 0, &[be32(1), be32(1), be32(n), be32(1)].concat());
        let mut stsz = [be32(0), be32(n)].concat();
        self.sizes.iter().for_each(|s| stsz.extend(be32(*s)));
        let stco = fbx(b"stco", 0, &[be32(1), be32(offset)].concat());
        let mut stbl = [stsd, stts].concat();
        if keys.len() < self.sizes.len() {
            stbl.extend(fbx(b"stss", 0, &stss));
        }
        stbl.extend([stsc, fbx(b"stsz", 0, &stsz), stco].concat());

        let minf = bx(b"minf", &[vmhd, dinf, bx(b"stbl", &stbl)].concat());
        let mdia = bx(b"mdia", &[fbx(b"mdhd", 0, &mdhd), hdlr, minf].concat());
        let trak = bx(b"trak", &[fbx(b"tkhd", 3, &tkhd), mdia].concat());
        bx(b"moov", &[fbx(b"mvhd", 0, &mvhd), trak].concat())
    }
}
