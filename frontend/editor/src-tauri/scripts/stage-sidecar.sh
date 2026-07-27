#!/usr/bin/env bash
# Stages the Rust processing backend as the Tauri externalBin sidecar and the
# pinned PDFium runtime as a bundled resource.
#
# Expects the release backend build and the PDFium install to exist already:
#   cargo build --release --locked -p stirling-processing   (in rust/)
#   bash rust/scripts/install-pdfium.sh
# `task desktop:stage-sidecar` runs all three steps in order.
#
# Tauri's externalBin contract requires the staged binary to carry the host
# target-triple suffix (binaries/stirling-processing-<triple>[.exe]); the
# bundler strips the suffix and installs the binary next to the app
# executable, where the launcher resolves it via ShellExt::sidecar.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri_dir="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$tauri_dir/../../.." && pwd)"

target_triple=$(rustc -vV | sed -n 's/^host: //p')
if [ -z "$target_triple" ]; then
  echo "Unable to determine the host target triple from rustc -vV" >&2
  exit 1
fi

exe_suffix=""
case "$target_triple" in
  *windows*) exe_suffix=".exe" ;;
esac

backend_binary="$repo_root/rust/target/release/stirling-processing$exe_suffix"
if [ ! -f "$backend_binary" ]; then
  echo "Release backend binary not found: $backend_binary" >&2
  echo "Run: cargo build --release --locked -p stirling-processing (in rust/)" >&2
  exit 1
fi

pdfium_dir="$repo_root/rust/.pdfium/current"
if [ ! -d "$pdfium_dir" ]; then
  echo "Pinned PDFium runtime not found: $pdfium_dir" >&2
  echo "Run: bash rust/scripts/install-pdfium.sh" >&2
  exit 1
fi

binaries_dir="$tauri_dir/binaries"
resources_dir="$tauri_dir/resources/pdfium"
mkdir -p "$binaries_dir" "$resources_dir"

staged_sidecar="$binaries_dir/stirling-processing-$target_triple$exe_suffix"
install -m 755 "$backend_binary" "$staged_sidecar"
# Ship the PDFium shared library together with its license files, exactly as
# rust/scripts/install-pdfium.sh laid them out.
cp -R "$pdfium_dir/." "$resources_dir/"

echo "Staged sidecar: $staged_sidecar"
echo "Staged PDFium:  $resources_dir"
