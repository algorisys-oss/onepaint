//! `.happypaint-book`: a chapter as a stored ZIP of `.happypaint` pages plus
//! `book.json` (happypaint `storage/happypaint-book.ts`).

use crate::FormatError;
use crate::zip::{read_zip, write_zip};
use serde::{Deserialize, Serialize};

pub const BOOK_EXT: &str = "happypaint-book";
const MANIFEST: &str = "book.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BookPage {
    pub file: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookManifest {
    pub version: u32,
    pub name: String,
    /// `pages` or `strip`.
    pub layout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_direction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starts_on: Option<String>,
    pub pages: Vec<BookPage>,
    pub app_version: String,
}

/// `pages/0001 Name.happypaint` (zero-padded so it sorts in reading order).
pub fn page_file_name(index: usize, name: &str) -> String {
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == ' ').collect();
    let safe = safe.trim();
    format!("pages/{:04} {}.happypaint", index + 1, if safe.is_empty() { "Page" } else { safe })
}

/// Pack pages (`.happypaint` bytes, in reading order) into a book.
pub fn encode_book(name: &str, page_names: &[String], pages: &[Vec<u8>], app_version: &str) -> Result<Vec<u8>, FormatError> {
    let manifest = BookManifest {
        version: 1,
        name: name.to_string(),
        layout: "pages".into(),
        reading_direction: None,
        starts_on: None,
        pages: page_names.iter().enumerate().map(|(i, n)| BookPage { file: page_file_name(i, n), name: n.clone() }).collect(),
        app_version: app_version.to_string(),
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| FormatError::Corrupt(e.to_string()))?;
    let mut files: Vec<(&str, &[u8])> = manifest.pages.iter().zip(pages).map(|(p, b)| (p.file.as_str(), b.as_slice())).collect();
    files.push((MANIFEST, &json));
    Ok(write_zip(&files))
}

/// Unpack a book: its manifest and the page files in reading order.
pub fn decode_book(bytes: &[u8]) -> Result<(BookManifest, Vec<Vec<u8>>), FormatError> {
    let files = read_zip(bytes)?;
    let get = |n: &str| files.iter().find(|f| f.0 == n).map(|f| f.1.clone());
    let manifest: BookManifest = serde_json::from_slice(&get(MANIFEST).ok_or_else(|| FormatError::Corrupt("book: no book.json".into()))?)
        .map_err(|e| FormatError::Corrupt(format!("book: {e}")))?;
    let pages = manifest
        .pages
        .iter()
        .map(|p| get(&p.file).ok_or_else(|| FormatError::Corrupt(format!("book: missing page {}", p.file))))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((manifest, pages))
}
