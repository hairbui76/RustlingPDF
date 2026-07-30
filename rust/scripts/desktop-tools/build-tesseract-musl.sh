#!/bin/sh
# Build a fully static, musl-linked Linux x86_64 `tesseract` command from
# checksum-pinned official upstream sources.
#
# This is the *only* producer of the Linux Tesseract binary that RustlingPDF
# ships. It is invoked by .github/workflows/desktop-tools-tesseract-linux.yml
# inside an `alpine:` container; the resulting binary is published to this
# repository's own GitHub Releases with a build-provenance attestation, and
# rust/scripts/install-desktop-tools.sh then fetches that artefact by pinned
# SHA-256. Nothing else in the day-to-day release path runs this script.
#
# Why we build it at all: tesseract-ocr/tesseract publishes no Linux binary
# (its only release asset is the Windows installer), and the third-party
# static build this bundle previously used was uploaded from a personal
# account eight months before that repository had any CI, so it is not
# reproducible from any recorded build and carries no signature, sigstore
# entry or SLSA attestation. A code-signed desktop app must not vouch for a
# binary nobody can rebuild.
#
# Usage (inside the container):
#   sh build-tesseract-musl.sh <output-directory>
#
# POSIX sh on purpose: the Alpine image has no bash.
set -eu

output_directory=${1:?usage: build-tesseract-musl.sh <output-directory>}

# Keep these in lockstep with rust/scripts/install-desktop-tools.sh (the macOS
# source build uses the same dependency set) and with the tables in
# rust/scripts/desktop-tools/SOURCES.md.
tesseract_version=5.5.3
tesseract_url="https://github.com/tesseract-ocr/tesseract/archive/${tesseract_version}.tar.gz"
tesseract_sha256=9218e62793116d42a9f6d14cd9348518b27f382096eea3d0f2d1a24616bb5884
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
# TIFF is not optional in practice. Leptonica decodes its built-in bitmap font
# from an embedded TIFF, so a Leptonica built without TIFF makes every single
# tesseract invocation print
#   Error in pixReadMemTiff: function not present
#   ... Error in bmfCreate: font pixa not made
# to stderr, and drops TIFF input support the previous binary had. It is built
# without jbig/lzma/zstd/webp/lerc: those add nothing here, and jbig in
# particular is GPL-2.0-or-later, which we will not link into an MIT product.
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

work=/tmp/tesseract-musl-build
source_directory="$work/src"
build_directory="$work/build"
prefix="$work/prefix"

rm -rf "$work"
mkdir -p "$source_directory" "$build_directory" "$prefix" "$output_directory"

fetch() {
  url=$1
  expected=$2
  output=$3

  echo "Downloading pinned source: $url"
  # HTTPS only, and a bounded redirect chain: the SHA-256 below is the
  # integrity control, these flags stop a hijacked redirect from downgrading
  # the transport or looping. Matches install-desktop-tools.sh.
  curl --fail --location --retry 3 \
    --proto '=https' --proto-redir '=https' --max-redirs 5 \
    --output "$output" "$url"
  actual=$(sha256sum "$output" | awk '{print $1}')
  if [ "$actual" != "$expected" ]; then
    echo "Checksum mismatch for $url: expected $expected, received $actual" >&2
    exit 1
  fi
}

cmake_build_install() {
  cmake_source=$1
  cmake_build=$2
  shift 2

  cmake -S "$cmake_source" -B "$cmake_build" -G Ninja \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DCMAKE_PREFIX_PATH="$prefix" \
    "$@"
  cmake --build "$cmake_build" --parallel
  cmake --install "$cmake_build"
}

fetch "$tesseract_url" "$tesseract_sha256" "$work/tesseract.tar.gz"
fetch "$leptonica_url" "$leptonica_sha256" "$work/leptonica.tar.gz"
fetch "$zlib_url" "$zlib_sha256" "$work/zlib.tar.gz"
fetch "$libpng_url" "$libpng_sha256" "$work/libpng.tar.xz"
fetch "$libjpeg_url" "$libjpeg_sha256" "$work/libjpeg.tar.gz"
fetch "$libtiff_url" "$libtiff_sha256" "$work/libtiff.tar.gz"

for archive in tesseract.tar.gz leptonica.tar.gz zlib.tar.gz libpng.tar.xz \
  libjpeg.tar.gz libtiff.tar.gz; do
  tar -xf "$work/$archive" -C "$source_directory"
done

# zlib has no usable static-only CMake path; its configure script does.
(
  cd "$source_directory/zlib-${zlib_version}"
  ./configure --static --prefix="$prefix"
  make -j"$(nproc)"
  make install
)
cmake_build_install \
  "$source_directory/libjpeg-turbo-${libjpeg_version}" "$build_directory/libjpeg" \
  -DENABLE_SHARED=OFF \
  -DENABLE_STATIC=ON \
  -DWITH_TURBOJPEG=OFF
cmake_build_install \
  "$source_directory/libpng-${libpng_version}" "$build_directory/libpng" \
  -DPNG_SHARED=OFF \
  -DPNG_STATIC=ON \
  -DPNG_TESTS=OFF \
  -DPNG_TOOLS=OFF
cmake_build_install \
  "$source_directory/tiff-${libtiff_version}" "$build_directory/libtiff" \
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
# libtiff 4.7.0 installs a CMake package config whose exported target links
# `CMath::CMath`, but it does not install a Find module for it. Leptonica's
# `find_package(TIFF)` then resolves in CONFIG mode and fails with
#   Target "leptonica" links to: CMath::CMath but the target was not found.
# Dropping the package config makes find_package(TIFF) fall back to CMake's own
# bundled FindTIFF module, which is what Leptonica expects. Everything libtiff
# needs (zlib, libjpeg, libm) is linked directly by Leptonica and Tesseract, so
# nothing is lost.
rm -rf "$prefix/lib/cmake/tiff" "$prefix/lib64/cmake/tiff"
cmake_build_install \
  "$source_directory/leptonica-${leptonica_version}" "$build_directory/leptonica" \
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

# DISABLE_CURL/DISABLE_ARCHIVE keep network and archive code out of the
# shipped command entirely; GRAPHICS_DISABLED drops the debug viewer;
# BUILD_TRAINING_TOOLS=OFF is what keeps ICU, pango, cairo and the rest of the
# training stack out of the dependency set. ENABLE_NATIVE=OFF is load-bearing:
# it stops the build baking in the builder's -march, so the published artefact
# stays portable across x86_64 machines (Tesseract still detects AVX2/FMA/SSE
# at run time).
PKG_CONFIG_PATH="$prefix/lib/pkgconfig:$prefix/lib64/pkgconfig" \
  cmake_build_install \
  "$source_directory/tesseract-${tesseract_version}" "$build_directory/tesseract" \
  -DBUILD_SHARED_LIBS=OFF \
  -DBUILD_TESTS=OFF \
  -DBUILD_TRAINING_TOOLS=OFF \
  -DDISABLE_ARCHIVE=ON \
  -DDISABLE_CURL=ON \
  -DENABLE_NATIVE=OFF \
  -DGRAPHICS_DISABLED=ON \
  -DOPENMP_BUILD=OFF \
  -DSW_BUILD=OFF \
  -DCMAKE_EXE_LINKER_FLAGS="-static"

install -m 755 "$prefix/bin/tesseract" "$output_directory/tesseract"

# Fail here rather than in the desktop bundle: a binary that still has an
# ELF INTERP segment is dynamically linked and would need a loader we do not
# ship.
if readelf -l "$output_directory/tesseract" | grep -qi 'program interpreter'; then
  echo "Built tesseract is dynamically linked (it has an ELF INTERP segment):" >&2
  readelf -l "$output_directory/tesseract" >&2
  exit 1
fi

echo "Built $(env -i "$output_directory/tesseract" --version | head -n 1)"
sha256sum "$output_directory/tesseract"
