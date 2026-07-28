# Running RustlingPDF (the Rust backend)

This is the operator's guide to running this repository: the Rust processing
service (`stirling-processing`) — a full port of upstream Stirling-PDF's Java
Spring Boot backend — plus the optional Rust AI engine (`stirling-ai-engine`),
which ports upstream's Python engine. This repository contains no Java and no
Python engine; the upstream Stirling-PDF repo is a separate project used only as
an external reference oracle.

**Status in one paragraph:** the Rust service serves the same `/api/v1/...` REST
surface the unchanged web UI talks to, and it is the only backend here — all
local development tasks (`task dev`, `task backend:dev`, `task dev:all`) run it.
Open mode (no login) is the supported way to run it today. Secured mode
(login/users/teams) exists behind an opt-in review gate and the binary
deliberately **refuses to start** when `SECURITY_ENABLELOGIN=true` or
`DOCKER_ENABLE_SECURITY=true` is set — see
[Limitations](#what-does-not-run-on-rust-yet). The remaining gates before
secured-mode/production packaging are tracked in
[`PORT_STATUS.md`](PORT_STATUS.md).

---

## 1. Prerequisites

| Requirement | Why | Install |
|---|---|---|
| Rust toolchain (stable) | builds the workspace in `rust/` | <https://rustup.rs> |
| [Task](https://taskfile.dev) | unified command runner used below | `brew install go-task` / see site |
| PDFium (pinned revision 7543) | native PDF rendering/processing paths | `task rust:install` (automatic) |
| Node.js + npm | only if you also run the frontend dev server | `task frontend:install` |

`task rust:install` fetches the Cargo dependencies and downloads the pinned PDFium
build for your platform into the git-ignored `rust/.pdfium/` directory, verifying
its SHA-256 digest. Deployments can instead point
`STIRLING_PDFIUM_LIBRARY_PATH` at an absolute PDFium shared-library path (or its
containing directory). A configured PDFium is treated as required: a bad path fails
the request rather than silently switching engines. Without any PDFium, the service
still starts and uses pure-Rust fallbacks where they exist, but the native
processing paths (and many endpoint tests) need it — install it.

### Optional external tools

Like the upstream Java backend, some conversions shell out to external tools. The
Rust service discovers them at startup (bounded discovery, with minimum versions
where upstream requires them). A missing tool does not crash anything: the affected
endpoints report as unavailable with reason `DEPENDENCY` in
`GET /api/v1/config/endpoints-availability`, exactly like upstream's alternatives
mechanism.

| Tool (binary) | Enables | Notes |
|---|---|---|
| LibreOffice (`soffice`) | office ↔ PDF (`convert/file/pdf`, `pdf/word`, `pdf/presentation`, `pdf/xml`) | |
| Ghostscript (`gs`) | PDF/A / PDF/X, repair, compress assist, color-space conversion, e-reader optimisation | |
| qpdf | repair (second choice), compress assist | minimum version 12 |
| OCRmyPDF (`ocrmypdf`) | preferred OCR path for `misc/ocr-pdf` | |
| Tesseract (`tesseract`) | OCR fallback path; language data under the configured tessdata dir | |
| WeasyPrint (`weasyprint`) | HTML/Markdown/EML/URL → PDF, AI create-PDF | minimum version 58 |
| Poppler (`pdftohtml`) | PDF → HTML, and Calibre's PDF→EPUB engine | |
| Calibre (`ebook-convert`) | ebook ↔ PDF | |
| `unrar` (or 7-Zip fallback) / `rar` | CBR → PDF / PDF → CBR (creating CBR requires `rar`) | |
| FFmpeg | PDF → video (route is an explicit opt-in, see below) | set `STIRLING_PROCESSING_FFMPEG_COMMAND` |
| veraPDF | strict PDF/A validation (optional) | set `STIRLING_PROCESSING_VERAPDF_COMMAND` |

Every tool's executable can be overridden explicitly with
`STIRLING_PROCESSING_<TOOL>_COMMAND` (e.g. `STIRLING_PROCESSING_GHOSTSCRIPT_COMMAND`,
`..._SOFFICE_COMMAND`, `..._QPDF_COMMAND`, `..._WEASYPRINT_COMMAND`,
`..._OCRMYPDF_COMMAND`, `..._TESSERACT_COMMAND`, `..._PDFTOHTML_COMMAND`,
`..._EBOOK_CONVERT_COMMAND`, `..._UNRAR_COMMAND`, `..._RAR_COMMAND`).

---

## 2. Quick start

From the repository root:

```bash
task rust:install     # once: deps + pinned PDFium
task backend:dev      # Rust backend on http://127.0.0.1:8080
```

In a second terminal, if you want the web UI:

```bash
task frontend:dev     # Vite dev server, proxies /api/* to localhost:8080
```

Or run both together on automatically chosen free ports:

```bash
task dev              # backend (Rust) + frontend
task dev:all          # backend (Rust) + frontend + Rust AI engine
```

`task backend:dev`, `task dev`, and `task dev:all` all run the **Rust** backend —
there is no other backend in this repository. (The upstream Java implementation
lives in the separate Stirling-PDF repo and remains the behavior reference the
contracts were verified against.)

Direct entry point without Task:

```bash
cd rust
STIRLING_PDFIUM_LIBRARY_PATH="$PWD/.pdfium/current" \
  cargo run -p stirling-processing --locked
```

Smoke-check a running instance:

```bash
curl http://127.0.0.1:8080/api/v1/info/status
curl http://127.0.0.1:8080/api/v1/config/app-config
```

### Ports and binding

- `task backend:dev` / `task rust:run` default to `127.0.0.1:8080`; pass
  `PORT=<n>` to either Task command.
- Invoking the binary directly: `STIRLING_PORT` (or the Spring-compatible
  `SERVER_PORT`) selects the port; `0` requests an OS-assigned ephemeral port.
  Startup prints `Stirling-PDF running on port: <port>`.
- The binary binds **loopback only** unless `STIRLING_HOST` (or Spring-compatible
  `SERVER_ADDRESS`) is set to an explicit IP. Container-shaped runs use
  `STIRLING_HOST=0.0.0.0`. Malformed host/port values fail startup instead of
  falling back.

---

## 3. Configuration

The Rust service reads the same YAML files as upstream Stirling-PDF:
`configs/settings.yml` then `configs/custom_settings.yml`, resolved below
`STIRLING_BASE_PATH` (default: the working directory). Behavior matches upstream
Java's `ConfigInitializer`, including:

- template merge on upgrade (new template keys arrive with defaults; user-set leaf
  values are preserved; comments/ordering kept; idempotent),
- truncated-file recovery (a `settings.yml` under 31 lines is backed up to
  `settings.yml.<epoch-millis>.bak` and recreated from the template),
- environment overrides layered on top (`SYSTEM_*`, `SECURITY_*`, `STIRLING_*`
  spellings that Java's relaxed binding accepts).

Commonly used environment variables:

| Variable | Purpose |
|---|---|
| `STIRLING_BASE_PATH` | root for `configs/` settings files |
| `STIRLING_HOST` / `SERVER_ADDRESS` | bind address (default loopback) |
| `STIRLING_PORT` / `SERVER_PORT` | port (default 8080; `0` = ephemeral) |
| `STIRLING_PDFIUM_LIBRARY_PATH` | PDFium shared library (file or directory) |
| `STIRLING_PROCESSING_MAX_UPLOAD_BYTES` | multipart upload cap |
| `SYSTEM_MAXFILESIZE`, `SYSTEM_MAXDPI` | Java-compatible processing limits |
| `SYSTEM_GOOGLEVISIBILITY` | `robots.txt` policy |
| `SYSTEM_ENABLEMOBILESCANNER` | mobile-scanner QR transfer feature gate |
| `STIRLING_PROCESSING_ENABLE_URL_TO_PDF` / `SYSTEM_ENABLEURLTOPDF` | opt-in URL→PDF (SSRF-guarded) |
| `STIRLING_JOB_QUEUE_*`, `STIRLING_JOB_RESULT_EXPIRY_MINUTES` | async job queue/result tuning |
| `AIENGINE_URL`, `AIENGINE_ENABLED`, `AIENGINE_TIMEOUTSECONDS` | AI-engine proxy wiring |

Async processing works on the ported POST endpoints via the same `?async=true`
contract as Java (`general/job/{jobId}`, `/result`, `/result/files`,
`general/files/{fileId}` serve status/results).

---

## 4. Optional: the Rust AI engine

The AI features (`/api/v1/ai/*`, MCP, classification, PDF question answering,
document creation, math audit, orchestration) are served by the separate
`stirling-ai-engine` crate, the Rust replacement for upstream Stirling-PDF's
Python engine (which stays in the upstream repo; it is not part of this one).

```bash
task engine:dev       # Rust AI engine on localhost:5001
# or: task dev:all    # starts it alongside backend + frontend
```

Point the processing service at it with `AIENGINE_URL` (default
`http://localhost:5001`) and `AIENGINE_ENABLED=true`. Provider credentials follow
the engine's own configuration (structured-output-capable providers, including
Anthropic/OpenAI-compatible APIs and native `ollama:<model>` for local models).
`STIRLING_ENGINE_SHARED_SECRET` protects the engine boundary when set.
The engine's quality gate is `task engine:check`.

---

## 5. Docker

The repository ships a self-contained container image: the `stirling-processing`
release binary, the built React SPA (served by the binary itself — see
`contracts/spa-serving.md`), a pinned checksum-verified PDFium, and the external
conversion tools, all in one image listening on `:8080`.

```bash
task docker:build     # build the image (tag: rustlingpdf)
task docker:up        # start docker/compose.yml detached on :8080
task docker:logs      # tail the stack's logs
task docker:down      # stop the stack
```

Equivalent raw commands: `docker build -t rustlingpdf -f docker/Dockerfile .`
and `docker compose -f docker/compose.yml up -d`.

### Pulling a prebuilt image from GHCR

Every tagged release (see `RELEASING.md` at the repo root) publishes both
image targets to GitHub Container Registry, so building locally is optional:

```bash
docker pull ghcr.io/hairbui76/rustlingpdf:latest             # or a specific vX.Y.Z tag
docker run -d -p 8080:8080 -v "$(pwd)/data:/data" \
    ghcr.io/hairbui76/rustlingpdf:latest

docker pull ghcr.io/hairbui76/rustlingpdf-ai-engine:latest   # optional AI sidecar
```

The pulled image is content-identical to a local `task docker:build` (same
Dockerfile, `runtime` target, plus OCI `version`/`source`/`revision`/
`created` labels stamped by the release workflow). To use it with
`docker/compose.yml` — which references the local tags — either retag it
(`docker tag ghcr.io/hairbui76/rustlingpdf:latest rustlingpdf:latest`, and
likewise for the sidecar) or point the compose `image:` fields at the GHCR
names. Pin `vX.Y.Z` (or a digest) instead of `latest` when you need
reproducible deployments; `latest` moves on every release.

### What the image contains

- **One process, one port.** The Rust binary serves the REST API and the web UI
  (`STIRLING_FRONTEND_DIST=/app/frontend` bakes the Vite `dist/` in; the SPA
  index, deep links, and static-asset serving follow the single-binary SPA
  contract). No nginx, no separate frontend container.
- **Frontend flavor: proprietary** — the same flavor upstream Stirling-PDF's
  self-hosted embedded image builds (`STIRLING_FLAVOR=proprietary`) and this
  repo's default dev/build mode. In open mode the runtime app-config flags keep
  login/premium features off, so it behaves like the core UI plus
  gracefully-gated extras.
- **External tools** (Debian trixie packages): LibreOffice `-nogui`
  (writer/calc/impress/draw), Ghostscript, qpdf 12.2, Tesseract (+`eng`+OSD),
  OCRmyPDF (+unpaper/pngquant), WeasyPrint 62, poppler-utils (`pdftohtml`),
  7zip (CBR fallback), and metric-compatible + Noto/CJK fonts. **Not**
  included, versus upstream's standard image: Calibre (~1 GB — the ebook↔PDF
  endpoints self-report `DEPENDENCY`), ImageMagick/unoserver/Python extras
  (Java-era needs), unrar (non-free), ffmpeg (PDF→video stays opt-in). Add a
  missing tool with your own derived image; the backend picks it up at startup.
- **PDFium** installed by `rust/scripts/install-pdfium.sh` inside the build for
  the image's architecture (`STIRLING_PDFIUM_LIBRARY_PATH=/app/pdfium/libpdfium.so`).
- **Non-root** (`stirling`, uid/gid 1000), `tini` as PID 1, `HEALTHCHECK`
  against `/api/v1/info/status`, `STIRLING_HOST=0.0.0.0`.

### State and configuration

`STIRLING_BASE_PATH=/data` (declared `VOLUME`): `settings.yml` /
`custom_settings.yml` are read from `/data/configs/` when present. An empty
volume works out of the box — built-in template defaults plus environment
overrides apply until you drop a `settings.yml` there (the automatic
first-start materialization of the file is a desktop/Tauri-mode behavior, not
a container one). Pipeline state and `customFiles/` overrides resolve beneath
`/data` as well. When bind
mounting (the compose example uses `./data:/data`), give the directory to
uid/gid 1000: `mkdir -p data && chown 1000:1000 data`. All the environment
variables from [Configuration](#3-configuration) apply unchanged; secured mode
remains fail-closed (`SECURITY_ENABLELOGIN=true` refuses startup) — run the
container in open mode on a trusted network or behind your own auth proxy.

### The optional AI-engine sidecar

The same Dockerfile has an `ai-engine` target with only the
`stirling-ai-engine` binary (`task docker:build:ai-engine`, port 5001,
`/health` healthcheck, its own `/data` volume for the documents store). The
compose file wires it behind the `ai` profile:

```bash
task docker:up:ai     # or: docker compose -f docker/compose.yml --profile ai up -d
```

Then set `AIENGINE_ENABLED=true` (and a provider credential such as
`ANTHROPIC_API_KEY` on the sidecar) to activate the `/api/v1/ai/*` proxy
routes. The main image never requires the sidecar; with it absent the AI
features simply stay off.

### Smoke-checking a running container

```bash
curl -s http://localhost:8080/api/v1/info/status        # {"status":"UP",...}
curl -s http://localhost:8080/ | head -1                 # SPA index.html
curl -s -o rotated.pdf -F fileInput=@some.pdf -F angle=90 \
     http://localhost:8080/api/v1/general/rotate-pdf     # a real PDF operation
```

---

## 6. Verifying parity yourself

- `task rust:check` — fmt + clippy + full test suite with PDFium bound (see
  `PORT_STATUS.md` for the latest full-gate numbers).
- **Per-surface contracts** (`rust/contracts/*.md`) — each ported surface documents
  routes, upstream Java counterparts, parity notes, and explicit gaps.

---

## 7. What does NOT run on Rust yet

These are deliberate, documented limits — the authoritative list with rationale is
[`PORT_STATUS.md`](PORT_STATUS.md):

- **Secured mode (login/users/teams).** A reviewed opt-in security router exists
  and is extensively tested, but production secure mode is gated on independent
  human security review. Setting `SECURITY_ENABLELOGIN=true` or
  `DOCKER_ENABLE_SECURITY=true` makes the Rust binary refuse startup (fail-closed,
  including on malformed boolean values) instead of serving an unsecured
  approximation. Run open mode; secured deployments must wait for the review
  gate (or use the upstream Stirling-PDF Java product from its own repo).
- **SaaS / hosted-cloud layer** (upstream's `app/saas`, account-link billing):
  deliberately not ported; depends on external cloud services unverifiable here.
- **SAML2 SSO**: deferred pending a maintainer decision on a native XML-signature
  dependency. (Generic OIDC login is ported inside the opt-in secured router;
  Supabase JWT verification is ported.)
- **H2 database backup/restore routes**: N/A — the Rust store is SQLite.
- **PDF → video** route: implemented but an explicit opt-in
  (`STIRLING_PROCESSING_FFMPEG_COMMAND`) while upstream FFmpeg CVEs are assessed —
  upstream's own Java route is itself commented out.
- **Desktop packaging**: the Tauri desktop app bundles the Rust backend as its
  default sidecar (`task desktop:stage-sidecar` stages the release
  `stirling-processing` binary and the pinned PDFium runtime into the bundle;
  ephemeral-port handshake, workspace migration, bundled-PDFium wiring via
  `STIRLING_PDFIUM_LIBRARY_PATH`). `STIRLING_NATIVE_BACKEND_PATH` remains as a
  development-only override. Cross-platform signed-bundle upgrade proof is
  still outstanding — see `contracts/desktop-native-startup.md`.
- **Deep PDF-fidelity edges** in the PDF↔JSON editor model (e.g. Type3 glyph
  synthesis, Type0/Type3 byte-parity, >4-component DeviceN JPEGs): see the
  "Remaining" section of `PORT_STATUS.md`.

## 8. Production readiness position

The Rust service is the only backend in this repository and open mode is
production-usable today; the [Docker image](#5-docker) is the supported packaged
form. The remaining gates before secured mode and full packaged distribution
are: independent security review of the secured router and signing subsystem,
cross-platform proof of the signed desktop bundles (the Rust binary + PDFium
are now bundled as the desktop sidecar), and the residual fidelity gaps
above. Follow `PORT_STATUS.md`,
`SECURITY_MIGRATION_DESIGN.md`, and `SIGNING_MIGRATION_DESIGN.md` for the live
state of each gate.
