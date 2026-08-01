# What RustlingPDF can do

This is the complete, code-derived feature reference. Every row below is a route
actually wired into the axum router in
[`rust/crates/rustling-processing/src/lib.rs`](../../rust/crates/rustling-processing/src/lib.rs)
(some registered by sibling modules and merged in) — not a wishlist or a
marketing inventory. **166 distinct `/api/v1/...` endpoints** are registered.
See [Methodology](#methodology) for how this list was produced and how it was
cross-checked.

For install/run instructions, ports, and environment variables, see
[`rust/RUNNING_WITH_RUST.md`](../../rust/RUNNING_WITH_RUST.md). Public behavior
details live in [`rust/contracts/`](../../rust/contracts/).

## No accounts, no server-side storage

There is no login, no user accounts, and no database. Every request is
self-contained: you upload a file, the service processes it in an ephemeral
scratch directory, and the result is swept away on a TTL. Nothing about your
documents is retained between requests, and there is nothing to configure to
get this — it is the only mode the service has. The `/api/v1/info/*` request
counters are in-memory only, are served solely to whoever asks those routes,
and reset on restart.

## No analytics, no tracking

RustlingPDF ships no analytics, no telemetry, and no tracking pixel. There is
no vendor SDK in the web app, no opt-in prompt, no consent banner, and no
setting to turn any of it on — the code to do it does not exist, so there is
nothing to trust us about. The web app talks to your own RustlingPDF backend
and to nothing else.

This also means: no roles, no per-user permissions, and no audit log. If you
need to restrict who can reach the service, put it behind your own reverse
proxy or on a trusted network — see the no-auth note in
[`rust/RUNNING_WITH_RUST.md`](../../rust/RUNNING_WITH_RUST.md).

## Endpoints that need an external tool

166 endpoints are always registered, but **14** only work when a matching
external program is installed on the machine (or container image) running the
backend. Each is probed once at startup; a missing or too-old tool never
crashes the service — the endpoint simply reports itself unavailable:

- `GET /api/v1/config/endpoints-availability` returns `{"enabled": false,
  "reason": "DEPENDENCY"}` for it.
- Calling it directly returns an explicit error (HTTP `501` in most cases)
  rather than a silent fallback or wrong output.

| Needs | Endpoints | Install |
|---|---|---|
| LibreOffice | `convert/pdf/word`, `convert/pdf/presentation`, `convert/pdf/xml`; and `convert/file/pdf` for every input other than `.docx`/`.xlsx`/`.pptx`, which a built-in pure-Rust engine converts with nothing installed | `soffice` on `PATH` |
| WeasyPrint ≥ 58 | `convert/html/pdf`, `convert/url/pdf`, `convert/markdown/pdf`, `convert/eml/pdf` | `weasyprint` on `PATH` |
| Calibre | `convert/pdf/epub`, `convert/ebook/pdf` | `ebook-convert` on `PATH` |
| unrar (or 7-Zip) / `rar` | `convert/cbr/pdf` (needs `unrar`/`7z`), `convert/pdf/cbr` (needs `rar`) | `unrar`/`7z` and `rar` on `PATH` |
| FFmpeg | `convert/pdf/video` | `ffmpeg` on `PATH` |
| Tesseract *or* OCRmyPDF | `misc/ocr-pdf` | either on `PATH`; disabled only when **both** are missing |

Every tool's binary can be pinned explicitly with a
`RUSTLING_PROCESSING_<TOOL>_COMMAND` environment variable; see
[`rust/RUNNING_WITH_RUST.md`](../../rust/RUNNING_WITH_RUST.md#optional-external-tools)
for the full list, including the "assist only" tools (qpdf, Poppler
`pdftohtml`) that improve specific endpoints when present but never gate them
off.

veraPDF is a special case worth knowing before you rely on `security/verify-pdf`.
It is an **input-conditional** dependency: a file that declares no validation
profile completes natively and reports `not-pdfa`, so the endpoint stays listed
as available. A file that *does* declare PDF/A, PDF/UA or WTPDF needs veraPDF to
validate it, and without veraPDF that request is refused with `501` rather than
answered. No shipped profile includes veraPDF — not a bare install, not the
desktop bundle, and the Docker image excludes it — so unless an operator installs
it, the conformance check that most people come to this tool for will refuse.
`endpoints-availability` cannot express "available for some inputs", which is
why the endpoint reads as enabled; see `rust/contracts/verify-pdf.md`.

`convert/url/pdf` (URL → PDF) has one more gate on top of WeasyPrint: it is
**disabled by default even when WeasyPrint is installed**, as an SSRF guard.
Set `RUSTLING_PROCESSING_ENABLE_URL_TO_PDF=true` (or
`SYSTEM_ENABLE_URL_TO_PDF`) to turn it on deliberately.

> **Desktop app:** desktop builds bundle qpdf and Tesseract with English
> language data. PDF repair and English OCR therefore work without a separate
> tool installation; operator-set command and tessdata paths still take
> precedence.

## The optional AI engine

`rustling-ai-engine` is a **separate container, disabled by default**
(`AIENGINE_ENABLED=false`; in Docker Compose it only starts under the `ai`
profile). It is the only part of RustlingPDF that talks to an external LLM
provider, and you supply your own key (`ANTHROPIC_API_KEY` or
`OPENAI_API_KEY`, or point it at a self-hosted Ollama server). **If the engine
is not running, no document content ever leaves the machine** — the processing
backend behaves exactly as if the AI tools didn't exist, and `ai/health`
reports it unreachable.

When enabled, it adds page-cited document summary, caller-schema extraction,
page/block-ordered translation, document classification, a math/claims auditor,
PDF review-comment generation, an AI PDF-edit planner, generating a PDF from a
structured description, and multi-step orchestration across these. Dedicated
document-understanding routes keep PDF bytes in the processing service and send
only locally extracted text within the configured page/character limits. They
are single-request operations and keep no document, prompt, or result store.
Translation returns structured text and does not claim pixel-perfect PDF layout
preservation. It does **not** do PDF question-answering — that capability, along
with the document store it depended on, was removed by maintainer decision. See
the table under [Optional AI engine tools](#optional-ai-engine-tools).

## Feature reference

Method column lists every HTTP verb registered on that path. A route marked
*(needs …)* is one of the 14 dependency-gated endpoints above.

### Organize pages

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/general/booklet-imposition` | Impose pages for booklet printing |
| POST | `/api/v1/general/crop` | Crop visible page area |
| POST | `/api/v1/general/edit-table-of-contents` | Replace the bookmark/outline tree from JSON |
| POST | `/api/v1/general/extract-bookmarks` | Read the bookmark/outline tree as JSON |
| POST | `/api/v1/general/merge-pdfs` | Merge multiple PDFs into one |
| POST | `/api/v1/general/multi-page-layout` | Lay out N source pages per output page (N-up) |
| POST | `/api/v1/general/overlay-pdfs` | Overlay one PDF's pages on top of another's |
| POST | `/api/v1/general/pdf-to-single-page` | Stack every page into one long page |
| POST | `/api/v1/general/rearrange-pages` | Reorder pages (explicit order or presets) |
| POST | `/api/v1/general/remove-pages` | Delete specific pages |
| POST | `/api/v1/general/rotate-pdf` | Rotate pages |
| POST | `/api/v1/general/scale-pages` | Resize page dimensions / paper size |
| POST | `/api/v1/general/split-by-size-or-count` | Split by target file size or page count per part |
| POST | `/api/v1/general/split-for-poster-print` | Split one large page into a tiled poster for printing |
| POST | `/api/v1/general/split-pages` | Split at explicit page numbers |
| POST | `/api/v1/general/split-pdf-by-chapters` | Split at chapter/bookmark boundaries |
| POST | `/api/v1/general/split-pdf-by-sections` | Split each page into a grid of equal sections |
| POST | `/api/v1/misc/add-page-numbers` | Stamp page numbers |
| POST | `/api/v1/misc/auto-split-pdf` | Auto-split a scanned batch at printed divider/QR pages |
| POST | `/api/v1/misc/remove-blanks` | Detect and remove blank pages |

### Convert to PDF

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/convert/cbr/pdf` | Comic book archive (.cbr) to PDF *(needs unrar)* |
| POST | `/api/v1/convert/cbz/pdf` | Comic book archive (.cbz/.zip) to PDF |
| POST | `/api/v1/convert/ebook/pdf` | Ebook (EPUB/MOBI/…) to PDF *(needs Calibre)* |
| POST | `/api/v1/convert/eml/pdf` | Email (.eml) to PDF *(needs WeasyPrint)* |
| POST | `/api/v1/convert/file/pdf` | Office document (Word/Excel/PowerPoint/…) to PDF *(built in for `.docx`/`.xlsx`/`.pptx`; other formats need LibreOffice)* |
| POST | `/api/v1/convert/html/pdf` | HTML to PDF *(needs WeasyPrint)* |
| POST | `/api/v1/convert/img/pdf` | Images (JPEG/PNG/…) to PDF |
| POST | `/api/v1/convert/markdown/pdf` | Markdown to PDF *(needs WeasyPrint)* |
| POST | `/api/v1/convert/svg/pdf` | SVG to PDF |
| POST | `/api/v1/convert/text-editor/pdf` | Structured JSON document model to PDF (text-editor save path) |
| POST | `/api/v1/convert/url/pdf` | Web page URL to PDF (off by default; see note) *(needs WeasyPrint)* |

### Convert from PDF

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/convert/pdf/cbr` | PDF to comic book archive (.cbr) *(needs unrar (write path needs `rar`))* |
| POST | `/api/v1/convert/pdf/cbz` | PDF to comic book archive (.cbz) |
| POST | `/api/v1/convert/pdf/csv` | Extract a table from the PDF to CSV |
| POST | `/api/v1/convert/pdf/epub` | PDF to EPUB *(needs Calibre)* |
| POST | `/api/v1/convert/pdf/html` | PDF to HTML |
| POST | `/api/v1/convert/pdf/img` | PDF pages to images |
| POST | `/api/v1/convert/pdf/markdown` | PDF to Markdown |
| POST | `/api/v1/convert/pdf/presentation` | PDF to PowerPoint presentation *(needs LibreOffice)* |
| POST | `/api/v1/convert/pdf/text` | PDF to plain text or RTF (`outputFormat`: `txt` or `rtf`) |
| POST | `/api/v1/convert/pdf/video` | PDF pages to an MP4/WebM slideshow *(needs FFmpeg)* |
| POST | `/api/v1/convert/pdf/word` | PDF to Word document *(needs LibreOffice)* |
| POST | `/api/v1/convert/pdf/xlsx` | Extract a table from the PDF to XLSX |
| POST | `/api/v1/convert/pdf/xml` | PDF to XML *(needs LibreOffice)* |

### PDF text editor (structured edit-in-place)

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/convert/pdf/text-editor` | Open a PDF as an editable structured JSON document |
| POST | `/api/v1/convert/pdf/text-editor/clear-cache/{job_id}` | Discard a text-editor session's server-side cache |
| GET | `/api/v1/convert/pdf/text-editor/fonts/{job_id}/{page_number}` | Fetch font data for one page |
| POST | `/api/v1/convert/pdf/text-editor/metadata` | Read document metadata for the text-editor session |
| GET | `/api/v1/convert/pdf/text-editor/page/{job_id}/{page_number}` | Fetch one page's editable content |
| POST | `/api/v1/convert/pdf/text-editor/partial/{job_id}` | Save partial in-progress edits |

### Scan cleanup, OCR, repair & size

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/misc/auto-rename` | Auto-rename a file from its detected title |
| POST | `/api/v1/misc/compress-pdf` | Compress / reduce file size |
| POST | `/api/v1/misc/decompress-pdf` | Decompress internal streams for inspection or editing |
| POST | `/api/v1/misc/extract-image-scans` | Detect and extract photographed pages/scans |
| POST | `/api/v1/misc/ocr-pdf` | OCR: add a searchable text layer to a scanned PDF *(needs Tesseract or OCRmyPDF)* |
| POST | `/api/v1/misc/repair` | Repair a damaged or malformed PDF |
| POST | `/api/v1/misc/replace-invert-pdf` | Replace or invert colors (e.g. dark-mode PDF) |
| POST | `/api/v1/misc/scanner-effect` | Simulate a scanned-document look |

### Security & document protection

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/misc/unlock-pdf-forms` | Remove form-field read-only/locking restrictions |
| POST | `/api/v1/security/add-password` | Add an owner/user password |
| POST | `/api/v1/security/add-watermark` | Add a text or image watermark |
| POST | `/api/v1/security/auto-redact` | Auto-redact text matching a search term/pattern |
| POST | `/api/v1/security/cert-sign` | Sign with a certificate (incremental PDF signature) |
| GET | `/api/v1/security/cert-sign/hardware/capabilities` | List available hardware-signing capabilities |
| POST | `/api/v1/security/cert-sign/hardware/pkcs11-certificates` | List certificates on a connected PKCS#11 token |
| GET | `/api/v1/security/cert-sign/hardware/windows-certificates` | List certificates in the Windows certificate store |
| POST | `/api/v1/security/get-info-on-pdf` | Full report: metadata, permissions, compliance, form fields, embedded content |
| POST | `/api/v1/security/redact` | Manually redact caller-specified regions |
| POST | `/api/v1/security/redact-execute` | Burn in redaction boxes (irreversibly remove the covered content) |
| POST | `/api/v1/security/remove-cert-sign` | Remove an existing certificate signature |
| POST | `/api/v1/security/remove-password` | Remove a password (with the correct password) |
| POST | `/api/v1/security/sanitize-pdf` | Strip JavaScript, embedded files, metadata, or links |
| POST | `/api/v1/security/timestamp-pdf` | Apply an RFC 3161 trusted timestamp |
| POST | `/api/v1/security/validate-signature` | Validate an existing digital signature |
| POST | `/api/v1/security/verify-pdf` | Verify PDF integrity/signature |

### Inspect a PDF

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/accessibility/check` | Report tagged structure, language, reading/tab order, Figure alternatives, and form labels |
| POST | `/api/v1/accessibility/remediate` | Apply explicit bounded accessibility repairs and return a PDF |
| POST | `/api/v1/analysis/annotation-info` | Annotation counts by type |
| POST | `/api/v1/analysis/basic-info` | Page count, PDF version, file size |
| POST | `/api/v1/analysis/document-properties` | Title/author/subject/keywords/creator/producer/dates |
| POST | `/api/v1/analysis/font-info` | Fonts used per page |
| POST | `/api/v1/analysis/form-fields` | Form field count and presence of signatures/XFA |
| POST | `/api/v1/analysis/page-count` | Page count |
| POST | `/api/v1/analysis/page-dimensions` | Per-page width/height |
| POST | `/api/v1/analysis/security-info` | Encryption status, key length, permission flags |

The accessibility report is checker-first and does not claim PDF/UA
certification. It can set language and structure tab order, mark an existing
structure tree, and apply user-authored Figure descriptions or form labels. It
does not synthesize tags, guess semantic reading order, add PDF/UA metadata, or
create PDF/A.

### Extract & annotate content

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/general/edit-text` | Edit visible page text in place |
| POST | `/api/v1/general/remove-image-pdf` | Remove images from the document |
| POST | `/api/v1/misc/add-attachments` | Attach embedded files |
| POST | `/api/v1/misc/add-comments` | Add sticky-note comments |
| POST | `/api/v1/misc/add-image` | Place an image on a page |
| POST | `/api/v1/misc/add-stamp` | Add a text or image stamp |
| POST | `/api/v1/misc/delete-attachment` | Remove an embedded file |
| POST | `/api/v1/misc/extract-attachments` | Download embedded files as a ZIP |
| POST | `/api/v1/misc/extract-images` | Extract embedded images |
| POST | `/api/v1/misc/flatten` | Flatten form fields / annotations into page content |
| POST | `/api/v1/misc/list-attachments` | List embedded files |
| POST | `/api/v1/misc/rename-attachment` | Rename an embedded file |
| POST | `/api/v1/misc/show-javascript` | Extract embedded document-level JavaScript |
| POST | `/api/v1/misc/update-metadata` | Edit document metadata fields |

### Forms

The editor exposes Fill, Create, Batch, and Modify modes. Create places and
resizes widgets directly on PDF pages; Batch accepts CSV/XLSX rows and downloads
the filled PDFs as a ZIP.

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/form/batch-fill` | Fill one PDF per CSV/XLSX row and return a ZIP |
| POST | `/api/v1/form/delete-fields` | Delete form fields |
| POST | `/api/v1/form/create-fields` | Create accessible AcroForm fields and widgets |
| POST | `/api/v1/form/extract-csv` | Export form field values to CSV |
| POST | `/api/v1/form/extract-xlsx` | Export form field values to XLSX |
| POST | `/api/v1/form/fields` | List form fields and their values |
| POST | `/api/v1/form/fields-with-coordinates` | List form fields with page position |
| POST | `/api/v1/form/fill` | Fill form field values |
| POST | `/api/v1/form/modify-fields` | Change form field properties |

### Filters (pipeline conditionals)

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/filter/filter-contains-image` | Pass/fail: page(s) contain an image |
| POST | `/api/v1/filter/filter-contains-text` | Pass/fail: page(s) contain given text |
| POST | `/api/v1/filter/filter-file-size` | Pass/fail: file size comparison |
| POST | `/api/v1/filter/filter-page-count` | Pass/fail: page count comparison |
| POST | `/api/v1/filter/filter-page-rotation` | Pass/fail: first-page rotation comparison |
| POST | `/api/v1/filter/filter-page-size` | Pass/fail: first-page paper-size comparison |

### Automation & async jobs

The `rustlingpdf` local CLI exposes generated catalog operations through
`operations`, `describe`, `run`, and `pipeline`. It validates parameters against
the same JSON Schemas used by the AI operation boundary, then runs the HTTP
pipeline router in-process: no listener, account, upload to another service, or
durable server state is involved. File bytes go to an explicit output path
unless `--output -` deliberately selects binary stdout. See
[`rust/contracts/cli.md`](../../rust/contracts/cli.md).

| Method | Endpoint | What it does |
|---|---|---|
| GET | `/api/v1/general/files/{file_id}` | Download one result file by id |
| GET | `/api/v1/general/files/{file_id}/metadata` | Get one result file's metadata |
| DELETE/GET | `/api/v1/general/job/{job_id}` | Check an async job's status, or cancel it |
| GET | `/api/v1/general/job/{job_id}/result` | Download an async job's result file |
| GET | `/api/v1/general/job/{job_id}/result/files` | List an async job's result-file metadata |
| POST | `/api/v1/general/send-email` | Email a processed file as an attachment (needs SMTP configured) |
| POST | `/api/v1/pipeline/handleData` | Run a multi-step pipeline of operations in one request |

### Job-queue operations (no auth; open)

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/jobs/cleanup` | Purge expired completed jobs |
| GET | `/api/v1/jobs/queue/stats` | Job queue depth/capacity counters |
| GET | `/api/v1/jobs/stats` | Job counters (total/active/completed/failed) |

### Local-first mobile scanner

`/mobile-scanner` is an installable browser scanner and also the target of the
desktop QR flow. It works without a session for local capture/export, supports
camera or multi-photo input, automatic edge detection, draggable perspective
corners, rotation and document cleanup filters, page reordering, and creates
one ordered multi-page PDF in the browser. After the scanner shell has loaded
successfully once, its same-origin code/static assets are available offline;
API responses and documents are never placed in that cache. A user may export
locally, continue to OCR or Sign, or explicitly send the PDF through the
ten-minute ephemeral transfer API:

| Method | Endpoint | What it does |
|---|---|---|
| POST | `/api/v1/mobile-scanner/create-session/{session_id}` | Start a phone-to-desktop transfer session |
| GET | `/api/v1/mobile-scanner/download/{session_id}/{filename}` | Download (and clear) one transferred file |
| GET | `/api/v1/mobile-scanner/files/{session_id}` | List files waiting in the session |
| DELETE | `/api/v1/mobile-scanner/session/{session_id}` | End the session |
| POST | `/api/v1/mobile-scanner/upload/{session_id}` | Upload scanned files from the phone |
| GET | `/api/v1/mobile-scanner/validate-session/{session_id}` | Check a session is still open |

### Optional AI engine tools

| Method | Endpoint | What it does |
|---|---|---|
| GET | `/api/v1/ai/health` | AI engine reachability check |
| POST | `/api/v1/ai/orchestrate` | Multi-step AI-driven workflow orchestration |
| POST | `/api/v1/ai/orchestrate/stream` | Same, as a streamed NDJSON progress feed |
| POST | `/api/v1/ai/pdf/edit` | AI-planned PDF edit (proxies to the engine, then runs the plan) |
| POST | `/api/v1/ai/tools/document-summary` | Summarize bounded locally extracted page text with validated page references |
| POST | `/api/v1/ai/tools/document-extraction` | Extract caller-defined structured fields with validated source pages |
| POST | `/api/v1/ai/tools/document-translation` | Translate text while preserving extracted page boundaries and stable block order |
| POST | `/api/v1/ai/tools/classify-and-label` | Classify a document against a caller-supplied label set |
| POST | `/api/v1/ai/tools/create-pdf-from-html-agent` | Generate a PDF from a structured document description |
| POST | `/api/v1/ai/tools/math-auditor-agent` | Audit mathematical claims/figures in a PDF |
| POST | `/api/v1/ai/tools/pdf-comment-agent` | Generate AI sticky-note review comments |

## Other API surface (for integrators, not document features)

25 more endpoints exist purely to back the SPA or an operator's tooling —
listed here for completeness, not because they process a PDF:

- `GET /api/v1/config/*` (`app-config`, `endpoint-enabled`,
  `endpoints-availability`, `endpoints-enabled`, `group-enabled`,
  `login-disclaimer`) — what the UI should show/hide, the optional anonymous
  agreement, and why an endpoint is disabled.
- `GET /api/v1/settings/get-endpoints-status` — same availability data via a
  legacy shape.
- `GET /api/v1/ui-data/*` (`footer-info`, `home`, `licenses`, `ocr-pdf`,
  `pipeline`, `sign`) — static/derived data the SPA renders (third-party
  license text, tool landing-page copy, etc.).
- `GET /api/v1/info/*` (`health`, `status`, `uptime`, `load*`, `requests*`,
  `wau`) — in-memory request counters and a health probe; see
  [No accounts, no server-side storage](#no-accounts-no-server-side-storage).

## Methodology

This list was produced by parsing every `.route(...)` call reachable from
`rustling_processing::app()` — the exact function the production binary calls
via `ProcessingRuntime` — across `rust/crates/rustling-processing/src/*.rs`,
resolving path constants to their literal strings, and deduplicating. It was
then cross-checked against the per-surface contracts in `rust/contracts/` and
against `ENDPOINT_GROUPS` in
[`runtime_config.rs`](../../rust/crates/rustling-processing/src/runtime_config.rs)
(the table that drives `DEPENDENCY`/`CONFIG` availability reporting) to find
the 17 tool-gated endpoints. It is not sourced from frontend menu labels or
marketing copy. `SwaggerDoc.json` is the committed API-generation input and is
validated separately against its consumers.

RustlingPDF is stateless at the server boundary: documents enter one operation
or pipeline and leave as explicit results. Browser-managed files, preferences,
and saved signatures stay on the user's device. PDF document security —
passwords, redaction, sanitization, watermarking, certificate signing,
hardware signing, RFC 3161 timestamping, and signature validation — remains
part of document processing.
