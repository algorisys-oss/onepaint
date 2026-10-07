# 25: The export dialog (slice 17)

The source is `routes/editor.tsx:473, 640-720` (exportImage, exportPpm),
relative to `../happypaint/frontend/src/`.

## Formats
| Format | Notes |
|---|---|
| PNG | RGBA. DPI goes in the `pHYs` chunk (pixels per metre = `round(dpi/0.0254)`). |
| JPEG | Flattened onto white. Quality 1..100 (default 92). DPI goes in the JFIF density (dots per inch). |
| TIFF | RGBA, lossless. |
| PSD | Spec 15 (scale ignored: layers are exported at document size). |
| PPM | Netpbm P6 (binary) or P3 (ASCII), flattened onto white, `maxval` 255. |

## Scale
- 10..400% of the document. Output size is `max(1, round(w·s))`, and the
  same for the height.
- Resampling is bilinear on premultiplied pixels (Canvas2D's `drawImage`
  smoothing), so there are no dark fringes.

## Dialog
- File ▸ Export… (Ctrl+Shift+E) opens a dialog with:
  - a format picker;
  - scale;
  - quality (JPEG);
  - ASCII (PPM);
  - the output pixel size.
- Then a save dialog with the matching extension.
- The composite is what you see (the current animation frame when
  animating). This matches happypaint, which exports `compositeToCanvas`.

## Acceptance
- PNG at 300 dpi has `pHYs` = 11811 ppm (metre unit).
- JPEG at 144 dpi has density 144 (dpi units), and its transparent pixels
  are white.
- PPM P6 and P3 headers and bytes are exact for a 2×1 image.
- 50% of 4×2 gives 2×1. A half-transparent edge stays un-darkened.
- TIFF decodes back to the same pixels.
