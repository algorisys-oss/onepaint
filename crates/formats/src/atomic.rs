//! Replacing a file without ever leaving it half-written.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Write `bytes` to `path` so a crash, a full disk or a lost mount never
/// leaves a truncated file: the bytes go to a temp file beside it, are
/// flushed to the disk, and the temp file is renamed over `path` (atomic on
/// one filesystem). On failure the old file is untouched and the temp file
/// removed.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{}.{}-{n}.tmp", name.to_string_lossy(), std::process::id()));
    let written = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
        return written;
    }
    // The rename itself survives a power cut once the directory is synced.
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("onepaint-atomic-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap_or_default();
        d
    }

    #[test]
    fn replaces_the_file_and_leaves_no_temp_behind() {
        let d = dir("ok");
        let p = d.join("a.happypaint");
        fs::write(&p, b"old").unwrap_or_default();
        write_atomic(&p, b"new bytes").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(fs::read(&p).unwrap_or_default(), b"new bytes");
        assert_eq!(fs::read_dir(&d).map(|r| r.count()).unwrap_or(0), 1, "no temp file left");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_write_keeps_the_old_file() {
        let d = dir("fail");
        let p = d.join("a.happypaint");
        fs::write(&p, b"old").unwrap_or_default();
        // Renaming onto a directory fails after the temp file was written.
        let target = d.join("sub");
        fs::create_dir_all(target.join("x")).unwrap_or_default();
        assert!(write_atomic(&target, b"new").is_err());
        assert_eq!(fs::read(&p).unwrap_or_default(), b"old");
        assert_eq!(fs::read_dir(&d).map(|r| r.count()).unwrap_or(0), 2, "the temp file was removed");
        let _ = fs::remove_dir_all(&d);
    }
}
