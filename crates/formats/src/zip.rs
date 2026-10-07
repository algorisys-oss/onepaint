//! Minimal zip: read stored/deflated entries, write stored ones. Enough for
//! Procreate `.swatches` (and, later, `.brushset`).

use crate::FormatError;
use std::io::Read;

fn bad(s: &str) -> FormatError {
    FormatError::Corrupt(format!("zip: {s}"))
}

fn u16_at(b: &[u8], p: usize) -> Result<u16, FormatError> {
    b.get(p..p + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| bad("truncated"))
}

fn u32_at(b: &[u8], p: usize) -> Result<u32, FormatError> {
    b.get(p..p + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| bad("truncated"))
}

/// All file entries as (name, bytes), via the central directory.
pub fn read_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, FormatError> {
    let eocd = (0..bytes.len().saturating_sub(21))
        .rev()
        .find(|&p| bytes[p..p + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| bad("no end record"))?;
    let count = u16_at(bytes, eocd + 10)? as usize;
    let mut p = u32_at(bytes, eocd + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(bytes, p)? != 0x0201_4b50 {
            return Err(bad("bad central directory"));
        }
        let method = u16_at(bytes, p + 10)?;
        let csize = u32_at(bytes, p + 20)? as usize;
        let usize_ = u32_at(bytes, p + 24)? as usize;
        let (nl, el, cl) = (u16_at(bytes, p + 28)? as usize, u16_at(bytes, p + 30)? as usize, u16_at(bytes, p + 32)? as usize);
        let local = u32_at(bytes, p + 42)? as usize;
        let name = String::from_utf8_lossy(bytes.get(p + 46..p + 46 + nl).ok_or_else(|| bad("name"))?).into_owned();
        p += 46 + nl + el + cl;
        if name.ends_with('/') {
            continue;
        }
        if u32_at(bytes, local)? != 0x0403_4b50 {
            return Err(bad("bad local header"));
        }
        let start = local + 30 + u16_at(bytes, local + 26)? as usize + u16_at(bytes, local + 28)? as usize;
        let data = bytes.get(start..start + csize).ok_or_else(|| bad("entry data"))?;
        let body = match method {
            0 => data.to_vec(),
            8 => {
                let mut v = Vec::with_capacity(usize_);
                flate2::read::DeflateDecoder::new(data).read_to_end(&mut v)?;
                v
            }
            _ => return Err(bad("unsupported compression")),
        };
        out.push((name, body));
    }
    Ok(out)
}

/// A zip of stored (uncompressed) entries.
pub fn write_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let (crc, len, off) = (crc.sum(), data.len() as u32, out.len() as u32);
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&20u16.to_le_bytes()); // version needed
            v.extend_from_slice(&0u16.to_le_bytes()); // flags
            v.extend_from_slice(&0u16.to_le_bytes()); // stored
            v.extend_from_slice(&0u32.to_le_bytes()); // time, date
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&len.to_le_bytes());
            v.extend_from_slice(&len.to_le_bytes());
            v.extend_from_slice(&(name.len() as u16).to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // extra
        };
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        common(&mut central);
        central.extend_from_slice(&[0u8; 6]); // comment len, disk, internal attrs
        central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central.extend_from_slice(&off.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let (cd_off, cd_len) = (out.len() as u32, central.len() as u32);
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_len.to_le_bytes());
    out.extend_from_slice(&cd_off.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
