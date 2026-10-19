# 62: Replaying happypaint recordings

## Status
Built (2026-10-19): `.happypaint-rec` files open in Replay recording and
Export timelapse (`formats::happy_rec`, `engine::happy_replay`).

## Source
happypaint `storage/recording-file.ts` (the container),
`engine/paint-engine.ts` `record()` and `applyCommand` (4014–4229),
`storage/recording-replay.ts`, `docs/recording-mode-spec.md`.

## Format
`HPRC`, u16 LE version 1, u16 LE flags, u32 LE manifest length, the
manifest as raw-deflated JSON (`doc` size and active layer, the layer
tree as in `.happypaint`, `commands: [{t, op, args}]`), then the PNG
chunks the nodes point into. No paper, dpi or depth is recorded.

## Replay
Each command becomes a `RecEvent::Happy` that the session maps onto its
own commands as it plays, keeping a happypaint id → onepaint id map for
the layers it creates (happypaint injects ids; onepaint makes its own).
- Mapped: strokes (the recorded brush as the current tool's brush, the
  colour, erase), live wet strokes (a live preset of the recorded load,
  size, impasto and tip), `wetTick` steps, dry, tilt, paper, wash fills,
  smudge and liquify strokes, flood and shape fills, gradients (old
  `{color, color2}` ones too), Background fills, layer add / delete /
  clear / select / duplicate / move / merge / props / label / clip and
  alpha lock, masks, adjustment layers and destructive filters,
  selections, transforms, canvas resize and scale, undo and redo.
- Skipped (named in a notice): text and path objects, panels, layer
  styles and smart filters, mixer patches, airbrush ticks, image inserts,
  and anything newer. Per-stroke symmetry transforms and dab seeds aren't
  carried: onepaint's own symmetry and RNG apply (happypaint itself aims
  at visual reproduction, not identical pixels).

## Acceptance criteria
1. A recording opens with its layers and commands; a non-recording is
   refused.
2. A stroke on a layer added mid-session, its renaming through
   happypaint's id, a fill, undo and redo replay; an old gradient replays;
   a live wet stroke dries through its recorded ticks.
