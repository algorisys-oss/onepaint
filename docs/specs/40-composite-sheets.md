# 40: Composite sheets past the GPU texture limit

## Status
Built. The canvas composite and the selection are texture arrays of sheets.
Compositing, downsampling, CPU uploads, the selection upload and the present
pass all address a sheet and a position within it. The app asks for the
adapter's own texture limits.

Follows specs 13 (GPU compositor) and 39 (level-of-detail compositing).
From the photocraft comparison (`ui-egui/src/gpu_canvas.rs`); happypaint
caps documents at 16384 and the WebGL texture size.

## Why
The composite was one texture of the document size. Past
`max_texture_dimension_2d`, `ensure_size` failed and nothing was shown.

egui's default device asks for only 8192, even on GPUs that allow 16384,
and onepaint (like happypaint) allows documents up to 16384 per side. So
any document between 8193 and 16384 px was blank on every machine.

## Model

### Layout (`gpu::sheets::Sheets`)
- A document within the limit is **one sheet of exactly its size**. Nothing
  changes for it, and the present pass keeps its single hardware-filtered
  sample.
- Past the limit, each axis is cut separately into the fewest sheets that
  fit. A sheet is `ceil(n / count)` rounded up to a multiple of 256 (and no
  more than the limit rounded down to 256). An axis that fits is not cut.
  - Multiples of 256 keep every tile in one sheet and make every level of
    detail (to `L_MAX = 5`, 8 px per tile) divide evenly, so a 2 × 2
    downsample block never crosses a sheet.
- Sheet `(sx, sy)` is array layer `sy * cols + sx`. Every sheet has the same
  size; the parts past the document are never written or read.
- The layout fails (`ensure_size` returns false) only when it needs more
  than `max_texture_array_layers` sheets.

### Passes
- **Compose** (`compose.wgsl`): a job's origin stays in document pixels at
  its level. The pass writes to sheet `g / sheet` at `g mod sheet`. `Params`
  carry `cols` and the sheet size at the level.
- **Downsample** (`mip.wgsl`, `mip_sheets`): as before, per tile and level,
  with the source and destination in the same sheet. Edges clamp to the
  document size at the level below, as the box oracle does.
- **CPU uploads and the selection** are copied per sheet.
- **Present** (`canvas.wgsl`): with one sheet, one `textureSampleLevel`, as
  before. With several, a hand-made bilinear sample: four `textureLoad`s at
  the display level, each from its own sheet, clamped to the document. That
  matches the hardware filter, so seams don't show at any zoom. The marching
  ants read the selection sheet by sheet as well.

### Device limits
`apps/onepaint` asks for the adapter's `max_texture_dimension_2d` and
`max_texture_array_layers` instead of egui's 8192 and 256. A 16384²
document is one sheet on most desktop GPUs, and 2 × 2 on GPUs limited to
8192.

### Memory
Sheets don't add memory: the composite is still `Rgba16Float` at document
size, plus a third for the mips, and the selection is one byte per pixel.
The only extra is the unused margin of the last row and column of sheets,
under 256 px per axis per sheet.

## Acceptance criteria
1. **Layout** (unit tests in `sheets.rs`):
   - A document within the limit is one sheet of its size.
   - Past it, each axis takes the fewest 256-aligned sheets, and only the
     axis that overflows is cut.
   - `locate` and `rect` agree.
   - Levels divide evenly.
   - Too many sheets, or an empty document, doesn't fit.
2. **Compose and downsample:** the spec 39 tests pass with the composite as
   one texture and as 3 × 3 sheets of 256 px. A level 0–3 composite into
   sheets equals the one-texture result exactly.
3. **Present:** a document drawn from 3 × 3 sheets matches the one-texture
   draw within 2/255, rotated, at 3× over a sheet corner, at 1:1, and zoomed
   out at level 1, with a selection crossing the seams. A present pass
   without the hand filter fails this test (off by 132).
4. **No regression:** the spec 13, 38 and 39 GPU tests and the present test
   pass unchanged.
5. **By eye:** `ONEPAINT_MAX_SHEET=256` shows the demo exactly as without it.

## Out of scope
- Documents past `max_texture_array_layers` sheets: 131072 px per side for a
  square document at 8192 px and 256 layers. The 16384 document cap is far
  below that.
- Composite memory itself. A 16384² document needs 2.7 GiB of composite
  with mips. Keeping only the visible sheets resident would be a separate
  spec, alongside 8-bit storage.
