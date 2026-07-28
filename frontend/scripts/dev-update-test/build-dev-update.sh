#!/usr/bin/env bash
# Builds a signed update bundle at version 99.0.0 using the dev key pair.
# After this runs, start the server with: npm run tauri:serve-dev-update
# The server will automatically serve the real bundle instead of the placeholder.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KEYS_DIR="$SCRIPT_DIR/.keys"
FRONTEND_DIR="$SCRIPT_DIR/../.."
OUTPUT_DIR="$SCRIPT_DIR/.update-dist"

# ── pre-flight ────────────────────────────────────────────────────────────────
if [ ! -f "$KEYS_DIR/dev-update-key" ]; then
    echo "Error: dev key not found."
    echo "Run setup first: npm run tauri:setup-dev-update"
    exit 1
fi

PRIVATE_KEY="$(cat "$KEYS_DIR/dev-update-key")"
VERSION="99.0.0"

mkdir -p "$OUTPUT_DIR"

# ── detect platform ───────────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

# With `createUpdaterArtifacts: true` (Tauri v2 updater format) the Linux
# updater artifact is the signed AppImage itself (foo.AppImage + .sig); the
# legacy "v1Compatible" mode wraps it (foo.AppImage.tar.gz + .sig). Probe the
# v1 shape first, fall back to v2. Globs are version-scoped so stale bundles
# from earlier runs in a cached target dir can never be picked up.
case "$OS" in
  Linux)
    BUNDLES="appimage"
    TAURI_PLATFORM="linux-x86_64"
    BUNDLE_GLOBS=("*_${VERSION}_*.AppImage.tar.gz" "*_${VERSION}_*.AppImage")
    ;;
  Darwin)
    BUNDLES="app"
    TAURI_PLATFORM="darwin-$([ "$ARCH" = "arm64" ] && echo aarch64 || echo x86_64)"
    BUNDLE_GLOBS=("*.app.tar.gz")
    ;;
  *)
    echo "Use build-dev-update.ps1 on Windows."
    exit 1
    ;;
esac

# ── build ─────────────────────────────────────────────────────────────────────
echo "Building signed update bundle (version $VERSION, platform $TAURI_PLATFORM)..."
echo "This takes a few minutes — Rust is compiling."
echo ""

cd "$FRONTEND_DIR/editor"

# Prepare desktop env/assets before building
npx tsx scripts/setup-env.mts --desktop && node scripts/generate-icons.js
# Build the Windows installer provisioner + thumbnail-handler (no-op on macOS/Linux).
# The WiX fragment references these binaries, so light.exe fails to bind without them.
node scripts/build-provisioner.mjs
# Build + stage the Rust backend sidecar and PDFium runtime; tauri build fails
# without the externalBin/resources staged in src-tauri.
(cd "$FRONTEND_DIR/.." && task desktop:stage-sidecar)

TAURI_SIGNING_PRIVATE_KEY="$PRIVATE_KEY" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
npx tauri build \
  --config "{\"version\":\"$VERSION\"}" \
  --bundles "$BUNDLES"

# ── locate outputs ────────────────────────────────────────────────────────────
BUNDLE=""
SIG=""
for glob in "${BUNDLE_GLOBS[@]}"; do
    BUNDLE="$(find src-tauri/target -name "$glob" -not -name "*.sig" 2>/dev/null | sort | tail -1)"
    if [ -n "$BUNDLE" ]; then
        SIG="$(find src-tauri/target -name "$(basename "$BUNDLE").sig" 2>/dev/null | sort | tail -1)"
        break
    fi
done

if [ -z "$BUNDLE" ]; then
    echo "Error: bundle matching '${BUNDLE_GLOBS[*]}' not found in src-tauri/target."
    exit 1
fi
if [ -z "$SIG" ]; then
    echo "Error: .sig file not found alongside bundle."
    exit 1
fi

BUNDLE_FILENAME="$(basename "$BUNDLE")"
SIGNATURE="$(cat "$SIG")"
PUB_DATE="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# ── assemble serve directory ──────────────────────────────────────────────────
cp "$BUNDLE" "$OUTPUT_DIR/$BUNDLE_FILENAME"

cat > "$OUTPUT_DIR/latest.json" << EOF
{
  "version": "$VERSION",
  "notes": "Dev test update v$VERSION (local signed build)",
  "pub_date": "$PUB_DATE",
  "platforms": {
    "$TAURI_PLATFORM": {
      "signature": "$SIGNATURE",
      "url": "http://localhost:8090/$BUNDLE_FILENAME"
    }
  }
}
EOF

# ── done ──────────────────────────────────────────────────────────────────────
echo ""
echo "Build complete."
echo "  bundle  : $OUTPUT_DIR/$BUNDLE_FILENAME"
echo "  manifest: $OUTPUT_DIR/latest.json"
echo ""
echo "Start the server:  npm run tauri:serve-dev-update"
echo "Run the app:       npm run tauri:dev-with-update"
echo ""
echo "The running app (v0.0.1) will see v$VERSION as an available update."
echo "Clicking 'Install' will download and verify the real signed bundle."
