#!/usr/bin/env bash
# The perf-budget gate (AGENTS.md: a change that slows a bench by more than
# 5% needs a justification).
#
#   scripts/bench-gate/gate.sh save    # on the commit to compare against
#   scripts/bench-gate/gate.sh check   # on the change; fails past the budget
#
# BENCHES picks the benches (default: the engine's CPU hot paths; the GPU
# ones are noisy on a shared iGPU, see handoff.md). Threads are capped as
# for tests, so runs compare like with like.
set -euo pipefail
cd "$(dirname "$0")/../.."
mode=${1:-check}
benches=${BENCHES:-"-p onepaint-engine --bench hot_paths"}
export RAYON_NUM_THREADS=${RAYON_NUM_THREADS:-2}
case "$mode" in
  save) cargo bench -j 2 $benches -- --save-baseline gate ;;
  check)
    find target/criterion -path '*/change/estimates.json' -delete 2>/dev/null || true
    # Lenient: a bench added since the baseline has nothing to compare yet.
    cargo bench -j 2 $benches -- --baseline-lenient gate
    python3 scripts/bench-gate/check.py target/criterion "${BUDGET:-0.05}"
    ;;
  *) echo "usage: $0 save|check" >&2; exit 2 ;;
esac
