# RustlingPDF

A locally hosted PDF toolbox with a **pure-Rust backend**: one `axum` service
serving 160+ PDF-processing REST endpoints (merge, split, convert, OCR, forms,
redaction, signing, pipelines, async jobs, …), an optional Rust AI engine, and a
React single-page UI. The server keeps **no accounts and no server-side state**:
every request is self-contained, results are ephemeral (TTL-swept scratch
space), and all user preferences live client-side.

RustlingPDF is an independent tool **based on [Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF)**.
It began as a full Java→Rust port of Stirling-PDF's Spring Boot backend, verified
endpoint-by-endpoint against the original as a compatibility oracle (route census:
zero unexplained gaps; during the port a live differential harness semantically
diffed both backends' outputs). This repository carries that Rust backend forward as its own
product — there is **no Java in this repo** and no dependency on a JVM at build
or run time. See [LICENSE](LICENSE) for upstream attribution.

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
Tesseract/OCRmyPDF, WeasyPrint ≥ 58, Poppler `pdftohtml`, Calibre, unrar.
Full operator guide, ports/binding, configuration and environment reference:
[`rust/RUNNING_WITH_RUST.md`](rust/RUNNING_WITH_RUST.md).

## Status

- **The product has no authentication and no server-side state — by design.**
  The former opt-in secured mode (login/users/teams/OIDC/MFA/audit/durable
  storage/policies/MCP) was removed entirely by maintainer decision on
  2026-07-28; legacy `security.*`/`mcp.*`/`storage.*`/`policies.*` settings
  keys are ignored with a one-line startup warning, never refused, so
  existing configs and desktop installs keep booting. PDF *document* security
  (password, redaction, sanitize, watermark, cert-sign + hardware signing,
  timestamping, signature validation) is unaffected.
- Test suite: 931 backend tests (809 processing + 115 AI engine + 7 catalog),
  0 failed (plus 1051 frontend vitest and a 10-test desktop-shell gate).
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
