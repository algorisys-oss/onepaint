# 20: Saved selections (slice 13)

The sources are `engine/selection/saved-selection.ts` and
`engine/paint-engine.ts:7440-7560`, relative to `../happypaint/frontend/src/`.

## Storage (`selection::saved`)
Saved selections are named coverage masks (0..255), stored as
`StoredSavedSelection {id, name, w, h, rle}` in the document's
`savedSelections` key.

**RLE formats:**
- **Hard masks** (only 0 and 255): base-36 run lengths joined by `,`,
  alternating unselected/selected and starting unselected.
  - Example: `[0,255,255,0,0,255,255,0]` encodes as `1,2,2,2,1`.
- **Soft masks:** `b:` followed by `len.value` pairs (both base 36), one
  per run of equal coverage.
- More than 400 000 runs is refused (`None`, "too detailed").
- Decoding returns `None` for junk, or for a string that doesn't cover
  exactly w·h pixels.

**Canvas changes:**
- Canvas Size: `shift(mask, w, h, nw, nh, offX, offY)`. Pixels outside the
  new canvas are dropped, and new area is unselected.
- Image Size: a nearest-neighbour resample sampled at pixel centres.

## Session
| Command | Behaviour |
|---|---|
| `SaveSelection {name}` | Refused when there's no selection. Appends an entry with id `sel-<uuid>`. One undo step. |
| `DeleteSavedSelection {id}` | One undo step. |
| `LoadSelection {id, mode}` | Combines with the current selection by mode, as one selection undo step. |

- **On open:** entries are restored when their `w`/`h` match the document,
  and others are skipped.
- **On save:** entries are written (the key is omitted when the list is
  empty).
- Resize and scale transform the saved masks inside the same Document undo
  step.

## Acceptance
- happypaint's codec cases:
  - hard and soft round trips;
  - `1,2,2,2,1`;
  - the `b:` run string;
  - junk and wrong sizes give `None`;
  - shift and crop;
  - the resample doubling;
  - soft resample;
  - the identity resample.
- Save, deselect and load gives the same selection. Load with Add or
  Subtract combines with the current selection.
- Undo removes a save and restores a delete.
- The list survives `.happypaint` save and open. An entry from another
  canvas size is skipped.
- Canvas Size shifts the saved masks, and undo restores them.
