# 64: PSD writing in the file's own mode and depth, and PSB

## Status
Built (2026-10-19). Saving a document opened from a PSD or PSB keeps the
file's colour mode, depth and version; anything past 30000 px is written
as a PSB.

## Source
happypaint `storage/psd/export.ts` (the shell, 186–451), `channels.ts`
(`encoder`, 350–445), `compression.ts`, `write.ts`, `bytes.ts`,
`model.ts` `LONG_KEYS`, `import.ts` 166–168 and 337–347.

## Reading (`psd.rs`)
- Photoshop's 16- and 32-bit files keep their layers in a global `Lr16`,
  `Lr32` or `Layr` block with an empty main layer info; the reader now
  takes them from there (`global_blocks`, `layer_block`). They used to
  open flattened.
- Every opened PSD keeps its source (`psd_source::Shell`: mode, depth,
  version, colour mode data, where the layers live, the merged image's
  channels). A flattened file's Background is fingerprinted too, so an
  untouched one saves byte for byte.

## Writing (`psd_write.rs`, `psd_enc.rs`)
- `Enc` is the source's mode and depth: RGB 8/16/32, CMYK 8/16, Grayscale,
  Duotone and Multichannel (one grey channel) 8/16/32, Lab 8/16. Indexed
  and Bitmap become 8-bit RGB once edited, without their colour table and
  resources 1046/1047.
- Edited layers are encoded in that mode:
  - RGB: sRGB values; 32-bit is linear light.
  - Grey: Rec. 601 luma of the sRGB values (of linear ones at 32 bits).
  - CMYK: through the file's ICC profile (resource 1039), else the
    built-in press; stored inverted (1 is no ink).
  - Lab: sRGB → D50 Lab, as `[L/100, (a+128)/255, (b+128)/255]`.
  - Samples: 8-bit rounded, 16-bit rounded to 65535, 32-bit floats; alpha
    straight and not linearised.
  - Compression: RLE at 8 bits; ZIP with prediction at 16 and 32.
- Untouched records, colour mode data and file-wide blocks are reused
  while the file is stored the same way (mode, depth, version). The
  layers go back where the source kept them (main or the global block).
- The merged image: RLE at 8 bits, raw above; colour channels, alpha when
  any pixel is translucent, then the source's alpha and spot channels as
  they were. Thumbnails (1033, 1036) are dropped on any re-encode.
- PSB (version 2): when the source was one or the document is wider or
  taller than 30000 px. Long lengths: the layer and mask section, the
  layer info, channel data lengths, `LONG_KEYS` blocks; RLE row counts
  are u32. Blocks kept from a PSD written as a PSB get their lengths
  re-sized (happypaint copies them with 4-byte lengths).
- 8-bit RGB output is byte-identical to what onepaint wrote before.

## Divergence
50: a resized document is written as a new 8-bit RGB file; happypaint
keeps the source's shell after a resize.

## Tests
`formats/tests/psd.rs`: `sixteen_bit_layers_in_a_global_block_read_and_write_back`,
`edits_go_back_in_the_files_mode_and_depth` (CMYK, grey, Lab, 32-bit RGB),
`past_30000_px_writes_a_psb` (and a PSB source in `Lr16`),
`flattened_files_keep_their_mode_spot_channels_and_bytes` (CMYK with a
spot channel, Indexed to RGB); `psd_enc` unit tests (prediction inverts,
colours by mode).
