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
  <b>162 REST endpoints for working with PDFs.</b><br>
  A pure-Rust <code>axum</code> backend, a React single-page UI, and a Tauri desktop app.<br>
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

162 distinct `/api/v1/...` paths are wired into the axum router (one of them,
`/api/v1/general/job/{job_id}`, serves both `GET` and `DELETE`). 137 of them are
document features — every one is listed below. The remaining 25 are support
endpoints the SPA and operators use (`config/*`, `ui-data/*`, `info/*`,
`settings/get-endpoints-status`) plus `GET /health`, `GET /robots.txt` and a
legacy language-code shim. They process no documents; the ones the SPA calls
are listed in [`docs/product/features.md`](docs/product/features.md).

**There is no authentication, no accounts, no database and no server-side
storage.** That is a deliberate product guarantee, not a missing feature: every
request is self-contained, uploads land in an ephemeral scratch directory that is
TTL-swept, every `/api/` response carries `Cache-Control: private, no-store`, and
all user state lives in the client. There are consequently no roles, no
per-user permissions and no audit log — put the service behind your own reverse
proxy or on a trusted network if you need to restrict who can reach it.
`/api/v1/config/login-disclaimer` is a compatibility shim for old clients, not a
login.

Reading the tables:

- Every path is `POST` unless the row spells out another verb.
- **†** — needs an external program, named in the row. Tools are probed once at
  startup; a missing or too-old one never crashes the service, it just makes that
  endpoint unavailable: `GET /api/v1/config/endpoints-availability` reports
  `{"enabled": false, "reason": "DEPENDENCY"}` and calls return
  `403 This endpoint is disabled`. 17 registered routes are gated this way
  (the dependency table names 18 keys; one, `pdf-to-rtf`, has no route).
- **‡** — off until you deliberately enable it in configuration. Most such
  routes stay mounted and answer the same `403` with `reason: "CONFIG"`;
  `send-email` is not mounted at all and answers `404`.
- Administrators can additionally disable endpoints or whole functional groups
  (PageOps, Convert, Security, Other, Advance, Automation, DeveloperTools,
  DeveloperDocs) with `endpoints.toRemove` / `endpoints.groupsToRemove`; the
  refusal is the same `403`.
- PDFium is bundled, not optional — but it is a native library bound through
  `RUSTLING_PDFIUM_LIBRARY_PATH` (`task rust:install` does this). If it is
  unbound, every rendering path — among them `convert/pdf/img`, `convert/pdf/cbz`,
  `convert/pdf/video` frames, `misc/flatten`, `misc/scanner-effect`, the redaction
  routes, table extraction, native PDF→HTML and the Tesseract OCR fallback —
  returns `501` instead of guessing.

The tables below cover the **HTTP surface**. The bundled web UI adds tools that
run entirely in your browser and never reach these endpoints — page reordering
and preview, annotation, form filling, side-by-side compare and the multi-tool
workspace — so the app can do a little more than this list, never less.

### Organise pages

| What it does | Endpoint |
|---|---|
| Merge several PDFs into one | `/api/v1/general/merge-pdfs` |
| Split at chosen page breaks | `/api/v1/general/split-pages` |
| Split by target file size or page count | `/api/v1/general/split-by-size-or-count` |
| Split at chapter boundaries in the bookmark outline | `/api/v1/general/split-pdf-by-chapters` |
| Split *each* page into a grid of tiles | `/api/v1/general/split-pdf-by-sections` |
| Split a scanned batch automatically at QR divider pages | `/api/v1/misc/auto-split-pdf` |
| Tile pages for poster printing | `/api/v1/general/split-for-poster-print` |
| Reorder / select pages — custom order, reverse, duplicate, odd/even split and merge, booklet and side-stitch orders | `/api/v1/general/rearrange-pages` |
| Delete pages | `/api/v1/general/remove-pages` |
| Rotate pages | `/api/v1/general/rotate-pdf` |
| N pages per sheet (N-up layout) | `/api/v1/general/multi-page-layout` |
| Booklet imposition | `/api/v1/general/booklet-imposition` |
| Resize / rescale pages to a paper size | `/api/v1/general/scale-pages` |
| Crop pages | `/api/v1/general/crop` |
| Concatenate every page into one long page | `/api/v1/general/pdf-to-single-page` |
| Overlay another PDF onto a base PDF | `/api/v1/general/overlay-pdfs` |
| Detect and remove blank pages | `/api/v1/misc/remove-blanks` |
| Stamp page numbers | `/api/v1/misc/add-page-numbers` |

### Convert to PDF

| What it does | Endpoint |
|---|---|
| Images (JPEG/PNG/…) to PDF | `/api/v1/convert/img/pdf` |
| Office or text document to PDF † LibreOffice | `/api/v1/convert/file/pdf` |
| HTML file or ZIP package to PDF † WeasyPrint ≥ 58 | `/api/v1/convert/html/pdf` |
| Markdown to PDF † WeasyPrint ≥ 58 | `/api/v1/convert/markdown/pdf` |
| Web page URL to PDF † WeasyPrint ≥ 58, ‡ off by default (SSRF guard) | `/api/v1/convert/url/pdf` |
| Email — `.eml` or Outlook `.msg` — to PDF † WeasyPrint ≥ 58 | `/api/v1/convert/eml/pdf` |
| eBook (EPUB/MOBI/AZW3/FB2/TXT/DOCX) to PDF † Calibre | `/api/v1/convert/ebook/pdf` |
| SVG to PDF | `/api/v1/convert/svg/pdf` |
| PostScript / EPS to PDF † Ghostscript | `/api/v1/convert/vector/pdf` |
| CBZ / ZIP comic archive to PDF | `/api/v1/convert/cbz/pdf` |
| CBR / RAR comic archive to PDF † `unrar` (a `7z` build with real RAR codecs also counts) | `/api/v1/convert/cbr/pdf` |

### Convert from PDF

| What it does | Endpoint |
|---|---|
| Pages to images, one per page or combined | `/api/v1/convert/pdf/img` |
| To Word (`doc`/`docx`/`odt`, also RTF) † LibreOffice | `/api/v1/convert/pdf/word` |
| To presentation (`ppt`/`pptx`/`odp`) † LibreOffice | `/api/v1/convert/pdf/presentation` |
| To XML † LibreOffice | `/api/v1/convert/pdf/xml` |
| To HTML (Poppler `pdftohtml` when present, native PDFium otherwise) | `/api/v1/convert/pdf/html` |
| To plain text or RTF | `/api/v1/convert/pdf/text` |
| To Markdown | `/api/v1/convert/pdf/markdown` |
| Ruled (fully bordered) tables to CSV | `/api/v1/convert/pdf/csv` |
| Ruled (fully bordered) tables to XLSX | `/api/v1/convert/pdf/xlsx` |
| To EPUB or AZW3 † Calibre | `/api/v1/convert/pdf/epub` |
| To a CBZ comic archive | `/api/v1/convert/pdf/cbz` |
| To a CBR comic archive † `rar` (7-Zip cannot substitute) | `/api/v1/convert/pdf/cbr` |
| To an MP4/WebM slideshow video † FFmpeg | `/api/v1/convert/pdf/video` |
| To archival PDF/A or PDF/X † Ghostscript | `/api/v1/convert/pdf/pdfa` |
| To EPS / PS / PCL / XPS † Ghostscript | `/api/v1/convert/pdf/vector` |
| Extract embedded images to a ZIP | `/api/v1/misc/extract-images` |
| Detect and crop photos out of scanned pages | `/api/v1/misc/extract-image-scans` |

### Security & signing

| What it does | Endpoint |
|---|---|
| Encrypt with a password and permission flags | `/api/v1/security/add-password` |
| Remove the password / decrypt | `/api/v1/security/remove-password` |
| Add a text or image watermark | `/api/v1/security/add-watermark` |
| Sanitize: strip JavaScript, embedded files, metadata, links, fonts | `/api/v1/security/sanitize-pdf` |
| Redact hand-drawn areas and whole pages | `/api/v1/security/redact` |
| Search-and-redact text or regex matches | `/api/v1/security/auto-redact` |
| Combined redaction run (text, regex, page ranges, image boxes, page wipes) | `/api/v1/security/redact-execute` |
| Sign with an X.509 certificate | `/api/v1/security/cert-sign` |
| Remove existing signatures | `/api/v1/security/remove-cert-sign` |
| Report hardware-signing capabilities (`GET`, desktop app only) | `/api/v1/security/cert-sign/hardware/capabilities` |
| List Windows certificate-store certificates (`GET`, desktop app, loopback callers only) | `/api/v1/security/cert-sign/hardware/windows-certificates` |
| List PKCS#11 token certificates (desktop app, loopback callers only; needs a PKCS#11 module) | `/api/v1/security/cert-sign/hardware/pkcs11-certificates` |
| Add an RFC 3161 trusted timestamp | `/api/v1/security/timestamp-pdf` |
| Validate existing digital signatures | `/api/v1/security/validate-signature` |

All three redaction routes rasterise the page: content under a redaction is gone,
but so are selectable text, links, form fields and metadata on the affected pages.
The upstream opt-out parameter is accepted for wire compatibility and ignored
(`redact` and `auto-redact` take `convertPDFToImage`, `redact-execute` takes
`convertToImage`) — rasterising
unconditionally is a deliberate divergence from upstream's recoverable
overlay-rectangle default.

### OCR, repair & size

| What it does | Endpoint |
|---|---|
| Make a scan searchable (OCR) † OCRmyPDF *or* Tesseract — either alone is enough | `/api/v1/misc/ocr-pdf` |
| Repair a damaged or malformed PDF | `/api/v1/misc/repair` |
| Compress / optimise | `/api/v1/misc/compress-pdf` |
| Expand compressed object streams for inspection (makes the file *larger*) | `/api/v1/misc/decompress-pdf` |

`repair` and `compress-pdf` use qpdf ≥ 12 and Ghostscript as accelerators when
they happen to be installed and fall back to an in-process structural rewrite
otherwise; neither tool gates either endpoint.

### Edit content

| What it does | Endpoint |
|---|---|
| Find and replace visible text | `/api/v1/general/edit-text` |
| Export a whole PDF as an editable JSON model | `/api/v1/convert/pdf/text-editor` |
| Start a lazy editing session (metadata + fonts + job id) | `/api/v1/convert/pdf/text-editor/metadata` |
| Fetch one page of an editing session (`GET`) | `/api/v1/convert/pdf/text-editor/page/{job_id}/{page_number}` |
| Fetch one page's fonts (`GET`) | `/api/v1/convert/pdf/text-editor/fonts/{job_id}/{page_number}` |
| Write only the edited pages back to PDF | `/api/v1/convert/pdf/text-editor/partial/{job_id}` |
| Discard a cached editing session | `/api/v1/convert/pdf/text-editor/clear-cache/{job_id}` |
| Rebuild a PDF from a full edited JSON model | `/api/v1/convert/text-editor/pdf` |
| Add a text or image stamp | `/api/v1/misc/add-stamp` |
| Place an image on pages | `/api/v1/misc/add-image` |
| Add sticky-note comments | `/api/v1/misc/add-comments` |
| Strip all images from pages | `/api/v1/general/remove-image-pdf` |
| Replace or invert page colours (dark-mode PDFs) | `/api/v1/misc/replace-invert-pdf` |
| Make a clean PDF look scanned (cosmetic; unrelated to OCR) | `/api/v1/misc/scanner-effect` |
| Flatten form fields and annotations into page content | `/api/v1/misc/flatten` |

The JSON text-editor subsystem is an explicitly phased port: CFF conversion,
Type3 normalisation and complete CID rendering are still deferred, and the
partial-save path regenerates text rather than patching it. See
[`rust/contracts/pdf-json.md`](rust/contracts/pdf-json.md).

### Forms

| What it does | Endpoint |
|---|---|
| List form fields and their values | `/api/v1/form/fields` |
| List form fields with page coordinates | `/api/v1/form/fields-with-coordinates` |
| Fill field values | `/api/v1/form/fill` |
| Rename fields or change field properties | `/api/v1/form/modify-fields` |
| Delete fields | `/api/v1/form/delete-fields` |
| Export field values as CSV | `/api/v1/form/extract-csv` |
| Export field values as XLSX | `/api/v1/form/extract-xlsx` |
| Clear read-only / lock flags so fields become fillable again | `/api/v1/misc/unlock-pdf-forms` |

### Attachments, metadata & bookmarks

| What it does | Endpoint |
|---|---|
| Attach files to a PDF | `/api/v1/misc/add-attachments` |
| List embedded attachments | `/api/v1/misc/list-attachments` |
| Rename an embedded attachment | `/api/v1/misc/rename-attachment` |
| Delete an embedded attachment | `/api/v1/misc/delete-attachment` |
| Extract embedded attachments | `/api/v1/misc/extract-attachments` |
| Edit document metadata (title, author, and arbitrary custom Info keys) | `/api/v1/misc/update-metadata` |
| Export the bookmark outline | `/api/v1/general/extract-bookmarks` |
| Rewrite the bookmark outline / table of contents | `/api/v1/general/edit-table-of-contents` |
| Rename the file from its detected title | `/api/v1/misc/auto-rename` |

### Analysis & inspection

| What it does | Endpoint |
|---|---|
| Page count | `/api/v1/analysis/page-count` |
| Basic info: pages, PDF version, file size | `/api/v1/analysis/basic-info` |
| Document properties: title, author, dates, producer | `/api/v1/analysis/document-properties` |
| Per-page dimensions | `/api/v1/analysis/page-dimensions` |
| Font inventory | `/api/v1/analysis/font-info` |
| Annotation inventory | `/api/v1/analysis/annotation-info` |
| Form-field inventory | `/api/v1/analysis/form-fields` |
| Encryption status and permission bits | `/api/v1/analysis/security-info` |
| Full read-only document report in one call | `/api/v1/security/get-info-on-pdf` |
| Check declared PDF/A, PDF/UA and WTPDF conformance | `/api/v1/security/verify-pdf` |
| Show embedded document-level JavaScript | `/api/v1/misc/show-javascript` |

Two names mislead and are worth spelling out: `get-info-on-pdf` performs no
security action — it is the full document report — and `verify-pdf` checks
*standards conformance*, not signatures (signatures are `validate-signature`).
`verify-pdf` answers natively for documents that declare no profile; a document
that does declare one needs veraPDF installed, otherwise that request alone
returns `501`.

### Pipeline filters

Conditionals for the pipeline: each passes the file through or rejects it.

| What it does | Endpoint |
|---|---|
| Pass only if it contains given text | `/api/v1/filter/filter-contains-text` |
| Pass only if it contains an image | `/api/v1/filter/filter-contains-image` |
| Pass only on page count | `/api/v1/filter/filter-page-count` |
| Pass only on page size | `/api/v1/filter/filter-page-size` |
| Pass only on file size | `/api/v1/filter/filter-file-size` |
| Pass only on page rotation | `/api/v1/filter/filter-page-rotation` |

### Automation & async jobs

| What it does | Endpoint |
|---|---|
| Run a multi-step tool pipeline in one request | `/api/v1/pipeline/handleData` |
| Email a processed file as an attachment ‡ mounted only when SMTP is enabled (404 otherwise) | `/api/v1/general/send-email` |
| Check an async job's status (`GET`) or cancel it (`DELETE`) | `/api/v1/general/job/{job_id}` |
| Download an async job's result (`GET`) | `/api/v1/general/job/{job_id}/result` |
| List an async job's result files (`GET`) | `/api/v1/general/job/{job_id}/result/files` |
| Download one result file by id (`GET`) | `/api/v1/general/files/{file_id}` |
| Get one result file's metadata (`GET`) | `/api/v1/general/files/{file_id}/metadata` |
| Job counters (`GET`) | `/api/v1/admin/job/stats` |
| Queue depth and capacity counters (`GET`) | `/api/v1/admin/job/queue/stats` |
| Purge expired completed jobs | `/api/v1/admin/job/cleanup` |

Asynchronous execution is cross-cutting rather than a tool of its own: add
`?async=true` to a supported processing request and it is admitted through the
resource-weighted job queue, returning a job id to poll with the endpoints above.
The `admin/job/*` routes are open job-queue introspection — there are no accounts
to administer.

### Mobile scanner (phone to desktop)

| What it does | Endpoint |
|---|---|
| Start a transfer session | `/api/v1/mobile-scanner/create-session/{session_id}` |
| Check that a session is still open (`GET`) | `/api/v1/mobile-scanner/validate-session/{session_id}` |
| Upload scanned files from the phone | `/api/v1/mobile-scanner/upload/{session_id}` |
| List files waiting in the session (`GET`) | `/api/v1/mobile-scanner/files/{session_id}` |
| Download and clear one transferred file (`GET`) | `/api/v1/mobile-scanner/download/{session_id}/{filename}` |
| End the session (`DELETE`) | `/api/v1/mobile-scanner/session/{session_id}` |

The whole surface can be switched off with `system.enableMobileScanner`
(default on).

### AI-assisted (optional, off by default)

These eight routes are always mounted, but they proxy to `rustling-ai-engine`, a
**separate process that is disabled by default** (`aiEngine.enabled` /
`AIENGINE_ENABLED`, default `false`; in Docker Compose it only starts under the
`ai` profile). While it is off they refuse every call and no document content ever
leaves the machine. It is the only part of RustlingPDF that talks to an external
LLM provider, and you supply your own key (Anthropic, OpenAI, or a self-hosted
Ollama). PDF question-answering is deliberately *not* offered — it and its
document store were removed.

| What it does | Endpoint |
|---|---|
| AI engine reachability check (`GET`) | `/api/v1/ai/health` |
| Plan PDF edits from a natural-language prompt (returns a JSON plan, not a PDF) | `/api/v1/ai/pdf/edit` |
| Run one AI workflow turn over uploaded files | `/api/v1/ai/orchestrate` |
| Stream an AI workflow turn as NDJSON | `/api/v1/ai/orchestrate/stream` |
| Generate review comments as sticky notes | `/api/v1/ai/tools/pdf-comment-agent` |
| Build a PDF from a structured document model (no HTML is accepted; the name is kept for workflow compatibility) | `/api/v1/ai/tools/create-pdf-from-html-agent` |
| Audit the maths and figures in a document | `/api/v1/ai/tools/math-auditor-agent` |
| Classify a PDF and write the label into it | `/api/v1/ai/tools/classify-and-label` |

### What a stock install actually exposes

On a default install `POST /api/v1/general/send-email` is not mounted at all
(`mail.enabled` is `false`), `convert/url/pdf` is off by configuration, and the AI
engine is off — so 161 paths are registered and the AI routes refuse. Of the
rest, 17 endpoint keys need one of LibreOffice, WeasyPrint ≥ 58, Ghostscript,
Calibre, FFmpeg, `unrar`, `rar`, or Tesseract/OCRmyPDF. The shipped Docker image
installs LibreOffice, Ghostscript, qpdf, Poppler, Tesseract, OCRmyPDF and
WeasyPrint, and deliberately omits Calibre, FFmpeg, veraPDF and the non-free RAR
tools — so `convert/pdf/epub`, `convert/ebook/pdf`, `convert/cbr/pdf`,
`convert/pdf/cbr` and `convert/pdf/video` report themselves disabled there until
you add the tool yourself. Nothing here hard-fails at startup and nothing silently
produces wrong output.

Two changes are in flight and the ledger, not this list, is authoritative on
whether they have landed:

- **Ghostscript is being withdrawn** by maintainer decision. When it goes, the
  three Ghostscript-gated conversions above (`convert/pdf/pdfa`,
  `convert/pdf/vector`, `convert/vector/pdf`) go with it, and the places where it
  only assists — `compress-pdf`, `crop`'s `removeDataOutsideCrop`, `ocr-pdf`'s
  `removeImagesAfter`, `replace-invert-pdf`'s CMYK conversion, `ebook/pdf`
  optimisation — move to pure Rust.
- **The desktop build discovers external tools from `PATH`** exactly as the server
  does; bundling qpdf and Tesseract into the Windows installer is planned work
  described in
  [`rust/contracts/desktop-windows-installer.md`](rust/contracts/desktop-windows-installer.md).

The exhaustive per-endpoint reference — every route including the 25 support
endpoints, with HTTP verbs and the full dependency table — is
[`docs/product/features.md`](docs/product/features.md). Remaining gaps and
documented divergences from upstream live in
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
their endpoints with reason `DEPENDENCY`): LibreOffice, Ghostscript, qpdf ≥ 12,
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
