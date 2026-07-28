#!/usr/bin/env bash
# Linux signed-upgrade e2e proof — host-side orchestrator.
#
# Runs the ENTIRE dev-update e2e flow inside a Docker container, so the only
# host requirement is Docker itself (no webkit headers, no node, no rust
# needed on the host). The container:
#
#   1. installs npm dependencies (cached),
#   2. generates the throwaway dev updater keypair (first run only),
#   3. builds + stages the Rust backend sidecar and pinned PDFium,
#   4. builds the signed v99.0.0 updater artifact + latest.json,
#   5. builds a v0.0.1 AppImage pinned to the dev pubkey + localhost endpoint,
#   6. serves the update on localhost:8090 (inside the container),
#   7. launches the v0.0.1 AppImage under Xvfb and drives it over the WebKit
#      remote inspector: asserts the update is detected (0.0.1 -> 99.0.0),
#      that tampered/mis-signed updates are REJECTED, and (with --install)
#      that the good update downloads, signature-verifies, installs (the
#      AppImage on disk is replaced) and the relaunched app reports 99.0.0.
#
# Usage:
#   bash frontend/scripts/dev-update-test/run-e2e-container.sh              # check-only
#   bash frontend/scripts/dev-update-test/run-e2e-container.sh --install   # full proof
#   bash frontend/scripts/dev-update-test/run-e2e-container.sh --skip-build # reuse bundles
#   bash frontend/scripts/dev-update-test/run-e2e-container.sh --shell     # debug shell
#
# Caching (all reruns are incremental):
#   E2E_CACHE_DIR (default ~/.cache/rustlingpdf-update-e2e) holds
#     cargo-registry/  — crates.io cache shared into CARGO_HOME/registry
#     tauri-target/    — src-tauri cargo target dir
#     npm/             — npm download cache
#     tauri-tools/     — tauri CLI's AppImage tooling cache (~/.cache/tauri)
#   The worktree itself caches: rust/target (backend build), rust/.pdfium,
#   frontend/node_modules, scripts/dev-update-test/{.keys,.update-dist,.e2e-work}.
#
# Evidence lands in frontend/scripts/dev-update-test/.e2e-work/evidence/.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
IMAGE="${E2E_IMAGE:-rustlingpdf-update-e2e}"
CACHE_DIR="${E2E_CACHE_DIR:-$HOME/.cache/rustlingpdf-update-e2e}"

command -v docker >/dev/null 2>&1 || { echo "Error: docker is required"; exit 1; }

MODE="run"
ARGS=()
for arg in "$@"; do
  case "$arg" in
    --shell) MODE="shell" ;;
    *) ARGS+=("$arg") ;;
  esac
done

mkdir -p "$CACHE_DIR"/{cargo-registry,tauri-target,npm,tauri-tools}

echo "=== Building container image ($IMAGE) ==="
docker build -t "$IMAGE" "$SCRIPT_DIR/container"

# Everything runs inside the container's own network namespace: the update
# server (8090) and the WebKit inspector (9222) never touch host ports.
DOCKER_ARGS=(
  --rm --init
  --shm-size=2g
  -v "$REPO_ROOT:$REPO_ROOT"
  -w "$REPO_ROOT"
  -v "$CACHE_DIR/cargo-registry:/usr/local/cargo/registry"
  -v "$CACHE_DIR/tauri-target:$REPO_ROOT/frontend/editor/src-tauri/target"
  -v "$CACHE_DIR/npm:/cache/npm"
  -v "$CACHE_DIR/tauri-tools:/root/.cache/tauri"
  -e npm_config_cache=/cache/npm
)

# Always restore ownership of everything the container (running as root)
# wrote into the bind-mounted worktree, even when the run fails.
HOST_UID="$(id -u)"
HOST_GID="$(id -g)"
restore_ownership() {
  echo "=== Restoring worktree ownership ($HOST_UID:$HOST_GID) ==="
  docker run --rm -v "$REPO_ROOT:$REPO_ROOT" "$IMAGE" \
    chown -R "$HOST_UID:$HOST_GID" "$REPO_ROOT" || true
}
trap restore_ownership EXIT

if [ "$MODE" = "shell" ]; then
  docker run -it "${DOCKER_ARGS[@]}" "$IMAGE" bash
  exit $?
fi

echo ""
echo "=== Running e2e inside container ==="
STATUS=0
docker run "${DOCKER_ARGS[@]}" "$IMAGE" \
  bash "$REPO_ROOT/frontend/scripts/dev-update-test/container-e2e.sh" "${ARGS[@]+"${ARGS[@]}"}" || STATUS=$?

echo ""
echo "Evidence: $SCRIPT_DIR/.e2e-work/evidence/"
exit $STATUS
