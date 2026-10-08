# 32: CMYK soft-proof, value check and CMYK TIFF (slice 20)

The sources are `engine/color/cmyk.ts`, `engine/color/press-profile.ts`,
`utils/tiff-cmyk.ts`, `engine/compositor.ts:197-261` and
`components/view-menu/view-menu.tsx:95-135`, relative to
`../happypaint/frontend/src/`.

## Press model (`color::cmyk`)
- **Print:** paper × Π inks, in linear light. Each ink's factor is
  `1 + dotGain(a)·(ink − 1)`, with `dotGain(a) = a + 4·0.13·a(1−a)`.
- **Separation (`separate_rgb`):**
  - Skeleton black: `K = 0.9·(grey − 0.35)/0.65` above a grey of 0.35.
  - Newton on C, M, Y against the ratio residual (Cramer 3×3, up to 24
    iterations), seeded naïvely or from the previous grid point.
  - A 300% total-ink limit.
- **`builtin_press()`:** baked once, from 33³ grids with r varying fastest.
  - The separation LUT holds CMYK bytes.
  - The proof LUT holds the printed sRGB and, in its alpha, ΔE76 (Lab D50)
    × 8 against the media-relative print.
  - It also stores the printed white and black.
- **Goldens:** `crates/color/tests/fixtures/cmyk.json`, exported with
  `scripts/cmyk-goldens/make.ts`.
  - The separations and prints match to 1e-9.
  - The LUT bytes match to ±1.

## View modes (`ViewModes`, `apply_view`)
- **Present only:** they apply to composited tiles before upload. Documents
  and exports are untouched.
- **Grayscale (Ctrl+Y):** Rec.709 luma of the encoded colour. Redo stays on
  Ctrl+Shift+Z, as in happypaint.
- **Print preview:**
  - The colour is placed on white and looked up in the proof LUT
    (trilinear).
  - It is normalised from the press's black and white to the display's,
    unless "Simulate paper white" or "Simulate black ink" is on.
  - The result is opaque.
- **Non-printable colours:** Off, ΔE > 5 or ΔE > 2. It is independent of the
  proof. Flagged pixels show flat red (0.78, 0.06, 0.10).
- **Where:** the guides popover, under View. Toggling re-presents every tile.

## CMYK TIFF (`formats::tiff_cmyk`)
- **Format:** little-endian, uncompressed and chunky, with Photometric 5,
  InkSet 1 and 8 bits per ink in one strip. The resolution is d/1 per inch.
  The bytes match happypaint's golden.
- **Export dialog:** TIFF has a "CMYK for print" option. The image is
  flattened on white, then separated through the active press. A loaded
  profile is embedded as InterColorProfile (tag 34675, UNDEFINED), with the
  strip word-aligned after it.

## Press profiles (`press_from_icc`, app `press.rs`)
- **Loading:** a CMYK output `.icc`/`.icm` (FOGRA39, SWOP, GRACoL …) is
  parsed with `moxcms`. Both LUTs are baked through the same `bake` as the
  built-in press, using relative colorimetric sRGB→CMYK and CMYK→sRGB. An RGB
  or unparseable profile is refused with a sentence.
- **Scope:** app-level, as in happypaint. One press drives the print
  preview, the gamut warning and the CMYK TIFF.
- **Persistence:** saved as `press.icc` beside `settings.json` and restored
  at launch. A file that no longer loads is deleted.
- **UI:** the guides popover's View section has "Press: <name>". Clicking it
  loads a profile, or clears it back to "Generic coated (built-in)".

## RGB profiles (`color::icc`, `formats::icc_embed`)
- **Generated profiles (`build_icc_profile`):** ICC v4.2 matrix/TRC
  profiles for sRGB and Display P3, built from `icc.ts`'s constants. The
  creation date is fixed, so the bytes are deterministic.
- **Export:** PNG gets an `iCCP` chunk right after IHDR, keyed by the
  profile name, and no `sRGB` chunk. JPEG gets one APP2 `ICC_PROFILE`
  segment after APP0/APP1. A Display-P3 document exports P3 values tagged P3
  (divergence 40). Netpbm and RGB TIFF are untagged.
- **Import:** an embedded RGB profile is converted to sRGB (relative
  colorimetric, alpha untouched), as the browser does when happypaint decodes
  an image. CMYK and grey profiles are left as the decoder gives them.
