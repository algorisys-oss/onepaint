# 06: Engine API

`engine::Session` is the single facade the UI talks to. It contains no
egui and no wgpu.

## Mapping from happypaint
| happypaint | onepaint |
|---|---|
| the store handlers (`DocumentHandler`, `HistoryHandler`, …) | `Session::execute(Command) -> Result<(), EngineError>` |
| `PaintEngineOptions.on*` callbacks | `Session::drain_events() -> Vec<Event>` |
| `PaintSink.begin/extend/end/cancel` | `Session::stroke_begin(StrokePoint) / stroke_extend(&[StrokePoint]) / stroke_end() / stroke_cancel()` |
| recorder `applyCommand()` | `Command` is `serde`-serializable and doubles as the replay format |

## Commands (slice 1)
These are the `Command` variants:

- Documents: `NewDocument{w,h,background}`, `Open{bytes}`.
- Layers:
  - `AddLayer{above}`, `AddGroup`, `DeleteLayer{id}`, `DuplicateLayer{id}`,
    `MoveLayer{id, to_parent, index}`
  - `SetActiveLayer{id}`, `SetLayerProps{id, name?, visible?, opacity?,
    blend_mode?, clipped?, alpha_lock?}`
  - `MergeDown{id}`, `ClearLayer{id}`
- Brushes and color: `SetBrush{preset_id}`, `SetBrushSize`,
  `SetBrushOpacity`, `SetBrushFlow`, `SetColor{rgba}`, `SetTool{Brush|Eraser}`.
- History: `Undo`, `Redo`.

The `Session` also has these methods:

- `save_happypaint() -> Vec<u8>`
- `export_png() -> Vec<u8>`
- `composite_dirty() -> Vec<TileKey>`, along with read access to the
  composite tiles for the GPU upload

## Events
`NeedsRender{dirty: Rect}`, `LayersChanged`, `HistoryChanged{can_undo,
can_redo}`, `DocSizeChanged`, `ActiveLayerChanged`.

## Acceptance tests
- A new document has two layers: Background (white) and "Layer 1", which
  is active.
- A stroke followed by `Undo` restores the pixels exactly. `Redo`
  re-applies them.
- Moving or deleting a layer can be undone.
- The composite after a stroke equals a full recomposite (incremental
  correctness).
- `Command` round-trips through serde_json.
