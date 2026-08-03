#!/usr/bin/env bash
# Repack the Linux bundles with stronger compression, then re-sign them.
#
# Tauri's bundler writes the .deb's data.tar with gzip and the AppImage's
# squashfs with light zstd. Measured on the v0.0.5 artifacts, the identical
# content repacked costs 25% less for the deb (60.7 -> 45.4 MB, xz -6) and 8%
# less for the AppImage (126.8 -> 116.7 MB, squashfs zstd-22). Nothing about the
# content changes — only the container's codec — so this is the rare size win
# with no behavioural surface at all.
#
# Compatibility was verified against the real artifacts, not assumed: dpkg has
# accepted data.tar.xz members since 2009 (it is Debian's own default). The
# AppImage stays on zstd — just at maximum level with 1 MB blocks — because the
# shipped runtime's squashfs reader answers "uses xz compression, this version
# supports only zlib, zstd" when handed an xz filesystem. xz would have saved
# 19.7 MB instead of ~10, and an earlier check that claimed the runtime could
# read it turned out to be reading a stale extraction directory. Note the
# runtime also exits 0 on a failed extraction, so the AppRun existence check
# below is the actual verification, not the exit code.
#
# Re-signing is why this script exists instead of a post-release fixup: tauri
# signs the bundles during `tauri build`, so any later repack invalidates the
# .sig. This runs between build and collect, and produces fresh signatures with
# `tauri signer sign` when the signing key is in the environment (the same env
# `tauri build` used). Without the key it repacks and leaves the stale .sig
# for the collect step to catch — which is what a keyless smoke run wants.
set -euo pipefail

bundle_root="${1:?usage: recompress-linux-bundles.sh <bundle_root>}"

resign() {
  local file="$1"
  if [ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
    echo "recompress: no signing key in env; leaving ${file##*/} unsigned"
    /bin/rm -f "${file}.sig"
    return 0
  fi
  npx tauri signer sign \
    --private-key "${TAURI_SIGNING_PRIVATE_KEY}" \
    ${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:+--password "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD}"} \
    "${file}" >/dev/null
  [ -f "${file}.sig" ] || { echo "::error::re-sign produced no ${file##*/}.sig"; exit 1; }
}

shopt -s nullglob

# ── .deb: data.tar.gz -> data.tar.xz ────────────────────────────────────────
for deb in "${bundle_root}"/deb/*.deb; do
  work="$(mktemp -d)"
  before=$(stat -c%s "${deb}")
  (cd "${work}" && ar x "$(realpath "${deb}")" 2>/dev/null || ar x "${deb}")
  if [ ! -f "${work}/data.tar.gz" ]; then
    echo "recompress: ${deb##*/} has no data.tar.gz (already repacked?); skipping"
    /bin/rm -rf "${work}"
    continue
  fi
  gzip -d "${work}/data.tar.gz"
  xz -6 -T0 "${work}/data.tar"
  # Member order is load-bearing for dpkg: debian-binary first.
  (cd "${work}" && ar rc repacked.deb debian-binary control.tar.* data.tar.xz)
  # Prove the result is a deb dpkg can read before letting it replace the
  # original; a corrupt archive must fail the build here, not on a user.
  dpkg-deb --info "${work}/repacked.deb" >/dev/null
  dpkg-deb --contents "${work}/repacked.deb" >/dev/null
  mv -f "${work}/repacked.deb" "${deb}"
  /bin/rm -rf "${work}"
  after=$(stat -c%s "${deb}")
  echo "recompress: ${deb##*/} $((before / 1048576)) MB -> $((after / 1048576)) MB"
  resign "${deb}"
done

# ── .AppImage: squashfs -> zstd-22/1M ───────────────────────────────────────────────
for appimage in "${bundle_root}"/appimage/*.AppImage; do
  work="$(mktemp -d)"
  before=$(stat -c%s "${appimage}")
  # The squashfs begins where the runtime ELF ends; read that from the ELF
  # section headers rather than scanning for magic bytes, which can (and did,
  # in testing) match stray strings inside the runtime.
  offset=$(python3 - "${appimage}" <<'PY'
import struct, sys
d = open(sys.argv[1], 'rb').read(72)
shoff, = struct.unpack_from('<Q', d, 40)
shent, shnum = struct.unpack_from('<HH', d, 58)
print(shoff + shent * shnum)
PY
)
  head -c "${offset}" "${appimage}" > "${work}/runtime"
  tail -c "+$((offset + 1))" "${appimage}" > "${work}/payload.sqfs"
  python3 - "${work}/payload.sqfs" <<'PY'
import sys
assert open(sys.argv[1], 'rb').read(4) == b'hsqs', "squashfs offset did not land on the superblock"
PY
  unsquashfs -q -d "${work}/AppDir" "${work}/payload.sqfs" >/dev/null
  # -all-root matches appimagetool's ownership. zstd because it is the only
  # strong codec the runtime reads (see header); level 22 + 1 MB blocks are
  # where the measured saving comes from.
  mksquashfs "${work}/AppDir" "${work}/repacked.sqfs" \
    -comp zstd -Xcompression-level 22 -b 1M -all-root -noappend -quiet -no-progress >/dev/null
  cat "${work}/runtime" "${work}/repacked.sqfs" > "${work}/repacked.AppImage"
  chmod +x "${work}/repacked.AppImage"
  # Prove the shipped runtime can still open its own payload. --appimage-extract
  # uses the runtime's embedded squashfs reader and needs no FUSE, so it works
  # on a bare CI runner.
  (cd "${work}" && ./repacked.AppImage --appimage-extract >/dev/null 2>&1)
  [ -e "${work}/squashfs-root/AppRun" ] || { echo "::error::repacked AppImage failed self-extraction"; exit 1; }
  mv -f "${work}/repacked.AppImage" "${appimage}"
  /bin/rm -rf "${work}"
  after=$(stat -c%s "${appimage}")
  echo "recompress: ${appimage##*/} $((before / 1048576)) MB -> $((after / 1048576)) MB"
  resign "${appimage}"
done
