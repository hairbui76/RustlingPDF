# Bundled desktop-tool sources

Every remote input used by `install-desktop-tools.sh` and
`install-desktop-tools.ps1` is version-pinned and verified with SHA-256 before
it is extracted, executed, or compiled.

## Provenance policy

An artefact goes into a code-signed RustlingPDF bundle only if it is one of:

1. a **first-party release asset** of the project that owns the code, or
2. built **by RustlingPDF's own CI** from checksum-pinned official sources,
   with an `actions/attest-build-provenance` attestation.

Third-party redistributables do not qualify. That rule is why the Linux
Tesseract command is now built in-house — see "Linux x86_64" below.

## Shared data

| Input | Version | URL | SHA-256 | License |
|---|---:|---|---|---|
| Tesseract source/config files | 5.5.3 | `https://github.com/tesseract-ocr/tesseract/archive/5.5.3.tar.gz` | `9218e62793116d42a9f6d14cd9348518b27f382096eea3d0f2d1a24616bb5884` | Apache-2.0 |
| English fast traineddata | 4.1.0 | `https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/4.1.0/eng.traineddata` | `7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2` | Apache-2.0 |

The Tesseract source archive supplies `tessdata/configs/pdf` (SHA-256
`54d56e81dfefe289b0d2e4c7bc9bbe662711f5b8255c128b73bcd66efb6bba1a`)
and `tessdata/pdf.ttf` (SHA-256
`c7845420925a23d88ed830a63957b8af85a66a8daf8d9fc90e843673b2ef1a59`).
Both files are byte-identical between Tesseract 5.5.0 and 5.5.3, so those two
checksums are unchanged by the version bump.

Caveat: `…/archive/5.5.3.tar.gz` is a GitHub **auto-generated** source tarball,
not an uploaded release asset. GitHub changed the archive compressor in
January 2023 and silently invalidated every pinned checksum of this URL shape
across the platform; after the rollback GitHub committed only to keeping these
archives byte-stable "for no less than a year" with six months' notice before
any future change, not to permanent immutability. A checksum pinned against
this URL therefore carries a small standing risk that a first-party uploaded
asset does not. If this pin ever fails in CI, verify against the tag contents
before assuming compromise.

## Linux x86_64

| Input | Version | URL | SHA-256 | License |
|---|---:|---|---|---|
| qpdf relocatable binary archive | 12.3.2 | `https://github.com/qpdf/qpdf/releases/download/v12.3.2/qpdf-12.3.2-bin-linux-x86_64.zip` | `44f2c53bf784c0143128d80d2b9946e9793962c5bb403b75c0024cb4d8e346b9` | Apache-2.0 + the runtime-library licenses in `THIRD-PARTY-NOTICES.txt` §3 |
| Tesseract (built by RustlingPDF CI) | 5.5.3 | RustlingPDF release `desktop-tools/tesseract-5.5.3`, asset `tesseract-5.5.3-linux-x86_64-musl` | pinned in `install-desktop-tools.sh` | Apache-2.0 |

### qpdf

Kept as the official upstream binary. qpdf's release is the strongest
provenance in this matrix: it publishes `qpdf-12.3.2.sha256`,
`qpdf-12.3.2.sha256.sigstore` and `qpdf-12.3.2.tar.gz.asc`, all uploaded by
the project maintainer. Building it ourselves would *lower* provenance
quality, so we do not.

The archive contains no license file of any kind, which is why
`license-texts/qpdf/LICENSE.txt` and `license-texts/qpdf/NOTICE.md` are
vendored in this repository (taken verbatim from tag `v12.3.2`) and copied
into `licenses/qpdf/` by both installers. Without that, the Linux bundle
produced an empty `licenses/qpdf/` directory and shipped no qpdf notice at
all, in breach of Apache-2.0 section 4(d).

### Tesseract — built in-house, not downloaded from a third party

`tesseract-ocr/tesseract` publishes **no Linux or macOS binary**: checked
against the releases API, the only asset on any of its releases is the Windows
installer. So there is no first-party Linux binary to pin.

Until 2026-07-30 this bundle pinned a third-party static build from
`DanielMYT/tesseract-static`. That pin was withdrawn. Verified against the
GitHub API:

- the `tesseract` asset was **uploaded from the maintainer's own user account
  on 2025-02-09**, while that repository's first GitHub Actions workflow was
  not committed until **2025-10-22** — eight months later. The binary is
  therefore not reproducible from any recorded build;
- no signature, no sigstore bundle, no SLSA provenance and no checksum file
  accompanied the release; the SHA-256 was transcribed from the release
  description;
- the repository is a solo personal project whose owner has no standing in the
  Tesseract or Leptonica ecosystems.

Signing that binary into a desktop application would mean vouching for
something nobody can audit or rebuild.

It is replaced by `.github/workflows/desktop-tools-tesseract-linux.yml`, which
builds `tesseract` from the checksum-pinned official sources below, statically
against musl inside an Alpine container, publishes it to this repository's own
GitHub Releases, and attests it with `actions/attest-build-provenance`. The
attestation is verifiable with
`gh attestation verify <asset> --repo <owner>/<repo>`. The build script is
`rust/scripts/desktop-tools/build-tesseract-musl.sh`; the bump procedure is in
`RELEASING.md`.

For reproducibility the Alpine base is pinned by immutable sha256 **digest**,
not by the mutable `alpine:3.22` tag: the workflow's `alpine_image` input
defaults to `alpine:3.22@sha256:14358309a308569c32bdc37e2e0e9694be33a9d99e68afb0f5ff33cc1f695dce`
(the multi-arch index digest for `alpine:3.22`, musl 1.2.5), which `docker run`
verifies on pull. A moved `3.22` tag therefore can no longer change the build
inputs silently.

That workflow runs **once per Tesseract version**, not per release.
`install-desktop-tools.sh` then fetches our published artefact by pinned
SHA-256 exactly like every other input, so day-to-day desktop release builds
are no slower than before.

Pinned build inputs (identical to the macOS source build's, below):

| Input | Version | SHA-256 |
|---|---:|---|
| Tesseract | 5.5.3 | shared source checksum above |
| Leptonica | 1.85.0 | `3745ae3bf271a6801a2292eead83ac926e3a9bc1bf622e9cd4dd0f3786e17205` |
| zlib | 1.3.1 | `9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23` |
| libpng | 1.6.46 | `f3aa8b7003998ab92a4e9906c18d19853e999f9d3bca9bd1668f54fa81707cb1` |
| libjpeg-turbo | 3.1.0 | `9564c72b1dfd1d6fe6274c5f95a8d989b59854575d4bbee44ade7bc17aa9bc93` |
| libtiff | 4.7.0 | `67160e3457365ab96c5b3286a0903aa6e78bdc44c4bc737d2e486bcecb6ba976` |

libtiff is not optional: Leptonica decodes its built-in bitmap font from an
embedded TIFF, so a Leptonica built without TIFF makes *every* `tesseract`
invocation print `Error in pixReadMemTiff: function not present` … `Error in
bmfCreate: font pixa not made` to stderr, and loses TIFF input support. It is
built without JBIG, LZMA, Zstd, WebP and LERC — JBIG-KIT is GPL-2.0-or-later
and is deliberately excluded from the commands we build ourselves.

### Linux runtime libraries and how their versions were established

The qpdf archive ships nine shared libraries next to `libqpdf`. Their versions
were **measured**, not assumed: each was either read from a version string
inside the shipped file, or confirmed by extracting the `.text` section of the
shipped `.so` and byte-comparing it against the same section of the
corresponding Ubuntu 22.04 (jammy) package. `libqpdf.so.30.3.2` records
`GCC: (Ubuntu 11.4.0-1ubuntu1~22.04.2)`, which is what identified jammy as the
build host in the first place.

| Shipped file | Version | How established | License (as distributed by us) |
|---|---|---|---|
| `libqpdf.so.30` (archive name `libqpdf.so.30.3.2`) | qpdf 12.3.2 | version string | Apache-2.0 |
| `libgnutls.so.30` | GnuTLS 3.7.3 | `Enabled GnuTLS 3.7.3 logging...` string | LGPL-2.1-or-later |
| `libnettle.so.8` | Nettle 3.7.3 | `.text` == `libnettle8 3.7.3-1build2` | LGPL-3.0-or-later (elected from GPL-2.0+/LGPL-3.0+) |
| `libhogweed.so.6` | Nettle 3.7.3 | `.text` == `libhogweed6 3.7.3-1build2` | LGPL-3.0-or-later (elected) |
| `libtasn1.so.6` | libtasn1 4.18.0 | version string; `.text` == `libtasn1-6 4.18.0-4ubuntu0.2` | LGPL-2.1-or-later |
| `libidn2.so.0` | libidn2 2.3.2 | version string; `.text` == `libidn2-0 2.3.2-2build1` | LGPL-3.0-or-later (elected) |
| `libunistring.so.2` | libunistring 1.0 | `.text` == `libunistring2 1.0-1` | LGPL-3.0-or-later (elected) |
| `libp11-kit.so.0` | p11-kit 0.24.0 | `.text` == `libp11-kit0 0.24.0-6build1` | BSD-3-Clause |
| `libffi.so.8` | libffi 3.4.2 | `.text` == `libffi8 3.4.2-4` | libffi (MIT-style) |
| `libjpeg.so.8` | libjpeg-turbo 2.1.2 | `libjpeg-turbo version 2.1.2 (build 20220221)` string | BSD-3-Clause, IJG, zlib |

`lib/libqpdf.so.30` ships as one real file under the library's own SONAME.
The curated archive holds it as `libqpdf.so.30.3.2` with `libqpdf.so.30` as a
symlink, and `install-desktop-tools.sh` preserves that — but Tauri's bundler
dereferences symlinks when copying resources, so the packaged installer used to
carry two byte-identical 4.4 MiB copies. This was found by extracting a shipped
`.deb` and comparing checksums; the text here previously asserted the opposite.
`stage-sidecar.sh` now collapses the link onto the target before packaging.
Dropping the fully-versioned name is safe: the SONAME recorded in the library
is `libqpdf.so.30`, and that is the exact string the qpdf executable's
`DT_NEEDED` asks the loader for.

Four further libraries are **not** shipped and resolve from the host:
`libgmp.so.10`, `libstdc++.so.6`, `libgcc_s.so.1`, `libz.so.1`. `libgmp` is
the notable one — it is pulled in by the bundled Nettle/Hogweed, not by qpdf
itself. The resulting Linux support floor is stated in
`rust/contracts/desktop-native-startup.md`.

## Windows x86_64

| Input | Version | URL | SHA-256 | License |
|---|---:|---|---|---|
| qpdf MSVC x64 archive | 12.3.2 | `https://github.com/qpdf/qpdf/releases/download/v12.3.2/qpdf-12.3.2-msvc64.zip` | `8941870a604e7c87ed24566b038d46c24ce76616254d2383c578f60c0677f202` | Apache-2.0 |
| Tesseract upstream x64 installer | 5.5.3 | `https://github.com/tesseract-ocr/tesseract/releases/download/5.5.3/tesseract-ocr-w64-setup-5.5.3.20260724.exe` | `bee9e3434bd94fd65387d9be28cd467a41f61b1275383b55b0f59a1331270ae4` | Apache-2.0 + the DLL licenses in `THIRD-PARTY-NOTICES.txt` §7 |
| libtiff (built by RustlingPDF CI, JBIG-free) | 4.7.2 | RustlingPDF release `desktop-tools/libtiff-4.7.2-nojbig`, asset `libtiff-4.7.2-windows-x86_64-mingw-nojbig.dll` | pinned in `install-desktop-tools.ps1` (`$LibtiffNoJbigSha256`) | libtiff (MIT-style) |

Windows keeps the **official** upstream installer. It is a genuine release
asset of `tesseract-ocr/tesseract` 5.5.3, uploaded on 2026-07-24 by `stweil`
(Stefan Weil), a Tesseract maintainer — first-party, so the provenance policy
above is satisfied without an in-house build. It carries no detached signature
or attestation, so the pinned SHA-256 is the only integrity control. The
checksum-pinned installer is run silently into the repository-local cache;
only the computed import closure is copied into the bundle.

### DLL closure, not "every adjacent DLL"

`install-desktop-tools.ps1` walks the PE import and delay-import tables
transitively from `qpdf.exe` and `tesseract.exe` and ships only what is
actually reachable. The previous blanket "copy every adjacent DLL" rule
shipped Tesseract's *training-tool* dependency stack — ICU (whose data DLL
alone is 33.12 MiB in the 5.5.3 installer), pango, cairo, glib, HarfBuzz,
FreeType and fontconfig — none of which `tesseract.exe` or `libtesseract-5.dll`
import.

That is measured, not assumed. In the 5.5.3 installer the only binaries that
import `libicuin78.dll`/`libicuuc78.dll` are `combine_lang_model.exe`,
`set_unicharset_properties.exe`, `unicharset_extractor.exe` and
`text2image.exe` — the training tools, which the bundle does not ship. Neither
`tesseract.exe` nor `libtesseract-5.dll` references ICU in its import table,
its delay-import table (both are empty), or as a `LoadLibrary` string.

### JBIG-free libtiff — removing the only GPL-2.0 component

The upstream installer's `libtiff-6.dll` is the MSYS2
`mingw-w64-x86_64-libtiff` 4.7.2 build, configured `--enable-jbig`, so it
NORMAL-imports `libjbig-0.dll` (JBIG-KIT, GPL-2.0-or-later — confirmed from the
Debian `libjbig0` copyright file). JBIG-KIT was the only GPL-2.0 component in
any bundle, and GPL-2.0 has no linking exception, which raised a combined-work
question for the signed installer.

That is resolved by building the JBIG-free libtiff ourselves (the "build it
without JBIG, mirroring Linux" option) instead of accepting GPL-2.0 terms or
relying on a contested no-combined-work reading:

- `.github/workflows/desktop-tools-libtiff-windows.yml` builds `libtiff-6.dll`
  from the checksum-pinned official libtiff 4.7.2 source
  (`https://download.osgeo.org/libtiff/tiff-4.7.2.tar.gz`, SHA-256
  `672bd7d10aee4606171afb864f3570b83340f6a33e2c186dc0512f7145ffdf6a` — the exact
  tarball and hash the MSYS2 `mingw-w64-libtiff` PKGBUILD pins), in MSYS2/MinGW-w64,
  mirroring that PKGBUILD's configure line with `--enable-jbig` replaced by
  `--disable-jbig` and the identical codec set otherwise (zlib, libdeflate,
  libjpeg-turbo, LZMA, zstd, libwebp, LERC). It attests the artefact with
  `actions/attest-build-provenance` and publishes it to
  `desktop-tools/libtiff-4.7.2-nojbig`.
- `install-desktop-tools.ps1` fetches that artefact by pinned SHA-256 and swaps
  it over the installer's `libtiff-6.dll` **before** the PE import closure is
  computed. Because the closure is import-driven, the JBIG-free libtiff means
  `libjbig-0.dll` is never reached and is not staged — nothing is deleted by
  hand. `Assert-NoJbigImport` then walks the import and delay-import tables of
  every staged file and fails the build if any JBIG reference survives.

libtiff is a C library (`libtiff-6.dll` imports `msvcrt.dll`/`KERNEL32.dll`, not
libstdc++/libgcc), so its stable C ABI is what the installer's
`libleptonica`/`libtesseract` link against, and the codec DLLs it imports are
resolved by name against the installer's own copies (only libtiff is swapped).
The residual risk — that the self-built libtiff is not ABI-compatible with the
installer's `libleptonica`/`libtesseract` — can only be settled by a Windows
load-test of the staged bundle (the ps1 runs `tesseract --version` at the end of
staging).

### Windows x86_64 — GPL/LGPL corresponding source

The Windows DLLs in `THIRD-PARTY-NOTICES.txt` §7 (except `libtiff-6.dll`, which
is our own separately-pinned JBIG-free build above) are **not separately
source-pinned inputs**: the single pinned input for the rest of the Windows tool
set is the official Tesseract Windows installer above (URL + SHA-256 recorded),
and these DLLs are the MSYS2 / MinGW-w64 builds extracted from it. For the GPL- and
LGPL-covered DLLs, the GNU licences require the *complete corresponding source*
— which for an MSYS2 binary is that project's MSYS2 source package (upstream
release + MSYS2's PKGBUILD and patches), not merely the vanilla upstream
tarball. The pointers below record where that corresponding source is obtained;
the **written offer of source in `THIRD-PARTY-NOTICES.txt` §9** is the operative
compliance mechanism and stands independently of them.

| Component (DLL) | Licence | Version shipped | MSYS2 corresponding source (PKGBUILD + patches) | Upstream source |
|---|---|---|---|---|
| GCC runtime — libgcc/libstdc++ (`libgcc_s_seh-1.dll`, `libstdc++-6.dll`) | GPL-3.0-or-later WITH GCC Runtime Library Exception 3.1 | not recorded by the DLL; the MSYS2 `mingw-w64-gcc` version at installer build time | `https://github.com/msys2/MINGW-packages/tree/master/mingw-w64-gcc` | `https://ftp.gnu.org/gnu/gcc/` |
| GNU libiconv (`libiconv-2.dll`) | LGPL-2.1-or-later | not recorded by the DLL; MSYS2 `mingw-w64-libiconv` (1.19 at time of writing) | `https://github.com/msys2/MINGW-packages/tree/master/mingw-w64-libiconv` | `https://ftp.gnu.org/gnu/libiconv/` |
| GNU gettext runtime — libintl (`libintl-8.dll`) | LGPL-2.1-or-later | not recorded by the DLL; MSYS2 `mingw-w64-gettext` at installer build time | `https://github.com/msys2/MINGW-packages/tree/master/mingw-w64-gettext` | `https://ftp.gnu.org/gnu/gettext/` |
| libidn2 (`libidn2-0.dll`) | LGPL-3.0-or-later (elected) | 2.3.8 (from §7) | `https://github.com/msys2/MINGW-packages/tree/master/mingw-w64-libidn2` | `https://ftp.gnu.org/gnu/libidn/libidn2-2.3.8.tar.gz` |
| libunistring (`libunistring-5.dll`) | LGPL-3.0-or-later (elected) | 1.4.2 (from §7) | `https://github.com/msys2/MINGW-packages/tree/master/mingw-w64-libunistring` | `https://ftp.gnu.org/gnu/libunistring/libunistring-1.4.2.tar.gz` |

Gap (honest): several versions above are left as "not recorded by the DLL"
because the DLL exports no version string and this checkout does not contain the
Windows installer, so the exact MSYS2 package version (with `pkgrel`) cannot be
read here. The NSIS installer bundles no MSYS2 package database, so there is no
in-installer metadata to read either. To pin each exactly, hash the shipped DLL
and match it to the corresponding MSYS2 `mingw-w64-x86_64-<pkg>` package
revision in the MSYS2 repo (SHA-256 identity — the same method used to establish
that, e.g., the shipped `libgif-7.dll` is MSYS2 giflib 6.1.3-1), and add the
resolved version + `pkgrel` to this table. Until then the written offer in §9 is
what carries the source obligation; it covers "the exact versions shipped"
regardless of what is pinned here.

## macOS arm64 source build

The macOS release has no upstream arm64 binary for either tool, so the
installer builds both as static commands from the pinned sources below and
rejects any non-system dylib dependency with `otool -L`.

| Input | Version | URL | SHA-256 | License |
|---|---:|---|---|---|
| qpdf | 12.3.2 | `https://github.com/qpdf/qpdf/releases/download/v12.3.2/qpdf-12.3.2.tar.gz` | `6cba2f9f2cd887d905faeb99e0e51a307b217920d1bbf3e9cfbb2e8178a2deda` | Apache-2.0 |
| Tesseract | 5.5.3 | shared source URL above | shared source checksum above | Apache-2.0 |
| Leptonica | 1.85.0 | `https://github.com/DanBloomberg/leptonica/releases/download/1.85.0/leptonica-1.85.0.tar.gz` | `3745ae3bf271a6801a2292eead83ac926e3a9bc1bf622e9cd4dd0f3786e17205` | BSD-2-Clause |
| zlib | 1.3.1 | `https://zlib.net/fossils/zlib-1.3.1.tar.gz` | `9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23` | Zlib |
| libpng | 1.6.46 | `https://download.sourceforge.net/libpng/libpng-1.6.46.tar.xz` | `f3aa8b7003998ab92a4e9906c18d19853e999f9d3bca9bd1668f54fa81707cb1` | libpng License |
| libtiff | 4.7.0 | `https://download.osgeo.org/libtiff/tiff-4.7.0.tar.gz` | `67160e3457365ab96c5b3286a0903aa6e78bdc44c4bc737d2e486bcecb6ba976` | libtiff license |
| libjpeg-turbo | 3.1.0 | `https://github.com/libjpeg-turbo/libjpeg-turbo/releases/download/3.1.0/libjpeg-turbo-3.1.0.tar.gz` | `9564c72b1dfd1d6fe6274c5f95a8d989b59854575d4bbee44ade7bc17aa9bc93` | BSD-3-Clause, IJG, Zlib |

## Where the vendored license texts came from

`license-texts/` holds the license texts the installers copy into the bundle.
They are vendored (not fetched at build time) so that a bundle can always be
produced offline and so that the texts are reviewable in-tree.

| File | Source |
|---|---|
| `APACHE-2.0.txt` | Apache License 2.0, canonical text (identical to qpdf's own `LICENSE.txt` apart from a leading blank line) |
| `LGPL-2.1.txt` | `https://www.gnu.org/licenses/old-licenses/lgpl-2.1.txt` |
| `LGPL-3.0.txt` | `https://www.gnu.org/licenses/lgpl-3.0.txt` |
| `GPL-3.0.txt` | `https://www.gnu.org/licenses/gpl-3.0.txt` |
| `GCC-RUNTIME-EXCEPTION-3.1.txt` | `gcc-mirror/gcc` tag `releases/gcc-14.2.0`, `COPYING.RUNTIME`. Kept at the top level (not under `windows/`) because it applies to the statically linked GCC runtime in the Linux/musl `tesseract` as well as to the Windows `libgcc_s_seh-1.dll`/`libstdc++-6.dll`, so Unix staging must not drop it. The exception text is version 3.1 (31 March 2009) and is the same regardless of the GCC version that produced the runtime |
| `qpdf/LICENSE.txt`, `qpdf/NOTICE.md` | `qpdf/qpdf` tag `v12.3.2` |
| `BSD-3-CLAUSE-p11-kit.txt` | `p11-glue/p11-kit` tag `0.24.0`, `COPYING` |
| `LIBFFI.txt` | `libffi/libffi` tag `v3.4.2`, `LICENSE` |
| `LIBJPEG-TURBO-2.1.2.txt`, `LIBJPEG-TURBO-2.1.2-README.ijg.txt` | `libjpeg-turbo/libjpeg-turbo` tag `2.1.2`, `LICENSE.md` + `README.ijg` — matches the `libjpeg.so.8` (2.1.2) qpdf ships on Linux |
| `LIBJPEG-TURBO-3.1.0.txt`, `LIBJPEG-TURBO-3.1.0-README.ijg.txt` | `libjpeg-turbo/libjpeg-turbo` tag `3.1.0`, `LICENSE.md` + `README.ijg` — matches the version statically linked into the Linux and macOS `tesseract` |
| `LIBJPEG-TURBO-3.2.0.txt`, `LIBJPEG-TURBO-3.2.0-README.ijg.txt` | `libjpeg-turbo/libjpeg-turbo` tag `3.2.0`, `LICENSE.md` + `README.ijg` — matches the `libjpeg-8.dll` (3.2.0) shipped in the Windows closure. libjpeg-turbo's `LICENSE.md` differs materially between 2.1.2, 3.1.0 and 3.2.0 (three→two licences, the added Component-Licenses/libspng section, and copyright years), so each shipped build carries its own text |
| `LEPTONICA.txt` | `DanBloomberg/leptonica` tag `1.85.0` |
| `LIBPNG.txt` | `pnggroup/libpng` tag `v1.6.58` |
| `LIBTIFF.txt` | `libtiff/libtiff` tag `v4.7.2`, `LICENSE.md` (unchanged since 4.7.0) |
| `ZLIB.txt` | `madler/zlib` tag `v1.3.1` |
| `windows/lz4.txt` | `lz4/lz4` tag `v1.10.0`, `lib/LICENSE` (the BSD-2-Clause text that covers `liblz4`; the repo-root `LICENSE` is explanatory prose about the two-licence split, not the licence itself) |
| `windows/giflib.txt` | giflib 6.1.3 `COPYING` (bare MIT body) prefixed with the actual copyright notices carried by the source files compiled into the shipped `libgif-7.dll`, taken from their SPDX-FileCopyrightText headers: Eric S. Raymond (`dgif_lib.c`, `egif_lib.c`, `gifalloc.c`, `gif_font.c`, `quantize.c`), `(C) Copyright 1989 Gershon Elber` (`gif_err.c`, `gif_hash.c`), and `Copyright (C) 2008 Otto Moerbeek <otto@drijf.net>` (`openbsd-reallocarray.c`) — so the shipped text carries all the copyright notices its MIT terms require, not just ESR's. giflib 6.1.3 source verified against the MSYS2 `mingw-w64-giflib` PKGBUILD pin (SHA-256 `b65b66b99f0424b93525f987386f22fc5efb9da2bfc92ad4a532249aaffbab0e`) |
| `MUSL-COPYRIGHT.txt` | `https://git.musl-libc.org/cgit/musl/plain/COPYRIGHT?h=v1.2.5` — musl 1.2.5 is the C library statically linked into the Linux `tesseract` (the Alpine base is digest-pinned in the build workflow); covers defect 3's missing musl notice |
| other `windows/*.txt` | each project's own `LICENSE`/`COPYING` at the tag closest to the version the shipped DLL reports; see `THIRD-PARTY-NOTICES.txt` §7 for the per-DLL mapping |

The three GNU texts were cross-checked against Debian's
`/usr/share/common-licenses/` copies; they differ only in the FSF's own URL
formatting, confirming both sources carry the same license text.
