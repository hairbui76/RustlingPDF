#!/usr/bin/env bash
# Stages the Rust processing backend as the Tauri externalBin sidecar and the
# pinned PDFium, qpdf, and Tesseract runtimes as bundled resources.
#
# Expects the release backend build and native runtime installs to exist:
#   cargo build --release --locked -p rustling-processing   (in rust/)
#   bash rust/scripts/install-pdfium.sh
#   bash rust/scripts/install-desktop-tools.sh
# `task desktop:stage-sidecar` runs all steps in order.
#
# Tauri's externalBin contract requires the staged binary to carry the host
# target-triple suffix (binaries/rustling-processing-<triple>[.exe]); the
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

backend_binary="$repo_root/rust/target/release/rustling-processing$exe_suffix"
if [ ! -f "$backend_binary" ]; then
  echo "Release backend binary not found: $backend_binary" >&2
  echo "Run: cargo build --release --locked -p rustling-processing (in rust/)" >&2
  exit 1
fi

pdfium_dir="$repo_root/rust/.pdfium/current"
if [ ! -d "$pdfium_dir" ]; then
  echo "Pinned PDFium runtime not found: $pdfium_dir" >&2
  echo "Run: bash rust/scripts/install-pdfium.sh" >&2
  exit 1
fi

tools_dir="$repo_root/rust/.desktop-tools/current"
qpdf_binary="$tools_dir/qpdf/bin/qpdf$exe_suffix"
tesseract_binary="$tools_dir/tesseract/bin/tesseract$exe_suffix"
tessdata_file="$tools_dir/tesseract/tessdata/eng.traineddata"
if [ ! -f "$qpdf_binary" ] ||
  [ ! -f "$tesseract_binary" ] ||
  [ ! -f "$tessdata_file" ]; then
  echo "Pinned desktop tools are incomplete: $tools_dir" >&2
  echo "Expected qpdf:      $qpdf_binary" >&2
  echo "Expected Tesseract: $tesseract_binary" >&2
  echo "Expected tessdata:  $tessdata_file" >&2
  echo "Run the platform installer in rust/scripts/install-desktop-tools.*" >&2
  exit 1
fi

binaries_dir="$tauri_dir/binaries"
pdfium_resources_dir="$tauri_dir/resources/pdfium"
tools_resources_dir="$tauri_dir/resources/tools"
/bin/rm -rf "$pdfium_resources_dir" "$tools_resources_dir"
mkdir -p "$binaries_dir" "$pdfium_resources_dir" "$tools_resources_dir"

staged_sidecar="$binaries_dir/rustling-processing-$target_triple$exe_suffix"
install -m 755 "$backend_binary" "$staged_sidecar"
# Ship the PDFium shared library together with its license files, exactly as
# rust/scripts/install-pdfium.sh laid them out.
cp -R "$pdfium_dir/." "$pdfium_resources_dir/"
# Ship the complete curated tools layout: executable(s), their private
# runtime libraries, English tessdata, and third-party license notices.
cp -R "$tools_dir/." "$tools_resources_dir/"

# Collapse soname symlinks onto real files.
#
# The curated tools tree keeps `libqpdf.so.30 -> libqpdf.so.30.3.2` as a
# symlink, and this script preserves it — but Tauri's bundler dereferences
# symlinks when it copies resources into the package, so the installer ended up
# carrying two byte-identical 4.4 MB copies of libqpdf. That was measured by
# extracting a shipped .deb and comparing checksums, not inferred; SOURCES.md
# used to assert the opposite.
#
# Keeping the link name and dropping the versioned one is safe because the
# library's own SONAME is `libqpdf.so.30` and that is the exact string qpdf's
# DT_NEEDED asks the loader for — the fully-versioned name is never requested.
# Restricted to same-directory links so nothing outside this tree can be moved.
while IFS= read -r link; do
  # `find` printing nothing still yields one empty line through the heredoc, and
  # `readlink ""` fails, which under `set -e` took the whole script down. That is
  # what broke the macOS and Windows legs while Linux passed: only Linux has a
  # symlink here, so only Linux ever entered the loop with real input.
  [ -n "$link" ] || continue
  link_target="$(readlink "$link" 2>/dev/null)" || continue
  [ -n "$link_target" ] || continue
  case "$link_target" in
    */*) continue ;;
  esac
  link_dir="$(dirname "$link")"
  [ -f "$link_dir/$link_target" ] || continue
  /bin/rm -f "$link"
  mv "$link_dir/$link_target" "$link"
  printf 'staged: collapsed %s (was a symlink to %s)\n' \
    "${link#"$tauri_dir/"}" "$link_target"
done <<EOF
$(find "$tools_resources_dir" -type l 2>/dev/null)
EOF

# Do NOT strip the bundled native binaries.
#
# It looks like free money — `strip --strip-all` takes 0.72 MB off the
# compressed Linux bundle — and it is not. Stripping libqpdf.so.30 leaves a
# library that loads, links, and answers `qpdf --version` correctly, then
# segfaults the moment it processes a document: exit 139 and a zero-byte
# output file. Isolated by running every combination of stripped and
# unstripped executable against stripped and unstripped library; the library
# is the one that matters, the executable is harmless either way.
#
# libpdfium.so and tesseract were verified to survive stripping (a real PDF
# operation through the backend, and byte-identical OCR output), but they are
# worth 0.63 MB on the Linux bundle alone — Windows and macOS binaries are not
# ELF — which does not pay for a per-file allow-list that someone must extend
# and re-verify every time a tool is added.
#
# Smoke-test what was actually staged with system command discovery disabled,
# so a host installation cannot mask an incomplete bundled runtime.
for staged_tool in \
  "$tools_resources_dir/qpdf/bin/qpdf$exe_suffix" \
  "$tools_resources_dir/tesseract/bin/tesseract$exe_suffix"; do
  if ! env -i PATH=/nonexistent "$staged_tool" --version >/dev/null 2>&1; then
    echo "Staged tool is not self-contained: $staged_tool" >&2
    env -i PATH=/nonexistent "$staged_tool" --version >&2 || true
    echo "Re-run the platform installer in rust/scripts/install-desktop-tools.*" >&2
    exit 1
  fi
done
if ! env -i PATH=/nonexistent TESSDATA_PREFIX="$tools_resources_dir/tesseract/tessdata" \
  "$tools_resources_dir/tesseract/bin/tesseract$exe_suffix" --list-langs 2>&1 |
  grep -qx 'eng'; then
  echo "Staged tessdata does not expose the 'eng' language: $tools_resources_dir" >&2
  exit 1
fi

echo "Staged sidecar: $staged_sidecar"
echo "Staged PDFium:  $pdfium_resources_dir"
echo "Staged tools:   $tools_resources_dir"
