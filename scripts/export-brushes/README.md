# Brush asset export

These scripts regenerate `crates/brush/assets/` from happypaint. They run
happypaint's own deterministic tip and grain generators, so the pixels match
happypaint exactly. They also copy the preset table to
`crates/brush/assets/presets.json`.

```sh
cd scripts/export-brushes
bun run dump.ts /tmp/brushdump
python3 topng.py /tmp/brushdump ../../crates/brush/assets   # also writes assets.rs; move it to crates/brush/src/
# presets.json: bun -e "import {BRUSH_PRESETS, BRUSH_CATEGORIES, BRUSH_FAVORITES} from '../../../happypaint/frontend/src/engine/brush/brush-presets.ts'; ..."
```
