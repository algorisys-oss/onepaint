# Parity delta: happypaint v0.204.17 → v0.208.4

What happypaint shipped after `parity-delta-2026-11-05.md` (2026-11-06 →
11-12, release notes 0.204.18–0.208.4, 18 commits), checked against onepaint
on 2026-11-12. Most of it comes from happypaint's 2026-11-11 code review
(`code-review/2026-11-11-happypaint-followup.md`, items F01–F26 and N1–N9).
Fold each item into `feature-parity.md` as it lands.

## Port: onepaint has the same bug

- [x] **1. Float drawings saved as 8-bit (0.207.0, review F07).** 16- and
      32-bit drawings now write `HPFT` float tiles (`formats::tile_float`, a
      port of `tile-float.ts` and its tests) for colour tiles and overflow,
      and the file is stamped v10. 8-bit drawings still write v9 PNGs. Masks
      stay 8-bit. Our 32-bit drawings live in f16 (divergence 2), so their
      f32 tiles hold f16 values. The "HDR" wording on New Canvas is true now.
- [x] **2. Tiled impasto relief, `.happypaint` v11 (0.208.0, review F14).**
      Relief is written as `heightTiles`, only where there is any (v11), and
      both forms are read. A relief tile that won't decode stays flat, and
      its layer is noted.
- [x] **3. Hostile or damaged files (0.205.3, review F10).** `parse()`
      checked the manifest span, but unpacking had no output cap, the canvas
      size wasn't checked, and the tree size had no limit. serde_json's
      128-level limit already stopped a stack overflow. Ported the budgets from
      `storage/container-guard.ts` as `formats::limits`: manifest ≤ 256 MiB
      inflated, side ≤ 32768, ≤ 50 000 nodes, depth ≤ 64. `happy_rec` uses them
      too. Failures are `FormatError::TooLarge` with a message for the user.
- [x] **4. One bad tile loses the whole file (0.205.2, review F03).**
      `read_tiles` failed the whole read on the first PNG that wouldn't decode.
      Now a broken colour tile is left blank, a broken mask tile reveals
      (coverage 1), and a broken legacy whole mask is dropped, as in
      happypaint. The layer is kept and noted on `Progress::damaged_layers`.
      A chunk reference outside the file still fails the read. On open, the
      app names the layers in a notice and leaves the tab unbound from the
      file, so Save asks for a new location and the original stays intact.
      That replaces happypaint's "<name> (original)" gallery copy (divergence
      noted).
- [x] **5. Dip recovery inks the hop to the next stroke (0.204.19).**
      `DipBridge::hover` now commits the lift when a near hover is more than
      `DIP_MAX_TRAVEL` (40 pt) from the lift point, or faster than
      `DIP_MAX_SPEED` (0.35 pt/ms). Speed is measured against the newest
      buffered point at least 40 ms old, over at least 40 ms. Ported the
      fast-hop and far-wander tests, and the slide test is now a slow, timed
      trace. Still wants a try with a real tablet.
- [x] **6. A hidden group's children are movable (0.205.3, review N8).**
      `transformable_ids` skips hidden groups.

## Port: new features

- [x] **7. Autosave modes (0.205.0).** Settings ▸ Documents ▸ Auto-save:
      When you pause (the default: 2 s after the last edit), every 2, 5, 10 or
      15 minutes (timed from the first unsaved edit, so continuous painting
      still saves), or Manual. Edits are counted by `HistoryChanged`, because
      the history length stops moving at its step cap. The recovery copy now
      re-encodes only the tiles that changed: `formats::ChunkCache` keeps each
      tile's chunk with its `Arc`, and an unchanged copy-on-write tile is the
      same pointer. That's the counterpart of happypaint's partial readback,
      and it keeps the 2 s default cheap. Not ported: Ctrl+S writing the
      browser copy and the leave-the-editor saves. A desktop Save writes the
      file and drops the recovery copy, open tabs stay in memory, and closing
      with unsaved work already asks (divergence 59).
- [x] **8. Memory estimate (0.208.1, review F15).** The budget is half the
      system RAM (`sysinfo`), or 2 GB if RAM can't be read. New Canvas gives
      the memory of a fully painted layer (4 B/px for 8-bit, 8 B/px for
      float, 32-bit included) and how many such layers fit, beside 4
      canvas-sized working buffers. It's coloured when tight or over, as a
      warning, not a refusal. Document Properties (the Paper popover) shows
      the memory held now: painted tiles, masks, relief and undo
      (`Session::memory_use`), since blank layers cost nothing here. A
      one-time notice per drawing appears once it passes the budget. Not
      ported: refusing a layer the GPU can't allocate. onepaint's layers live
      on the CPU, and tiles that can't be GPU-resident already fall back to
      the CPU compositor (spec 54). Divergence 60.
- [x] **9. Brush library: Recent, folds, actions on top (0.205.1).** A
      Recent section holds the last eight brushes picked, newest first. Brush,
      eraser and smudge keep separate lists, stored in Settings
      (`brush_recent`). Recent and the shown category fold under a
      `widgets::fold_header`, folds persist (`brush_folded`), and a search
      shows matches whatever is folded. Import and Export sit under the search
      box. onepaint shows one category at a time through its chips, so only
      Recent and that category have headers.
- [x] **10. Brushes panel: header pinned while scrolling (0.204.18).** Our
      title bars were already outside the body scroll, docked and floating,
      but the search box and library actions scrolled away with the list. The
      dock now lets the Brushes panel scroll itself (`scrolls_itself`): its
      search and actions stay pinned, and the list and sliders scroll under
      them. As a popover, only the list scrolls, as before.
- [ ] **11. Stored size in Document Properties (0.208.2, F25).** Counts every
      saved part once: tiles, masks, relief, overflow, PSD data and the
      reference image.

## Blocked on a missing feature (pre-existing gap)

onepaint's Move tool transforms only the active layer
(`begin_transform` → `doc.layer_mut(&active)`). Layer picks (`app.layer_picks`)
exist only for Align. happypaint has moved several layers as one since v0.168,
and `feature-parity.md` doesn't list that gap. These depend on it:

- [ ] **Multi-layer and group Move (v0.168, 0.205.1).** Move lifts the
      picks, else the active layer, and an active group means its visible,
      unlocked children (`selectedTransformableIds`).
- [ ] **Merge non-adjacent picked layers (0.208.3).** The lower picks are
      gathered under the topmost one and merged there; layers in between stay.
      One undo step. We have only Merge Down and Merge Visible.

## Low priority

- **Reopen on the layer you left (0.205.1).** The drawing reopens on its last
  active layer. Check that we save it: `activeLayerId` is read only by
  `happy_rec`.
- **Alt-eyedropper stuck (0.208.3).** In the browser, a lone Alt release can
  swallow the keyup. egui takes modifiers from winit, but an Alt released while
  the window is unfocused (Alt+Tab) can still look held. Worth one manual try
  on Windows.
- **Command palette scrolling (N6).** Arrow keys should keep the highlighted
  row in view after filtering. Rebinding (N5) and the typing guard are already
  handled by `keymap`.
- **One drawing, one instance (0.206.0, F11).** Browser tabs are not our
  problem, but two onepaint processes can open the same file and write the
  same autosave path. A lock file next to the autosave would cover it.

## Checked, nothing to do

- **Autosave freeze, tree changes mid-save, saves during replay (0.205.0,
  0.205.2 N1).** Our autosave writes a cloned `Document` snapshot on a worker
  thread. It never reads back from the GPU and never walks the live tree.
- **Every way out saves first, hidden-tab throttling, PWA reload (0.205.3 F02,
  F12, N2).** Browser-only. Closing a tab or the app already asks when the
  drawing is unsaved.
- **Old recordings and the stabilizer (0.205.3 N4).** `brush/src/lib.rs:189`
  already falls back to `view_scale.unwrap_or(1.0)`.
- **Command palette rebinding and typing (N5, N7).** The palette is a `keymap`
  action, so it follows rebinding and the typing guard.
- **GPU reset restores at the stored size, replay restore (0.205.3 F04/F05,
  0.208.0).** WebGL context-loss and IndexedDB only. Our CPU tiles are the
  source of truth.
- **Bounded tile decoding (0.208.0 F13).** Our `read_tiles` decodes on rayon
  into per-tile surfaces. Memory peaks at about one decoded layer, not the
  whole file. Revisit only if big-file opens show a spike.
- **Liquify snapshot freed on close (0.208.2 F26).** `Session::liquify` drops
  with the session.
- **Reference image mix-up, PNG worker failure, desktop CSP (F16, F17,
  F22).** Browser or Tauri only.
- **0.208.4.** A deterministic browser test, with no app change.
