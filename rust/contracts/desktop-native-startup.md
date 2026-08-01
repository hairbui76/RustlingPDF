# Native desktop processing startup

The Tauri desktop launcher starts the bundled `rustling-processing` sidecar.
Bare launch arguments open files; Windows Explorer tool intents are specified
in `desktop-explorer-context-menu.md`, and MSI lifecycle behavior is specified
in `desktop-windows-installer.md`.

## Staging

`task desktop:stage-sidecar`:

1. builds the release processing binary;
2. installs the pinned PDFium runtime;
3. installs pinned qpdf and Tesseract runtimes with English OCR data; and
4. stages binaries, resources, notices, and licenses below `src-tauri`.

Tauri bundles `binaries/rustling-processing`, `resources/pdfium`, and
`resources/tools`. A bundle missing its sidecar fails startup with a visible
error. `RUSTLING_NATIVE_BACKEND_PATH` is a development-only override for an
explicit processing executable.

## Launch contract

- The launcher requests an ephemeral loopback port and waits at most 90
  seconds.
- The backend prints `RustlingPDF running on port: <port>` independently of
  log filtering. The launcher parses the stable `running on port: ` suffix.
- Early exit, malformed handshakes, and timeouts are reported to the UI.
- PID plus start-time parent monitoring stops orphaned sidecars.
- Stale processes and stale port records are removed defensively.
- The desktop workspace is rooted below the RustlingPDF application-data
  directory and contains configuration, logs, and temporary working data.

The sidecar receives `RUSTLING_BASE_PATH` and `RUSTLING_PDF_TAURI_MODE`. When
the environment does not already define `RUSTLING_PDFIUM_LIBRARY_PATH`, the
launcher points it at the bundled PDFium directory.

For bundled tools, explicit operator values take precedence. Otherwise the
launcher sets:

- `RUSTLING_PROCESSING_QPDF_COMMAND`;
- `RUSTLING_PROCESSING_TESSERACT_COMMAND`; and
- `TESSDATA_PREFIX`.

Unpackaged development runs leave an unavailable bundle path unset so normal
runtime discovery can continue.

Tools the bundle does not stage — LibreOffice above all — are found by the
backend's own discovery, which on Windows falls back to the installers'
well-known directories when `PATH` misses (see `runtime-config.md`). The
overrides above still win outright and are never subject to that fallback.

## Configuration initialization

On a fresh Tauri workspace, the processing service writes the bundled
`settings.yml.template` only when `configs/settings.yml` is absent and creates
an empty `custom_settings.yml` only when absent.

A `settings.yml` shorter than `MIN_SETTINGS_FILE_LINES` is treated as
truncated, moved to `settings.yml.<epoch-millis>.bak`, and replaced from the
template. `custom_settings.yml` is never subject to this recovery.

On upgrade, the merge is template-shaped:

- template structure, comments, blank lines, and inline comments are retained;
- values for leaf keys present in both files come from the user file;
- new template keys keep their defaults;
- keys absent from the template are omitted;
- the file is rewritten only when the merged result differs.

Carried values preserve the template leaf's quoting style where safe.
Plain values that would change meaning or become invalid YAML are quoted by the
YAML emitter. Inline scalars and flow sequences are supported; block mappings
or block sequences at a leaf fall back to the template default. An existing
long file that no longer parses is left untouched and logged rather than
preventing desktop startup.

## Bundled-tool platform floor

The Linux Tesseract command is static. Bundled qpdf expects
`libgmp.so.10`, `libstdc++.so.6`, `libgcc_s.so.1`, and `libz.so.1`; the measured
floor is glibc 2.34 and `GLIBCXX_3.4.29`. If a required library is unavailable,
qpdf discovery marks repair support unavailable without crashing the desktop
process.

Provenance, import closures, checksums, and redistribution decisions live in
`rust/scripts/desktop-tools/SOURCES.md`.

## Updates

**The packaged app does not check for updates.** There is no updater plugin, no
update endpoint, no update manifest, and no in-app update UI. A packaged app
makes no outbound request of its own, so running it discloses nothing — not the
machine's IP, not the installed version, not the fact that RustlingPDF is
installed at all. Moving to a newer version is a manual download from the
releases page.

Release bundles are still minisign-signed and each is published with its `.sig`
alongside, so a download fetched by hand can be verified. The signing key has
minisign id `9ADA2DC8FC4FAF0B`; the public half is published in `RELEASING.md`
and its private counterpart is stored outside the repository and provided to
release CI through `TAURI_SIGNING_PRIVATE_KEY`. Signatures serve verification
only — nothing in the application consumes them at runtime.
