# Parity delta: happypaint v0.204.7 → v0.204.17

What happypaint shipped after `parity-delta-2026-10-18.md` (2026-10-21 →
11-03, release notes 0.204.9–0.204.17), checked against onepaint on
2026-11-05. Fold each item into `feature-parity.md` as it lands.

## Port: onepaint has the same bug

Ordered by priority. 1–3 are the latency and zoom complaints from happypaint's
artist round 32.

- [x] **1. Live taper tail (0.204.15).** Brushes with an exit taper hold the
      last `exit` px of dabs back until lift (`crates/brush/src/lib.rs:484`),
      so the ink stops short of the nib: 26 px on Comic Inker, 34 px on G-Pen.
      happypaint now draws that stretch live, tapered toward the pen, and
      redraws it each move; lift leaves exactly what was on screen.
      Display only: never flattened, recorded or saved. Port
      `engine/brush/taper-tail.ts` (`tailFactor`, `buildTail`, shared by
      the live and the final tail) and `taper-tail.test.ts`; show the tail
      as a stroke overlay.
      Done 2026-11-05: `brush::taper_tail`, `StrokeEngine::live_tail`, and
      `ActiveStroke::live` in the engine (a display surface sharing the
      stroke buffer's COW tiles). Unlike happypaint, which redraws the tail
      on every move, onepaint redraws it once per frame in
      `take_dirty_tiles`: about 48 dabs (≈70 µs) for G-pen, against ~4 pen
      events a frame. Tail tiles are overwritten in place
      (`Tile::clone_from` reuses the buffer), so the stroke buffer's tiles
      are never shared and never copied by the next body dab. Tests:
      `crates/engine/tests/live_tail.rs`; bench `g-pen 14px stroke`. With a
      stabilizer, lifting still adds the string's catch-up point, as in
      happypaint.
- [x] **2. Stabilizer string measured on screen (0.204.17).**
      `Stabilizer::new(strength)` (`crates/stroke/src/lib.rs:148`) measures
      the string in canvas px, so Studio Pen and Millipen trail 4× further
      at 400% and swallow small strokes at 800%. happypaint:
      `createStabilizer(strength, viewScale)`, with the zoom of the last
      render stamped into the stroke's `brush.stabilizer.viewScale` at
      begin, so recordings replay the same. Same at 100%. Port
      `stabilizer.test.ts`; carry `viewScale` in `.happyrec` and stroke
      commands.
      Done 2026-11-05: `Stabilizer::new(strength, view_scale)`;
      `StabilizerSettings::view_scale` (serde `viewScale`, so happypaint
      recordings carry theirs); `Preferences::view_zoom`, which the app sets
      every frame and `stroke_brush` stamps into the stroke's copy of the
      brush (never the preset); `RecEvent::Begin { zoom }`, so onepaint
      recordings replay with the zoom they were drawn at (older ones: 100%).
      Tests: `crates/stroke/tests/stabilizer.rs`,
      `crates/engine/tests/stabilizer_zoom.rs`. Smoothing and exit taper
      still lag in doc px, so the on-screen trail creeps up a little when
      zoomed in, as in happypaint.
- [x] **3. Ctrl zoom keys zoom the canvas (0.204.12).** eframe's
      `zoom_with_keyboard` is on by default, so Ctrl+= / Ctrl+− / Ctrl+0
      (what tablet express keys send) scale the whole UI. Turn it off, add
      the Ctrl/⌘ variants (main row and numpad) to `ZoomIn` / `ZoomOut` /
      `ZoomFit` in `apps/onepaint/src/keymap.rs`, and make Ctrl+scroll zoom
      the canvas about its centre anywhere in the window, not only over the
      canvas. happypaint: `browserZoomAction` in `input/keybindings.ts`.
      Done 2026-11-05. onepaint already hard-wired Ctrl/⌘ with = + − 0 1 to
      the canvas (`App::shortcuts`), but egui's own keyboard zoom still
      scaled the UI whenever the app left the keys alone (a menu open, a
      shortcut being captured): `zoom_with_keyboard` is now off. Ctrl+scroll
      away from the canvas zooms it about its centre. Shortcut help lists
      the Ctrl variants. Test: `ctrl_zoom_keys_and_wheel_zoom_the_canvas_not_the_ui`.
- [x] **4. Pen: cut the last point's handle (0.204.13).** `pen_press`
      (`apps/onepaint/src/canvas.rs:562`) stacks a duplicate anchor when
      you click the point just placed. Photoshop behaviour: click or
      Alt+click it cuts its outgoing handle (the curve drawn stays);
      dragging from it pulls a new outgoing handle on its own; Alt held
      mid-drag on a new point moves only the outgoing handle. Port
      `cutOutHandle` / `setOutHandleOnly` (`engine/stroke/bezier-path.ts`)
      and `bezier-path-handles.test.ts`; update the Pen help text.
      Done 2026-11-05: `bezier::cut_out_handle` / `set_out_handle_only`
      (tests in `crates/stroke/tests/bezier.rs`), `Drag::PenNew { out_only }`
      in the canvas, the manual's Pen entry. App test:
      `pen_cuts_the_last_anchors_handle`. The Alt-mid-drag case has no app
      test (kittest can't hold a modifier through a drag here).
- [x] **5. A cancelled finger is not a tap (0.204.9).**
      `apps/onepaint/src/canvas.rs:1085` treats `TouchPhase::Cancel` like
      `End`, so two- to four-finger taps with a cancelled finger still
      undo, redo or hide the UI. Any cancelled finger voids the tap
      (`input/gesture-recognizer.ts`, its tests).
      Done 2026-11-05: `Gestures::cancel`, which `TouchPhase::Cancel` now
      takes. onepaint also tapped when the *last* finger was cancelled
      (happypaint never did); both are voided. Test:
      `gestures::tests::a_cancelled_finger_voids_the_tap`.

## Port: new features

- [ ] **6. Shift+click straight lines (0.204.14).** With the Brush (dry
      brushes) and the Eraser, Shift+click draws a straight line from where
      the last stroke ended; Shift+click again chains. Dragging before
      release aims the end. Current brush, pressure of the click, no
      smoothing or taper. One undo step per line. The line start is
      remembered per drawing. Shift at the *start* of a stroke now means a
      line; Shift during a stroke (QuickShape constrain) is unchanged.
- [ ] **7. Command palette, Ctrl/⌘+K (0.204.11).** A searchable dialog over
      every tool, edit command, adjustment, filter, layer command,
      animation step and view option: substring and keyword matching
      ("bucket" finds Fill), category badges, shortcut tags, ↑/↓/Enter/Esc.
      Also under Edit ▸ and Help ▸ Command palette…, and rebindable. Build
      it on the `keymap` action registry with `ui_kit` components.

## Low priority

- [ ] **QuickShape hold from input timestamps (0.204.10).** onepaint
      handles each frame's queued events before checking the 600 ms hold
      (`canvas.rs:1446`), so happypaint's frozen-frame race hardly arises.
      Judging stillness from the events' own times would still be cleaner
      (`input/idle-hold.ts`, `idle-hold.test.ts`).
- [ ] **Pen dip recovery (0.204.9).** happypaint replays the pen's path
      through a tip dip into the stroke when tracing slowly. onepaint has
      no dip handling at all; port it with the hover replay buffer
      (`input/pointer-router.ts`).

## Checked, nothing to do

- **Text supersampling cost (0.204.17).** happypaint composited at 2–3× per
  side when zoomed in, even without text. onepaint rasterizes text at
  document resolution and never supersamples.
- **Dock duplicated a grouped panel (0.204.9).** `Layout::set_placement`
  already leaves the group first.
- **Adjustment and fill layer masks lost on reopen (0.204.9).** The
  `.happypaint` reader and writer keep `maskTiles` on every layer kind.
- **Impasto relief lost on undo of delete or merge down (0.204.9).** Relief
  lives on the layer node (`Layer::height`), which undo restores whole.
- **Failed save loses work on switching drawings (0.204.9).** onepaint keeps
  every open drawing in memory as a tab; switching saves nothing.
- **Resized drawing reverts after a graphics reset (0.204.9).** WebGL
  context-loss only.
- **0.204.16 housekeeping.** Repository clean-up, no app change.
