# AGENTS.md

This file provides guidance to AI Agents when working with code in this repository.

## What this repository is

**RustlingPDF** is a standalone PDF-toolbox product with a pure-Rust backend and a
React SPA. It is based on Stirling-PDF (the Java project it was ported from), but it
is a **separate repository and a separate tool**: there is no Java code here, no
Gradle, no JVM dependency, and it must never be wired back into Stirling-PDF as a
submodule. All development effort in this repository focuses on RustlingPDF itself.

The original Stirling-PDF checkout (if present at `../Stirling-PDF`) is a **read-only
reference oracle**: consult its Java sources when a behavior question arises. Never
edit that repo from here, and never add build-time dependencies on it. (The
differential harness that drove a live upstream instance was removed by maintainer
decision on 2026-07-28; a replacement harness is planned.)

The coordinated `Stirling` → `Rustling` rename has been executed: crates are
`rustling-*` and `RUSTLING_*` is the primary env-var spelling (every legacy
`STIRLING_*` spelling keeps working as a deprecated alias; `RUSTLING_*` wins
when both are set — see `env_compat` in each crate). Identifiers deliberately
kept under the old spelling for continuity with shipped releases and existing
installs — the tauri bundle identifier `stirling.pdf.dev`, the deep-link
scheme, the `Stirling-PDF` desktop app-data directory, persisted storage keys,
the `StirlingPDFClassification` PDF Info key, `X-Stirling-*` wire headers, and
the `stirling_*` MCP tool ids — must not be renamed piecemeal.

## Taskfile

[Task](https://taskfile.dev) is the unified command runner. `task --list` shows all
commands. Key ones:

- `task rust:install` — Cargo deps + pinned PDFium (required before backend tests)
- `task backend:dev` — the Rust backend on 127.0.0.1:8080 (`PORT=<n>` to override)
- `task frontend:dev` — Vite dev server on 5173, proxies `/api` (override target with `BACKEND_URL`)
- `task dev` / `task dev:all` — backend + frontend (+ AI engine)
- `task engine:dev|check|test|fix` — the Rust AI engine (localhost:5001)
- `task rust:check` — the full backend quality gate (fmt + clippy + tests with PDFium)
- `task rust:clean:deps` — remove exactly `rust/target/debug/deps`

After modifying files, run the matching gate: `task frontend:check` for frontend,
`task engine:check` for the AI engine, `task rust:check` (or targeted `cargo test`
filters plus fmt/clippy) for the processing backend.

## Rust Build Storage Guard

- Before every Rust compile or test command, check the size of `rust/target/debug/deps`
  in the active worktree.
- If it exceeds 50 GB, run `task rust:clean:deps` to remove exactly that directory first.
- Never remove a broader `target` directory as part of this automatic guard.

## Backend architecture (rust/)

- `rust/crates/rustling-processing` — the axum HTTP service. Routes mirror the
  `/api/v1/...` REST surface the SPA calls. Configuration comes from
  `configs/settings.yml` under `RUSTLING_BASE_PATH` plus `SYSTEM_*`/`SECURITY_*`/
  `RUSTLING_*` env overrides (legacy `STIRLING_*` spellings are accepted as
  deprecated aliases). PDFium is the native processing engine
  (`RUSTLING_PDFIUM_LIBRARY_PATH`, or `task rust:install`); pure-Rust fallbacks
  exist where implemented. External tools (LibreOffice, Ghostscript, qpdf,
  Tesseract/OCRmyPDF, WeasyPrint, pdftohtml, Calibre, unrar) are discovered at
  startup; missing ones disable their endpoints with reason `DEPENDENCY` — never
  hard-fail on a missing optional tool.
- **Secured mode is fail-closed by design**: the binary refuses to start when
  `SECURITY_ENABLELOGIN=true`/`DOCKER_ENABLE_SECURITY=true` until the independent
  security review gate is lifted. Do not weaken this.
- `rust/contracts/*.md` are the per-surface behavior contracts. When changing an
  endpoint's behavior, update its contract in the same change; when adding a
  surface, add one. `rust/PORT_STATUS.md` is the authoritative ledger of feature
  status and documented divergences — keep it truthful (claims there are audited).
- `rust/crates/rustling-ai-engine` — typed contracts in, typed contracts out, AI
  only where it adds reasoning value. The frontend reaches it through the
  processing service as a proxy (`AIENGINE_URL`, `AIENGINE_ENABLED`).
- `rust/crates/rustling-operation-catalog` — regenerates the typed operation
  catalog from the frozen `SwaggerDoc.json` snapshot at the repo root. Generated
  catalog files are committed; keep them in sync via the taskfile target when the
  snapshot changes.
- New Rust dependencies go in the owning crate's `Cargo.toml`; keep `Cargo.lock`
  committed and use `--locked` in gates.

## Team workflow for substantial backend work

Any substantial backend feature or subsystem change (multi-step; a new endpoint
family, a new engine capability) is executed as **dev + tester agent pairs**, not
solo:

- The orchestrating agent acts as project manager: decompose into work-items,
  spawn a dev and an **independent** tester per item (never the same agent grading
  its own work), integrate the results, keep `rust/PORT_STATUS.md` and the
  relevant contract docs updated, and report outcomes.
- Parallel work-items run in separate git worktrees (dev and its tester share the
  same worktree path); coupled items sharing hot files run sequentially on one tree.
- Definition of done: the relevant gate is clean; the tester has adversarially
  attacked the change (edge cases, malformed input, security/SSRF, resource
  bounds) and signed off; contracts/ledger updated. Reference-oracle comparison
  (against the Stirling-PDF sources) applies whenever the surface has an
  upstream counterpart.
- Trivial one-line edits, config tweaks, doc fixes, and pure questions are handled
  directly without a team.

## Frontend (frontend/editor)

- Tech stack: Vite + React + TypeScript + Mantine + TailwindCSS.
- **ALWAYS import via `@app/*`** — never `@core/*`/`@proprietary/*` except when a
  higher layer deliberately wraps a lower-layer module it shadows. The alias
  resolves per build flavor (core → proprietary → cloud → saas/desktop cascade);
  read `frontend/editor/DeveloperGuide.md` before touching the layering.
- Extension modules are named for **what they do**, never for which build overrides
  them; core code never checks `isDesktop()`/`isTauri()`.
- All `VITE_*` vars belong in the committed `.env`/`.env.<flavor>` files; local
  secrets go in uncommitted `.local` siblings. Never inline `|| 'fallback'`.
- Colours/theming: read `frontend/editor/src/core/theme/README.md` first; literal
  colours live only in `primitives.css`, components use `--c-*` tokens.
- All file operations go through `FileContext`; manual cleanup of PDF.js documents
  and blob URLs is load-bearing (100GB+ target) — never remove cleanup code.
- New tools follow the `useToolOperation` hook pattern (see `ADDING_TOOLS`-style
  hooks under `core/hooks/tools/`).
- Translations: update `en-US` JSON under `frontend/editor/public/locales/` only;
  other languages are handled separately.

## Testing

- Backend: unit + integration tests in the crates (run with PDFium bound). The
  former differential harness (`testing/differential`) was removed by maintainer
  decision on 2026-07-28; the maintainer plans to build a new harness.
- Frontend: `task frontend:check`; stubbed Playwright specs under
  `frontend/editor/src/core/tests/` for backend-free UI verification.

## Communication Style

- The user may write prompts in English, but agents must reply to the user in
  Vietnamese unless the user explicitly requests another response language.
- Only user-facing natural-language responses are translated; source code, code
  comments, identifiers, file contents, patches, commands, logs, and other
  technical artifacts must remain in English.
- Be direct and to the point; no apologies or conversational filler; answer
  questions directly without preamble.

## Decision Making

- Ask clarifying questions before making assumptions on product direction.
- Confirm approach before structural changes (crate splits, cross-cutting renames,
  security-surface behavior changes).
- Agents may freely delegate independent work to subagents and use all available
  concurrency; respect the dev/tester pair workflow for substantial backend work.

## Stack reality check

The frontend tracks current Vite/React/TS releases and the backend tracks a recent
stable Rust toolchain with workspace lints (`cargo clippy --workspace --all-targets
--locked -- -D warnings` must stay clean). Ground code in what is actually in
`Cargo.toml`/`package.json` and the existing source — do not assume APIs from
training data; open the dependency source when unsure.
