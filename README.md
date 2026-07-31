<p align="center">
  <img src="docs/assets/logo.png" alt="RustlingPDF" width="280">
</p>

<p align="center">
  <strong>Own your PDFs.</strong><br>
  A powerful, open-source PDF workbench for desktop, web, mobile, and CLI.<br>
  Pure-Rust backend. No account. No database. No document storage.
</p>

<p align="center">
  <a href="https://github.com/hairbui76/RustlingPDF/actions/workflows/backend.yml"><img alt="Backend CI" src="https://github.com/hairbui76/RustlingPDF/actions/workflows/backend.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/hairbui76/RustlingPDF/actions/workflows/frontend.yml"><img alt="Frontend CI" src="https://github.com/hairbui76/RustlingPDF/actions/workflows/frontend.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/hairbui76/RustlingPDF/actions/workflows/desktop.yml"><img alt="Desktop CI" src="https://github.com/hairbui76/RustlingPDF/actions/workflows/desktop.yml/badge.svg?branch=main"></a>
  <br>
  <a href="https://github.com/hairbui76/RustlingPDF/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/hairbui76/RustlingPDF?color=60C948&label=release"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-60C948"></a>
  <img alt="Rust backend" src="https://img.shields.io/badge/backend-100%25%20Rust-60C948">
  <img alt="No account required" src="https://img.shields.io/badge/account-not%20required-60C948">
  <img alt="Local CLI" src="https://img.shields.io/badge/CLI-rustlingpdf-60C948">
</p>

<p align="center">
  <a href="https://github.com/hairbui76/RustlingPDF/releases/latest"><strong>Download desktop</strong></a>
  ·
  <a href="#run-with-docker"><strong>Run with Docker</strong></a>
  ·
  <a href="#cli-yes-it-is-built-in"><strong>Use the CLI</strong></a>
  ·
  <a href="docs/product/features.md"><strong>Explore every feature</strong></a>
</p>

---

RustlingPDF brings a broad PDF toolkit into one local-first application. Use the
visual React interface, install the Tauri desktop app, self-host the web service,
scan from a phone, call the REST API, or automate local files with the
`rustlingpdf` CLI.

The processing service is written in Rust and exposes **166 `/api/v1/...`
endpoints**. There is no Java or JVM at build or run time.

## Why RustlingPDF?

| | |
| :-- | :-- |
| **Private by design** | No login, account, database, or durable server-side document storage. Temporary request files are swept automatically. |
| **One toolbox, many surfaces** | Desktop app, self-hosted web UI, Docker image, installable mobile scanner PWA, REST API, and local CLI. |
| **Built for real workflows** | Go beyond merge and split: design forms, batch-fill records, check accessibility, OCR scans, sign documents, redact content, and compose pipelines. |
| **Automation without a server** | The typed `rustlingpdf` CLI runs the same processing pipeline in-process against local files. |
| **AI only when you choose it** | Summary, extraction, and translation are optional, stateless, disabled by default, and support BYOK or local Ollama. |
| **Honest dependency handling** | Optional native tools are detected at startup. Missing tools disable only the features that require them. |

## Feature highlights

| Area | What you can do |
| :-- | :-- |
| **Page workshop** | Merge, reorder, rotate, crop, rescale, overlay, remove blanks, add page numbers, and split by page, size, count, chapter, section, tile, or QR divider |
| **Visual editing** | Edit structured text, add images and stamps, replace text, annotate, recolor for dark mode, manage bookmarks, and flatten content |
| **Forms at scale** | Visually draw and resize accessible fields, align or duplicate them, edit properties and tab order, fill forms, and batch-fill CSV/XLSX rows into PDFs |
| **Accessibility** | Inspect language, tags, reading structure, figure alternative text, form labels, and tab order; apply bounded, user-reviewed remediation |
| **Scan, OCR, and repair** | Capture and clean multi-page scans from a phone, correct perspective, reorder pages, add searchable OCR text, compress files, and repair damaged PDFs |
| **Convert almost anything** | Convert images, Office files, HTML, Markdown, email, eBooks, SVG, and comic archives to PDF; export PDF to images, text, Word, PowerPoint, HTML, Markdown, CSV/XLSX, EPUB, video, and more |
| **Protect and sign** | Set passwords and permissions, sanitize metadata, redact content, add watermarks, sign with X.509 or hardware/smart-card certificates, timestamp, and validate signatures |
| **Inspect and manage** | Review fonts, pages, annotations, forms, encryption, attachments, metadata, and PDF/A, PDF/UA, or WTPDF conformance |
| **Automate** | Build multi-step pipelines visually, drag and drop to reorder steps, save or exchange them as JSON, run typed CLI operations, call the REST API, or submit asynchronous jobs |
| **AI-assisted, optional** | Produce page-cited summaries, schema-driven extraction, ordered translation, edit plans, review comments, generated documents, math audits, and classification |

The detailed, code-derived reference lists every registered route and dependency
requirement in [What RustlingPDF can do](docs/product/features.md).

## Run it your way

| Surface | Best for | Notes |
| :-- | :-- | :-- |
| **Desktop app** | Private day-to-day use | Native Tauri shell; desktop packages bundle qpdf and Tesseract with English OCR data |
| **Docker** | Home lab, team network, or server | Web UI and REST API in one container |
| **From source** | Development and customization | Rust backend plus Vite/React frontend |
| **CLI** | Shell scripts, CI, and batch jobs | Processes local files directly; no server or account |
| **Mobile scanner PWA** | Capturing paper documents | Local multi-page capture and PDF export, with optional temporary phone-to-desktop transfer |

### Run with Docker

```bash
docker pull ghcr.io/hairbui76/rustlingpdf:latest
docker run --rm -p 8080:8080 ghcr.io/hairbui76/rustlingpdf:latest
```

Open <http://localhost:8080>.

For Compose, local builds, configuration mounts, and the optional AI sidecar,
see [Running RustlingPDF](rust/RUNNING_WITH_RUST.md).

### Run from source

Prerequisites: [Rust](https://rustup.rs), [Task](https://taskfile.dev), and
Node.js with npm.

```bash
task rust:install     # fetch Cargo dependencies and install pinned PDFium
task dev              # start the Rust backend and web UI
```

The UI opens on the address printed by Task. To include the optional stateless
AI engine, use:

```bash
task dev:all
```

You can also run each component separately:

```bash
task backend:dev      # http://127.0.0.1:8080
task frontend:dev     # http://127.0.0.1:5173, proxies /api to the backend
```

Smoke check:

```bash
curl http://127.0.0.1:8080/api/v1/info/status
```

## CLI: yes, it is built in

`rustlingpdf` is a first-class local automation CLI. It generates its operation
bindings from the same catalog used by the HTTP pipeline, validates parameters
against the catalog JSON Schemas, and invokes the processing runtime in-process.
It does **not** start a listener, upload files to a RustlingPDF server, require
an account, or create durable server state.

Install it from a source checkout:

```bash
task rust:install
cargo install --path rust/crates/rustling-cli --locked
```

Discover operations and inspect their parameters:

```bash
rustlingpdf operations
rustlingpdf operations --json
rustlingpdf describe general-rotate-pdf
```

Run one operation:

```bash
rustlingpdf run general-rotate-pdf \
  --input report.pdf \
  --output report-rotated.pdf \
  --param angle=90
```

Compose repeatable workflows in `pipeline.json`:

```json
{
  "pipeline": [
    {
      "operation": "general-rotate-pdf",
      "parameters": { "angle": 90 }
    },
    {
      "operation": "misc-compress-pdf",
      "parameters": { "optimizeLevel": 2 }
    }
  ]
}
```

```bash
rustlingpdf pipeline \
  --spec pipeline.json \
  --input report.pdf \
  --output report-ready.pdf
```

CLI behavior is designed for safe scripting:

- explicit output paths are required;
- existing files are preserved unless `--force` is supplied;
- `--output -` is the only binary-stdout mode;
- diagnostics go to stderr; and
- stable exit codes distinguish usage, I/O, processing, dependency, and
  internal failures.

See the [CLI contract](rust/contracts/cli.md) for JSON parameters, repeated
inputs, pipeline semantics, stdout rules, optional dependencies, and exit codes.

## Privacy model

RustlingPDF has one server mode: stateless and account-free.

- No authentication, users, teams, database, audit log, or durable document
  store exists in the application.
- Requests use bounded temporary workspace and result storage that expires.
- In-memory counters reset when the process restarts and are not external
  analytics.
- The optional AI engine is disabled by default. When enabled, dedicated
  document-understanding requests keep PDF bytes in the processing service and
  send only bounded extracted text to the configured provider.
- Because the service has no built-in authentication, expose it only on a
  trusted network or behind your own authenticated reverse proxy.

## Optional native tools

Most processing is implemented in the Rust workspace. Some conversions require
specialized external programs:

- LibreOffice for Office ↔ PDF. Office → PDF also has a built-in pure-Rust
  engine that handles `.docx`, `.xlsx`, and `.pptx` with nothing installed, so
  that direction always works; LibreOffice is used when present because its
  fidelity is better, and it is required for every other input format and for
  PDF → Office;
- WeasyPrint for HTML, Markdown, email, and URL → PDF;
- Tesseract or OCRmyPDF for OCR;
- Calibre for eBook conversion;
- FFmpeg for PDF → video;
- `unrar`/7-Zip and `rar` for CBR workflows; and
- qpdf and Poppler for selected repair/conversion assistance.

Each dependency is probed at startup. A missing tool reports its feature as
unavailable instead of crashing the service or silently producing a different
result. Desktop packages bundle qpdf and Tesseract; the Docker image includes
the common conversion toolchain. See the
[operator guide](rust/RUNNING_WITH_RUST.md#optional-external-tools) for versions
and command overrides.

## Project layout

| Path | Purpose |
| :-- | :-- |
| `rust/crates/rustling-processing` | Axum processing service and in-process pipeline runtime |
| `rust/crates/rustling-ai-engine` | Optional stateless AI document-understanding and orchestration engine |
| `rust/crates/rustling-cli` | Local `rustlingpdf` automation binary |
| `rust/crates/rustling-operation-catalog` | Typed operation-catalog generator |
| `rust/contracts` | Behavior contracts for processing surfaces |
| `frontend/editor` | Vite, React, TypeScript, and Mantine application |
| `SwaggerDoc.json` | OpenAPI snapshot used for catalog and type generation |

Crate names use the `rustling-*` namespace and product environment variables
use the `RUSTLING_*` prefix.

## Documentation

- [Complete feature reference](docs/product/features.md)
- [Installation and operator guide](rust/RUNNING_WITH_RUST.md)
- [CLI contract](rust/contracts/cli.md)
- [Behavior contracts](rust/contracts)
- [Roadmap](ROADMAP.md)
- [Release process](RELEASING.md)
