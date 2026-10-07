# 15: PSD import and export (slice 8)

happypaint uses ag-psd 30 through `storage/psd-file.ts`, `psd-map.ts` and
`engine/psd-blend.ts`. onepaint implements the format natively in
`formats::psd`, following ag-psd's byte layout (big-endian).

## Export (matches happypaint, plus DPI)
- **Header:** PSD v1, RGB, 8-bit. 3 channels, or 4 when the composite has
  any alpha below 255. Width and height are at most 30000.
- **Image resources:** 1005 resolution (DPI). This is an onepaint
  addition: happypaint loses DPI on export.
- **Layer records** go bottom-first. A group is written as a
  `</Layer group>` divider (`lsct` 3), then its children, then the group
  record (`lsct` 1 with its blend key).
- **Per layer:**
  - The rect is the content bounds on the canvas.
  - Channels −1 (alpha), 0, 1, 2 (RGB) and −2 (mask), each RLE-compressed
    (PackBits, per-row byte counts).
  - Opacity is `round(op·255)`. The clipping byte is set for clipped
    layers. Flags: 0x08, plus 0x02 when hidden.
  - The name is a Pascal string padded to 4, plus `luni` (UTF-16BE).
  - Empty layers get a 0×0 rect and empty channels.
- **Masks:** a full-canvas rect, default colour 255, flags 2 when disabled.
  The R coverage is written ×255.
- **Pixels:** straight-alpha sRGB 8-bit, rounded half to even.
- **Blend keys:**

  | Mode | Key | Mode | Key |
  |---|---|---|---|
  | normal | `norm` | multiply | `mul ` |
  | screen | `scrn` | overlay | `over` |
  | add | `lddg` | darken | `dark` |
  | lighten | `lite` | color-dodge | `div ` |
  | color-burn | `idiv` | hard-light | `hLit` |
  | soft-light | `sLit` | difference | `diff` |
  | exclusion | `smud` | linear-burn | `lbrn` |
  | divide | `fdiv` | hue | `hue ` |
  | saturation | `sat ` | color | `colr` |
  | luminosity | `lum ` | pigment | `norm` |

  On import, unknown keys (`pass`, `diss`, …) become normal.
- **Adjustment layers** are written as empty records carrying their key:
  `nvrt`, `post`, `thrs`, `brit`, `hue2`, `levl` and `curv`, with
  psd-map.ts's value mapping. Other kinds are written as empty layers.
- **Merged composite:** RLE; pixels that are partly transparent are matted
  against white.

## Import
- PSD v1 and PSB v2, at 8 bits (and 16 bits, which happypaint truncates).
  Colour modes are RGB and Grayscale. Compression: raw, RLE, and ZIP
  (with and without prediction).
- Groups are rebuilt from `lsct`. Pass-through becomes normal.
- Layers keep their name (`luni` first), opacity, blend, clipping, hidden
  flag and pixels at (left, top). Pixels are clipped to the canvas.
- Masks are filled with the default colour, then the mask rect is placed.
  The disabled flag maps to `maskEnabled = false`.
- Adjustment keys map back as the export table does; unmapped ones keep
  their pixels.
- DPI comes from resource 1005.
- A file with no layers imports its composite as a single layer.

## Acceptance
- PackBits round-trips random rows. Long runs encode as repeats.
- Round trip through onepaint's own writer and reader: the tree (groups,
  order), names (Unicode), opacity, blend, hidden, clipped, mask and its
  enabled flag, and pixels within 1/255 all survive.
- A fixture written by **ag-psd** (happypaint's library, through
  `scripts/psd-fixture`) imports with the expected tree, names, blend
  modes, mask and pixels.
