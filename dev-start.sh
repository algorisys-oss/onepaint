#!/usr/bin/env bash
# Run onepaint in debug mode.
#   ./dev-start.sh            build and run (debug profile)
#   ./dev-start.sh --release  build and run optimized
#   ./dev-start.sh -- <args>  pass arguments to the app (e.g. a file to open)
set -euo pipefail
cd "$(dirname "$0")"

command -v cargo >/dev/null || { echo "cargo not found: install Rust from https://rustup.rs" >&2; exit 1; }

profile=()
if [[ "${1:-}" == "--release" ]]; then profile=(--release); shift; fi
[[ "${1:-}" == "--" ]] && shift

export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"
export RUST_LOG="${RUST_LOG:-onepaint=debug,onepaint_engine=debug,onepaint_gpu=debug,wgpu_core=warn,wgpu_hal=warn,naga=warn,eframe=info,egui=warn}"

exec cargo run "${profile[@]}" -p onepaint -- "$@"
