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

RustlingPDF is a self-contained PDF toolbox with a **pure-Rust backend**, a React
web UI, and a Tauri desktop app. Run it on your desktop, or behind your own proxy
on a trusted network.

It is an independent product **based on
[Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF)** — originally a
full Java→Rust port of its backend, now carried forward on its own. There is no
Java, and no JVM at build or run time. See [LICENSE](LICENSE) for attribution.

## Features

| Category | Capabilities |
| :-- | :-- |
| **Organise pages** | Merge; split by breaks, size, page count, chapters, tiles, or QR dividers; reorder, reverse, duplicate, odd/even split-and-merge; rotate, crop, rescale; N-up, booklet, and poster layouts; overlay; remove blank pages; page numbers |
| **Convert to PDF** | Images, Office documents, HTML, Markdown, web pages, email, eBooks, SVG, and comic archives |
| **Convert from PDF** | Images, Word, PowerPoint, XML, HTML, text, Markdown, tables to CSV/XLSX, EPUB, comic archives, slideshow video, and image extraction |
| **Security & signing** | Passwords and permissions, watermarks, sanitisation, redaction; X.509 and hardware / smart-card signing; RFC 3161 timestamps; signature validation |
| **OCR & repair** | Searchable-text OCR, repair of damaged files, compression |
| **Edit** | Find-and-replace text, a full visual editor, stamps, images, comments, dark-mode recolouring, and flattening |
| **Forms** | Visually create, align, duplicate, fill, rename, and delete accessible fields; batch-fill CSV/XLSX rows; export values; unlock read-only fields |
| **Metadata & attachments** | Embedded files, document properties, and bookmark / table-of-contents editing |
| **Inspect** | Page, font, annotation, and form inventories; encryption status; PDF/A, PDF/UA, and WTPDF conformance checks |
| **Automation** | A catalog-validated local `rustlingpdf` CLI, multi-tool pipelines, asynchronous jobs, an installable local-first mobile scanner with optional phone-to-desktop transfer, and content filters |
| **AI-assisted** | Page-cited summary, structured extraction, page/block-ordered translation, edit planning, review comments, document generation, maths auditing, and classification — optional, off by default |

**Privacy by design.** No login, no database, no server-side storage — and unless
you deliberately enable the optional AI engine with your own API key, nothing
ever leaves the machine.

**Optional tools.** A handful of conversions call an external program
(LibreOffice, WeasyPrint, Calibre, Tesseract or OCRmyPDF, FFmpeg, `unrar`); the
Docker image bundles the common ones. A missing tool disables only its own
feature and never crashes the service — everything else is pure Rust.

The complete per-endpoint reference is in
[`docs/product/features.md`](docs/product/features.md); feature status and
documented differences from upstream are in
[`rust/PORT_STATUS.md`](rust/PORT_STATUS.md).

## Layout

| Path | What it is |
|---|---|
| `rust/crates/rustling-processing` | The backend: axum HTTP service mirroring the `/api/v1/...` REST surface |
| `rust/crates/rustling-ai-engine` | Optional stateless AI engine (summary, extraction, translation, classification, PDF edit/review/create agents, math audit, orchestration) |
| `rust/crates/rustling-cli` | Local `rustlingpdf` automation CLI generated from the operation catalog |
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

Local automation needs no running server:

```bash
cargo run --manifest-path rust/Cargo.toml --locked -p rustling-cli -- \
  run general-rotate-pdf -i report.pdf -o report-rotated.pdf -p angle=90
```

See the [CLI contract](rust/contracts/cli.md) for operation discovery,
pipelines, JSON parameters, binary stdout, overwrite behavior, and stable exit
codes.

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
- The locked Rust workspace test suite passes with PDFium bound, alongside the
  frontend Vitest suite and desktop-shell gate. All three CI workflows above
  run on every push.
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
