# 24: Animation Assist (slice 16)

The sources, relative to `../happypaint/`:

- `docs/animation-spec.md`
- `docs/animation-tracks-spec.md`
- `frontend/src/state/animation-store.ts` (AS)
- `frontend/src/state/animation-tracks.ts` (AT)
- `frontend/src/engine/compositor.ts:1415-1525` (ghosts)

## Model (engine `anim`)
**Per-node fields** (in the node's `extra`, verbatim):
- `holdFrames` (≥1, default 1);
- `animRole` (background or foreground);
- `animTrack` (groups);
- `animStart`.

**Document settings:** `doc.animation` = `StoredAnimation`, which is
`{enabled, fps 12, loop loop|pingpong|once, onionEnabled true, onionBefore 1, onionAfter 1, onionOpacity .35, onionTint true, onionColorBefore #ff2626, onionColorAfter #1ad940, onionFront false, onionMuted []}`.
Markers and the play range (`markers`, `markIn`, `markOut`) are typed. Other unknown keys (pegs) are preserved. The settings are written
once animation has been enabled.

**Flipbook** (no track groups):
- The frames are the top-level non-role nodes, bottom-first, each covering
  `hold` slots.
- `frame_at_slot` clamps to the last frame.

**Tracks:**
- A top-level group with `animTrack` is a track. Its children are sheets
  with `start` (an explicit `animStart`, or packed after the previous sheet)
  and `hold`.
- Any other top-level node is static.
- The column count is the latest sheet end.
- At column t, a track shows the sheet with `start ≤ t < start+hold`, or
  nothing.

**Playback step** (`step_slot`, AS:292):
- loop: wraps.
- ping-pong: bounces (`hi−1` and `lo+1`).
- once: stops at `hi` with done.

## Present-only view
While animation is enabled, display compositing and the eyedropper use a
view of the document. Pixels, history and visibility flags never change.

| What | Shown how |
|---|---|
| Flipbook | The frame under the playhead and the role nodes. Other frames are hidden. |
| Tracks | Each track shows its sheet at the column; static tracks always. |
| Onion skins (not playing, `onionEnabled`) | Up to N previous and M next frames (flipbook) or sheets of the active track (tracks), skipping muted distances. Each is drawn as a ghost: opacity `node.opacity · onionOpacity / distance`, normal blend, tinted `rgb = mix(rgb, tint·a, .65)` in its colour (previous/next) when `onionTint`. Ghosts go under the current frame, or over it with `onionFront`. Roles are never ghosted. |

## Session API
| Command | Behaviour |
|---|---|
| `SetAnimation {settings}` | Enable/disable and settings. |
| `SetAnimSlot {slot}` | Clamped. The active layer follows the frame under the playhead, unless it already sits inside that frame or in a role node. |
| `SetAnimPlaying {playing}` | Hides the ghosts. |
| `AddFrame` | An empty layer after the current frame; selected, playhead moved to it. |
| `DuplicateFrame` | |
| `SetFrameHold {id, hold}` | 1..99, one undo step. |
| `SetFrameRole {id, role}` | One undo step. |

Selecting a layer moves the playhead to its frame (`syncPlayheadToActive`).

## Export
Each frame is composited without ghosts and repeated `hold` times at `fps`.
- **GIF:** palette-quantised, delay `round(100/fps)` cs, infinite loop.
- **PNG sequence:** `frame_0001.png…` in a stored zip.
- **MP4:** H.264 (OpenH264, Constrained Baseline, 12 Mbit/s buffer-based
  rate control, a key frame every second). The timescale is fps × 1000, so
  any fps is exact. The `moov` box comes first (fast start). Odd sizes are
  padded to even by repeating the edge. Frames go on white, because video
  has no alpha. The muxer is `formats::video`.

The background is transparent, white or a hex colour. WebM is not offered
(divergence 20).

## Timeline (app)
- The film button toggles animation mode. A bottom bar appears with:
  - transport: first, previous, play/pause, next, last;
  - fps and loop controls;
  - onion settings (before/after count, opacity, tint, front);
  - frame cells: thumbnail and hold. Click selects, and a cell menu offers
    duplicate, delete, hold ±, background/foreground/frame.
  - Add frame.
- Shortcuts: `,` and `.` step frames; Shift+Space plays.
- Playback is an app clock at `fps` using `step_slot` and the loop mode.

## Acceptance
- **Pure maths:**
  - frames and holds;
  - tracks with explicit starts and gaps;
  - `step_slot` in every mode (AS tests);
  - the preview sets.
- **View:**
  - in a flipbook, the playhead shows only that frame, plus roles;
  - onion shows red-tinted previous and green-tinted next ghosts at the
    configured opacity;
  - playing hides the ghosts;
  - disabled means the normal composite is unchanged.
- The active layer follows the playhead, and selecting a layer moves the
  playhead.
- The settings and node fields round-trip through `.happypaint`.
- GIF export decodes with the right frame count (holds expanded) and delay.
  PNG-zip has one PNG per slot.

## Markers and play range (`animation-store.ts:525-590`)
- **Markers:** `{slot, name, color}` on the ruler, sorted by slot. They
  stay put when frames are retimed.
  - Shift+M adds `M<n>` at the playhead in the next of five colours. A slot
    that is already marked is left alone.
  - Alt+, and Alt+. jump to the previous and next marker.
  - The timeline's marker chips jump on click and are removed on
    right-click.
- **Play range:**
  - Shift+I and Shift+O, or the timeline's In and Out buttons, set
    `markIn` and `markOut`. The range is clamped to the timeline and
    ordered.
  - Playback steps inside it. Play from outside it, or at the end of a
    play-once, starts at the in-point.
  - "Clear range" removes it.
- **Export:** "Export marked range (MP4 / GIF / PNG .zip)" exports just the range.
- **Not yet:** renaming and recolouring markers.

## Exposure-sheet grid (`track_grid.rs`; happypaint `track-grid.tsx`, `animation-store.ts:398-520`)
- **When it shows:** tracks documents (any top-level group with
  `animTrack`) show the grid above the strip. A flipbook offers "Convert to
  tracks".
- **Conversion:** the layer frames become one track, keeping their starts
  and holds. Role frames stay as static tracks. It needs two or more frames
  and no group frames.
- **Ruler:**
  - Click or drag to scrub.
  - The playhead tick is filled, and ticks outside the play range are
    shaded.
  - Markers show as a coloured bar.
  - Numbers appear every 5 columns, or on every column up to 12.
- **Rows:**
  - One row per track, top-first, with name, up/down and delete. The last
    sheet track can't be deleted.
  - Static tracks read "every frame".
  - Columns stretch to fit, at least 14 px wide.
- **Sheets:**
  - Click to select a sheet and jump to it.
  - Drag to retime (`Command::MoveSheet`). The sheet is clamped between its
    neighbours and never crosses them.
  - The column width is frozen at the press, so the sheet keeps up with the
    pointer while the timeline grows.
- **Gaps:** click moves the playhead and picks the track; double-click adds
  a drawing there.
- **Buttons:**
  - "+ Drawing" adds a drawing at the playhead on the active track
    (`AddSheet`), and is refused where a sheet already shows.
  - "+ Track" adds a track on top with one blank sheet.
- **Undo:** each edit is one undo step, and a whole drag is one.

## Batch retiming (`frame_ops.rs`, app `timing.rs`; `animation-store.ts:589-760`)
- **Selection (app state):**
  - The ☑ button opens the ops bar. Shift-click selects the range from the
    current frame. Ctrl/⌘-click, or any click while the bar is open,
    toggles a frame.
  - With nothing selected, the ops act on the frame under the playhead.
  - "Done", or leaving animation mode, drops the selection.
- **Ops (one undo step each; engine commands):**
  - **Set duration** (`SetFramesDuration`): every target holds N slots
    (1–99).
  - **Split** (`SplitFrames`): a frame held longer than N becomes
    `ceil(hold/N)` copies of N slots, and the last takes the remainder.
    Total timing is unchanged. Groups and short frames are skipped.
  - **In-betweens** (`AddInbetweens`): an empty frame after each target.
    The last one added is selected.
  - **Duplicate** (`DuplicateFrames`): copies with content; groups are
    skipped.
  - **Delete** (`DeleteFrames`): one frame always stays.
- **Afterwards:** the playhead stays on a real slot.
- **In a tracks document:** the ops act on the active track's sheets. Split
  pieces keep explicit starts, and a sheet that would overlap the one
  before it is pushed right (divergence 21).

## Out of pegs (`animation-store.ts:796-850`, `compositor.ts:2457-2490`)
- **`AnimSettings.out_of_pegs`:** frame id → `Peg {dx, dy, rot, scale}`,
  saved as `outOfPegs`. Scale is clamped to 0.1–4.
- **Display only:** a ghost's group carries `GHOST_PEG_KEY`. Compose samples
  its content through the inverse of "scale and turn about the document
  centre, then offset" (bilinear), compositing each source tile it reaches
  once. Pixels, layers and history never change.
- **UI (the Onion menu):**
  - "Out of pegs" picks a ghost (`−1`, `+1`, …; `•` marks a pegged one).
  - Rotate and Scale sliders; Reset and Reset all.
  - While it is on, a canvas drag slides the target ghost in document px,
    whatever the zoom and rotation.
