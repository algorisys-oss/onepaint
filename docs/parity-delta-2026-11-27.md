# Parity delta: happypaint v0.209.0 → v0.212.0

What happypaint shipped after `parity-delta-2026-11-12.md` (2026-11-12 →
11-26, release notes 0.209.1–0.212.0, 20 commits), checked against onepaint
on 2026-11-27. None of it is a performance change; it is artist-feedback
fixes and features. Fold each item into `feature-parity.md` as it lands.

## Port: onepaint has the same bug

- [x] **1. Slow strokes zoomed in break into dashes (0.211.8).** The sampler
      floored the carried distance-to-next-dab at 0.5 doc px after every
      move, so moves shorter than that never ran it down.
- [x] **2. New groups are Pass Through (0.211.11).** Group Layer / Group
      Layers made an isolated (Normal) group, so a Color Dodge layer lost its
      glow once grouped. Add Group stays Normal, as in happypaint.
- [x] **3. Pen dip recovery is opt-in (0.211.7).** Settings ▸ Brush ▸
      "Rejoin broken pen strokes", off by default; off, a pen lift ends the
      stroke at once.

## Port: new features

- [x] **4. Pen pressure setting with calibration (0.212.0).**
- [x] **5. QuickShape keeps following the pen after the snap (0.211.5),
      Shift on a still pointer (0.211.6).** Rect and Ellipse gained `rot`.
      happypaint's Shift on a snapped line's end handle has no counterpart:
      onepaint has no post-snap handles (divergence 64).
- [x] **6. Checksums and must-understand flags in `.happypaint` (0.211.4).**
      Written with flag bit 0, a `crc` on every chunk ref and the `HPCK`
      trailer; no version bump.
- [x] **7. Gradient stops take picked colours (0.211.11).** While a gradient
      editor shows a selected Bg or Colour stop, the Colour panel edits it
      (divergence 63).
- [x] **8. Export dialog preview (0.211.12).** `formats::export_preview`
      plus the dialog thumbnail (divergence 62).
- [x] **9. Dockable Text panel (0.210.0), dock a floating group as one unit
      (0.211.9), text balloon/BG colour and typed Line/Spacing (0.209.1).**
      `PanelId::Text`: closed, the editor is the popover under the text as
      before; open, the editor lives in the panel, which floats with the Text
      tool and stays under every tool when docked or grouped (View ▸ Text).
      A group's strip dragged to an edge or a docked header docks every
      member; a dock's tab strip (empty space) or its Float button floats the
      side's open panels as one group. The balloon sync bug didn't exist here
      (the editor edits the spec directly) and Line was already typed; a typed
      letter Spacing field is new.
- [x] **10. Stroke Path paints wet and live brushes wet (0.211.2).** Already
      so here: onepaint routes brushes inside `Session::stroke_begin`, which
      Stroke Path calls. A test now guards it.

## Not ported

- Scripting API `window.happypaint` (0.211.0–1): the serde `Command` enum is
  onepaint's scripting surface already.
- Desktop WebView cache clearing (0.211.10), README move, test chores.
