# RustlingPDF

A locally hosted PDF toolbox with a **pure-Rust backend**: one `axum` service
serving 340+ PDF-processing REST endpoints (merge, split, convert, OCR, forms,
redaction, signing, pipelines, async jobs, …), an optional Rust AI engine, and a
React single-page UI.

RustlingPDF is an independent tool **based on [Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF)**.
It began as a full Java→Rust port of Stirling-PDF's Spring Boot backend, verified
endpoint-by-endpoint against the original as a compatibility oracle (route census:
zero unexplained gaps; a live differential harness semantically diffs both
backends' outputs). This repository carries that Rust backend forward as its own
product — there is **no Java in this repo** and no dependency on a JVM at build
or run time. See [LICENSE](LICENSE) for upstream attribution.

## Layout

| Path | What it is |
|---|---|
| `rust/crates/stirling-processing` | The backend: axum HTTP service mirroring the `/api/v1/...` REST surface |
| `rust/crates/stirling-ai-engine` | Optional AI engine (classification, PDF Q&A, document creation, orchestration, MCP) |
| `rust/crates/stirling-operation-catalog` | Generates the typed operation catalog from the OpenAPI snapshot |
| `rust/contracts/` | Per-surface behavior contracts (routes, semantics, documented divergences) |
| `frontend/editor` | Vite + React + TypeScript + Mantine SPA |
| `testing/differential` | Harness that drives the backend (and optionally an upstream Stirling-PDF instance) and semantically diffs responses |
| `SwaggerDoc.json` | Frozen OpenAPI snapshot used for catalog regeneration |

Internal names (crate names, `STIRLING_*` environment variables, `stirling.*`
config keys) intentionally retain their upstream spellings for now so existing
deployments, contracts, and tests keep working; a coordinated rename is a
tracked roadmap item.

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

- **Open mode (no login) is the supported way to run today.** The secured mode
  (login/users/teams/OIDC/MFA/audit/storage/signing) is fully implemented and
  extensively tested behind an opt-in review gate, but the binary deliberately
  refuses to start with `SECURITY_ENABLELOGIN=true` until an independent human
  security review signs off (fail-closed, including malformed values).
- Test suite: 1535 + 144 backend tests, 0 failed (plus 1647 frontend vitest
  and an 11-test desktop-shell gate), plus a differential harness with a
  pinned known-difference registry.
- The authoritative feature/parity ledger is
  [`rust/PORT_STATUS.md`](rust/PORT_STATUS.md); per-surface details live in
  [`rust/contracts/`](rust/contracts/).

## Roadmap

The detailed, living plan — current batch, queue, deferred items with unblock
conditions, and session hand-off instructions — is in [ROADMAP.md](ROADMAP.md).
Headlines: GitHub CI, single-binary SPA serving, Docker packaging, the
tag-driven GHCR release pipeline, and the Tauri Rust-sidecar desktop port have
landed; next up are desktop release completion (updater signing + Windows
staging), the coordinated `Stirling` → `Rustling` rename, and the independent
security review that unlocks secured mode.

## Relationship to Stirling-PDF

RustlingPDF is a separate, standalone repository — not a fork remote, not a
submodule. Upstream Stirling-PDF remains the reference implementation its
behavior contracts were verified against; `testing/differential` can still drive
any running Stirling-PDF instance side-by-side for regression comparison.
