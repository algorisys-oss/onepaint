# 04: History

Sources: `engine/history/history.ts` and `pixel-delta.ts`.

## Behaviour (match happypaint)
- There are undo and redo stacks of entries. Each entry carries its size in
  bytes.
- **Default limits:** `max_entries = 200` and `budget = 192 MiB`. The entry
  cap is clamped to at least 1.
- **`push`:**
  - Clear the redo stack, freeing its bytes.
  - While the stacks are over the byte budget or the entry cap, evict the
    oldest entries. The newest entry is always kept.
  - Set `memory_limited` when an eviction happens because of bytes while
    the stack is still under the entry cap.
- **`transact`:** nested transactions collapse into one compound entry.
  - Undo applies the children newest-first.
  - Redo applies them oldest-first.
- Each stroke produces exactly one entry. There is no time-based
  coalescing.

## onepaint representation (performance)
- **Pixel entries** store `(layer_id, tile_key, before: Option<Arc<Tile>>,
  after: Option<Arc<Tile>>)` for the touched tiles. These are cheap
  copy-on-write snapshots.
  - Their `bytes` count is the size of tiles that are not shared with the
    live surface. As an approximation, count each unique `Arc` once.
- **Structural entries** cover adding, deleting or moving a layer and
  changing its properties. They hold the before/after node data.

## Acceptance tests (`history.test.ts`)
Every test entry is 1 byte.

- **Entry cap:**
  - Pushing 225 entries keeps 200.
  - With the cap set to 5, pushing 12 keeps 5.
  - Lowering the cap from 40 to 10 trims the stack immediately.
- **Byte budget** (budget 3, cap 1000):
  - Push 3: the stack holds 3 entries and `memory_limited` is false.
  - Push 2 more: the stack still holds 3 and `memory_limited` is true.
- **Order:**
  - Undoing a transaction applies `[undo3, undo2, undo1]`.
  - Redo applies the entries in the original order.
- **Undo and redo of a stroke:** they restore the layer tiles bit-exactly.
