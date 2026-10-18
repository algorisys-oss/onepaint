# 51: PSD files save back as they were

## Status
Built (2026-10-18).

## Why
onepaint rebuilt every PSD on save, so what it doesn't model was lost:
layer styles, text and smart objects (kept only as their pixels), guides,
slices, ICC profiles and file-wide data such as patterns. happypaint keeps
untouched layers' records and saves an untouched file byte for byte
(`storage/psd/export.ts`).

## Model
- Reading an **8-bit RGB PSD** (not PSB) keeps a `formats::psd_source::
  PsdSource` on the document (`Document::source`, never saved):
  - the file's bytes;
  - each layer's and group's record and channel data, by node id;
  - the image resources and the layer section's tail (global layer mask,
    file-wide additional info);
  - the records' order (dividers included);
  - a fingerprint of each node as read: its properties, and `Weak`s to its
    tiles. Tiles are copy-on-write, so any edit gives new tiles; undoing
    brings the old ones back, and the layer matches again.
- **Writing:**
  - nothing changed (every node matches, same order, size and DPI): the
    original file;
  - otherwise a node that still matches reuses its record and channel data
    byte for byte; the rest are written as before. The original image
    resources are kept while the DPI is unchanged, and the layer section's
    tail always.
- 16-bit and grey PSDs and PSBs are rewritten as 8-bit RGB as before
  (divergence 50): their records can't be copied into that format.

## Acceptance criteria
1. A PSD from another program (`fixtures/agpsd.psd`) and one onepaint wrote
   save back byte-identical when untouched, also through the session and
   after an edit that is undone.
2. After painting on one layer and renaming a group, the other layers'
   records are in the new file unchanged, and the edits read back.
