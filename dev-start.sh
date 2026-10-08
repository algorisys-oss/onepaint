#!/usr/bin/env bash
# Run onepaint in debug mode.
#   ./dev-start.sh            build and run (debug profile)
#   ./dev-start.sh --release  build and run optimized
#   ./dev-start.sh --watch    rebuild and restart when sources change
#   ./dev-start.sh -- <args>  pass arguments to the app (e.g. a file to open)
# Flags combine: ./dev-start.sh --watch --release -- drawing.happypaint
set -euo pipefail
cd "$(dirname "$0")"

command -v cargo >/dev/null || { echo "cargo not found: install Rust from https://rustup.rs" >&2; exit 1; }

profile=()
watch=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) profile=(--release); shift ;;
    --watch) watch=1; shift ;;
    --) shift; break ;;
    *) break ;;
  esac
done

export RUST_BACKTRACE="${RUST_BACKTRACE:-1}"
export RUST_LOG="${RUST_LOG:-onepaint=debug,onepaint_engine=debug,onepaint_gpu=debug,wgpu_core=warn,wgpu_hal=warn,naga=warn,eframe=info,egui=warn}"

if [[ $watch == 0 ]]; then
  exec cargo run "${profile[@]}" -p onepaint -- "$@"
fi

# ── Watch mode ──
# What counts as a source change: Rust, shaders, manifests and bundled assets.
watched=(crates apps Cargo.toml Cargo.lock)

if command -v watchexec >/dev/null; then
  exec watchexec --restart --exts rs,wgsl,toml,json,png --watch crates --watch apps --watch Cargo.toml \
    -- cargo run "${profile[@]}" -p onepaint -- "$@"
fi
if cargo watch --version >/dev/null 2>&1; then
  exec cargo watch -w crates -w apps -w Cargo.toml -x "run ${profile[*]} -p onepaint -- $*"
fi

# No watcher installed: poll for changed files once a second. A failed build
# keeps the running app open, so a typo doesn't cost you the session.
stamp="$(mktemp -t onepaint-watch.XXXXXX)"
app=""
stop_app() {
  if [[ -n "$app" ]] && kill -0 "$app" 2>/dev/null; then
    kill "$app" 2>/dev/null || true
    wait "$app" 2>/dev/null || true
  fi
  app=""
}
trap 'stop_app; rm -f "$stamp"; exit 0' INT TERM
trap 'stop_app; rm -f "$stamp"' EXIT

changed() {
  [[ -n "$(find "${watched[@]}" -newer "$stamp" -type f \
    \( -name '*.rs' -o -name '*.wgsl' -o -name '*.toml' -o -name '*.json' -o -name '*.png' -o -name 'Cargo.lock' \) \
    -not -path '*/target/*' -print -quit 2>/dev/null)" ]]
}

echo "watching ${watched[*]} (install watchexec or cargo-watch for instant reloads)"
while true; do
  touch "$stamp"
  if cargo build "${profile[@]}" -p onepaint; then
    stop_app
    cargo run -q "${profile[@]}" -p onepaint -- "$@" &
    app=$!
  else
    echo "build failed; waiting for the next change" >&2
  fi
  until changed; do
    sleep 1
  done
  echo "── change detected, rebuilding ──"
done
