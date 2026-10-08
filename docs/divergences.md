# Deliberate divergences from happypaint

Each entry says what differs, why, and when it goes away. Keep this list
short by fixing entries, not by growing it.

| # | Area | onepaint | happypaint | Why / when it goes away |
|---|---|---|---|---|
| 1 | Blend mode `pigment` | Composites as `normal` | Kubelka–Munk glaze (`engine/color/pigment.ts`) | The KM port is planned for slice 2+. |
| 2 | Document depth | Every document works internally in RGBA16F linear. 8-bit and 32F files open as 16F; their `depth` is kept on save. | Per-document 8-bit (gamma-space compositing, fixed multiply quirk) or 32F | 8-bit and 32F modes come in a later slice. |
| 3 | Off-canvas pixels | Dropped on save. Existing `overflow` chunks are passed through unchanged. | Saved as an `overflow` PNG | Planned with the move/transform tool. |
| 4 | Tile dedup key | PNG bytes | Hash of the GPU readback, shared between layer and mask tiles | Avoids a latent cross-namespace collision (see spec 05). Readers are unaffected. |
| 5 | Thumbnail | JPEG composited over white | JPEG from a canvas `toBlob` | It is cosmetic only. |
| 6 | Pen pressure on Linux | Velocity-synthesised pressure (winit exposes no tablet pressure on X11/Wayland) | Pointer Events pressure and tilt | A tablet backend (Wayland tablet-v2, XInput2, Windows Ink, macOS) is planned. |
| 7 | Wet/oil media, mask tips, colour (RGBA) tips, grain `move` | Not applied (preset fields are preserved) | Implemented | Wet-media engine port. |
| 8 | Merge down | Bakes the upper layer into the lower one and keeps the lower layer's opacity and blend mode | (to verify) | Check against happypaint's `mergeDown` behaviour. |
| 9 | Spatial adjustment layers (blur, distort, stylize, light rays) | Preserved in files and editable, but not rendered | Rendered over the backdrop | The tile compositor needs a neighbourhood-aware pass; this comes with GPU compositing. |
| 10 | Adjustment maths precision | f32 CPU reference (spec 10) | GPU with 16F storage and an 8-bit LUT texture | Within 16F rounding; the LUT lookup replicates the texel-centre bilinear sampling. |
| 11 | Text rendering | cosmic-text/swash with system fonts; the outline is a dilated glyph mask; balloons are not drawn | Canvas2D `fillText`/`strokeText`, balloons drawn | Metrics differ slightly by font engine; balloons come next. |
| 12 | PSD text layers | Exported as raster layers (their rendering); no `TySh` editable text | Editable text through ag-psd | Port the `TySh` engine data writer. |
| 13 | Filter handles | Centres, ends and blur pins are panel sliders | On-canvas handles (`filter-handles.ts`) | Port the handle geometry with the canvas overlay work. |
| 14 | Lightning raster | The bolt path is identical (mulberry32, checked against a golden). Strokes are antialiased round-capped polylines with a Gaussian shadow of σ = `shadowBlur`/2 | Canvas2D `stroke` with `shadowBlur` | No Canvas2D in Rust. The glow profile differs slightly. |
| 15 | Pattern fill | Built-in patterns only, sampled bilinearly with repeat | Also imported `patternImage` PNGs; mipmapped sampling | The image needs a decoder in `formats` and mips in `gpu`. Until then an imported pattern falls back to the built-in. |
| 16 | Screentone | `fwidth` is estimated from neighbouring doc pixels; never flattened for preview | GPU `fwidth`; preview flattens unresolvable dots to grey | The flattening is preview-only and needs the view scale, so it comes with the GPU canvas. Export output already matches. |
| 17 | `sin` hashes (glitch, wind, tone sand) | CPU f32 `sin` | GPU `sin` | Large arguments differ between GPUs too. The CPU stays the oracle for the wgpu port. |
| 18 | Fish-eye (curved 5-point) perspective | The `curved` flag is preserved in files; guides draw and snap straight | Arcs through the poles, arc snapping | Port `drawFisheyeLines` and `pickFisheyeCurve`. |
| 19 | Rulers on a rotated canvas | Hidden while the canvas is rotated (pinned rulers only) | A document-attached tick band that turns with the artwork | Draw the doc-edge band when `rulers_pinned` is false. |
| 20 | Animation video export | GIF and PNG-sequence zip only | Also MP4/WebM via WebCodecs | Needs a native encoder (openh264 or ffmpeg); a dependency decision for the owner. |
| 21 | Exposure-sheet editing | Tracks render, play and onion correctly, but the timeline shows the active track as a strip | A row-per-track grid with drag retiming, markers and out-of-pegs | UI work only; the model is complete. |
| 22 | Recordings | `.onepaint-rec` (start document, lossless tiles, session state, serde command log); timelapse as GIF | `.happypaint-rec` logs PaintEngine calls; MP4 timelapse | The engines differ, so logs are not interchangeable. |
