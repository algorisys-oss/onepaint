#![allow(clippy::unwrap_used)]
use onepaint_formats::book::*;

#[test]
fn page_names_are_padded_and_safe() {
    assert_eq!(page_file_name(0, "Cover"), "pages/0001 Cover.happypaint");
    assert_eq!(page_file_name(11, "  Page: 12/b!  "), "pages/0012 Page 12b.happypaint");
    assert_eq!(page_file_name(2, "???"), "pages/0003 Page.happypaint");
}

#[test]
fn books_round_trip_in_reading_order() {
    let pages = vec![b"one".to_vec(), b"two".to_vec(), b"three".to_vec()];
    let names: Vec<String> = ["Cover", "Page 2", "Page 3"].iter().map(|s| s.to_string()).collect();
    let bytes = encode_book("Chapter 1", &names, &pages, "onepaint 0.1").unwrap();
    let (m, back) = decode_book(&bytes).unwrap();
    assert_eq!(m.version, 1);
    assert_eq!(m.name, "Chapter 1");
    assert_eq!(m.layout, "pages");
    assert_eq!(back, pages);
    let json: serde_json::Value =
        serde_json::from_slice(&onepaint_formats::zip::read_zip(&bytes).unwrap().iter().find(|f| f.0 == "book.json").unwrap().1).unwrap();
    assert_eq!(json["appVersion"], "onepaint 0.1");
    assert_eq!(json["pages"][1]["file"], "pages/0002 Page 2.happypaint");
}

#[test]
fn a_book_missing_a_page_is_an_error() {
    let manifest = br#"{"version":1,"name":"x","layout":"pages","pages":[{"file":"pages/0001 A.happypaint","name":"A"}],"appVersion":"t"}"#;
    let z = onepaint_formats::zip::write_zip(&[("book.json", manifest)]);
    assert!(decode_book(&z).unwrap_err().to_string().contains("missing page"));
}
