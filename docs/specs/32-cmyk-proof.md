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
  flattened on white, then separated through the built-in press.

## Not ported
- User ICC press profiles (`pickPressProfile`) and ICC embedding (tag 34675).
