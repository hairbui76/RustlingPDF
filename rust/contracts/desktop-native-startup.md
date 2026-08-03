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

Because the desktop UI is served from the webview's own protocol and calls the
sidecar at an absolute loopback URL, its requests are cross-origin.
`RUSTLING_PDF_TAURI_MODE` is what enables the narrow origin allow-list that
makes them work; see `desktop-cors.md`.

## How the frontend finds the sidecar

The port is ephemeral, so it is neither a build constant nor stable for the
lifetime of a window. The frontend resolves it in two stages:

1. **Seed.** After a successful start the launcher writes
   `http://127.0.0.1:<port>` to `localStorage` under
   `rustlingpdf.desktopBackendUrl` and reloads the webview once, so the first
   render already has an address.
2. **Live getter.** `core/services/desktop/desktopBackend.ts` invokes
   `get_backend_port` and republishes what Rust reports. The axios request
   interceptor prefixes relative URLs from that getter **per request**, not
   from the base URL captured when the client module loaded. A sidecar that
   restarts on a different port is therefore picked up without a reload; the
   seed alone would leave every request going to a dead port until the user
   fully quit the app.

Raw `fetch` call sites that bypass axios — the AI orchestrate stream and the
AI result-file download — read the same getter through
`core/services/aiBaseUrl.ts`.

The loopback literal `127.0.0.1` is used deliberately rather than `localhost`:
on macOS and some Linux setups `localhost` resolves to `::1` first, but the
sidecar binds the IPv4 wildcard, so a `::1` connection is refused and the app
reports the backend offline while it is in fact up.

## Readiness

`core/services/backendHealthMonitor.ts` polls
`/api/v1/config/app-config` every five seconds while any subscriber is
mounted, and treats the backend as usable only when the response reports
`dependenciesReady: true`. The HTTP listener accepts connections before native
tool discovery finishes, so a plain 200 is not readiness — running a tool in
that window fails with a confusing dependency error instead.

`ensureBackendReady` gates every server-backed tool run on that state and
forces a fresh probe before refusing, so clicking Run the instant the backend
comes up is not rejected on a stale snapshot. Tool Run buttons are disabled
while the backend is not online.

Web builds are served by the backend they call, so both the monitor and the
guard are constant-true there and never poll.

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

## Files on disk

Desktop builds read and write the user's filesystem directly; web builds
cannot and keep their browser behaviour unchanged. Both live in
`frontend/editor/src/core` behind `isDesktopRuntime()`, with every
`@tauri-apps/*` module reached by dynamic import from
`core/services/desktop/**` — the only directory ESLint permits to name one, so
the web bundle never evaluates Tauri code.

- **Opening.** The add-files action opens a native dialog and records each
  selected file's absolute path as that file's `localFilePath`. Files handed
  over by the OS (double-click, "Open with", drag onto the shortcut, an
  Explorer verb) arrive the same way; see
  `desktop-explorer-context-menu.md`.
- **Path identity.** A `File` is bound to the path it was read from by object
  identity, in `core/services/localFilePathRegistry.ts` — never by any key
  derived from the file's metadata. `quickKey` (`name|size|lastModified`) is
  not an identity: two documents of equal name and size collide on it, and a
  `File` built without an explicit `lastModified` takes `Date.now()`, so files
  read in one tick collide outright. Resolving a path through a colliding key
  gives one document another's path, which the next in-place save then
  overwrites. Files read from disk also carry their **real mtime**, so
  `quickKey` continues to identify a document rather than recording when it
  was read.
- **Deduplication prefers the path.** A file opened from disk is a duplicate
  only if the workspace already holds *that path*; a metadata match alone is
  not enough, because `cp -p`, `rsync -a` and file-sync clients preserve
  mtime, so a copy in another folder is indistinguishable by name, size and
  timestamp. Files with no path (browser uploads, tool outputs) fall back to
  the metadata key unchanged. A skip is logged rather than silent.
- **Path ownership is exclusive.** At most one workspace stub may hold a given
  `localFilePath`. Tool outputs never inherit one implicitly; a path is carried
  forward only where exactly one output is known to descend from exactly one
  input (`hooks/tools/shared/localFilePathCarry.ts`), using provenance recorded
  at the point each output was produced. Where provenance is unknowable — a
  single backend call returning a ZIP whose member order carries no
  correspondence to the uploads — no path is carried and the output is saved
  through a dialog instead. A fan-out (1→N) likewise carries nothing, since no
  single part replaces the original.
- **Saving.** A file with a `localFilePath` is overwritten in place. A file
  without one gets a native Save As dialog whose type filter is derived from
  the output's own extension, so a non-PDF result is not mislabelled `.pdf`.
  Ctrl/Cmd+S saves the selection, or everything when nothing is selected;
  the shortcut is not bound on web, where Ctrl+S belongs to the browser.
- **Writes do not truncate the target.** An in-place save writes a sibling
  temp file and renames it over the target, so a crash or a full disk leaves
  the original intact rather than truncated. `rename` replaces an existing
  destination on both POSIX and Windows.
- **Dirty state.** A successful write returns the path it wrote, and only that
  return clears `isDirty`. A cancelled dialog and a failed write both leave
  the file marked unsaved, and a failed save is reported to the user — the
  dirty marker alone is too easy to read as "saved".
- **Tool results.** On desktop, outputs that own a source path are written back
  to it. Outputs with no on-disk origin — the parts of a split, the product of
  a merge — are saved through a single destination prompt for the whole group
  rather than one dialog each. Web receives the aggregate download (a ZIP for
  multiple outputs), because a browser cannot write back to the files it was
  given.

## Plugin permissions

Tauri authorises each plugin command by a permission identifier listed in
`src-tauri/capabilities/default.json`. A call whose identifier is not granted
is refused at the IPC layer — **in a packaged app only**. Nothing in the
frontend test suite can see it, because `@tauri-apps/*` is mocked everywhere,
so a missing grant is invisible to typecheck, lint, build and tests while
being deterministically broken for every user.

Every plugin API the desktop bridge calls, and what authorises it:

| API | Command | Identifier |
| --- | --- | --- |
| `plugin-fs` `readFile` | `fs\|read_file` | `fs:allow-read-file` |
| `plugin-fs` `writeFile` | `fs\|write_file` | `fs:allow-write-file` |
| `plugin-fs` `rename` | `fs\|rename` | `fs:allow-rename` |
| `plugin-fs` `remove` | `fs\|remove` | `fs:allow-remove` |
| `plugin-fs` `stat` | `fs\|stat` | `fs:allow-stat` |
| `plugin-dialog` `open` | `dialog\|open` | `dialog:allow-open` |
| `plugin-dialog` `save` | `dialog\|save` | `dialog:allow-save` |
| `api/path` `join` | `path\|join` | `core:default` |
| `api/webviewWindow` `listen` | `event\|listen` | `core:default` |

Rules that follow from this, all enforced by
`core/services/desktop/desktopPermissions.test.ts`:

- Every `@tauri-apps/plugin-*` API used by the bridge must be granted by an
  **explicit** `<plugin>:allow-<command>` entry. Relying on a
  `<plugin>:default` set is not accepted: a set's contents are not visible
  from the frontend, so the guard cannot check it.
- Every `fs:` grant carries the `**` scope. The app opens and saves whatever
  the user picks in a native dialog, so a narrower scope refuses real
  documents — and `rename` resolves **both** its source and destination
  against the scope, so a partial scope breaks in-place save specifically.
- `requireLiteralLeadingDot` is `false` in `tauri.conf.json`. This is
  required, not incidental: with the platform default (`true` on unix), `**`
  matches no path containing a dot-leading component, so any document under
  `~/.local`, a dotted sync folder, or any hidden file would be refused. A
  filename merely *containing* dots — including the
  `<name>.<suffix>.rustling-tmp` staging file an in-place save creates — is
  matched by `**` either way.

## Known gap, verifiable only in a packaged bundle

- **The startup reload races the opened-file queue.** The launcher seeds the
  backend URL and calls `window.location.reload()` when the sidecar reports
  ready. The frontend's queue drain is a *destructive* pop. If a drain
  completes and the reload lands before the popped batch has been added to the
  workspace, those files are gone from both sides and the launch opens empty.
  The window is small and needs an unlucky interleaving, and no such loss has
  been observed — but nothing in the current design prevents it. A fix would
  make the reload conditional on nothing being in flight, or replace the
  reload with a live re-resolve now that `get_backend_port` exists.

## Updates

**The packaged app checks for updates once per start, and the check can be
turned off.** Ten seconds after the frontend mounts (`DesktopUpdateBanner`,
delay chosen to stay clear of the sidecar boot and the backend-ready reload),
it fetches
`https://github.com/hairbui76/RustlingPDF/releases/latest/download/latest.json`
— the endpoint is pinned in `tauri.conf.json` under `plugins.updater`. That
request is the packaged app's only self-initiated outbound request; it carries
no version report and no identifier beyond what any HTTP request discloses
(IP, user agent). The preference `checkForUpdatesOnStartup` (Settings →
General, default on) disables it entirely; there is no timer and no re-check
while the app runs.

What each install does with the manifest:

- **Windows (MSI)** and **Linux AppImage**: a newer version raises a
  dismissible banner. Nothing downloads until the user clicks "Update and
  restart"; the download is then verified against the minisign public key
  baked into `tauri.conf.json` before it is installed
  (`tauri-plugin-updater`), the MSI/AppImage runs, and the app relaunches.
- **Linux deb/rpm**: the plugin rejects in-place updates there;
  `desktopUpdater.ts` swallows that rejection silently by design — package
  installs update through the package manager and must never see an error
  dialog for it.
- **Web/Docker**: never checks; the updater code is behind the desktop bridge
  and the web bundle does not load it.

The manifest is generated by release.yml from the same signed artifacts the
release publishes: `latest.json` names the MSI (windows-x86_64) and AppImage
(linux-x86_64) URLs with their minisign signatures inline. The signing key has
minisign id `9ADA2DC8FC4FAF0B`; the public half is published in `RELEASING.md`
and its private counterpart is stored outside the repository and provided to
release CI through `TAURI_SIGNING_PRIVATE_KEY`. The standalone `.sig` assets
remain published for manual verification of hand-fetched downloads.

## Fallback fonts

Fallback fonts for PDFs that name a font without embedding it are **bundled
into the build and served from the app's own origin**. No font is ever fetched
from a third party. `@embedpdf/engines` defaults its font config to
`cdn.jsdelivr.net`, and that default is selected by leaving the option
`undefined`; the frontend therefore pins a local config in a single wrapper
(`@app/services/pdfiumEngine`), enforced by an ESLint rule and by
`noRemoteAssetDefaults.test.ts`.

The bundled set — copied out of `node_modules` at build time by
`viteStaticCopy`, the same mechanism that emits `pdfium.wasm` — is Noto Sans
(Latin, Cyrillic, Greek, Vietnamese), Noto Naskh Arabic and Noto Sans Hebrew.
It adds about 11 MB to `dist/`, and therefore to the desktop bundle and the
Docker image; nothing is added to initial page load, because a font is fetched
only when a document actually needs a substitute.

**CJK is deliberately absent.** The Japanese, Korean, Simplified and
Traditional Chinese sets total roughly 141 MB. A CJK document that does not
embed its fonts renders no glyphs for those runs. That is accepted so that no
document, in any script, causes a request to a third party.

Untree-shaken `cdn.jsdelivr.net` string constants remain in the compiled
JavaScript. No code path reaches them; the guards above are what establish
that, and a bundle grep finding those strings is not a finding.
