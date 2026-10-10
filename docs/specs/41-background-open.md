# 41: Opening files in the background

## Status
Built. `.happypaint`, PSD/PSB and image files are read and decoded on a
worker thread, with progress and Cancel. `.happypaint` tiles decode in
parallel. Books still open on the UI thread.

Read timing (`cargo run --release -p onepaint-formats --example
read_timing`, a 328 MB 4096² × 8-layer file, 12 threads, on a loaded
machine with a load average of about 8): median 10.7 s with
`RAYON_NUM_THREADS=1`, 3.6 s in parallel (minimum 2.4 s).

From the photocraft comparison (photocraft loads files in the background).
happypaint reads files on the main thread with no progress.

## Why
Opening a large `.happypaint` or PSD blocked the UI thread. One thread
decoded every tile PNG, the window froze, and there was no way to stop it.

## Model

### Progress (`formats::Progress`)
- This is a cloneable handle holding a step count, a total and a cancel
  flag, all atomics.
- A reader sets the total, advances it per step, and calls `check()?`
  between steps. Once cancelled, the reader stops with
  `FormatError::Cancelled`.
- `.happypaint`: one step per tile and per mask tile. The total is counted
  from the manifest first, groups included.
- PSD: one step per layer record.
- Images decode in one step, so the bar shows an indeterminate sweep.

### Parallel decode
`read_happypaint` decodes one layer's tile PNGs with rayon. Each tile is
decoded into its own surface, then the tiles are gathered in manifest
order. The output is the same as decoding them one by one.

### Engine
`decode_happypaint`, `decode_psd` and `decode_image` are free functions
(`Send`, no `Session`). `Session::open_document(doc)` installs their result.
`open_happypaint`, `open_psd` and `open_image` are now these two steps.

### App (`loading.rs`)
- `open_path` starts a `Loading`: a thread that reads the file, decodes it
  and sends the result back on a channel. A load already in progress is
  cancelled.
- Each frame polls the channel. A finished load is installed as before:
  the gallery entry, the save path for native files, and the clean state.
- A recovered autosave (`recover_path`) opens as unsaved work with no path,
  and stays out of the gallery.
- A cancelled load leaves the current drawing alone and is silent. A failed
  load shows "Couldn't open …".
- After 150 ms the load shows a modal card: "Opening <name>", a ui_kit
  `progress_bar`, and Cancel. The modal keeps the canvas from taking
  presses while the drawing is about to be replaced. Quicker opens show no
  card.

## Acceptance criteria
1. `Progress` counts steps across clones and cancels (unit test).
2. `read_happypaint_with` and `read_psd_with` reach a fraction of 1, count
   tiles inside groups and mask tiles, and return `Cancelled` when
   cancelled.
3. The existing `.happypaint` round-trip tests pass with parallel decode.
4. App:
   - a file opens in the background and is installed with its path;
   - a cancelled load keeps the current drawing;
   - a missing file reports an error;
   - a recovered autosave opens unsaved, with no path, and stays out of the
     gallery;
   - the card shows and its Cancel works in both themes;
   - the gallery test still passes.

## Out of scope
- Books open on the UI thread, as before.
- Saving in the background.
- Streaming tiles in while a file loads. The document appears when it is
  complete.
