# 05: `.happypaint` v9 and PNG export

The source is `storage/happypaint-file.ts` (F).

## Container (F:19-46, 311-337)
| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `HPNT` |
| 4 | 2 | u16 LE version. Write 9; reject anything above 9. |
| 6 | 2 | u16 LE flags. Write 0; ignore on read. |
| 8 | 4 | u32 LE `manifestLen` |
| 12 | manifestLen | **raw DEFLATE** (RFC 1951, no zlib header) of the UTF-8 JSON manifest |
| 12+manifestLen | … | chunks, concatenated with no padding |

- Chunk refs `{offset, len}` are relative to the start of the chunk section.
- **Validate the bounds.** happypaint doesn't check them, but onepaint
  must.

## Manifest
The top level is `{app, doc, thumbnail?, nodes}`.

`doc` holds:

- `id, name, width, height`
- `dpi`: absent means 300
- `bitDepth`: written but ignored on read
- `colorSpace`: absent means `srgb`
- `depth`: absent means `16-bit-float`
- `paper?, animation?, guides?, savedSelections?`
- `createdAt, updatedAt`

**Nodes:**
- A node is a group if `children` is present. Read all versions
  (v1–v9) by checking which keys are present. Never branch on the version
  number.
- Leaf keys:
  - `id, name, visible, opacity, blendMode`
  - `clipped, locked, alphaLock, inheritAlpha` (default false)
  - `maskEnabled` (default true)
  - pixels as `tiles` (v7+) **or** `bbox` + `chunk` (v1–v6)
  - mask as `maskTiles` (v8+) **or** `mask` (full canvas)
- **Preserve these as opaque JSON, in order:**
  - Keys: `texts`, `text` (v3, read only), `paths`, `adjustmentKind`,
    `adjustmentParams`, `filters`, `metallic`, `holdFrames`, `animTrack`,
    `animStart`, `animRole`, `colorLabel`, `collapsed`, `panel`.
  - Chunk refs: `height` and `overflow`. Keep their bytes and re-emit them
    on write.
- Keep unknown keys too. Writes use the key order of F:265-294.

## Tiles
- Each tile ref is `{key, hash?, chunk}`, with
  `key = ((tx&0xffff)<<16)|(ty&0xffff)`.
- Tiles are written row-major (y outer, x inner).
- A paint tile that is entirely transparent is omitted.
- Every mask tile is written.
- Edge tiles are clipped to the document size, not padded.
- **Hash:** an optional, opaque dedup key that is never verified on read.
  - onepaint uses happypaint's double FNV-1a over the PNG bytes:
    - `a = 0x811c9dc5`, prime `0x01000193`
    - `b = 0xcbf29ce4`, prime `0x01000197`
    - The result is `hex(a)-hex(b)` in lowercase, with no padding.
  - Dedup keys on the PNG bytes, which avoids a latent bug in happypaint.

## Pixel encoding of chunks
- **Layer PNGs:** 8-bit RGBA, **straight alpha, sRGB**.
  - Write: `rgb = linear_to_srgb(rgb/a)·255`, `a = a·255`, rounded half to
    even and clamped. If a = 0, rgb = 0.
  - Read: decode the PNG, `srgb_to_linear`, then premultiply.
- **Mask PNGs:** the raw value is the coverage in R, with no transfer
  curve. A new mask is all white.
- **Thumbnail:** JPEG, fitted inside 320×240 (never upscaled), at quality 0.8.
  It is the first chunk, at offset 0. It is optional on read.

## PNG export
- Flatten the composite and write it as 8-bit straight-alpha sRGB, at the
  document size.

## Acceptance tests
- The header bytes for version 6 with a manifest of 1234 bytes are
  `48 50 4E 54 06 00 00 00 D2 04 00 00`.
- A bad magic number is rejected, and so is a version of 10.
- Out-of-bounds chunk refs give an error, not a panic.
- **Round trip:** write a document with groups, a clipped layer, a mask,
  a sparse tile and opaque extras. Reading it back gives an equal tree,
  and the pixels match within 8-bit quantization.
- Every `../happypaint/samples/examples/*.happypaint` (v5, bbox form)
  opens. Its composite matches `*.preview.png` with a mean absolute error
  of at most 2/255.
- The output can be decompressed with any raw-deflate decoder.
