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
| 9 | Spatial adjustment layers (blur, motion blur, sharpen) | Preserved in files and editable, but not rendered | Rendered over the backdrop | The tile compositor needs a neighbourhood-aware pass; this comes with GPU compositing. |
| 10 | Adjustment maths precision | f32 CPU reference (spec 10) | GPU with 16F storage and an 8-bit LUT texture | Within 16F rounding; the LUT lookup replicates the texel-centre bilinear sampling. |
| 11 | Text rendering | cosmic-text/swash with system fonts; the outline is a dilated glyph mask; balloons are not drawn | Canvas2D `fillText`/`strokeText`, balloons drawn | Metrics differ slightly by font engine; balloons come next. |
| 12 | PSD text layers | Exported as raster layers (their rendering); no `TySh` editable text | Editable text through ag-psd | Port the `TySh` engine data writer. |
