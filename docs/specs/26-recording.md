# 26: Session recording, replay and timelapse (slice 18)

happypaint records its PaintEngine calls into `.happypaint-rec`
(`storage/recording-file.ts`, `engine/recording/recorder.ts`). onepaint's
engine API is different (serde `Command`s plus the stroke API), so it
records its own log in `.onepaint-rec`. That is a divergence: happypaint
recordings don't open in onepaint.

## Format
`.onepaint-rec` is a zip containing:
- `start.happypaint`: the document when recording started;
- `events.json`: `[{"op": …, "t": ms since start, …}]`.

| op | Fields |
|---|---|
| `cmd` | `cmd` (a serialised `Command`) |
| `begin` | `p` (a `StrokePoint`) |
| `extend` | `pts` |
| `end` | |
| `cancel` | |
| `tick` | `now` (airbrush) |

Only top-level calls are recorded. Commands that run strokes internally
(Stroke Path) are not double-logged.

## Replay
- Open the start document in a fresh session, then apply the events in
  order.
- `Replay::step(n)` applies the next n events. The result is identical to
  the recorded session's final document (the engine is deterministic: seeds
  advance per stroke).

## Timelapse
- Up to 240 frames, sampled evenly across the events. Each frame is a flat
  composite scaled to fit 720 px.
- Exported as a GIF at 12 fps, with the final frame held for one second.

## App
- File ▸ Start recording, or Stop recording (save `.onepaint-rec`…).
- File ▸ Replay recording… plays it on the canvas, about 4 seconds for the
  whole log, then leaves the result open.
- File ▸ Export timelapse… (pick a recording, save a GIF).

## Acceptance
- Recording strokes, commands, undo and a Stroke Path, then replaying,
  reproduces the final pixels exactly, with no double strokes.
- The file round-trips.
- A timelapse of N events has `min(N, 240) + hold` frames, decodes as a GIF,
  and its last frame equals the final composite (scaled).
