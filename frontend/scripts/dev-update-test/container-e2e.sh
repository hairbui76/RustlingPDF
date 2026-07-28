#!/usr/bin/env bash
# Linux signed-upgrade e2e proof — container-side flow.
#
# Runs inside the image built from container/Dockerfile (see
# run-e2e-container.sh for the mounts/env it expects). Root of the repo is
# the working directory.
#
# Flags:
#   --install     also run the negative-signature tests and the real
#                 download + install + relaunch proof (recommended)
#   --skip-build  reuse existing v99 updater artifact and v0.0.1 AppImage
set -euo pipefail

INSTALL=false
SKIP_BUILD=false
for arg in "$@"; do
  case "$arg" in
    --install)    INSTALL=true ;;
    --skip-build) SKIP_BUILD=true ;;
    *) echo "Unknown flag: $arg"; exit 2 ;;
  esac
done

REPO_ROOT="$(pwd)"
FRONTEND_DIR="$REPO_ROOT/frontend"
EDITOR_DIR="$FRONTEND_DIR/editor"
TAURI_DIR="$EDITOR_DIR/src-tauri"
SCRIPT_DIR="$FRONTEND_DIR/scripts/dev-update-test"
KEYS_DIR="$SCRIPT_DIR/.keys"
DIST_DIR="$SCRIPT_DIR/.update-dist"
WORK_DIR="$SCRIPT_DIR/.e2e-work"
LOG_DIR="$WORK_DIR/logs"
EVIDENCE_DIR="$WORK_DIR/evidence"
UPDATE_PORT=8090
INSPECTOR_PORT=9222

export HOME="${HOME:-/root}"
# AppImage tooling + the built AppImage itself must run without FUSE.
export APPIMAGE_EXTRACT_AND_RUN=1
export NO_STRIP=1
export CI=true

mkdir -p "$WORK_DIR" "$LOG_DIR" "$EVIDENCE_DIR" "$DIST_DIR"

echo ""
echo "=== [1/7] npm dependencies ==="
if [ ! -x "$FRONTEND_DIR/node_modules/.bin/tauri" ]; then
  (cd "$FRONTEND_DIR" && npm ci --no-audit --no-fund 2>&1 | tail -3)
else
  echo "  node_modules present — skipping npm ci"
fi

echo ""
echo "=== [2/7] Dev updater keys ==="
# Idempotent: reuses an existing key pair, always rewrites the config
# override (so config-shape fixes propagate to stale generated files).
bash "$SCRIPT_DIR/setup-dev-updater.sh"
echo "  dev key:   $KEYS_DIR/dev-update-key"
# A second throwaway key that signs the negative-test manifest. It is a
# perfectly valid minisign key — just NOT the one pinned in the app config —
# so a rejection proves the updater checks against the pinned pubkey.
if [ ! -f "$WORK_DIR/wrong-key" ]; then
  (cd "$EDITOR_DIR" && npx tauri signer generate -w "$WORK_DIR/wrong-key" --ci -p "")
fi
echo "  wrong key: $WORK_DIR/wrong-key (for the negative signature test)"

echo ""
echo "=== [3/7] Stage Rust backend sidecar + PDFium ==="
(cd "$REPO_ROOT" && task desktop:stage-sidecar 2>&1 | tail -4)

echo ""
echo "=== [4/7] Build signed v99.0.0 updater artifact ==="
# The driver rewrites the SERVED manifest ($DIST_DIR/latest.json) per test
# phase (same-version, downgrade, wrong-key signature, tampered url), so a
# run killed mid-driver leaves a poisoned manifest behind. The PRISTINE copy
# below is written exactly once per build and never touched by the driver:
# it is the sole authority for resolving the v99 artifact on reuse runs, and
# the served manifest is restored from it before every run.
PRISTINE_MANIFEST="$WORK_DIR/latest-good.pristine.json"
V99_ARTIFACT=""
find_v99_artifact() {
  # The authoritative artifact is the one the pristine manifest points to —
  # the driver asserts the installed AppImage's sha against it, so resolving
  # it any other way (e.g. globbing the dist dir, or trusting the served
  # latest.json a killed run may have tampered) can drift from what a green
  # run must serve.
  V99_ARTIFACT=""
  [ -f "$PRISTINE_MANIFEST" ] || return 0
  local fname
  fname="$(python3 -c '
import json, sys, urllib.parse
url = json.load(open(sys.argv[1]))["platforms"]["linux-x86_64"]["url"]
print(urllib.parse.unquote(url.rsplit("/", 1)[-1]))
' "$PRISTINE_MANIFEST" 2>/dev/null)" || return 0
  [ -n "$fname" ] && [ -f "$DIST_DIR/$fname" ] && V99_ARTIFACT="$DIST_DIR/$fname"
  return 0
}
if [ "$SKIP_BUILD" = true ]; then
  find_v99_artifact
fi
if [ -n "$V99_ARTIFACT" ]; then
  echo "  Reusing: $V99_ARTIFACT"
else
  bash "$SCRIPT_DIR/build-dev-update.sh" 2>&1 | tail -8
  cp "$DIST_DIR/latest.json" "$PRISTINE_MANIFEST"
  find_v99_artifact
fi
[ -n "$V99_ARTIFACT" ] || { echo "Error: v99 updater artifact (per the pristine manifest) not found in $DIST_DIR"; exit 1; }
# Restore the served manifest from the pristine copy (a killed previous run
# may have left a phase manifest in place) and hand the driver its own copy.
cp "$PRISTINE_MANIFEST" "$DIST_DIR/latest.json"
cp "$PRISTINE_MANIFEST" "$WORK_DIR/latest-good.json"

echo ""
echo "=== [5/7] Build v0.0.1 base AppImage (dev pubkey + localhost endpoint) ==="
# The cached build product is the PRISTINE copy; every run gets its own
# working copy. T5 replaces the working copy in place (that IS the install
# proof), so reusing it directly would poison the next --skip-build run
# with an already-upgraded v99 binary posing as the v0.0.1 base.
BASE_PRISTINE="$WORK_DIR/base/RustlingPDF-0.0.1.pristine.AppImage"
BASE_APPIMAGE="$WORK_DIR/base/RustlingPDF-0.0.1.AppImage"
if [ "$SKIP_BUILD" = true ] && [ -f "$BASE_PRISTINE" ]; then
  echo "  Reusing: $BASE_PRISTINE"
else
  (
    cd "$EDITOR_DIR"
    npx tsx scripts/setup-env.mts --desktop
    node scripts/generate-icons.js
    node scripts/build-provisioner.mjs
    TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEYS_DIR/dev-update-key")" \
    TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
    npx tauri build --config src-tauri/tauri.conf.dev-update.json --bundles appimage 2>&1 | tail -5
  )
  BUILT_BASE="$(find "$TAURI_DIR/target/release/bundle/appimage" -maxdepth 1 -name "*_0.0.1_*.AppImage" | sort | tail -1)"
  [ -n "$BUILT_BASE" ] || { echo "Error: v0.0.1 AppImage not found"; exit 1; }
  mkdir -p "$WORK_DIR/base"
  cp "$BUILT_BASE" "$BASE_PRISTINE"
fi
cp "$BASE_PRISTINE" "$BASE_APPIMAGE"
chmod +x "$BASE_APPIMAGE"

echo ""
echo "=== [6/7] Update server + negative-test artifacts ==="
# Wrong-key signature over the REAL artifact bytes (valid minisign signature,
# wrong signer) — proves pubkey pinning.
cp "$V99_ARTIFACT" "$WORK_DIR/artifact-for-wrong-sig"
(cd "$EDITOR_DIR" && \
  TAURI_SIGNING_PRIVATE_KEY="$(cat "$WORK_DIR/wrong-key")" \
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  npx tauri signer sign "$WORK_DIR/artifact-for-wrong-sig")
mv "$WORK_DIR/artifact-for-wrong-sig.sig" "$WORK_DIR/wrong-key.sig"
rm -f "$WORK_DIR/artifact-for-wrong-sig"

# Tampered artifact (one byte flipped mid-file) served next to the good one,
# with the GOOD signature — proves content binding.
rm -f "$DIST_DIR"/tampered-*
TAMPERED_NAME="tampered-$(basename "$V99_ARTIFACT")"
python3 - "$V99_ARTIFACT" "$DIST_DIR/$TAMPERED_NAME" <<'PYEOF'
import sys
src, dst = sys.argv[1], sys.argv[2]
data = bytearray(open(src, 'rb').read())
data[len(data) // 2] ^= 0xFF
open(dst, 'wb').write(bytes(data))
PYEOF
echo "  tampered artifact: $TAMPERED_NAME"

(cd "$DIST_DIR" && exec python3 -m http.server "$UPDATE_PORT" >"$LOG_DIR/update-server.log" 2>&1) &
SERVER_PID=$!
cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  # productName is "Stirling PDF" (space); the working-copy AppImage is
  # RustlingPDF-0.0.1.AppImage — match both process shapes.
  pkill -f "Stirling PDF" 2>/dev/null || true
  pkill -f "RustlingPDF-0.0.1" 2>/dev/null || true
  pkill -f "stirling-processing" 2>/dev/null || true
  pkill Xvfb 2>/dev/null || true
}
trap cleanup EXIT
server_up=""
for _ in $(seq 1 30); do
  if curl -sf "http://localhost:$UPDATE_PORT/latest.json" >/dev/null; then
    server_up=1
    break
  fi
  sleep 1
done
[ -n "$server_up" ] || { echo "Error: update server not responding after 30s"; exit 1; }
echo "  serving $DIST_DIR on :$UPDATE_PORT"

echo ""
echo "=== [7/7] Launch v0.0.1 app under Xvfb and drive the updater ==="
export DISPLAY=:99
Xvfb :99 -screen 0 1280x800x24 >"$LOG_DIR/xvfb.log" 2>&1 &
display_up=""
for _ in $(seq 1 30); do
  if [ -e /tmp/.X11-unix/X99 ]; then
    display_up=1
    break
  fi
  sleep 1
done
[ -n "$display_up" ] || { echo "Error: Xvfb display :99 did not come up after 30s"; exit 1; }

INSTALL_FLAG=""
[ "$INSTALL" = true ] && INSTALL_FLAG="--install"

python3 "$SCRIPT_DIR/e2e-driver.py" \
  --appimage "$BASE_APPIMAGE" \
  --v99-artifact "$V99_ARTIFACT" \
  --tampered-name "$TAMPERED_NAME" \
  --wrong-sig "$WORK_DIR/wrong-key.sig" \
  --dist-dir "$DIST_DIR" \
  --work-dir "$WORK_DIR" \
  --inspector-port "$INSPECTOR_PORT" \
  --update-port "$UPDATE_PORT" \
  $INSTALL_FLAG
