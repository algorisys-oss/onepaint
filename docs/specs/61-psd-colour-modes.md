# 61: PSD colour modes and depths on read

## Status
Built (2026-10-19): PSD and PSB files in Bitmap, Grayscale, Indexed, RGB,
CMYK, Multichannel, Duotone and Lab, at 1, 8, 16 and 32 bits, open with
their layers. onepaint still writes 8-bit RGB (divergence 50).

## Source
happypaint `storage/psd/channels.ts` (`colorChannels`, `sampleAt`,
`converter`, `transparentIndex`, `decodeMerged`), `compression.ts`
(`unpredict`), `engine/color/press-profile.ts` (`labToSrgb`),
`engine/color/cmyk.ts` (`cmykToProofRgb`), `psd-file.test.ts`.

## Reading
- Channels stay at the file's depth until converted: 1-bit (1 is black),
  8 and 16-bit integers, 32-bit floats; ZIP prediction undone per depth
  (32-bit rows are byte-planar).
- Colour: RGB as is (32-bit is linear light); Grayscale, Duotone (its
  grey), Multichannel (its first channel) and Bitmap as grey; CMYK
  inverted (255 is no ink) through the file's ICC profile (resource 1039)
  when it is a usable CMYK one, else the built-in press, baked to a 17⁴
  grid; Lab through D50 (`lab_to_srgb`); Indexed through the colour table
  (planar R, G, B) with resource 1047's transparent index. Alpha is the −1
  channel, never gamma-converted.
- The merged image (files without layers): its colour channels first; the
  next is alpha only when the layer count is negative.
- 16 and 32-bit samples reach the document's float tiles at full
  precision (happypaint rounds to 8 bits); Blend If is read for RGB only.

## Divergences
- A 1-bit row starts on a byte (Photoshop's layout); happypaint's
  `sampleAt` ignores the row padding, which shears bitmaps whose width
  isn't a multiple of 8 (divergence 53).

## Acceptance criteria
1. happypaint's 16-bit and 32-bit `psd-file` tests.
2. CMYK paper and black, Lab white and mid grey, an indexed colour and its
   transparent index, and a padded bitmap row read as expected.
