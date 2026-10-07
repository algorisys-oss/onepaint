# 22: Pen tool and vector path layers (slice 15)

The sources, relative to `../happypaint/frontend/src/`:

- `engine/stroke/bezier-path.ts` (BP) and `bezier-path-ops.test.ts`
- `engine/paint-engine.ts:4768-5330` (path layers and path ops)
- `state/path-ops-store.ts`

## Geometry (`stroke::bezier`, a port of BP)
**Anchors:** `PathAnchor {x, y, inX?, inY?, outX?, outY?, kind: corner|smooth}`.
- Handles are relative offsets.
- An absent handle makes that side of the segment straight.

**Specs:**
- `Subpath {anchors, closed}`.
- `PathSpec {subpaths, stroke?: {brushId, size, color (straight RGB 0..1), opacity, seed?}, fill?: {color, opacity}}`.
- Serde field names are those of the happypaint JSON (camelCase).

**Functions:**
- `segment_controls`, `cubic_at`, `segment_length` (the mean of the chord
  and the control net).
- `flatten_subpath(sub, step)`:
  - curved segments get `clamp(ceil(len/step), 2, 96)` samples;
  - straight segments contribute just their end point;
  - closed subpaths wrap.
- Editing helpers:
  - `constrain_handle` (45° steps, length kept);
  - `catmull_rom_anchors`;
  - `move_anchor`, `set_handle` (a smooth anchor mirrors its partner);
  - `toggle_anchor_kind` (smoothing gives ⅓-chord tangent handles);
  - `delete_anchor` (keeps at least 2);
  - `closest_segment` (24 samples per segment);
  - `insert_anchor_at` (a de Casteljau split).
- Whole-path helpers:
  - `bounds` (padded by half the stroke width);
  - `affine` (handles take only the linear part; the stroke width scales by
    `√|det|`);
  - `area_contours` (open paths are closed; fewer than 3 points is
    skipped);
  - `simulated_pressure` (smoothstep taper over `min(30%·len, 400 px)` at
    each end, with a floor of .05).

## Path layers (engine)
- Specs live in `layer.extra["paths"]`, as verbatim JSON.
- The pixels are always the rendering:
  1. fill first, even-odd across the closed subpaths, source-over at the
     fill opacity;
  2. then the stroke, through the brush engine with the preset `brushId`
     at `size`, smoothing 0, no stabiliser, seed `stroke.seed ?? 0x5eed`,
     pressure 1, each subpath composited at the stroke opacity.

**Commands:**

| Command | Behaviour |
|---|---|
| `CreatePathLayer {spec}` | A new layer "Path" above the active one. |
| `AddPathObject {id, spec}` | Adds an object to an existing path layer. |
| `UpdatePathObject {id, index, spec}` | Live; coalesced into one undo step per object, like text. |
| `RemovePathObject {id, index}` | When the last object goes, the layer becomes a plain paint layer. |
| `RasterizePath {id}` | Drops the specs and keeps the pixels. |

Pixel tools on a path layer rasterise it first, as they do for text.

**Path operations** act on the active path layer. The target is the plain
paint layer directly below it in the same group; with none, the operation
is refused.

| Operation | Behaviour |
|---|---|
| `SelectFromPath {mode}` | The area (open subpaths closed, even-odd), thresholded hard at 50%, combined by mode. |
| `FillPath` | The target gets the current colour × coverage × selection, source-over, as one step. |
| `StrokePath {tool, simulate_pressure}` | Strokes each subpath polyline onto the target with the brush, eraser, smudge or mixer (current settings), with simulated pressure if asked. One transaction. |
| `MaskFromPath` | The target's layer mask becomes `existing × coverage`, or just `coverage` when there was no mask. |

## Pen tool (P)
**Building a path:**
- A click adds a corner anchor.
- Press-and-drag adds a smooth anchor and pulls its out-handle (in = −out).
  Shift constrains to 45°.
- A click within 10 screen px of the first anchor closes the path and
  finishes it.
- Enter or a double-click finishes an open path. Esc cancels. Backspace
  removes the last anchor.
- Finishing creates a path layer (or adds to the active one, when it is a
  path layer):
  - the stroke takes the current brush preset, size and colour, opacity 1,
    and a fresh seed;
  - with "Fill" on, a fill in the current colour.

**Editing (pen on a path layer, nothing being built):**
- Dragging an anchor moves it.
- Dragging a handle end sets that handle (mirrored when smooth).
- Alt-click toggles corner/smooth. Ctrl-click on a segment inserts an
  anchor.
- Delete removes the last-touched anchor.
- Each drag is one undo step.

**Display:** a path outline, anchor squares and handle lines for the active
path layer, plus the rubber band while building.

**The Path menu** (pen options popover): Make selection, Fill path, Stroke
path (tool and Simulate pressure), Mask from path, Rasterize, Delete path.

## Acceptance
- **Geometry (happypaint cases):**
  - translation moves anchors, not handles;
  - a rotation, non-uniform scale and mirror map the control points
    exactly, and the stroke size scales by `√|det|`;
  - bounds are padded;
  - an open path is closed for its area, and a lone anchor encloses
    nothing;
  - pressure tapers at both ends, is capped, and is symmetric;
  - plus inserting an anchor keeps the curve (the split point lies on the
    original), and `set_handle` mirrors.
- A path layer renders the stroke and fill. Its spec round-trips through
  `.happypaint`. An update is one undo step. Removing the last object
  leaves a paint layer.
- Make selection, fill and mask from path act as specified. Stroke path
  paints along the path on the target layer, as one undo step.
- **UI:** clicking three points and pressing Enter creates a path layer
  whose stroke is painted. Dragging an anchor moves it.
