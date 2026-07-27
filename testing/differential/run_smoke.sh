#!/usr/bin/env bash
# Boot the Rust backend, run the differential harness in --rust-only mode against
# it, then tear the backend down. This is the mode that works in this sandbox (no
# Java bootJar / native toolchain required).
#
# Usage:
#   ./run_smoke.sh                 # build if needed, start on 8091, run, stop
#   PORT=9099 ./run_smoke.sh       # different port
#   RUST_BIN=/path/to/binary ./run_smoke.sh
#
# For the FULL Java-vs-Rust diff, do NOT use this script -- run differential.py
# directly with both --rust-url and --java-url (see README.md).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
PORT="${PORT:-8091}"
RUST_BIN="${RUST_BIN:-$REPO_ROOT/rust/target/debug/stirling-processing}"

if [[ ! -x "$RUST_BIN" ]]; then
  echo "Rust binary not found at $RUST_BIN"
  echo "Build it first:  (cd $REPO_ROOT && cargo build -p stirling-processing)  or  task engine:build"
  exit 2
fi

LOG="$(mktemp -t diff-rust-XXXX.log)"
# Run the backend from a throwaway CWD: on startup it creates a ./pipeline/
# watched-folder tree in its working directory, which we do not want inside the
# repo.
WORKDIR="$(mktemp -d -t diff-rust-cwd-XXXX)"
echo "Starting Rust backend on port $PORT (log: $LOG, cwd: $WORKDIR)"
( cd "$WORKDIR" && STIRLING_PORT="$PORT" exec "$RUST_BIN" ) >"$LOG" 2>&1 &
RUST_PID=$!

cleanup() {
  echo "Stopping Rust backend (pid $RUST_PID)"
  kill "$RUST_PID" 2>/dev/null || true
  wait "$RUST_PID" 2>/dev/null || true
  rm -rf "$WORKDIR" 2>/dev/null || true
}
trap cleanup EXIT

# Wait for health; fail fast with the backend log if it never comes up, instead
# of letting every differential case surface as a connection error.
healthy=0
for _ in $(seq 1 60); do
  if curl -fsS -o /dev/null "http://localhost:$PORT/api/v1/info/status" 2>/dev/null; then
    healthy=1
    break
  fi
  sleep 0.5
done
if [[ "$healthy" -ne 1 ]]; then
  echo "ERROR: Rust backend did not become healthy on port $PORT within 30s."
  echo "Last backend log lines ($LOG):"
  tail -n 20 "$LOG" || true
  exit 3
fi

python3 "$HERE/differential.py" --rust-url "http://localhost:$PORT" --rust-only "$@"
