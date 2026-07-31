#!/usr/bin/env bash
set -euo pipefail

# Pinned desktop runtime tools. The release matrix currently produces
# linux-x86_64 and macos-arm64 bundles; other Unix targets fail explicitly
# instead of falling back to a host-installed command.
qpdf_version=12.3.2
tesseract_version=5.5.3
tessdata_version=4.1.0

qpdf_linux_url="https://github.com/qpdf/qpdf/releases/download/v${qpdf_version}/qpdf-${qpdf_version}-bin-linux-x86_64.zip"
qpdf_linux_sha256=44f2c53bf784c0143128d80d2b9946e9793962c5bb403b75c0024cb4d8e346b9

# The Linux tesseract command is built by THIS repository's CI, not downloaded
# from a third party: tesseract-ocr/tesseract publishes no Linux binary at all,
# and the third-party static build previously pinned here was uploaded from a
# personal account eight months before that repository had any CI, with no
# signature or attestation of any kind. See rust/scripts/desktop-tools/SOURCES.md.
#
# .github/workflows/desktop-tools-tesseract-linux.yml builds it from
# checksum-pinned official sources against musl and publishes it, attested with
# actions/attest-build-provenance, to the release tag below. That workflow runs
# once per Tesseract version; this script just fetches the result by checksum,
# so release builds are no slower than before.
tesseract_linux_repository="${RUSTLING_DESKTOP_TOOLS_REPOSITORY:-hairbui76/RustlingPDF}"
tesseract_linux_tag="desktop-tools/tesseract-${tesseract_version}"
tesseract_linux_asset="tesseract-${tesseract_version}-linux-x86_64-musl"
tesseract_linux_url="https://github.com/${tesseract_linux_repository}/releases/download/desktop-tools%2Ftesseract-${tesseract_version}/${tesseract_linux_asset}"
# Filled in from the workflow's run summary after the artefact is published.
# See "Bumping the bundled Tesseract" in RELEASING.md.
tesseract_linux_sha256=72bd90b806af05b9ff18021d41ad8d076ec8c86926b46c46f46d20ed2936f95a

# Escape hatch for a maintainer who has published the artefact somewhere else
# (a fork, a pre-release run). Both must be set together; the checksum is still
# enforced, so this never weakens the integrity control.
if [[ -n "${RUSTLING_DESKTOP_TESSERACT_LINUX_URL:-}" ||
  -n "${RUSTLING_DESKTOP_TESSERACT_LINUX_SHA256:-}" ]]; then
  if [[ -z "${RUSTLING_DESKTOP_TESSERACT_LINUX_URL:-}" ||
    -z "${RUSTLING_DESKTOP_TESSERACT_LINUX_SHA256:-}" ]]; then
    echo "RUSTLING_DESKTOP_TESSERACT_LINUX_URL and" \
      "RUSTLING_DESKTOP_TESSERACT_LINUX_SHA256 must be set together" >&2
    exit 1
  fi
  tesseract_linux_url="$RUSTLING_DESKTOP_TESSERACT_LINUX_URL"
  tesseract_linux_sha256="$RUSTLING_DESKTOP_TESSERACT_LINUX_SHA256"
fi

qpdf_source_url="https://github.com/qpdf/qpdf/releases/download/v${qpdf_version}/qpdf-${qpdf_version}.tar.gz"
qpdf_source_sha256=6cba2f9f2cd887d905faeb99e0e51a307b217920d1bbf3e9cfbb2e8178a2deda
tesseract_source_url="https://github.com/tesseract-ocr/tesseract/archive/${tesseract_version}.tar.gz"
tesseract_source_sha256=9218e62793116d42a9f6d14cd9348518b27f382096eea3d0f2d1a24616bb5884
leptonica_version=1.85.0
leptonica_url="https://github.com/DanBloomberg/leptonica/releases/download/${leptonica_version}/leptonica-${leptonica_version}.tar.gz"
leptonica_sha256=3745ae3bf271a6801a2292eead83ac926e3a9bc1bf622e9cd4dd0f3786e17205
zlib_version=1.3.1
zlib_url="https://zlib.net/fossils/zlib-${zlib_version}.tar.gz"
zlib_sha256=9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23
libpng_version=1.6.46
libpng_url="https://download.sourceforge.net/libpng/libpng-${libpng_version}.tar.xz"
libpng_sha256=f3aa8b7003998ab92a4e9906c18d19853e999f9d3bca9bd1668f54fa81707cb1
libjpeg_version=3.1.0
libjpeg_url="https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/${libjpeg_version}/libjpeg-turbo-${libjpeg_version}.tar.gz"
libjpeg_sha256=9564c72b1dfd1d6fe6274c5f95a8d989b59854575d4bbee44ade7bc17aa9bc93
# Leptonica decodes its built-in bitmap font from an embedded TIFF; without
# TIFF support every tesseract invocation prints "Error in pixReadMemTiff:
# function not present" ... "Error in bmfCreate: font pixa not made" to stderr
# and TIFF input support is lost. Built without jbig/lzma/zstd/webp/lerc —
# jbig in particular is GPL-2.0-or-later. Kept identical to the flag set in
# rust/scripts/desktop-tools/build-tesseract-musl.sh.
# 12-bit dual-mode JPEG is disabled. libtiff force-enables it whenever it finds
# a libjpeg-turbo >= 3.0 (the `jpeg12` option only exists on the *other* branch,
# so -Djpeg12=OFF alone does nothing), and the resulting tif_jpeg_12.c.o then
# needs `jpeg12_*` symbols from libjpeg.a. In a fully static link libjpeg.a is
# processed before libtiff.a, so those symbols are already discarded by the time
# libtiff needs them and the final link fails with `undefined reference to
# jpeg12_read_scanlines`. Pre-seeding libtiff's own feature-probe cache variable
# is the switch that actually turns the codec off. 12-bit JPEG-in-TIFF is not a
# format RustlingPDF ever feeds Tesseract (the backend hands it PNG).
libtiff_version=4.7.0
libtiff_url="https://download.osgeo.org/libtiff/tiff-${libtiff_version}.tar.gz"
libtiff_sha256=67160e3457365ab96c5b3286a0903aa6e78bdc44c4bc737d2e486bcecb6ba976

eng_url="https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/${tessdata_version}/eng.traineddata"
eng_sha256=7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2
pdf_ttf_sha256=c7845420925a23d88ed830a63957b8af85a66a8daf8d9fc90e843673b2ef1a59
pdf_config_sha256=54d56e81dfefe289b0d2e4c7bc9bbe662711f5b8255c128b73bcd66efb6bba1a

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace="$(cd "$script_dir/.." && pwd)"
cache_root="$workspace/.desktop-tools"
archive_dir="$cache_root/archives"
current="$cache_root/current"
license_source="$script_dir/desktop-tools"

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    echo "A SHA-256 utility (sha256sum or shasum) is required" >&2
    return 1
  fi
}

verify_file() {
  local path=$1
  local expected=$2
  local actual
  actual=$(sha256_file "$path")
  if [[ "$actual" != "$expected" ]]; then
    echo "Checksum mismatch for $path: expected $expected, received $actual" >&2
    return 1
  fi
}

fetch() {
  local url=$1
  local expected=$2
  local output=$3
  local actual=''

  if [[ -f "$output" ]]; then
    actual=$(sha256_file "$output")
  fi
  if [[ "$actual" == "$expected" ]]; then
    return
  fi

  /bin/rm -f "$output"
  echo "Downloading pinned desktop tool input: $url"
  # Redirects are unavoidable here — GitHub sends release assets to
  # release-assets.githubusercontent.com and SourceForge to a third-party
  # mirror — but they must stay on HTTPS and must not chain indefinitely. The
  # SHA-256 below is the integrity control; these flags stop a hijacked
  # redirect from silently downgrading the transport or looping.
  curl --fail --location --retry 3 \
    --proto '=https' --proto-redir '=https' --max-redirs 5 \
    --output "$output" "$url"
  actual=$(sha256_file "$output")
  if [[ "$actual" != "$expected" ]]; then
    /bin/rm -f "$output"
    echo "Checksum mismatch for $url: expected $expected, received $actual" >&2
    exit 1
  fi
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required build command is unavailable: $1" >&2
    exit 1
  fi
}

copy_tessdata() {
  local install_root=$1
  local source_root=$2
  local eng_archive="$archive_dir/eng-${tessdata_version}.traineddata"
  local tessdata_dir="$install_root/tesseract/tessdata"

  fetch "$eng_url" "$eng_sha256" "$eng_archive"
  mkdir -p "$tessdata_dir/configs"
  cp -f "$eng_archive" "$tessdata_dir/eng.traineddata"
  cp -f "$source_root/tessdata/pdf.ttf" "$tessdata_dir/pdf.ttf"
  cp -f "$source_root/tessdata/configs/pdf" "$tessdata_dir/configs/pdf"

  verify_file "$tessdata_dir/eng.traineddata" "$eng_sha256"
  verify_file "$tessdata_dir/pdf.ttf" "$pdf_ttf_sha256"
  verify_file "$tessdata_dir/configs/pdf" "$pdf_config_sha256"
}

copy_licenses() {
  local install_root=$1
  mkdir -p "$install_root/licenses"
  cp -f "$license_source/THIRD-PARTY-NOTICES.txt" \
    "$install_root/licenses/THIRD-PARTY-NOTICES.txt"
  cp -f "$license_source/SOURCES.md" "$install_root/licenses/SOURCES.md"
  # Every vendored license text, including licenses/qpdf/{LICENSE.txt,NOTICE.md}
  # — the upstream qpdf binary archives carry no license file of their own, so
  # without these the bundle shipped an empty licenses/qpdf/ directory and no
  # qpdf NOTICE, which Apache-2.0 section 4(d) requires.
  #
  # Only the `windows/` subdirectory is dropped on Unix, and it now holds ONLY
  # texts for DLLs that exclusively the Windows bundle ships (libarchive, brotli,
  # bzip2, curl, expat, giflib, LERC, lz4, xz, openjpeg, libpsl, libwebp,
  # libssh2, zstd, libdeflate, libb2, winpthreads). Texts that also apply to the
  # Unix builds live at the top level and are retained here — in particular
  # GPL-3.0.txt,
  # GCC-RUNTIME-EXCEPTION-3.1.txt (the Linux/musl `tesseract` statically links
  # the GPL-3.0-with-exception GCC runtime) and MUSL-COPYRIGHT.txt.
  cp -R "$license_source/license-texts/." "$install_root/licenses/"
  /bin/rm -rf "$install_root/licenses/windows"
}

copy_named_notices() {
  local source_root=$1
  local destination=$2
  mkdir -p "$destination"
  while IFS= read -r notice; do
    cp -f "$notice" "$destination/$(basename "$notice")"
  done < <(
    find "$source_root" -type f \
      \( -iname 'LICENSE*' -o -iname 'LICENCE*' -o -iname 'NOTICE*' \) \
      -print
  )
}

extract_source_archive() {
  local archive=$1
  local destination=$2
  local expected_directory=$3

  mkdir -p "$destination"
  tar -xf "$archive" -C "$destination"
  if [[ ! -d "$destination/$expected_directory" ]]; then
    echo "Archive $archive did not contain $expected_directory" >&2
    exit 1
  fi
}

install_linux_x86_64() {
  require_command unzip
  require_command readelf

  local platform_id=linux-x86_64
  local install_root="$cache_root/${qpdf_version}-${tesseract_version}-${platform_id}"
  local work="$cache_root/work-$platform_id"
  local qpdf_archive="$archive_dir/qpdf-${qpdf_version}-bin-linux-x86_64.zip"
  local tesseract_archive="$archive_dir/${tesseract_linux_asset}"
  local tesseract_source_archive="$archive_dir/tesseract-${tesseract_version}.tar.gz"

  if [[ "$tesseract_linux_sha256" == PENDING_CI_BUILD ]]; then
    cat >&2 <<EOF
No published Linux Tesseract artefact is pinned for ${tesseract_version} yet.

The Linux tesseract command is built by this repository's own CI rather than
downloaded from a third party. Run the "Desktop tools — build Linux Tesseract"
workflow (.github/workflows/desktop-tools-tesseract-linux.yml) with publish=true,
then copy the SHA-256 it reports into tesseract_linux_sha256 in this script.
The full procedure is under "Bumping the bundled Tesseract" in RELEASING.md.

Expected release: https://github.com/${tesseract_linux_repository}/releases/tag/${tesseract_linux_tag}
Expected asset:   ${tesseract_linux_asset}

To build against an artefact published elsewhere, set both
RUSTLING_DESKTOP_TESSERACT_LINUX_URL and RUSTLING_DESKTOP_TESSERACT_LINUX_SHA256.
EOF
    exit 1
  fi

  /bin/rm -rf "$work" "$install_root"
  mkdir -p "$work/qpdf" "$work/tesseract-source" \
    "$install_root/qpdf/bin" "$install_root/qpdf/lib" \
    "$install_root/tesseract/bin" "$install_root/tesseract/lib"

  fetch "$qpdf_linux_url" "$qpdf_linux_sha256" "$qpdf_archive"
  fetch "$tesseract_linux_url" "$tesseract_linux_sha256" "$tesseract_archive"
  fetch "$tesseract_source_url" "$tesseract_source_sha256" "$tesseract_source_archive"

  unzip -q "$qpdf_archive" -d "$work/qpdf"
  local qpdf_binary
  qpdf_binary=$(find "$work/qpdf" -type f -path '*/bin/qpdf' -print -quit)
  if [[ -z "$qpdf_binary" ]]; then
    echo "The qpdf archive did not contain bin/qpdf" >&2
    exit 1
  fi
  local qpdf_root
  qpdf_root=$(cd "$(dirname "$qpdf_binary")/.." && pwd)
  install -m 755 "$qpdf_binary" "$install_root/qpdf/bin/qpdf"
  if [[ -d "$qpdf_root/lib" ]]; then
    cp -R "$qpdf_root/lib/." "$install_root/qpdf/lib/"
  fi
  find "$(dirname "$qpdf_binary")" -maxdepth 1 -type f -name '*.so*' \
    -exec cp -f {} "$install_root/qpdf/lib/" \;

  install -m 755 "$tesseract_archive" "$install_root/tesseract/bin/tesseract"
  # Our CI publishes a fully static musl binary. An ELF INTERP segment would
  # mean it needs a dynamic loader we do not ship, which fails on the user's
  # machine rather than here — catch it now.
  if readelf -l "$install_root/tesseract/bin/tesseract" |
    grep -qi 'program interpreter'; then
    echo "The published Tesseract artefact is dynamically linked:" >&2
    readelf -l "$install_root/tesseract/bin/tesseract" >&2
    exit 1
  fi
  extract_source_archive \
    "$tesseract_source_archive" "$work/tesseract-source" \
    "tesseract-${tesseract_version}"
  copy_tessdata "$install_root" "$work/tesseract-source/tesseract-${tesseract_version}"
  copy_licenses "$install_root"
  copy_named_notices "$qpdf_root" "$install_root/licenses/qpdf"
  copy_named_notices \
    "$work/tesseract-source/tesseract-${tesseract_version}" \
    "$install_root/licenses/tesseract"

  env -i PATH=/nonexistent \
    LD_LIBRARY_PATH="$install_root/qpdf/lib" \
    "$install_root/qpdf/bin/qpdf" --version
  env -i PATH=/nonexistent \
    "$install_root/tesseract/bin/tesseract" --version

  /bin/rm -rf "$current"
  ln -s "$(basename "$install_root")" "$current"
  /bin/rm -rf "$work"
}

cmake_build_install() {
  local source=$1
  local build=$2
  shift 2

  # CMAKE_FIND_FRAMEWORK defaults to FIRST on macOS, which makes find_package()
  # and find_library() prefer a `.framework` bundle over a static archive *even
  # when the archive sits in CMAKE_PREFIX_PATH* — framework directories are
  # searched ahead of every prefix path, not within them. That silently turns a
  # "static" dependency chain into a dylib one. LAST keeps genuine system
  # frameworks findable (the staticness gate below allows /System/Library/)
  # while making our own static prefix win.
  cmake -S "$source" -B "$build" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_FIND_FRAMEWORK=LAST \
    -DCMAKE_OSX_DEPLOYMENT_TARGET=11.0 \
    "$@"
  cmake --build "$build" --parallel
  cmake --install "$build"
}

install_macos_arm64() {
  require_command cmake
  require_command make
  require_command pkg-config
  require_command otool

  local platform_id=macos-arm64
  local install_root="$cache_root/${qpdf_version}-${tesseract_version}-${platform_id}"
  local work="$cache_root/work-$platform_id"
  local source_dir="$work/sources"
  local build_dir="$work/build"
  local prefix="$work/prefix"

  local qpdf_archive="$archive_dir/qpdf-${qpdf_version}.tar.gz"
  local tesseract_archive="$archive_dir/tesseract-${tesseract_version}.tar.gz"
  local leptonica_archive="$archive_dir/leptonica-${leptonica_version}.tar.gz"
  local zlib_archive="$archive_dir/zlib-${zlib_version}.tar.gz"
  local libpng_archive="$archive_dir/libpng-${libpng_version}.tar.xz"
  local libjpeg_archive="$archive_dir/libjpeg-turbo-${libjpeg_version}.tar.gz"
  local libtiff_archive="$archive_dir/tiff-${libtiff_version}.tar.gz"

  /bin/rm -rf "$work" "$install_root"
  mkdir -p "$source_dir" "$build_dir" "$prefix" \
    "$install_root/qpdf/bin" "$install_root/qpdf/lib" \
    "$install_root/tesseract/bin" "$install_root/tesseract/lib"

  fetch "$qpdf_source_url" "$qpdf_source_sha256" "$qpdf_archive"
  fetch "$tesseract_source_url" "$tesseract_source_sha256" "$tesseract_archive"
  fetch "$leptonica_url" "$leptonica_sha256" "$leptonica_archive"
  fetch "$zlib_url" "$zlib_sha256" "$zlib_archive"
  fetch "$libpng_url" "$libpng_sha256" "$libpng_archive"
  fetch "$libjpeg_url" "$libjpeg_sha256" "$libjpeg_archive"
  fetch "$libtiff_url" "$libtiff_sha256" "$libtiff_archive"

  extract_source_archive "$qpdf_archive" "$source_dir" "qpdf-${qpdf_version}"
  extract_source_archive "$tesseract_archive" "$source_dir" "tesseract-${tesseract_version}"
  extract_source_archive "$leptonica_archive" "$source_dir" "leptonica-${leptonica_version}"
  extract_source_archive "$zlib_archive" "$source_dir" "zlib-${zlib_version}"
  extract_source_archive "$libpng_archive" "$source_dir" "libpng-${libpng_version}"
  extract_source_archive \
    "$libjpeg_archive" "$source_dir" "libjpeg-turbo-${libjpeg_version}"
  extract_source_archive "$libtiff_archive" "$source_dir" "tiff-${libtiff_version}"

  (
    cd "$source_dir/zlib-${zlib_version}"
    CFLAGS="-mmacosx-version-min=11.0" ./configure --static --prefix="$prefix"
    make -j2
    make install
  )
  cmake_build_install \
    "$source_dir/libjpeg-turbo-${libjpeg_version}" "$build_dir/libjpeg" \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DENABLE_SHARED=OFF \
    -DENABLE_STATIC=ON \
    -DWITH_TURBOJPEG=OFF
  # PNG_FRAMEWORK is declared *only* under `if(APPLE)` in libpng's CMakeLists and
  # defaults to ON, so PNG_SHARED=OFF alone does not stop libpng from also
  # building and installing a shared `png.framework` bundle next to libpng16.a.
  # That is why the identical PNG_SHARED/PNG_STATIC pair in
  # desktop-tools/build-tesseract-musl.sh is sufficient on Linux but not here.
  cmake_build_install \
    "$source_dir/libpng-${libpng_version}" "$build_dir/libpng" \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_PREFIX_PATH="$prefix" \
    -DPNG_FRAMEWORK=OFF \
    -DPNG_SHARED=OFF \
    -DPNG_STATIC=ON \
    -DPNG_TESTS=OFF \
    -DPNG_TOOLS=OFF
  cmake_build_install \
    "$source_dir/tiff-${libtiff_version}" "$build_dir/libtiff" \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_PREFIX_PATH="$prefix" \
    -DBUILD_SHARED_LIBS=OFF \
    -Dcxx=OFF \
    -Djbig=OFF \
    -Djpeg=ON \
    -DHAVE_JPEGTURBO_DUAL_MODE_8_12=FALSE \
    -Djpeg12=OFF \
    -Dlerc=OFF \
    -Dlzma=OFF \
    -Dold-jpeg=OFF \
    -Dtiff-contrib=OFF \
    -Dtiff-docs=OFF \
    -Dtiff-tests=OFF \
    -Dtiff-tools=OFF \
    -Dwebp=OFF \
    -Dzlib=ON \
    -Dzstd=OFF
  # libtiff 4.7.0's installed CMake package config links `CMath::CMath` without
  # shipping a Find module for it, which makes Leptonica's find_package(TIFF)
  # resolve in CONFIG mode and fail with "Target leptonica links to:
  # CMath::CMath but the target was not found". Removing it falls back to
  # CMake's bundled FindTIFF module. Same treatment as build-tesseract-musl.sh.
  /bin/rm -rf "$prefix/lib/cmake/tiff" "$prefix/lib64/cmake/tiff"
  cmake_build_install \
    "$source_dir/leptonica-${leptonica_version}" "$build_dir/leptonica" \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_PREFIX_PATH="$prefix" \
    -DBUILD_SHARED_LIBS=OFF \
    -DBUILD_PROG=OFF \
    -DSW_BUILD=OFF \
    -DENABLE_GIF=OFF \
    -DENABLE_JPEG=ON \
    -DENABLE_OPENJPEG=OFF \
    -DENABLE_PNG=ON \
    -DENABLE_TIFF=ON \
    -DENABLE_WEBP=OFF \
    -DENABLE_ZLIB=ON

  PKG_CONFIG_PATH="$prefix/lib/pkgconfig" cmake_build_install \
    "$source_dir/tesseract-${tesseract_version}" "$build_dir/tesseract" \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_PREFIX_PATH="$prefix" \
    -DBUILD_SHARED_LIBS=OFF \
    -DBUILD_TRAINING_TOOLS=OFF \
    -DDISABLE_ARCHIVE=ON \
    -DDISABLE_CURL=ON \
    -DENABLE_NATIVE=OFF \
    -DGRAPHICS_DISABLED=ON \
    -DOPENMP_BUILD=OFF \
    -DSW_BUILD=OFF
  PKG_CONFIG_PATH="$prefix/lib/pkgconfig" cmake_build_install \
    "$source_dir/qpdf-${qpdf_version}" "$build_dir/qpdf" \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_PREFIX_PATH="$prefix" \
    -DBUILD_DOC=OFF \
    -DBUILD_SHARED_LIBS=OFF \
    -DBUILD_STATIC_LIBS=ON \
    -DINSTALL_CMAKE_PACKAGE=OFF \
    -DINSTALL_EXAMPLES=OFF \
    -DINSTALL_MANUAL=OFF \
    -DINSTALL_PKGCONFIG=OFF \
    -DREQUIRE_CRYPTO_NATIVE=ON \
    -DUSE_IMPLICIT_CRYPTO=OFF

  install -m 755 "$prefix/bin/qpdf" "$install_root/qpdf/bin/qpdf"
  install -m 755 "$prefix/bin/tesseract" "$install_root/tesseract/bin/tesseract"
  copy_tessdata "$install_root" "$source_dir/tesseract-${tesseract_version}"
  copy_licenses "$install_root"
  copy_named_notices \
    "$source_dir/qpdf-${qpdf_version}" "$install_root/licenses/qpdf"
  copy_named_notices \
    "$source_dir/tesseract-${tesseract_version}" \
    "$install_root/licenses/tesseract"
  copy_named_notices \
    "$source_dir/leptonica-${leptonica_version}" \
    "$install_root/licenses/leptonica"
  copy_named_notices \
    "$source_dir/zlib-${zlib_version}" "$install_root/licenses/zlib"
  copy_named_notices \
    "$source_dir/libpng-${libpng_version}" "$install_root/licenses/libpng"
  copy_named_notices \
    "$source_dir/libjpeg-turbo-${libjpeg_version}" \
    "$install_root/licenses/libjpeg-turbo"
  copy_named_notices \
    "$source_dir/tiff-${libtiff_version}" "$install_root/licenses/libtiff"

  for binary in \
    "$install_root/qpdf/bin/qpdf" \
    "$install_root/tesseract/bin/tesseract"; do
    if otool -L "$binary" | tail -n +2 | awk '{print $1}' |
      grep -Ev '^(/usr/lib/|/System/Library/)' >/dev/null; then
      echo "Non-system dynamic dependency found in statically built $binary:" >&2
      otool -L "$binary" >&2
      exit 1
    fi
    env -i PATH=/nonexistent "$binary" --version
  done

  /bin/rm -rf "$current"
  ln -s "$(basename "$install_root")" "$current"
  /bin/rm -rf "$work"
}

mkdir -p "$archive_dir"
os=$(uname -s)
architecture=$(uname -m)
echo "Installing pinned desktop tools for OS=$os architecture=$architecture"

case "$os/$architecture" in
  Linux/x86_64 | Linux/amd64)
    install_linux_x86_64
    ;;
  Darwin/arm64 | Darwin/aarch64)
    install_macos_arm64
    ;;
  *)
    echo "No pinned desktop-tool artifact set for OS=$os architecture=$architecture" >&2
    echo "Supported release targets: Linux/x86_64 and Darwin/arm64" >&2
    exit 1
    ;;
esac

echo "Pinned desktop tools installed at: $current"
find -L "$current" -type f -print | sort
