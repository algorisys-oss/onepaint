#![allow(clippy::unwrap_used)]

#[test]
fn cmyk_tiff_matches_happypaint_bytes() {
    let g: serde_json::Value = serde_json::from_str(include_str!("../../color/tests/fixtures/cmyk.json")).unwrap();
    let want: Vec<u8> = g["tiff"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
    let rgba = [255u8, 0, 0, 255, 0, 128, 255, 255, 30, 200, 90, 255, 250, 250, 250, 255];
    let got = onepaint_formats::tiff_cmyk::encode_cmyk_tiff(&rgba, 2, 2, 300.0);
    assert_eq!(got.len(), want.len());
    let header = got.len() - 16;
    assert_eq!(got[..header], want[..header]);
    for (a, b) in got[header..].iter().zip(&want[header..]) {
        assert!((*a as i32 - *b as i32).abs() <= 1);
    }
}
