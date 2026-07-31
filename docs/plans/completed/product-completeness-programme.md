# Execution Plan: Product Completeness Programme

Date: 2026-07-31

## Status

Complete

## Outcome

Turn RustlingPDF's broad processing backend into a polished local-first product
in this order: desktop completeness, visual form creation, accessibility
checking/remediation, stateless AI document understanding, mobile scanning, and
a first-class CLI. A fresh desktop install must provide the core local workflow
without manual runtime setup, and every later capability must preserve the
product's no-account, no-database, ephemeral-processing model.

PDF/A creation or conversion is explicitly excluded from this programme.

## Context

- Product breadth and current privacy contract:
  [`README.md`](../../../README.md) and
  [`docs/product/features.md`](../../product/features.md).
- Current processing/parity ledger:
  [`rust/PORT_STATUS.md`](../../../rust/PORT_STATUS.md).
- Existing pure-Rust runtime programme:
  [`pure-rust-port-programme.md`](../active/pure-rust-port-programme.md). It remains
  authoritative for removing external runtimes; this plan owns the
  user-facing product sequence and consumes its desktop-bundling result.
- Existing desktop-tools implementation branch:
  `port/bundle-desktop-tools`. It adds qpdf and Tesseract but diverged from
  `main` before later crop, Ghostscript-removal, branding, endpoint-gating,
  Windows lifecycle, and v3.1.2 release changes. Its final behavior and
  validation evidence are reusable; its old base is not.
- Current forms surface:
  backend list/fill/modify/delete/export routes in
  `docs/product/features.md`, while
  `frontend/editor/src/core/tools/formFill/FormFill.tsx` exposes Fill only and
  marks Create, Batch, and Modify unready.
- Optional AI engine is already stateless and supports BYOK/OpenAI/Anthropic
  or local Ollama, but has no dedicated summary or translation tools.

## Authority

The user's 2026-07-31 request authorizes the ordered programme proposed in the
preceding comparison, with PDF/A omitted. The following externally observable
policies are inherited from existing repository authority and must not change
without a new decision:

- no login, user accounts, database, durable server-side document storage, or
  audit log;
- document processing remains ephemeral and local unless the user explicitly
  enables the optional AI engine;
- AI document-understanding operations are single-request/stateless and do not
  restore the removed document/RAG store;
- accessibility work may inspect and remediate PDF structure, but does not add
  PDF/A conversion;
- optional third-party runtimes must be availability-gated honestly.

Detailed form-field defaults, accessibility repair choices, translation layout
policy, mobile platform scope, and CLI compatibility contract must be resolved
from current code/standards or recorded here before their respective edits.

## Scope

In scope, in order:

1. Bundle qpdf and Tesseract plus English trained data into supported desktop
   installers; wire explicit sidecar paths, dependency discovery, notices,
   staging, and release validation. Reconcile the existing implementation with
   current `main` instead of merging its old base.
2. Add visual creation and editing for text, multiline, checkbox, radio,
   choice, button, and signature form fields; add alignment/duplication,
   required/tooltips/tab order, automatic field detection where it can be
   proven, and CSV/XLSX batch filling.
3. Add accessibility checking and bounded remediation: tagged structure,
   reading order, document language, alternative text, accessible form labels,
   and actionable reports.
4. Add stateless AI summary, translation, and structured extraction with page
   references through the optional engine and existing provider model.
5. Add a local-first mobile scanner experience with multi-page capture, edge
   detection, perspective correction, cleanup, reordering, OCR/sign handoff,
   and direct export/desktop transfer.
6. Add a typed `rustlingpdf` CLI for local files and pipeline execution.

Out of scope:

- PDF/A creation, conversion, or Ghostscript restoration;
- accounts, authentication, billing, teams, cloud document storage,
  collaborative signing, server-side RAG, or MCP;
- bundling LibreOffice, WeasyPrint, Calibre, FFmpeg, or proprietary RAR tools
  unless separately authorized;
- claiming mobile-native platform support before platform/package authority is
  chosen; a capable installable PWA may satisfy the first mobile delivery;
- release publication or deployment without a separate explicit request.

## Approach

Deliver one independently verifiable slice at a time.

1. Reconcile `port/bundle-desktop-tools` against current `main` at the behavior
   level. Preserve new provenance/license assets and focused proof, while
   retaining all newer mainline behavior. Complete focused staging, Rust,
   frontend-license, and desktop-shell validation.
2. Inspect the PDF form object model, viewer coordinate conventions, existing
   mutation routes, and competitor-neutral PDF semantics. Record any remaining
   field-appearance or auto-detection choices before editing. Land backend
   create/batch contracts first, then the visual UI and end-to-end proof.
3. Define an accessibility report schema grounded in PDF/UA structure rules
   without coupling it to PDF/A. Implement checker before repair operations so
   every repair can be proven by before/after findings.
4. Extend the stateless AI capability manifest and processing proxy with
   bounded request/response contracts. Implement summary and extraction before
   layout-preserving translation.
5. Reuse the existing mobile-scanner session and scan-cleanup primitives.
   Decide PWA versus native packaging only after browser capability and offline
   proof are measured.
6. Generate CLI operation bindings from the existing operation catalog so the
   HTTP, AI, and CLI schemas do not drift.

After each slice, update this plan, the product feature reference, affected
contracts, and executable proof before beginning the next slice.

## Risks And Recovery

- The desktop branch is 69 mainline commits behind and cannot be merged
  safely as a unit. Recovery: keep `main` as the base, replay bounded final
  changes, and compare each affected file against both tips.
- Bundled native tools have supply-chain and license obligations. Recovery:
  retain checksum pins, generated dependency inventory, complete notices, and
  fail staging on a missing/mismatched artifact.
- Form creation can produce malformed AcroForm trees or invisible widgets.
  Recovery: reopen every generated file with independent parsers/renderers and
  validate field-tree/page-annotation round trips before exposing the mode.
- Accessibility "repair" can damage reading order. Recovery: checker-first,
  explicit user preview, bounded reversible edits, and preserve unknown
  structure.
- Translation can alter layout or leak content. Recovery: optional AI only,
  page/character limits, no persistence, explicit provider disclosure, and
  keep the source file unchanged.
- Mobile camera APIs vary by platform. Recovery: capability detection and
  graceful file-upload fallback; do not advertise unsupported offline paths.
- CLI schema drift can make automation unsafe. Recovery: generate bindings
  from the operation catalog and pin golden invocation/exit-code tests.

## Progress

- [x] Product sequence authorized; PDF/A explicitly excluded.
- [x] Existing repository/product authority and overlapping plans identified.
- [x] Desktop qpdf/Tesseract branch reconciled with current `main`.
- [x] Linux desktop installer, staging, license inventory, and real qpdf repair
      validation green.
- [ ] Native Windows/macOS release-runner validation green.
- [x] Visual form creation and batch fill complete.
- [x] Accessibility checker and remediation complete.
- [x] Stateless AI summary/translation/extraction complete.
- [x] Local-first mobile scanner complete.
- [x] First-class CLI complete.
- [x] Programme-wide documentation and validation complete.

## Decisions

- 2026-07-31: PDF/A is not required and is excluded from this programme.
- 2026-07-31: Keep the local/stateless product identity; competitor cloud
  account, collaboration, and storage features are not part of this sequence.
- 2026-07-31: Desktop completeness means qpdf + Tesseract only. The existing
  pure-Rust plan records prior authority to skip bundled LibreOffice,
  WeasyPrint, Calibre, FFmpeg, Ghostscript, and RAR tooling.
- 2026-07-31: Reconcile the desktop-tools branch by final behavior, not by
  merging its obsolete base.
- 2026-07-31: The current-main reconciliation preserves v3.1.2, Windows MSI
  lifecycle/provisioner checks, Ghostscript removal, crop fixes, and endpoint
  gating. Only the final qpdf/Tesseract installer, staging, launcher,
  provenance, notice, and release behavior was replayed.
- 2026-07-31: Linux local proof passed: checksum-pinned qpdf 12.3.2,
  repository-built Tesseract 5.5.3, English tessdata 4.1.0, complete Tauri
  resource staging, and a real damaged-PDF repair accepted by bundled qpdf.
  Windows/macOS installers retain their independently validated pinned
  artifacts, but the reconciled current-main release matrix still requires
  hosted native runners.
- 2026-07-31: Form creation uses a new multipart `create-fields` contract:
  one source PDF plus an ordered JSON array of logical fields, each with one
  or more widgets in the existing zero-based, upper-left CropBox coordinate
  system. Supported types are text (including multiline), checkbox, radio,
  combobox, listbox, button, and signature. Fields default to editable and
  optional; names are collision-suffixed with the existing `_1`, `_2`
  convention; invalid page/geometry/type/default combinations reject the
  whole request without writing a partial result.
- 2026-07-31: New form fields receive static appearances and accessible
  alternate names. Explicit tab order is represented by page annotation
  order with `/Tabs /A`; existing annotations retain their relative order and
  precede newly created widgets.
- 2026-07-31: Batch fill accepts a source PDF plus CSV or XLSX. The first row
  is the field-name header, each nonblank remaining row produces one filled
  PDF, and `_filename` is an optional reserved output-name column. The result
  is a ZIP; source files remain unchanged. The repository's existing upload
  limits apply, with no new account- or tenant-based quota.
- 2026-07-31: The form editor now exposes Fill, Create, Batch, and Modify.
  Create supports direct page drawing, drag/resize, multi-select alignment,
  duplication, multi-widget radio groups, accessible labels/tooltips, field
  flags, defaults, and tab order. Batch downloads a ZIP from CSV/XLSX input;
  Modify and Delete use the existing mutation contracts. Automatic field
  detection remains outside this slice because there is no repository
  authority for an inference confidence/preview policy.
- 2026-07-31: Accessibility uses a native checker with rule-scoped results,
  not a conformance claim. Bounded repairs may set catalog language, set
  `/MarkInfo /Marked` when a structure tree already exists, select structure
  tab order, and apply user-authored Figure alternative text or form
  tooltips. The tool will not synthesize tags, infer/reorder semantic reading
  order, invent descriptions, add PDF/UA identification metadata, or touch
  PDF/A. Every repair is validated atomically and followed by a checker
  rerun.
- 2026-07-31: Dedicated AI summary, structured extraction, and translation
  operations remain single-request and stateless. The processing service keeps
  PDF bytes local and forwards only configured-page/character-bounded text to
  the optional provider. Summary claims and extraction values carry validated
  one-based source-page references.
- 2026-07-31: Translation preserves extracted page boundaries and stable block
  order in a structured JSON result. It does not rewrite the PDF or claim
  pixel-perfect/font-preserving layout: that would require unowned overflow,
  font-substitution, geometry, and human-review policy. Unknown model block IDs
  are discarded and omitted source blocks remain visibly untranslated. The
  source PDF is unchanged.
- 2026-07-31: Large translation requests use bounded, concurrent provider
  completions under the existing chunk-size/concurrency settings. Each
  completion may populate only the stable block IDs it received; Rust
  reassembles all responses in original page/block order and exposes any
  omitted block instead of silently dropping content.
- 2026-07-31: The first mobile delivery is an installable PWA, not an
  unsubstantiated native Android/iOS claim. `/mobile-scanner` may run without
  a QR session; captured pages stay in browser memory, and after one successful
  load the service worker caches only same-origin scanner code/static assets,
  never API responses or documents.
- 2026-07-31: Mobile export creates one ordered PDF in-browser. The existing
  ten-minute temporary QR session remains an explicit optional transfer path
  and now receives that PDF rather than unordered individual images. Local OCR
  and Sign handoff downloads the PDF before opening the selected tool, because
  browser sandboxing provides no authority to claim invisible file injection.
- 2026-07-31: Browser memory is bounded by resampling only photos above 3,000
  pixels on their longest edge; automatic edge detection uses a separate
  240-pixel working copy, while correction and export use the bounded
  full-resolution page.
- 2026-07-31: The first-class CLI is a separate `rustlingpdf` binary that runs
  the existing HTTP pipeline router in-process. It does not start a listener,
  require an account, upload to another service, or create durable server
  state. `operations` and `describe` expose bindings generated at build time
  from the committed operation catalog; `run` creates a one-step pipeline and
  `pipeline` accepts the existing pipeline JSON shape.
- 2026-07-31: CLI operation identifiers are the unambiguous catalog path below
  `/api/v1/` joined with hyphens (for example `general-rotate-pdf`), while the
  canonical HTTP path remains accepted. Parameters use repeatable
  `--param key=value` values or a JSON object and are validated against the
  same catalog schema before execution.
- 2026-07-31: CLI document bytes go to an explicit output file and diagnostics
  go to stderr. `--output -` is the only binary-stdout mode; `operations
  --json` and `describe --json` are the only metadata-stdout modes. Existing
  outputs are not replaced without `--force`, and file outputs are staged
  before an atomic persist.
- 2026-07-31: CLI exit codes are stable automation policy: `0` success, `2`
  usage/schema/pipeline-definition errors, `3` local input/output I/O errors,
  `4` processing rejected the request, `5` an optional runtime dependency is
  unavailable, and `6` an internal runtime/catalog failure occurred.

## Validation

- Desktop focused proof: installer scripts' checksum/provenance tests, staged
  qpdf/Tesseract smoke operations, sidecar environment resolution, generated
  license inventory, Tauri unit tests, and release dry-run assertions.
- Forms focused proof: PDF object-tree unit tests, HTTP create/modify/fill
  round trips, independent reopen/render checks, frontend interaction tests,
  and viewer end-to-end scenarios.
- Accessibility focused proof: rule fixtures, before/after checker reports,
  screen-reader-relevant structure inspection, and preservation corpus.
- AI focused proof: provider-independent structured-output tests, bounded
  multipart/proxy integration tests, page-reference tests, and no-persistence
  assertions.
- Mobile focused proof: camera/file fallback tests, offline behavior, scan
  geometry fixtures, multi-page export, and desktop-session transfer.
- CLI focused proof: golden argument parsing, operation-catalog parity, exit
  codes, binary/stdout discipline, and end-to-end file processing.
- Repository-required checks after affected slices:
  `cargo fmt --check`, strict workspace Clippy, locked workspace tests with
  PDFium, frontend typecheck/lint/vitest/build, desktop fmt/clippy/tests, and
  platform release dry-runs where available.

Desktop reconciliation evidence on 2026-07-31:

- `bash rust/scripts/install-desktop-tools.sh`: passed checksums, native
  version smoke checks, and installed the complete Linux tree.
- `bash frontend/editor/src-tauri/scripts/stage-sidecar.sh`: passed isolated
  qpdf/Tesseract execution and English language discovery from staged paths.
- `cargo test ... --lib
  pdf_repair::tests::repairs_a_damaged_document_with_a_real_qpdf`: passed
  against the bundled qpdf command.
- `cargo fmt --manifest-path frontend/editor/src-tauri/Cargo.toml -- --check`,
  Bash syntax checks, generated-license assertions (113 entries), and
  `git diff --check`: passed.
- Native Tauri unit execution remains a hosted-runner gate on this machine:
  Linux lacks GTK/WebKit development packages and an MSVC cross-check lacks
  `lib.exe`. No product failure was observed.

Forms evidence on 2026-07-31:

- `cargo fmt --manifest-path rust/Cargo.toml --all -- --check`: passed.
- Locked `pdf_form_creation::tests` and `pdf_form_batch::tests`: 6 passed,
  covering all seven field types, atomic validation, CropBox coordinates,
  appearances, flags, CSV/XLSX parsing, output naming, and filled-value
  round trips.
- Locked `form_fields_endpoints` integration suite: 19 passed, including
  create and batch multipart routes plus existing fill/modify/delete/export
  behavior.
- Form designer context and pointer-interaction tests: 3 passed, covering
  draw/move/resize, alignment/duplication, API payloads, and multi-widget
  radio groups.
- Core frontend typecheck, focused ESLint/Prettier, and production build:
  passed. Vite emitted a non-fatal host warning because local Node 20.14 is
  below its recommended 20.19 version.

Accessibility evidence on 2026-07-31:

- Strict `rustling-processing` library/test Clippy with `-D warnings` and Rust
  formatting: passed.
- Native checker/remediation unit tests: 4 passed, covering role mapping,
  ordered structure preview, language, tagged state, annotation tab order,
  Figure alternatives, form labels, atomic targets, and refusal to mark an
  untagged document.
- HTTP integration tests: 3 passed, including before/after checker proof and
  invalid multipart/JSON/target responses.
- Runtime endpoint-group suite: 40 passed, proving both new routes remain
  availability-gated and reachable.
- Accessibility repair-panel interaction test, core TypeScript typecheck,
  focused ESLint/Prettier, and production build: passed. The local Playwright
  page-load smoke could not start because its Chromium binary is not installed;
  this is an environment gate, not an observed product failure.

AI document-understanding evidence on 2026-07-31:

- Strict `rustling-ai-engine` Clippy with `-D warnings` and all 112 engine
  library tests: passed, including typed route output, grounded references,
  caller-ordered extraction, large translation batching, stable reassembly,
  and visible omissions.
- Strict `rustling-processing` Clippy and the real-PDF multipart/proxy suite:
  passed. The proxy extracted only the configured first page, never forwarded
  PDF bytes or a temporary path, exercised all three operations, and left no
  content artifact in the configuration directory.
- Runtime route/registry suite: 40 passed. Core TypeScript, focused
  ESLint/Prettier, two panel interaction tests, OG metadata check, and
  production build passed.

Mobile scanner evidence on 2026-07-31:

- Core TypeScript and focused ESLint/Prettier passed. Fifteen focused tests
  passed for no-session local launch, expired-session fallback, QR URL
  construction, page ordering, perspective bounds, image-size bounds, and a
  generated two-page PDF reopened through an independent parse.
- Production core build passed and emitted a lazy scanner route chunk,
  dedicated install manifest/service worker, OpenCV, and jscanify. The local
  Node 20.14 host remains below Vite's recommended 20.19 but did not fail the
  build.
- Real system-Chrome proof loaded the production scanner, waited for the
  worker's cache-complete acknowledgement, observed 16 cached same-origin
  resources and no cached `/api/` URL, forced the browser offline, reloaded the
  scanner, selected two images, downloaded one PDF, and reopened it with
  exactly two ordered pages.
- Locked Rust transfer and SPA integration suites passed: 2 mobile-session
  tests and 17 static/deep-route tests, including ephemeral download removal,
  feature disablement, and `/mobile-scanner` SPA serving.

CLI evidence on 2026-07-31:

- The `rustlingpdf` binary generated 67 build-time bindings from the committed
  operation catalog. Golden parsing, unique-ID/path resolution, exact
  catalog/schema parity, required/enum schema validation, typed/repeated
  parameters, and the fixed exit-code map passed in 8 unit tests.
- Four executable integration tests passed with real child processes:
  one-operation and two-step pipeline rotation round trips, PDF output reopen,
  binary/file stdout discipline, machine-readable operation discovery, and
  exit `2`/`3`/`4`/`5` distinctions including a deliberately unavailable
  LibreOffice runtime.
- Strict all-target `rustling-cli` Clippy and `rustling-processing` library
  Clippy passed with `-D warnings`; Rust formatting, the runtime availability
  accessor test, Task command dry-run, and `git diff --check` passed.

Programme-wide evidence on 2026-07-31:

- `cargo fmt --all --check`, strict workspace/all-target Clippy with
  `-D warnings`, and the complete locked Rust workspace test suite passed with
  PDFium bound. This includes every AI-engine, operation-catalog, processing,
  CLI, integration, and documentation test; the only ignored case is the
  intentionally hosted desktop-helper test.
- The open-source core frontend passed TypeScript, full ESLint, Prettier,
  dependency-cycle, theme, and production-build checks. Vitest passed all 881
  tests across 85 files, including the new forms, accessibility, document
  understanding, scanner, translation, and generated-contract coverage.
- The operation catalog, frontend API types, OG metadata, translation key
  inventory, and 67 generated CLI bindings are current. Desktop shell scripts
  pass Bash syntax checks, and the final worktree passes `git diff --check`.
- The local Node 20.14 runtime remains below Vite's recommended 20.19 version;
  it emitted a warning but the production build completed successfully.
  Native Windows/macOS installer execution remains assigned to hosted release
  runners. Neither limitation represents an observed product failure.
- PDF/A remains excluded. No account, cloud-storage, product-license key, or
  license-generation gate was introduced; generated license assets are only
  open-source third-party attribution and redistribution notices. No release
  was published and no deployment was performed.

## Result

All six authorized slices are implemented and validated: bundled desktop
qpdf/Tesseract support, visual form creation and batch fill, bounded
accessibility checking/remediation, stateless AI document understanding, the
local-first mobile scanner PWA, and the typed `rustlingpdf` CLI. The product
continues to require no account or product license and stores no durable
server-side document state. Native installer proof remains a hosted-runner
release gate; release publication was outside this programme.
