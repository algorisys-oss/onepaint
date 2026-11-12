# 05: `.happypaint` v9–v11 and PNG export

The source is `storage/happypaint-file.ts` (F).

## Container (F:19-46, 311-337)
| Offset | Size | Field |
|---|---|---|
| 0 | 4 | magic `HPNT` |
| 4 | 2 | u16 LE version. Write the lowest version that can read the file: 9, 10 if it holds float tiles, 11 if it holds tiled relief. Reject anything above 11. |
| 6 | 2 | u16 LE flags. Write 0; ignore on read. |
| 8 | 4 | u32 LE `manifestLen` |
| 12 | manifestLen | **raw DEFLATE** (RFC 1951, no zlib header) of the UTF-8 JSON manifest |
| 12+manifestLen | … | chunks, concatenated with no padding |

- Chunk refs `{offset, len}` are relative to the start of the chunk section.
- **Validate the bounds.** Every chunk ref must lie inside the file.
- **Budgets** (happypaint `container-guard.ts`, v0.205.3): the manifest
  inflates to at most 256 MiB; each canvas side is 1–32768; the tree holds at
  most 50 000 nodes, nested at most 64 deep. Past one, the read fails with
  `FormatError::TooLarge` and a message for the user.

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
  (v1–v11) by checking which keys are present. Never branch on the version
  number.
- Leaf keys:
  - `id, name, visible, opacity, blendMode`
  - `clipped, locked, alphaLock, inheritAlpha` (default false)
  - `maskEnabled` (default true)
  - pixels as `tiles` (v7+) **or** `bbox` + `chunk` (v1–v6)
  - mask as `maskTiles` (v8+) **or** `mask` (full canvas)
  - impasto relief as `heightTiles` (v11+) **or** `height` (full canvas)
- **Preserve these as opaque JSON, in order:**
  - Keys: `texts`, `text` (v3, read only), `paths`, `adjustmentKind`,
    `adjustmentParams`, `filters`, `metallic`, `holdFrames`, `animTrack`,
    `animStart`, `animRole`, `colorLabel`, `collapsed`, `panel`.
  - Chunk refs: a `height` PNG onepaint can't decode keeps its bytes and is
    re-emitted on write. `overflow` is decoded and re-encoded.
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

- **Damage:** a tile that won't decode is left blank (a mask tile reveals,
  coverage 1; a relief tile stays flat) and its layer is listed on
  `Progress::damaged_layers`. The layer and the file still open.

## Float tiles (v10, happypaint `tile-float.ts`, v0.207.0)
- A 16- or 32-bit drawing writes its colour tiles and its `overflow` with
  `fmt: "f16"` or `"f32"`. Masks stay PNG. A `fmt` of any other value fails
  the read.
- The chunk is a 16-byte header (`HPFT`, u8 format 1 = f16 / 2 = f32, 3
  reserved bytes, u32 LE width, u32 LE height), then raw DEFLATE of the
  premultiplied linear RGBA samples, rows top to bottom. The samples are
  little-endian bytes split into byte planes, and each byte is stored as the
  difference from the previous one.
- onepaint holds 32-bit drawings in f16 (divergence 2), so their f32 tiles
  are exact widenings of f16 values.

## Tiled relief (v11, happypaint v0.208.0)
- `heightTiles: [{key, hash?, chunk}]`, only for tiles that hold relief.
  Each chunk is a PNG of that tile's clipped rect, in the full-canvas
  `height` encoding: `q = h/16·65535`, with R the high byte and G the low.

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
- A bad magic number is rejected, and so is a version of 12.
- Out-of-bounds chunk refs give an error, not a panic.
- **Round trip:** write a document with groups, a clipped layer, a mask,
  a sparse tile and opaque extras. Reading it back gives an equal tree,
  and the pixels match within 8-bit quantization.
- Every `../happypaint/samples/examples/*.happypaint` (v5, bbox form)
  opens. Its composite matches `*.preview.png` with a mean absolute error
  of at most 2/255.
- The output can be decompressed with any raw-deflate decoder.

## Notes from implementation
- The sample previews in `../happypaint/samples/examples/*.preview.png` come
  from happypaint's headless generator, which blends in sRGB (gamma) space.
  A document without a `depth` key opens as 16F, and happypaint composites
  normal mode in linear light there. So the app's own composite differs from
  these previews wherever layers are semi-transparent.
- The sample test therefore rebuilds a gamma-space composite from the decoded
  layers. The measured mean absolute error is 0.007–0.43/255.
- Pixels outside the canvas are dropped on save. happypaint's `overflow`
  support comes in a later slice; until then, existing `overflow` chunks are
  preserved untouched.
