# 54: When the GPU can't take the work

## Status
Built (2026-10-19). From the photocraft comparison (`todo.md` §2.1).

## Why
Two failures had no answer. A tile whose sources didn't fit the atlases
was composited with layers missing (the compositor logged "skipped" and
went on). And a lost device (a driver reset, a removed GPU, a timeout) left
the app drawing nothing, with the work since the last autosave at risk.

## What doesn't fit
- `GpuCompositor::run_level` already cuts a frame into batches whose
  sources fit the atlases, and a program reads at most `MAX_SOURCES` (128)
  tiles against an atlas of 256, so a batch fails only past what the
  planner and batching promise (a device with fewer array layers, a
  budget split between 8-bit and f16 atlases).
- If a batch's sources still can't all be resident, nothing of the batch
  is drawn: its tiles keep their pixels, and the cache and filtered slots
  it claimed are forgotten (their images were never filled). The tiles go
  to `take_failed`; the canvas composites them on the CPU on the next
  frame and keeps them there until the CPU has drawn them.
- A whole-canvas layer (spec 53) fills its backdrop in chunks the cache and
  the atlases both hold. If one still fails, every tile reading the layer's
  image fails with it.
- Submits stay bounded: a batch is at most `MAX_JOBS` programs, and the
  heaviest whole-canvas refilter measured (radial blur at 4096²) is
  173 ms, well under the 2 s Windows allows a submit.

## Device loss
`gpu::health::Health` watches the device (`set_device_lost_callback`).
eframe owns the device and can't make a new one, so on loss the app writes
the autosave at once, on the UI thread (`Autosave::save_now`), and keeps a
message up for 30 s asking for a restart; the autosave reopens as unsaved
work at the next launch.

## Tests
- `tiles_that_dont_fit_are_left_for_the_cpu` (gpu): two-slot atlases, a
  tile reading three tiles fails and keeps its pixels, its neighbour draws.
- `a_destroyed_device_reads_as_lost` (gpu), `saves_now_on_the_calling_thread`
  and `a_lost_gpu_is_reported_once` (app).
