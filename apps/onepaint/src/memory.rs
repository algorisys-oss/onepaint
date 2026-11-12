//! How much memory a drawing needs, and roughly how much this computer can
//! give it (happypaint `engine/memory-budget.ts`, review F15, v0.208.1).
//!
//! happypaint keeps every layer as a canvas-sized GPU texture. onepaint keeps
//! layers as sparse CPU tiles (the GPU holds the tiles it composites), so a
//! layer costs what is painted on it; a fully painted one costs a whole canvas.
//! The budget is half the computer's RAM, a guess good for a warning, not a
//! refusal.

use onepaint_doc::Depth;

pub const MIB: u64 = 1024 * 1024;
pub const GIB: u64 = 1024 * MIB;

/// Canvas-sized working buffers besides the layers: stroke, scratch and the
/// composite caches (happypaint `FIXED_CANVAS_TARGETS`).
pub const FIXED_CANVAS_TARGETS: u64 = 4;

/// Bytes a pixel takes. Float drawings, 32-bit included, are held in f16
/// (divergence 2); 8-bit drawings in bytes.
pub fn bytes_per_pixel(depth: Depth) -> u64 {
    match depth {
        Depth::Rgba8 => 4,
        Depth::Rgba16F | Depth::Rgba32F => 8,
    }
}

/// Bytes one fully painted canvas-sized layer takes.
pub fn layer_bytes(width: u32, height: u32, depth: Depth) -> u64 {
    width as u64 * height as u64 * bytes_per_pixel(depth)
}

/// The memory this computer can probably spare for one drawing: half its RAM,
/// else half of an assumed 4 GB.
pub fn budget_bytes(total_ram: Option<u64>) -> u64 {
    total_ram.filter(|&r| r > 0).unwrap_or(4 * GIB) / 2
}

/// This computer's budget, read once.
pub fn budget() -> u64 {
    static BUDGET: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        let ram = sysinfo::System::new().ok().map(|mut sys| {
            sys.refresh_memory();
            sys.total_memory()
        });
        budget_bytes(ram)
    })
}

/// How many fully painted layers fit beside the working buffers (may be ≤ 0).
pub fn layers_that_fit(width: u32, height: u32, depth: Depth, budget: u64) -> i64 {
    let per = layer_bytes(width, height, depth).max(1);
    (budget / per) as i64 - FIXED_CANVAS_TARGETS as i64
}

/// "128 MB", "1.5 GB".
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= GIB {
        let gb = bytes as f64 / GIB as f64;
        if bytes >= 10 * GIB { format!("{gb:.0} GB") } else { format!("{gb:.1} GB") }
    } else {
        format!("{} MB", (bytes as f64 / MIB as f64).round().max(1.0))
    }
}

/// A file's size: "340 KB", "12.4 MB", "1.2 GB".
pub fn format_file_size(bytes: u64) -> String {
    if bytes < MIB {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    } else if bytes < GIB {
        format!("{:.1} MB", bytes as f64 / MIB as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    }
}

/// The Storage row of Document Properties (happypaint's stored size,
/// v0.208.2, review F25): the saved file's real size, and the recovery copy.
pub fn storage_text(file: Option<u64>, unsaved: bool, copy: Option<u64>) -> String {
    let mut out = match file {
        Some(n) if unsaved => format!("The saved file is {}; unsaved changes aren't in it yet.", format_file_size(n)),
        Some(n) => format!("The saved file is {}.", format_file_size(n)),
        None => "Not saved to a file yet.".to_string(),
    };
    if let Some(n) = copy {
        out.push_str(&format!(" Recovery copy: {}.", format_file_size(n)));
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// Fewer than 8 full layers fit.
    Tight,
    /// Not even 2.
    Over,
}

/// The one-line note where a canvas size is chosen.
pub fn size_advice(width: u32, height: u32, depth: Depth, budget: u64) -> (Level, String) {
    let fit = layers_that_fit(width, height, depth, budget);
    let level = if fit < 2 {
        Level::Over
    } else if fit < 8 {
        Level::Tight
    } else {
        Level::Ok
    };
    let per = format!("About {} of memory per fully painted layer", format_bytes(layer_bytes(width, height, depth)));
    let text = match level {
        Level::Over => format!("{per}: likely more than this computer can hold. Try a smaller size or a lower bit depth."),
        Level::Tight => format!("{per}; only about {fit} such layers are likely to fit on this computer."),
        Level::Ok if fit > 99 => format!("{per}; 100+ such layers fit on this computer."),
        Level::Ok => format!("{per}; about {fit} such layers fit on this computer."),
    };
    (level, text)
}

/// A drawing's memory now: its painted tiles and undo, plus the working
/// buffers. True when it is past the budget.
pub fn document_memory(width: u32, height: u32, depth: Depth, held: u64, budget: u64) -> (bool, String) {
    let bytes = held + FIXED_CANVAS_TARGETS * layer_bytes(width, height, depth);
    let over = bytes > budget;
    let text = if over {
        format!("About {} of memory: more than this computer is likely to spare (about {}).", format_bytes(bytes), format_bytes(budget))
    } else {
        format!("About {} of memory, of about {} this computer can spare.", format_bytes(bytes), format_bytes(budget))
    };
    (over, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    // memory-budget.test.ts, with onepaint's pixel sizes.
    #[test]
    fn sizes_and_budget() {
        assert_eq!(layer_bytes(4096, 4096, Depth::Rgba16F), 128 * MIB);
        assert_eq!(layer_bytes(4096, 4096, Depth::Rgba32F), 128 * MIB, "32-bit is held in f16");
        assert_eq!(layer_bytes(4096, 4096, Depth::Rgba8), 64 * MIB);
        assert_eq!(budget_bytes(Some(16 * GIB)), 8 * GIB);
        assert_eq!(budget_bytes(None), 2 * GIB);
        assert_eq!(budget_bytes(Some(0)), 2 * GIB);
        assert_eq!(layers_that_fit(4096, 4096, Depth::Rgba16F, 2 * GIB), 12);
    }

    #[test]
    fn advice_levels() {
        let (l, t) = size_advice(2048, 2048, Depth::Rgba16F, 8 * GIB);
        assert_eq!(l, Level::Ok);
        assert!(t.contains("32 MB") && t.contains("100+"), "{t}");
        assert_eq!(size_advice(8192, 8192, Depth::Rgba16F, 2 * GIB).0, Level::Over);
        let (l, t) = size_advice(4096, 4096, Depth::Rgba16F, GIB);
        assert_eq!(l, Level::Tight, "{t}");
        assert!(t.contains("only about 4 such layers"), "{t}");
    }

    #[test]
    fn formats_bytes() {
        assert_eq!(format_bytes(128 * MIB), "128 MB");
        assert_eq!(format_bytes(GIB + GIB / 2), "1.5 GB");
        assert_eq!(format_bytes(12 * GIB), "12 GB");
        assert_eq!(format_bytes(10), "1 MB");
    }

    #[test]
    fn storage_row() {
        assert_eq!(format_file_size(1), "1 KB");
        assert_eq!(format_file_size(340 * 1024), "340 KB");
        assert_eq!(format_file_size(12 * MIB + MIB / 2), "12.5 MB");
        assert_eq!(storage_text(None, true, None), "Not saved to a file yet.");
        assert_eq!(storage_text(Some(2048), false, None), "The saved file is 2 KB.");
        assert_eq!(
            storage_text(Some(2048), true, Some(4096)),
            "The saved file is 2 KB; unsaved changes aren't in it yet. Recovery copy: 4 KB."
        );
    }

    #[test]
    fn document_memory_counts_the_working_buffers() {
        let (over, text) = document_memory(1000, 1000, Depth::Rgba8, 0, GIB);
        assert!(!over);
        assert!(text.starts_with("About 15 MB"), "{text}");
        assert!(document_memory(1000, 1000, Depth::Rgba8, 2 * GIB, GIB).0);
    }
}
