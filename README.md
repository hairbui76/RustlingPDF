<p align="center">
  <img src="docs/assets/logo.png" alt="RustlingPDF" width="260">
</p>

<p align="center">
  <a href="https://github.com/hairbui76/RustlingPDF/actions/workflows/backend.yml"><img alt="Backend CI" src="https://github.com/hairbui76/RustlingPDF/actions/workflows/backend.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/hairbui76/RustlingPDF/actions/workflows/frontend.yml"><img alt="Frontend CI" src="https://github.com/hairbui76/RustlingPDF/actions/workflows/frontend.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/hairbui76/RustlingPDF/actions/workflows/desktop.yml"><img alt="Desktop CI" src="https://github.com/hairbui76/RustlingPDF/actions/workflows/desktop.yml/badge.svg?branch=main"></a>
  <br>
  <a href="https://github.com/hairbui76/RustlingPDF/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/hairbui76/RustlingPDF?color=60C948&label=release"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-60C948"></a>
  <img alt="Rust backend" src="https://img.shields.io/badge/backend-100%25%20Rust-60C948">
  <img alt="No account required" src="https://img.shields.io/badge/accounts-none-60C948">
</p>

<p align="center">
  <b>A local, do-everything PDF toolbox.</b><br>
  A pure-Rust backend, a React web UI, and a Tauri desktop app.<br>
  No login, no database, no server-side state &mdash; your files stay yours.
</p>

---

A locally hosted PDF toolbox with a **pure-Rust backend**: one `axum` service
covering merge, split, convert, OCR, forms, redaction, signing, pipelines and
async jobs, an optional Rust AI engine, and a React single-page UI. The server
keeps **no accounts and no server-side state**: every request is
self-contained, results are ephemeral (TTL-swept scratch space), and all user
preferences live client-side.

RustlingPDF is an independent tool **based on [Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF)**.
It began as a full Java→Rust port of Stirling-PDF's Spring Boot backend, verified
endpoint-by-endpoint against the original as a compatibility oracle (route census:
zero unexplained gaps; during the port a live differential harness semantically
diffed both backends' outputs). This repository carries that Rust backend forward as its own
product — there is **no Java in this repo** and no dependency on a JVM at build
or run time. See [LICENSE](LICENSE) for upstream attribution.

## What it can do

Every tool below runs **on your machine** — the backend takes a file, does the
work, and hands it back. No account, no upload to anyone's cloud, no server-side
storage: uploads land in a scratch directory that is swept on a timer, and all
your settings live in the app. (Put it behind your own proxy or a trusted
network if you want to limit who can reach the server build.)

A **†** marks a tool that needs an external program installed (LibreOffice,
WeasyPrint, Calibre, Tesseract/OCRmyPDF, FFmpeg, `unrar`). A
missing one never crashes anything — that one tool just shows as unavailable
until you install it. Everything unmarked is pure Rust and always works.

### Organise pages
Merge PDFs · split (at chosen breaks, by size or page count, at chapters, into a
grid of tiles, or automatically at QR divider pages) · reorder, reverse,
duplicate, odd/even split-and-merge · delete, rotate, crop, rescale pages ·
N-up layout · booklet imposition · poster tiling · overlay one PDF on another ·
join all pages into one long page · remove blank pages · stamp page numbers.

### Convert to PDF
Images · Office & text documents † · HTML files or ZIP packages † · Markdown † ·
web page URLs † · email (`.eml`/`.msg`) † · eBooks (EPUB/MOBI/AZW3/FB2/…) † ·
SVG · comic archives (CBZ, and CBR †).

### Convert from PDF
To images · Word/RTF, presentation, XML † · HTML · plain text · Markdown ·
bordered tables to CSV or XLSX · EPUB/AZW3 † · comic archives · slideshow video † ·
extract embedded images · detect and crop photos out of scanned pages.

### Security & signing
Password-encrypt / decrypt · text or image watermarks · sanitize (strip
JavaScript, embedded files, metadata, links, fonts) · redaction — hand-drawn
areas, whole pages, or search-and-redact by text/regex, rasterised so the
content is truly gone · sign with an X.509 certificate, remove signatures,
validate signatures · hardware / smart-card signing (Windows cert store and
PKCS#11 tokens, desktop app) · RFC 3161 trusted timestamps.

### OCR, repair & compress
Make a scan searchable with OCR † · repair a damaged or malformed PDF · compress
and optimise.

### Edit content
Find-and-replace visible text · a full visual editor (export the PDF to an
editable model, change it, write only the edited pages back) · add text or image
stamps · place images · add sticky-note comments · strip all images · invert or
recolour pages for dark mode · make a clean PDF look scanned · flatten form
fields and annotations into the page.

### Forms
List fields and values · fill fields · rename fields or change their properties ·
delete fields · export field values to CSV or XLSX · unlock read-only fields so
they can be filled again.

### Attachments, metadata & bookmarks
Attach, list, rename, delete and extract embedded files · edit document metadata
(title, author, custom keys) · export or rewrite the bookmark outline / table of
contents · rename the file from its detected title.

### Inspect a PDF
Page count and dimensions · document properties · font, annotation and form-field
inventories · encryption status and permissions · a full read-only report in one
call · check PDF/A, PDF/UA and WTPDF conformance · show embedded JavaScript.

### Automation
Chain several tools into one pipeline · run long jobs asynchronously and poll or
cancel them · a phone-to-desktop **mobile scanner** transfer · content filters
that pass a file through only if it matches (contains text or an image, a page
count, a page or file size, a rotation).

### AI-assisted — optional, off by default
A **separate AI engine**, disabled unless you turn it on and supply your own key
(Anthropic, OpenAI, or a self-hosted Ollama). No document content ever leaves the
machine while it is off. When on: plan edits from a natural-language prompt,
generate review comments as sticky notes, build a PDF from a structured document
model, audit the maths and figures in a document, and classify-and-label a PDF.
PDF question-answering is deliberately **not** offered.

---

**Optional tools, and what ships where.** The tools marked **†** are discovered on
`PATH` at startup; a missing one only disables its own feature. The Docker image
bundles LibreOffice, qpdf, Poppler, Tesseract, OCRmyPDF and WeasyPrint, and omits
Calibre, FFmpeg and the non-free RAR tools — so eBook, video and CBR conversion
show as unavailable there until you add the tool. The AI engine and outbound
email are off by default. Nothing hard-fails at startup and nothing silently
produces wrong output.

The exhaustive per-endpoint reference — every REST route, HTTP verb and dependency —
is in [`docs/product/features.md`](docs/product/features.md). Feature status and
documented differences from upstream Stirling-PDF live in
[`rust/PORT_STATUS.md`](rust/PORT_STATUS.md).

## Layout

| Path | What it is |
|---|---|
| `rust/crates/rustling-processing` | The backend: axum HTTP service mirroring the `/api/v1/...` REST surface |
| `rust/crates/rustling-ai-engine` | Optional AI engine (classification, PDF edit/review/create agents, math audit, orchestration) |
| `rust/crates/rustling-operation-catalog` | Generates the typed operation catalog from the OpenAPI snapshot |
| `rust/contracts/` | Per-surface behavior contracts (routes, semantics, documented divergences) |
| `frontend/editor` | Vite + React + TypeScript + Mantine SPA |
| `SwaggerDoc.json` | Frozen OpenAPI snapshot used for catalog regeneration |

The coordinated `Stirling` → `Rustling` rename has been executed: crates are
`rustling-*` and `RUSTLING_*` is the primary env-var spelling (legacy
`STIRLING_*` spellings keep working as deprecated aliases). A few identifiers
deliberately keep the old spelling for continuity with shipped releases —
the Tauri bundle identifier, desktop app-data directory, persisted storage
keys, and `X-Stirling-*` wire headers.

## Quick start

Prerequisites: [Rust](https://rustup.rs), [Task](https://taskfile.dev), Node.js + npm.

```bash
task rust:install     # Cargo deps + pinned PDFium (rev 7543, SHA-256 verified)
task backend:dev      # backend on http://127.0.0.1:8080
task frontend:dev     # SPA on http://127.0.0.1:5173 (proxies /api → 8080)
# or both at once:
task dev
# with the AI engine too:
task dev:all
```

Smoke check: `curl http://127.0.0.1:8080/api/v1/info/status`

Optional external tools (discovered at startup; missing ones simply disable
their endpoints with reason `DEPENDENCY`): LibreOffice, qpdf ≥ 12,
Tesseract/OCRmyPDF, WeasyPrint ≥ 58, Poppler `pdftohtml`, Calibre, unrar,
FFmpeg. Full operator guide, ports/binding, configuration and environment
reference: [`rust/RUNNING_WITH_RUST.md`](rust/RUNNING_WITH_RUST.md).

## Status

- **The product has no authentication and no server-side state — by design.**
  The former opt-in secured mode (login/users/teams/OIDC/MFA/audit/durable
  storage/policies/MCP) was removed entirely by maintainer decision on
  2026-07-28; legacy `security.*`/`mcp.*`/`storage.*`/`policies.*` settings
  keys are ignored with a one-line startup warning, never refused, so
  existing configs and desktop installs keep booting. PDF *document* security
  (password, redaction, sanitize, watermark, cert-sign + hardware signing,
  timestamping, signature validation) is unaffected.
- Test suite: **975 backend tests, 0 failed** on `main` (100 suites,
  `cargo test --workspace --locked` with PDFium bound), plus the frontend vitest
  suite and a desktop-shell gate. All three CI workflows above run on every push.
- The authoritative feature/parity ledger is
  [`rust/PORT_STATUS.md`](rust/PORT_STATUS.md); per-surface details live in
  [`rust/contracts/`](rust/contracts/).

## Roadmap

The detailed, living plan — current batch, queue, deferred items with unblock
conditions, and session hand-off instructions — is in [ROADMAP.md](ROADMAP.md).
Headlines: GitHub CI, single-binary SPA serving, Docker packaging, the
tag-driven GHCR release pipeline, and the Tauri Rust-sidecar desktop port have
landed; the coordinated `Stirling` → `Rustling` product rename has been
executed (crates, env-var spellings with back-compat aliases, UI branding,
startup handshake); and the no-auth/stateless-server decision has been
executed (auth subsystem, server-side state, MCP, and the AI PDF Q&A store
all removed). Next up is desktop release completion (updater signing +
Windows staging).

## Relationship to Stirling-PDF

RustlingPDF is a separate, standalone repository — not a fork remote, not a
submodule. Upstream Stirling-PDF remains the reference implementation its
behavior contracts were verified against.
