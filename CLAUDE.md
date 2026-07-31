# Repository Guide

RustlingPDF is an MIT-licensed PDF workbench with a Rust backend, React
frontend, Tauri desktop shell, Docker packaging, local CLI, and an optional
Rust AI sidecar. This repository is the authority for product behavior; do not
add dependencies on another product or repository to build, test, or run it.

## Commands

[Task](https://taskfile.dev) is the shared command runner:

- `task install` installs Rust, frontend, and engine dependencies.
- `task dev` starts the processing service and core web frontend.
- `task dev:all` also starts the optional AI engine.
- `task rust:check` runs Rust formatting, Clippy, and tests.
- `task frontend:check` runs frontend formatting, lint, types, tests, and build.
- `task engine:check` validates the AI engine.
- `task desktop:test` validates the Tauri application.
- `task check:all` runs the repository-wide quality gates.

Before a Rust compile or test, check `rust/target/debug/deps`. If it exceeds
50 GB, use `task rust:clean:deps`; do not delete a broader target directory as
an automatic cleanup.

## Architecture

- `rust/crates/rustling-processing` is the Axum HTTP service and in-process
  processing runtime. It serves `/api/v1/...`, reads configuration below
  `RUSTLING_BASE_PATH`, and uses the `RUSTLING_*` environment namespace.
- `rust/crates/rustling-cli` exposes the same catalog-backed processing runtime
  without starting an HTTP listener.
- `rust/crates/rustling-ai-engine` is an optional stateless AI service. The
  processing service connects through `AIENGINE_URL` only when explicitly
  enabled.
- `rust/crates/rustling-operation-catalog` generates typed operation metadata
  from the committed `SwaggerDoc.json`.
- `frontend/editor/src/core` is the only frontend product layer. Web, Docker,
  and desktop builds all use Vite's `core` mode.
- `frontend/editor/src-tauri` contains the native desktop shell, sidecar
  lifecycle, file integration, and installers.
- `rust/contracts` contains behavior contracts for public processing surfaces.

The service has no application authentication, account database, billing
system, commercial license key, or durable server-side document store. PDF
security operations such as encryption, redaction, signing, timestamping, and
signature validation remain processing features.

## Backend conventions

- Keep `Cargo.lock` committed and use `--locked` in gates.
- Install pinned PDFium with `task rust:install`; set
  `RUSTLING_PDFIUM_LIBRARY_PATH` when running Cargo commands directly.
- Optional native programs are discovered at startup. Missing dependencies
  make only their affected endpoints unavailable.
- Update the relevant file under `rust/contracts/` whenever externally
  observable behavior changes.
- Keep the OpenAPI snapshot, operation catalog, generated frontend API types,
  and their generators synchronized.
- Preserve input bounds, temporary-file cleanup, SSRF defenses, and explicit
  dependency failures.

## Frontend conventions

- Import application modules through `@app/*`.
- Use the core source tree; do not recreate commercial, proprietary, prototype,
  cloud, or SaaS overlay layers.
- Route file operations through `FileContext`, and preserve PDF.js/blob cleanup.
- New tools should use the shared operation-hook and catalog patterns.
- Follow `frontend/editor/src/core/theme/README.md` for color and token changes.
- Keep translatable UI copy in the locale system and generated metadata in sync.

## Product constraints

- The product is open source under the root `LICENSE`.
- Third-party license generation is dependency attribution, not a product
  license-key mechanism, and must remain accurate.
- Do not reintroduce accounts, billing, usage credits, commercial plan gates,
  or license activation.
- Do not add PDF/A roadmap work unless the maintainer requests it.
- Runtime identifiers use RustlingPDF naming only; do not add compatibility
  aliases for removed product identities.

## Communication

Users may prompt in English, but agents reply in Vietnamese unless the user
explicitly requests another language. Code, comments, commands, logs, and
technical files remain in English.
