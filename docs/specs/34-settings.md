# 34: Settings (slice 21)

The sources are `state/settings-store.ts` and
`components/settings-dialog/settings-dialog.tsx`, relative to
`../happypaint/frontend/src/`.

## Model (`apps/onepaint/src/settings.rs`)
- **Fields:** `Settings` holds the store's fields and serialises them in
  camelCase. Absent keys keep their defaults.
- **Clamps on load and on every edit:**
  - Maximum brush size: 10 to 10000 (default 1000).
  - Open tabs: 2 to 12 (default 6).
  - Cursor thickness: 1 to 4.
  - Undo steps: 5 to 1000 (default 200).
- **Storage:** `<config dir>/settings.json`, written atomically through a
  `.tmp` file. Tests and scripted runs pass no path, so they never save.

## Engine (`engine::Preferences`)
- **Updates:** the app pushes the preferences to the active session whenever
  they change, including the zoom when brush size is screen-relative.
- **`stroke_brush`:**
  - Screen size divides the brush size by the zoom.
  - Forced size pressure sets `size_pressure = 1` and drops the size
    dynamics, so `radius_for` becomes size·max(0.15, cp)/2 (BE:358).
  - Forced flow does the same for flow, giving flow·cp.
  - Presets are never changed.
- **Size ceiling:** `SetBrushSize` clamps to `max_brush_size`. Lowering the
  ceiling shrinks the current brushes.
- **Undo cap:** `max_history_steps` sets the history entry cap.

## App
- **Opening:** Menu ▸ Settings… opens a centred two-column dialog. It has
  Reset to defaults and Done.
- **Tab strip:** with tabs off, the strip shows only the active drawing.
- **Tab cap:** opening past the cap closes the least recently active tab
  that has no unsaved changes. happypaint closes the oldest regardless
  (divergence 31).
- **Brush cursor:**
  - Ring, crosshair, ring plus crosshair, or dot.
  - The line is white or the paint colour, over a dark halo.
  - Ring mode falls back to the crosshair when the ring is under 2 px.
- **Size slider:** it runs up to the maximum brush size.

- **Traced tip cursor** ("Trace the tip", default on): image tips draw their
  silhouette, traced at half coverage (`brush::tip_outline`, happypaint's
  `tip-outline.ts` with its tests). It is placed in the dab's frame (tip
  angle and roundness) and scaled to the brush. Square tips draw a square.
  Smudge, Mixer and Liquify keep the ring. Crosshair and Dot styles ignore
  it, and Both adds the crosshair.

## Keyboard shortcuts (`apps/onepaint/src/keymap.rs`)
- **Actions:** happypaint's registry (`input/keybindings.ts`) with the same
  ids, labels, categories, defaults and order, less the five marker and
  play-range actions onepaint has no feature for.
- **Combos:** happypaint's strings: `ctrl+alt+shift+key`, `ctrl` meaning
  Ctrl or Cmd. Symbols drop shift (`?`, `+`); Shift+, and Shift+. are `<`
  and `>`.
- **Matching:** modifiers match exactly. When a combo is bound twice, the
  first action in the registry wins.
- **Storage:** overrides are `keybindings` in `settings.json`
  (id → combo). An override replaces that action's defaults. Unknown ids
  and unreadable combos are dropped on load.
- **Settings ▸ Keyboard shortcuts:**
  - The section lists each category.
  - Clicking a shortcut, then pressing a combo, captures it; Esc cancels.
  - Each row has a per-row reset, and a "duplicate" badge appears when a
    combo is shared.
  - The footer has "Reset all shortcuts".
- **Hard-wired keys:** keys the app hard-wires (undo and redo, save, open,
  new, export, clipboard, the selection commands, Ctrl+J, the Ctrl zoom
  keys, Enter, Tab, Escape, Space, Backspace and Delete) are refused at
  capture.
- **Help:** the help popover lists any changed shortcuts under "Your
  shortcuts".

## Not yet
- Following pen tilt and the stroke direction on the cursor, the timeline
  metric, and hold-E to erase then spring back to the previous tool.
