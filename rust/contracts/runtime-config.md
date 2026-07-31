# Runtime configuration

The processing service loads `configs/settings.yml` followed by
`configs/custom_settings.yml` below `RUSTLING_BASE_PATH`. The custom file
recursively overrides the base file, and supported environment variables take
final precedence.

## Binding

`RUSTLING_HOST` selects the bind address and defaults to loopback.
`RUSTLING_PORT` selects the port and defaults to 8080; port `0` requests an
OS-assigned port for desktop startup. Malformed, non-Unicode, or out-of-range
values fail startup instead of silently falling back.

`SERVER_ADDRESS` and `SERVER_PORT` remain supported as generic deployment
conventions. Product-specific variables use only the `RUSTLING_*` namespace.

## Public routes

| Route | Response |
|---|---|
| `GET /api/v1/config/app-config` | Public UI, legal, timestamp, feature, dependency-readiness, and platform configuration. |
| `GET /api/v1/config/login-disclaimer[?lang=<locale>]` | Configured agreement markdown with locale fallback. |
| `GET /api/v1/config/endpoint-enabled?endpoint=<key>` | Boolean for one endpoint key. |
| `GET /api/v1/config/endpoints-enabled?endpoints=<key>,<key>` | Boolean map for requested endpoint keys. |
| `GET /api/v1/config/endpoints-availability[?endpoints=<key>,<key>]` | Map of `{ "enabled": boolean, "reason": null | "CONFIG" | "DEPENDENCY" }`. |
| `GET /api/v1/config/group-enabled?group=<name>` | Boolean for a functional or dependency group. |
| `GET /api/v1/settings/get-endpoints-status` | Explicitly disabled endpoint keys mapped to `false`. |

The public app configuration contains only operational UI and processing
settings.

## Endpoint availability

Endpoint keys remove one leading slash. `endpoints.toRemove` or
`ENDPOINTS_TOREMOVE` disables exact keys with reason `CONFIG`.
`endpoints.groupsToRemove` or `ENDPOINTS_GROUPSTOREMOVE` disables the registered
members of a functional group while preserving a route that has another
available implementation.

Infrastructure required by the UI stays ungated:

- `/api/v1/info/*`;
- `/api/v1/config/*`;
- `/api/v1/ui-data/*`;
- `/api/v1/settings/*`;
- temporary job and file plumbing; and
- mobile-scanner session routes.

Routes with stronger switches use those switches. `send-email` is registered
only when mail is enabled, AI proxy routes return 503 unless
`AIENGINE_ENABLED=true`, and URL-to-PDF remains disabled by default unless
explicitly enabled.

All API responses use `Cache-Control: private, no-store`.

## Dependency discovery

The service probes optional executables once before accepting requests. The
discovery table records resolved executable paths and drives endpoint
availability. A missing mandatory dependency produces reason `DEPENDENCY`;
explicit configuration removal takes precedence as `CONFIG`.

The current discovery set includes OCRmyPDF, Tesseract, LibreOffice,
WeasyPrint, Poppler, qpdf, RAR read/write tools, Calibre, FFmpeg, and veraPDF.
Version or capability checks are applied where required:

- qpdf must be at least version 12;
- WeasyPrint must be at least version 58;
- a 7-Zip candidate must expose both a RAR format handler and a RAR
  decompression codec;
- every process probe has a five-second timeout.

Resolution order per tool group is fixed:

1. An explicit `RUSTLING_PROCESSING_*_COMMAND` override (empty value = unset)
   resolves on its own and **never** falls through to any other source, so a
   stale or broken override leaves the group missing rather than silently
   resolving an unrelated installation. This is what the desktop sidecar uses
   to point the service at its staged qpdf and Tesseract.
2. Otherwise the platform command names are looked up on `PATH`, with `PATHEXT`
   expansion on Windows.
3. **Windows only**, and only when `PATH` yielded nothing *capable*: the tool's
   well-known installation directories are probed. "Capable" matters for the RAR
   extraction group — a `7z` on `PATH` built without the RAR codec fails the
   capability probe, and the directory fallback then still runs and can find a
   genuine `UnRAR.exe`. Several Windows installers
   deliberately leave `PATH` alone, so a correctly installed tool was otherwise
   invisible — the cause of Office→PDF conversion reporting `DEPENDENCY` on
   Windows desktop installs despite a working LibreOffice.

The probed directories are environment-rooted templates (`%ProgramFiles%`, the
separate 32-bit `%ProgramFiles(x86)%`, and per-user `%LOCALAPPDATA%`), never
hardcoded drive letters; a template naming a variable the host does not set is
skipped. The covered groups are LibreOffice (`LibreOffice\program`), Tesseract
(`Tesseract-OCR`, also under `%LOCALAPPDATA%\Programs` for the installer's
per-user mode), Calibre (`Calibre2`), RAR creation (`WinRAR`), RAR extraction
(`WinRAR` then `7-Zip`), and qpdf (`qpdf\bin` — the documented install
directory, though qpdf's plain zips and version-suffixed directories stay
uncovered, so this is a bonus rather than a guarantee). OCRmyPDF, WeasyPrint,
`pdftohtml`, FFmpeg and veraPDF have no directory list: they arrive as pip
wheels, plain zip archives unpacked wherever the operator chooses, or an
installer with a user-chosen target — none has a citable default.

**Security note on the per-user root.** `%ProgramFiles%` and `%ProgramFiles(x86)%`
are administrator-writable by default, but `%LOCALAPPDATA%\Programs` is writable
by the profile's own user, and a binary discovered there is executed when a
request reaches the feature that needs it. This root is kept because it is the
UB-Mannheim installer's per-user mode — a real installation shape — and because
it does not cross a privilege boundary in any shipped configuration: the desktop
bundle sets `RUSTLING_PROCESSING_TESSERACT_COMMAND` at its staged binary, which
short-circuits probing entirely, and a service account's `%LOCALAPPDATA%` lives
under `C:\Windows` where only administrators can write. The one configuration
where it does matter is a backend an operator launches **elevated** from their
own desktop session: `%LOCALAPPDATA%` is then still the medium-integrity user
profile, so a planted `tesseract.exe` would run at high integrity. Do not run the
service elevated. Note also that no directory-probed root is executed during
*startup* discovery: only the `unrar` and `qpdf` groups run their binary at
startup (a capability probe and a version probe), and both probe administrator-
writable roots only.

Each probe is a single existence check, so no process is spawned for a path
that does not exist and startup cost stays bounded by the number of templates.
Non-Windows hosts skip step 3 entirely and neither read the environment nor
touch the filesystem for it, so resolution there is unchanged.

Conditional enhancements do not disable an otherwise functional route. For
example, native PDF-to-HTML remains available without `pdftohtml`, and
verification remains available without veraPDF for documents that do not
request a strict validation profile.

`dependenciesReady` means probing is complete; it does not mean every optional
program is installed.

## Processing limits and paths

The service supports explicit environment overrides for upload limits, PDFium,
native-tool commands, job queue capacity, temporary result expiry, CMap data,
frontend assets, and the optional AI engine. All paths are validated by their
own runtime consumers and do not create hidden compatibility roots.

The Type0 CMap search checks `RUSTLING_PROCESSING_CMAP_PATH` followed by common
Poppler data directories. Missing CMap data does not prevent startup; affected
fonts use conservative metrics.

## Security properties

- URL-to-PDF applies host, DNS, IP-range, redirect, and response bounds.
- Temporary workspaces and results are swept.
- Legal URLs are absent unless configured.
- Document encryption, signatures, trust lists, revocation, and timestamping
  are PDF-processing policy, not application entitlement.
- The service is stateless and has no built-in account database or durable
  document store.

Configuration behavior is covered by `runtime_config` unit tests and config
endpoint integration tests.
