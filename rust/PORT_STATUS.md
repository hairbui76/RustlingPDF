# Rust Port Status

> **Repository note (RustlingPDF).** This repository is **RustlingPDF** — the
> standalone Rust product. It contains no Java, Gradle, or Python engine code.
> This ledger is kept as the historical *and* living record of the Java → Rust
> port that was executed inside the upstream Stirling-PDF monorepo, so its body
> still speaks in that porting context: references to Java sources (`app/core`,
> `app/proprietary`, `app/saas`, `*.java` classes), the Python oracle
> (`engine/`), Java/legacy Task entry points, and upstream CI workflows (e.g.
> `differential-parity`, `rust-processing`) all point at the **upstream
> Stirling-PDF repository** or an externally running instance of it — none of
> them exist in this repo. The Rust facts (crates, routes, tests, contracts)
> describe this repository.

Tracks the Java → Rust port of the Stirling-PDF backend (UI excluded). The Rust
service lives in this `rust/` workspace as the `rustling-processing` crate — an
axum HTTP service mirroring the upstream Java `/api/v1/...` endpoints.

> **Batch 7 (2026-07-29) — no auth, no server-side state.** By maintainer
> decision (final, 2026-07-28) the product has **no authentication and no
> server-side state**: the entire auth subsystem
> (login/sessions/MFA/OIDC/invites/teams/user-admin), the opt-in secured
> router and every subsystem mounted only inside it (durable storage,
> collaborative/workflow signing, policies + webhook receiver,
> integrations/purview/external-API, audit + fleet stats, portal surfaces,
> admin settings, license admin, server certificate, tessdata download,
> personal-signature store), **MCP** (routes, OAuth verifier, and tool
> catalog supplement), SQLite itself (`rusqlite` is gone — the backend keeps
> no database), the watched-folder daemon, and the AI engine's document/RAG
> store + PDF question-answer capability were **removed**. Legacy
> `security.*`/`mcp.*`/`storage.*`/`policies.*`/audit/supabase settings keys
> and their env spellings are **ignored with a one-line startup warning,
> never refused** (existing installs keep booting). Kept: every PDF
> processing endpoint, PDF *document* security (password/redact/sanitize/
> watermark), single-shot cert-sign + hardware signing + RFC 3161
> timestamping + signature validation, SSRF guards, transport rate limits,
> robots.txt, mobile scanner, the pdf-json cache, single-tenant ephemeral
> async jobs, settings *reading* (`RUSTLING_*`/`STIRLING_*` aliases), and
> the desktop (Tauri-mode) settings write-backs. Sections below that
> describe the removed surfaces are retained as **historical record** and
> are marked as such — they no longer describe running code.

**Batch-7 validation (2026-07-29, `batch7/no-auth-stateless`):**
`cargo fmt --check` and strict locked all-target workspace Clippy are clean.
With PDFium bound via `RUSTLING_PDFIUM_LIBRARY_PATH`,
`cargo test --workspace --locked` reports **931 passed / 0 failed / 1
ignored** (`rustling-processing` **809 / 0** with 1 ignored,
`rustling-ai-engine` **115 / 0**, `rustling-operation-catalog` **7 / 0**).
The containerized `src-tauri` gate (fmt + strict clippy + tests) is green at
**10 / 0**, and the frontend gate (typecheck / eslint / **1051 vitest** /
`vite build`) is green. The pre-removal totals quoted by the historical
validation blocks below (1508/144/1647 etc.) are records of their time.

**Coordinated product rename (2026-07-28, batch 6):** the workspace crates are
`rustling-processing` / `rustling-ai-engine` / `rustling-operation-catalog`,
`RUSTLING_*` is the primary environment-variable spelling (every legacy
`STIRLING_*` spelling is accepted as a deprecated alias resolved by the
per-crate `env_compat` module — `RUSTLING_*` wins when both are set, and the
binaries log one stderr deprecation line at startup listing legacy spellings),
the startup handshake prints `RustlingPDF running on port: <port>` (the desktop
launcher parses the name-agnostic `running on port: ` suffix, so both spellings
parse), and user-visible branding — UI strings, PDF producer/creator labels
(`RustlingPDF v<version>`), SMTP defaults (the TOTP issuer and MCP server
self-description were rebranded too, then removed with their features in
batch 7) — says RustlingPDF. **Documented
divergences from the frozen upstream `SwaggerDoc.json` defaults:** stamp and
watermark `stampText`/`watermarkText` default to `RustlingPDF` (upstream
documents `Stirling Software`), producer/creator labels are
`RustlingPDF v<version>` (upstream: `Stirling-PDF v<version>`), and the SMTP
notification defaults drop the Stirling Software marketing body. **Deliberately
kept under the old spelling** (continuity with shipped v2.14.2 apps and
existing installs): the tauri bundle identifier `stirling.pdf.dev`,
`Stirling-PDF` desktop app-data directory, the pinned WiX UpgradeCode,
persisted browser-storage keys, the `StirlingPDFClassification` PDF Info key,
`StirlingSig*`/`StirlingPageNumber*` XObject names, and `X-Stirling-*` HTTP
headers (the deep-link scheme and `stirling_*` MCP tool identifiers were on
this list until their features were removed in batch 7).

**Batch-6 validation (2026-07-28, historical — pre-batch-7 totals):** `cargo fmt --check`
and strict locked all-target workspace Clippy are clean; the full workspace
suite (`cargo test --workspace --locked` with PDFium bound via
`RUSTLING_PDFIUM_LIBRARY_PATH`) reports **1692 passed / 0 failed / 1 ignored**
across all 118 targets, including new targeted coverage for the alias
mechanisms (env-spelling precedence unit tests, a legacy-`STIRLING_*`-only
boot that must warn exactly once on stderr, a both-spellings boot where
`RUSTLING_*` must win, and `rustling.*`-vs-`stirling.*` settings-root
precedence). The frontend gate (typecheck/eslint/1647 vitest/`vite build` +
`og:check`) and the containerized `src-tauri` gate (fmt + strict clippy +
tests, with the renamed `rustling-processing` sidecar stub) are green.

**Batch-3 validation (2026-07-28, historical — pre-batch-7 totals; RustlingPDF `main` after batch 3 — GitHub CI,
single-binary SPA serving, Docker packaging, the parity trio, and the identity-
persistence fix pair):** `cargo fmt --check` and strict locked all-target workspace
Clippy (`--workspace --all-targets --locked -- -D warnings`) are clean. With PDFium
bound via `RUSTLING_PDFIUM_LIBRARY_PATH` (as `task rust:test` does),
`cargo test -p rustling-processing --locked` reports **1508 passed / 0 failed**
(one pre-existing ignored test) across the library suite and all integration suites,
`cargo test -p rustling-ai-engine --locked` reports **144 passed / 0 failed** across
all targets, and the frontend gate (typecheck/eslint/**1647 vitest**/`vite build`)
is green (the differential rust-only smoke gate was **13/13** at its final run
before the harness was removed by maintainer decision on 2026-07-28). (This is the standalone
RustlingPDF repository; test totals differ slightly from the upstream Stirling-PDF
port tree because a handful of upstream-CI-specific assertions were dropped in the
repo split.) Four previously-red areas are now green rather than excused: the
`pdf_markdown` heading test (it required PDFium — earlier snapshots ran without the
library bound and misread the fallback as a pre-existing failure); the two
`rustling-ai-engine` `process_smoke` timeouts (root-caused, not environmental:
`tracing-subscriber` wrote ANSI escapes into piped output and broke the handshake
parse); and six endpoint tests that had rotted against later features (webhook
trigger listing, P-521 signing message, admin-only custom-API authoring, and the
OIDC login-CSRF browser-binding cookie). The clean-checkout build defect is fixed:
`build.rs` now stages `version.properties` into `OUT_DIR` from the committed
`rust/VERSION` file (in this repository; the upstream monorepo variant staged
the Gradle-generated file), so the crate compiles on a fresh clone. (At the
time, a security-mode guard read every boolean spelling Spring accepts and
failed closed on `SECURITY_ENABLELOGIN`/`security.enableLogin`; batch 7
replaced that guard with the ignored-with-warning layer described at the top
of this file — those keys never refuse startup now.)
External-runtime happy paths remain conditional on their respective tools and
services.

**Security-review hardening (2026-07-25, historical):** an AI-assisted security review of the
then-present secured/crypto/SSRF surface (adversarially verified) found it broadly sound (no
critical/high) and surfaced 4 Medium issues, all fixed and test-proven at the time: (1) bcrypt off
the global connection mutex + per-IP auth rate-limiting; (2) tower-http request/body timeouts +
concurrency limit at `into_router()` + a bounded webhook body assemble; (3) the OIDC callback
login-CSRF browser-binding cookie; (4) the cloud-metadata SSRF deny extended to all
embedded-IPv4 forms. Of these, only (2) still describes running code — the auth, OIDC, and
webhook surfaces were removed in batch 7, and with them the planned independent human security
review became moot (its queue item was deleted).

**Live Java-vs-Rust parity signal (historical — 2026-07-26; the differential harness was removed
from this repo by maintainer decision on 2026-07-28, final state preserved in git history):** the
`differential-parity` CI workflow drove BOTH backends and semantically diffed their output
(`testing/differential/`). **Final result: 13 PASS / 13 — green**, with 5 declared known
differences. The first live run found 4 real divergences; two were fixed
(scale-pages leaked inherited page-tree attributes, causing double rotation; get-info field/format parity,
which took the diff from 116 field mismatches down to 5). The 5 remaining field differences are declared in
`testing/differential/known_diffs.py` with a mandatory root-cause `reason` and pinned expected values:
(a) the word/character/paragraph counts on a rotated page — Java's bare `PDFTextStripper` runs with
`sortByPosition=false`, so its line-breaking splits per glyph on `/Rotate 90`; the Rust value is the correct
reading and replicating the Java quirk is deliberately not planned; (b) `XMPMetadata` — Java re-serialises
the packet through xmpbox while Rust returns it verbatim (equivalent content, unpinnable because it embeds
per-run timestamps). The registry does not blind the gate: a pinned value that drifts, or any unregistered
field, still fails; a declared difference that disappears is reported STALE.

**Route-count scoping note:** the Rust service registers approximately **164**
HTTP route paths as of 2026-07-29 (production registrations, hand-recounted
after the batch-7 removal; the pre-removal census was ~321 as of 2026-07-26 —
the ~157-route drop is the deleted auth/MCP/stateful surface). This figure is
a hand count, not test-pinned — no route-census test asserts it, and a fixed
total is deliberately deferred to the versioned baseline-to-Rust manifest
(see `README.md`) so conditional endpoints are counted by method and path
rather than inferred from source literals.
Most of these are directly comparable to Java's OSS `controller/api`
PDF-operation surface (~140 endpoint mappings, several of which are the
project's composed `@AutoJobPostMapping` annotation rather than the four
plain Spring mapping annotations); the remainder are job control, config/info
/ui-data, mobile scanner, the AI proxy, and hardware-signing discovery.
"PDF processing operations are ~90% done" refers to the
PDF-operation-comparable subset.

## Ported compatibility endpoints

Every surviving surface has a `contracts/<name>.md` compatibility document,
and all have focused unit/integration coverage. (Batch 7 deleted the
contracts of the removed surfaces with their code — durable-storage,
workflow-signing, admin-settings, account-lifecycle/account-admin-routes,
audit, portal-audit, fleet-usage, login-agreement-admin, license-entitlement,
mcp, policy-config, webhook-receiver, purview, resource-access-integrations,
personal-signatures, classification-labels, and classification-meter — and
rewrote the survivors that referenced them.)
Each route in the surviving docs was verified against the Rust registrations
and the Java counterpart
controllers, with the residual divergences recorded in the docs themselves. Coverage spans merge/split/rearrange/remove/rotate,
crop/scale/layout/booklet/poster, page numbers/stamp/watermark/comments/AI comments/attachments,
metadata/info/analysis/filters, forms (inspect/fill/modify/delete/export), password/
sanitize/flatten/repair/decompress, image↔PDF, PDF→image/text/vector/comic-book,
SVG→PDF, signature validation/verify, bookmarks/TOC, blank-page removal,
auto-rename/auto-split, plus:

- `misc/replace-invert-pdf` — FULL_INVERSION (PDFium raster + invert),
  COLOR_SPACE_CONVERSION (Ghostscript CMYK), and pure-Rust HIGH_CONTRAST/CUSTOM page-background
  plus page/nested-Form text recoloring with Java-compatible color parsing.
- `misc/scanner-effect` — full image pipeline (gradient border, rotation, feather,
  box-blur, brightness/contrast/yellowing/noise) with quality presets + DPI clamp.
- `ai/tools/pdf-comment-agent` — bounded multipart PDF comment workflow: PDFium
  positioned text segments are submitted to the Rust AI engine, trusted returned
  IDs become 20-point sticky-note annotations, and the PDF response carries a
  Java-compatible applied/instructions report. See `contracts/pdf-comment-agent.md`.
- `ai/tools/create-pdf-from-html-agent` — structured AI-document model → fixed,
  escaped A4 HTML template → WeasyPrint PDF. Arbitrary HTML is never accepted by
  this agent route; only six-digit hexadecimal colour overrides are allowed. See
  `contracts/create-pdf-agent.md`.
- `ai/tools/math-auditor-agent` — keeps the PDF locally while orchestrating the
  Rust engine's examine/deliberate rounds with bounded PDFium text and ruled-table
  CSV evidence; OCR requests remain explicitly unauditable as in Java. See
  `contracts/math-auditor-agent.md`.
- `ai/health`, `ai/pdf/edit`, `ai/orchestrate`, and `ai/orchestrate/stream` —
  Java-compatible integration with the separately deployed Rust AI engine. The
  multipart workflow routes drive bounded NDJSON turns, per-request content
  extraction (the RAG ingest arm was removed with the engine's document store
  in batch 7), local tool execution, report resume, job-scoped multi-file
  downloads, and SSE progress/heartbeat/result/error delivery with disconnect
  cancellation. The only engine-boundary credential is the shared
  `X-Engine-Auth` secret; there is no per-user identity. PDF question
  answering was removed; edit/review/create/draft orchestration remains. See
  `contracts/ai-proxy.md`.
- `convert/file/pdf` — office/text → PDF via LibreOffice shell-out, with strict HTML
  sanitization and bounded OOXML/ODF package rewriting that removes external relationships.
- `convert/pdf/word`, `convert/pdf/presentation`, `convert/pdf/xml` — PDF → office
  via LibreOffice shell-out (`--infilter=writer_pdf_import`/`impress_pdf_import`),
  single-file or ZIP output.
- `misc/ocr-pdf` — OCR via preferred OCRmyPDF shell-out or Java-compatible
  PDFium-rendered, text-aware per-page Tesseract fallback, with tessdata language
  discovery/filtering, ordered page reassembly, and configurable per-tool process
  pools with timeout/tree cleanup (sidecar → ZIP, OCRmyPDF-only
  `removeImagesAfter` via Ghostscript).
- `misc/repair` — Java-compatible Ghostscript-first and qpdf-second recovery,
  retaining startup-discovered executable paths and using the shared bounded
  process runner with qpdf warning-exit handling; the normalized in-process
  rewrite remains the fallback when neither external tool is available.
- Fresh-PDF metadata parity — image-to-PDF now writes the versioned RustlingPDF
  creator/producer label and creation/modification dates; booklet and poster
  outputs retain Java's selected standard source fields and valid dates while
  dropping custom Info keys. Form-only and full-raster flattening now apply the
  corresponding loaded/rebuilt Java policies after PDFium writes the result.
  (Pro user-aware metadata substitution was a secured-mode idea and died with
  it in batch 7.)
- `convert/pdf/html` — PDF → HTML via `pdftohtml -c` shell-out, all output files
  bundled into a ZIP.
- `convert/pdf/markdown` — Markdown in page order with literal escaping, bullets, and
  soft-hyphen repair. When the PDFium runtime is available, headings are inferred from
  text geometry, porting Java's `HeadingDetector` size-ratio thresholds (dominant glyph
  size vs. the document body median, or line height when sizes are degenerate; ≤12-word,
  non-sentence lines only), and two-column reading order is inferred from line geometry
  (porting Java's `detectsTwoColumns`/`splitIntoColumns` gutter analysis), emitting the left
  column before the right. Falls back to the text-only lopdf baseline (no headings/columns) when
  PDFium is unavailable. Java's geometry-aware table inference (borderless/ruled, cross-page
  stitching), image placement, and bold-label emphasis remain documented parity gaps.
- `convert/html/pdf` — sanitized HTML/ZIP package → PDF via WeasyPrint, including
  parser-backed active-content removal, resource SSRF restrictions, and ZIP limits.
- `convert/markdown/pdf` — CommonMark/GFM table Markdown or Markdown ZIP package →
  sanitized HTML → PDF via the shared WeasyPrint renderer.
- `convert/ebook/pdf` — EPUB/MOBI/AZW3/FB2/TXT/DOCX → PDF via Calibre, including
  font/TOC/page-number flags and best-effort Ghostscript e-reader optimization.
- `convert/pdf/epub` — PDF → EPUB/AZW3 through Calibre's `pdftohtml` engine,
  including Java's heuristic, CSS filtering, chapter-detection, and device-profile flags.
- `convert/eml/pdf` — native MIME EML and Outlook MSG parsing, CID-image inlining,
  safe HTML export, and WeasyPrint PDF rendering with bounded PDF attachments.
- `convert/url/pdf` — opt-in, DNS-pinned public HTTP(S) fetch → sanitized HTML →
  WeasyPrint PDF, with Java-compatible redirects for disabled or rejected targets.
- `convert/cbr/pdf` — RAR/CBR → naturally ordered image PDF through `unrar` or
  a read-only 7-Zip fallback, with bounded extraction and link rejection.
- `convert/pdf/cbr` — PDFium-rendered PNG pages → RAR-backed CBR through `rar`.
- `convert/pdf/pdfa` — PDF/A-1b/2b/3b and PDF/X through Ghostscript, with embedded
  sRGB/Gray ICC profiles and optional strict veraPDF validation.
- `misc/extract-image-scans` — PDFium page rasterization or raster upload → the
  native Rust median-background, mask/contour, and Canny/Hough splitter, with
  bounded and link-safe PNG/ZIP output and no Python/OpenCV runtime.
- `convert/pdf/video` — PDFium-rendered frames, native embedded-font watermarking, and
  FFmpeg MP4/WebM encoding. The current Java mapping is commented out while FFmpeg CVEs are
  assessed; Rust exposes the route as a documented opt-in cutover endpoint.
- `convert/pdf/csv` and `convert/pdf/xlsx` — Java-compatible ruled-table (Tabula lattice)
  extraction through PDFium paths and character bounds; no-table 204, CSV/ZIP, or a
  one-sheet-per-table XLSX output.
- `security/redact` — manual areas and whole-page redaction through a deliberately secure,
  image-only PDF pipeline; unlike Java's default overlay branch, source page objects are never
  copied to the response.
- `security/auto-redact` — case-insensitive literal or bounded-regex text matching from PDFium
  glyph bounds, line-aware painted boxes, and the same image-only output guarantee.
- `security/redact-execute` — unified exact/regex, page wipe, range, image-box, and detected-image
  redaction plan, finalised as an image-only PDF regardless of legacy overlay strategy hints.
- `convert/pdf/text-editor/{metadata,partial,page,fonts,clear-cache}` — lazy editor job cache with
  30-minute expiry, per-page COS projection, bounded page-scoped font resources/programs and
  ToUnicode extraction, cache clear, and source-preserving partial export. Cached-page updates now
  distinguish omitted from explicitly empty arrays, preserve incomplete lightweight COS payloads,
  apply complete resource/content/annotation updates in place, and regenerate edited text/images
  over bounded retained vector content while preserving untouched pages, forms, and catalog data.
- `general/job/{jobId}`, its `/result` and `/result/files` children, and
  `general/files/{fileId}`/`metadata` — configurable-TTL single-node ephemeral async job storage
  and result download. `convert/pdf/text-editor?async=true` retains its specialized worker; the
  other ported processing POST endpoints support generic `?async=true` by streaming their
  original multipart request and result through the job directory instead of RAM. Jobs are
  single-tenant (batch 7 deleted `JobOwner` with the auth subsystem — there are no owners to
  isolate). A bounded
  resource-weighted queue gates light/medium/heavy/extra-heavy work, supports queued cancellation
  and queue positions, and exposes Java-compatible admin job/queue stats and cleanup.
- `edit-text` — ordered literal find/replace in selected-page PDF text-showing content streams,
  whole-word filtering, and strict active-font encoding validation.
- `config/app-config`, endpoint/group status/availability, and
  `settings/get-endpoints-status` —
  base/custom YAML configuration, public bootstrap values, endpoint-disable
  status, and timestamp configuration. (`settings/update-enable-analytics` —
  the server-persisted analytics consent — was removed in batch 7; consent is
  client-side state now. The secured `admin/settings` mutation family and
  `admin/server-certificate` routes, with cert-sign's `certType=SERVER`, were
  removed with the secured router.)
- `config/login-disclaimer` — live bounded markdown lookup with Java-compatible
  locale fallback, served to anonymous users from operator-provisioned
  `customFiles/disclaimer/` files. (The administrator management routes under
  `admin/login-agreement` were removed in batch 7; the desktop shell still
  provisions disclaimer files locally.) See `contracts/login-disclaimer.md`.
- `info/status`, `info/health`, request/load counters, uptime, and `info/wau` —
  process-local Java-compatible metrics and no-login weekly-active-browser tracking,
  governed by `metrics.enabled`.
- `ui-data/footer-info`, `home`, `licenses`, `pipeline`, `ocr-pdf`, and `sign` —
  read-only client metadata from the Rust runtime tree: legal/analytics settings,
  survey visibility, bundled notices, pipeline templates, Tesseract languages, and
  shared-signature/font discovery. The Rust dependency manifest is generated from
  `Cargo.lock` at build time, with
  `UNKNOWN` and native-tool notices retained as release-compliance gates.
- `GET /js/additionalLanguageCode.js` — legacy language-bootstrap JavaScript with
  build-time bundled locales and the configured `ui.languages` allowlist.
- `GET /robots.txt` — Java-compatible search-engine policy, controlled by
  `system.googlevisibility` or `SYSTEM_GOOGLEVISIBILITY`.
- `general/signatures/{filename}` — shared PNG/JPEG signature-asset retrieval
  from operator-provisioned `customFiles/signatures/` files, with basename
  validation and symlink rejection. (The secured `proprietary/signatures`
  personal-signature store was removed in batch 7; the SPA stores personal
  signatures in browser localStorage.)
- `mobile-scanner/*` — anonymous QR-session transfer with multipart upload,
  safe temporary storage, ten-minute inactivity expiry, download-after-read cleanup,
  and `system.enableMobileScanner` feature gating.
- `pipeline/handleData` — synchronous multipart pipelines through the in-process
  Rust router, streamed intermediate files, ZIP fan-out, endpoint allowlisting,
  and confirmed SISO/MISO execution shapes. See `contracts/pipeline.md`.
- (Watched-folder pipelines — the server-side 60-second directory-scan daemon
  — were removed in batch 7. The SPA's client-side watched folders, via the
  File System Access API, are the replacement and watch the user's real
  folders.)
- Conditional `general/send-email` — bounded HTML MIME plus one attachment through the existing
  `mail.*` SMTP relay settings, including authentication and plaintext/STARTTLS/implicit-TLS
  modes. (The invitation-link and password-change notification mails died with accounts in
  batch 7.) Rust deliberately rejects wildcard
  certificate trust and disabled hostname verification.
  See `contracts/send-email.md`.
- (Removed in batch 7 — recorded here as history: the secured audit APIs and
  fleet-usage statistics, the commercial entitlement policy and administrator
  license lifecycle, the tessdata download routes, durable storage,
  collaborative (group) signing, the portal audit views and
  `proprietary/ui-data` projections + portal API keys, the policy subsystem
  with its webhook receiver/spool and implied-folder-roots route,
  integrations/resource grants with the `external-api-call` step, Purview
  labelling, and the classification meter. Their `contracts/*.md` files were
  deleted with them; per-surface history lives in git.)
- `general/send-email` honors `?async=true` (async-job allowlist), matching
  Java's `@AutoJobPostMapping`. Before batch 7, a systematic route
  cross-check had found no remaining bounded parity gap in the OSS-core +
  proprietary `controller/api` route surface; batch 7 then deliberately
  removed the auth/stateful part of that surface, so parity with upstream's
  account/storage/policy controllers is an explicit non-goal now, not a gap.
  What remains open on the kept surface: upstream-blocked items
  (Windows-cert async), the H2-only `ui-data/database` (N/A — no database),
  and unbounded PDF-fidelity work (Type3 glyph synthesis, Type0/Type3
  byte-parity — the latter confirmed blocked, needing a net-new
  embedded-font-program parser AND poisoned by the Java oracle's C0-stripping
  of 2-byte CIDs).

## Remaining (not yet ported)

### Large pure-Rust subsystems — fully verifiable, multi-session each

- **PDF ↔ JSON editor model** (`ConvertPdfJson`): the page-level COS model, lazy endpoint surface,
  bounded page-font resource/program export, and an initial pure-Rust parser for page and Form
  XObject text-showing content streams are ported. Text runs preserve device fill/stroke colours,
  rendering mode, and simple-font `/Widths` geometry. Type0 `/ToUnicode` source codes and
  horizontal descendant `/DW`/`/W` advances are now applied. Vertical Type0 writing applies
  `/DW2` defaults and both `/W2` forms to glyph origins, displacement, and `TJ` movement. Embedded
  encoding CMaps now apply bounded `cidchar`/`cidrange` source-code-to-CID mappings before those
  metrics. Named non-identity CMaps additionally resolve bounded recursive `usecmap` inheritance
  from the production image's Poppler Adobe mapping data, with safe collection-scoped paths and a
  shared bounded cache; missing data retains the conservative source-code fallback. Type3 fonts
  now export the Java-shaped bounded CharProc code/name/Unicode metadata and preserve their source
  CharProcs for generated-text rebuilds; outline-derived normalization and broader font synthesis
  remain. Direct and Form-nested image
  XObjects now export page-space transforms
  and bounded JPEG or 1/2/4/8/16-bit RGB/gray/CMYK image data, apply `/Decode` ranges and grayscale
  `/SMask` alpha, and expand packed 1/2/4/8-bit Indexed images with Gray/RGB/CMYK palettes;
  JSON-only pages rebuild ordered raster images, including alpha through PDF soft masks.
  Unfiltered and bounded single-filter Flate/LZW/ASCII85/DCT 8-bit device-colour inline
  images are extracted. Color-key `/Mask` arrays and explicit 1-bit stencil masks are applied
  for bounded supported rasters. ICCBased Gray/RGB/CMYK XObjects and ICCBased Indexed palette bases
  now use their bounded embedded profiles for pure-Rust conversion to sRGB, including Gray/RGB DCT
  images, with compatible declared device-`/Alternate` fallback for invalid profiles. Four-channel
  ICCBased DCT images (including YCCK/Adobe-marker variants) now decode natively and convert
  through their bounded embedded profile to sRGB instead of silently keeping the decoder's device
  projection, and DCT CMYK color-key `/Mask` ranges are applied against the pre-`/Decode` decoder
  output per PDF 32000-1 §8.9.6.4; rasters above the editor byte bound deliberately keep the
  bounded device fallback. Real-valued numeric `DecodeParms` entries (e.g.
  `/Predictor 2.0`) now truncate to integers with PDFBox
  `COSNumber.intValue` semantics (toward zero, NaN→0, saturating at `i32`
  bounds); the DCT `/ColorTransform` read deliberately stays on the PDF.js
  oracle's integer-only check, a documented divergence in
  `contracts/pdf-json.md`. Complex inline filter decoders
  (CCITTFax/JBIG2/JPX) remain. Device-alternate Separation and one-to-eight-component DeviceN XObjects
  with bounded order-1 sampled Type 0, single-input exponential Type 2, recursively bounded
  single-input stitching Type 3, or bounded PostScript calculator Type 4 tint transforms are
  evaluated into Gray/RGB/CMYK, including
  one-component DCT Separation images after applying `/Decode`. The Type 4 interpreter implements the
  full PDF 7.10.5.2 operator set (arithmetic, relational/boolean/bitwise, `if`/`ifelse`, and stack
  operators) over bounded token, step, and stack limits. CalGray/CalRGB/Lab direct images,
  Indexed bases, ICC fallbacks, and spot-color alternates use bounded calibrated conversion,
  including Gray/RGB/Lab DCT. One-to-four-component DCT DeviceN images retain native JPEG planes,
  perform Adobe/`ColorTransform` conversion, apply `/Decode` in PDF.js order, and evaluate their
  tint functions. Separation and DeviceN images whose alternate is an `ICCBased` space now
  convert the tint output through the embedded profile (falling back to the declared device
  `/Alternate` when the profile is invalid); DeviceN DCT above four components remains.
  Full editor responses also inspect root AcroForm fields plus their
  inherited metadata and first widget location, and export structured page annotations (with
  full-mode COS data). JSON→PDF rebuilds root fields/one fresh widget and non-widget page
  annotations. JSON-only pages can draw ordered Latin Standard-14/WinAnsi text and raster images
  with matrix/state/color data. Generated text can also restore bounded embedded font dictionaries,
  nested font-program streams, Type0/CID encodings, and existing Type3 CharProcs, refusing edits that
  cannot round-trip through the source encoding. Document XMP packets round-trip as bounded base64
  metadata. Info and annotation dates now round-trip through the PDF `D:...`↔ISO-8601 conversion
  (offset normalized to `+00'00'`, the key omitted on a parse failure, and the annotation overlay
  converts ISO→`D:` so it never writes an invalid literal), and `/Trapped` is read/written as a COS
  Name — both previously documented parity gaps, now closed. Cached partial export can redraw edited
  text/images over bounded retained vector content.
  The full-document rebuild path now ports that same strip-and-regenerate strategy for a page that
  mixes a preserved `content_streams` entry with edited `textElements`/`imageElements`: it strips only
  the represented text or represented-image draws whose element list was actually resubmitted, and
  leaves the other content type's preserved draws/resources untouched — since the page model's
  `textElements`/`imageElements` are plain lists rather than optional, an empty list is read as "not
  resubmitted," not "delete everything of this type," so a client cannot clear just one content
  type on a mixed page through this endpoint (the lazy/partial endpoint already supports that). This
  is verified PARITY, not a Rust gap: Java behaves identically — `PdfJsonPage` defaults both lists to
  empty (`@Builder.Default`), `convertJsonToPdf` null-coalesces absent/null/empty to the same state
  (`PdfJsonConversionService.java:692-707`), with preserved streams an empty list never strips that
  content type (`:731-772`), and `extractVectorGraphics` strips only image draws whose `objectName`
  is in the submitted list (`:3163-3172` — empty strips nothing). Clearing one content type on a
  mixed page would need a shared nullable-list schema decision across Java, Rust, and the frontend. This
  mixed-edit regeneration is now the fallback: for a text-only mixed edit (non-empty `textElements`, no
  image edits) on a simple `Type1`/`TrueType`/`MMType1` font, the full-document rebuild first attempts
  Java's token-preserving in-place `Tj`/`TJ` rewrite (`rewrite_text_operators`, porting
  `rewriteTextOperators`) — it swaps only each show-text string operand for the replacement re-encoded
  through the same font and carries every other token (positioning, `TJ` kerning, vector ops) through
  byte-for-byte, so a boundary-aligned edit round-trips token-for-token. It defers wholesale to
  strip-and-regenerate, with no partial rewrite, on any unsupported case (`Type0`/`Type3` or
  unresolvable font, a Standard-14 fallback being needed, an encode failure, a glyph-count/cursor
  mismatch, invoked-Form text, or an interior-kerned multi-string `TJ`). This partially closes the
  byte-parity gap with `PdfJsonConversionService`; still open are `Type0`/`Type3`, interior-kerning-run
  rewrite, true Type3 glyph synthesis, and byte-parity for those deferred classes. Two seeming gaps are
  confirmed parity rather than Rust shortfalls: Java's `TextRunAccumulator` also merges same-baseline
  kerned glyphs with no kerning-gap check (so an interior-kerning run defers on both sides), and Java's
  partial-export path (`determineRegenerateMode` with `forceRegenerate=true`) also always regenerates
  (so the Rust `partial/{jobId}` path always regenerating is parity). Generated text that mixes a character the
  restored font (Type3 or otherwise) can represent with one it cannot now degrades gracefully —
  the unrepresentable run falls back to Standard-14 instead of refusing the whole element's edit —
  rather than fabricating a genuinely new glyph. A character representable by neither the restored
  font nor Standard-14 still fails the edit. True Type3 glyph synthesis (drawing a novel outline for
  a character absent from every available source) remains missing and would need a new font-outline
  extraction/Bezier-to-PDF-path subsystem this crate does not have; it is not a bounded follow-on to
  the graceful-fallback work above. One widget per
  field matches Java's own `PdfJsonFormField` wire model (`rect`/`pageNumber` are singular there too,
  and `PdfJsonConversionService` likewise reconstructs only one widget per field) — a radio-button-
  style multi-widget field is not a Rust port gap versus Java; it would need a new shared schema
  design across Java, Rust, and the frontend contract before either side could port it. Restored
  `Tx`/`Ch` (text/choice) form-field widgets now get a real `/AP` normal appearance stream — the
  widget's current value drawn with the shared Helvetica `DR` resource, sized to the field's `rect`
  — so headless consumers (flatteners, rasterizers, printers) that ignore `NeedAppearances` still
  render the value. `Btn` (checkbox) widgets get a two-state `{on_state, Off}` `/AP/N` appearance
  dictionary matching `/AS`, with a plain `X` mark for the checked state (not a byte-match for
  Java's own checkbox glyph). Non-widget annotation appearance streams remain `NeedAppearances`-only.
- **Advanced text editing parity** (`edit-text`): selected-page content-stream replacements are
  ported; every edited page receives a private clone of its indirect Form graph so shared source
  Forms cannot leak changes across page filters. Every repeated visual invocation on one selected
  page is also rewritten to a private Form graph, so instance-specific cross-stream matches cannot
  mutate a sibling. Matching joins strings across separate `Tj`/quote operators, `TJ` array entries,
  and page↔invoked/nested-Form stream boundaries in content order, anchors cross-object replacements
  in the first object, and preserves the last object's suffix. Cyclic Form back-edges remain a safe
  sequence boundary.
- **Advanced redaction parity**: `redact-execute` is ported with all request target classes and a
  secure raster output. Automatic image discovery now descends nested Form XObjects with composed
  placement matrices and a conservative depth-limit fallback. Range anchors now use Java-compatible
  regex/literal/letter-spacing/punctuation/first-line fallbacks; range content is selected from
  line and image boxes in detected one/two-column reading order. Exact glyph boxes can still differ
  between PDFium and PDFBox for exotic fonts and unusual three-plus-column layouts.

### App infrastructure

Distributed job storage/backplane and cross-node queue/retry semantics are not an OSS parity gap:
Java's own default (`InProcessJobStore`) is single-node, identical in spirit to what Rust already
has; the Redis-backed `ValkeyJobStore` clustering path is an opt-in proprietary/enterprise add-on
Rust hasn't built, and whether the Rust port should ever target multi-node deployment is a product
decision, not a coding task. Generic async-job wiring for the routes the existing `?async=true`
wrapper doesn't yet cover is narrower than it sounds: PKCS#11 hardware-certificate enumeration is
now wired (the wrapper is content-type-agnostic, so a plain-JSON POST route needed only an allowlist
entry, no new mechanism); Windows certificate enumeration cannot use it because it's a bodyless GET
and the wrapper's shared detection is POST-only for the whole allowlist — matching Java's own
`AutoJobPostMapping` annotation, itself hardcoded POST-only, so this is genuine upstream parity, not
a Rust-specific limitation. Job/control routes (`general/job/*` plus the admin job
stats/queue/cleanup trio) and the mobile-scanner API are wired in
the production router today (admin settings mutation was removed in batch 7). The
Tauri desktop shell now launches the Rust binary as its **packaged sidecar by
default** (batch 4): the Java JRE/JAR launch path is deleted,
`RUSTLING_NATIVE_BACKEND_PATH` is demoted to a dev-only override, and the
launcher wires bundled PDFium (`RUSTLING_PDFIUM_LIBRARY_PATH` pointed at the
bundle's `resources/pdfium` unless the operator already set it) alongside the
unconditional ephemeral-port handshake, desktop/base-path/login-agreement
environment, legacy-workspace migration, a bounded startup wait, early-exit
reporting, stale-port cleanup, PID/start-time parent-death enforcement, and
atomic fresh-install settings/template initialization. Open-mode local `backend:dev`, `dev`, and default `dev:all` now
launch `rustling-processing` (in this repository they are the only backend entry
points; the Java-oracle and SaaS Task paths existed in the upstream monorepo).
Container distribution shipped in batch 3 (Docker image; batch 4 added the
tag-driven GHCR release pipeline) and the desktop bundle ships the Rust
sidecar since batch 4 (`task desktop:stage-sidecar` stages the release binary
plus PDFium — Windows dispatches to `install-pdfium.ps1` since batch 5; a
desktop CI workflow compiles and tests the shell). Batch 5 completed the
desktop release path: a tagged release now also builds and signs desktop
bundles on a three-OS runner matrix (repo-controlled updater key) and
publishes them with a composer-generated `latest.json`, and the Linux
signed-upgrade e2e proof passed 8/8 including negative-signature tests
(see `contracts/desktop-native-startup.md`). macOS/Windows upgrade-proof
legs, mac-Intel, and notarization remain follow-ups.
Java-compatible short-file recovery is now ported: a `settings.yml` with fewer than
`MIN_SETTINGS_FILE_LINES` (31, matching `ConfigInitializer`) lines is treated as truncated by an
interrupted write, backed up to `settings.yml.<epoch-millis>.bak`, and recreated from the template,
exactly as Java does; `custom_settings.yml` is never subject to this check. Upgrade-template merging
is now also ported (matching Java's `ConfigInitializer`/`YamlHelper`): when an existing long-enough
`settings.yml` is present, the bundled template is walked line-by-line and each leaf value the user
customized is substituted into the template's own structure — preserving the template's comments,
blank lines, key order, indentation, and inline comments — so new template keys arrive with their
defaults, user-only keys are dropped, and the file is only rewritten if the merge changed it
(idempotent). Values are re-emitted through `serde_yaml`'s own scalar emitter so a plain-styled
value containing `#`/`:`/`*` (e.g. a DB password) is correctly quoted and round-trips exactly
instead of being silently truncated or corrupting the file — a corruption bug an adversarial review
caught and fixed before merge. A user override of a block/nested-map value (the template currently
has no block sequences) falls back to the template default, a documented scalar/inline-scope
limitation. Hostile hand-authored settings shapes (flow-collection roots and
sections, block-sequence roots and sections, block-scalar leaves) are refused
with the file left byte-for-byte untouched — identity initialization degrades
to a clean fail-open ephemeral identity. (These `settings.yml` write-backs —
template creation, truncation recovery, upgrade merge, install identity — run
**only in Tauri desktop mode** since batch 7; a server boot never writes
settings. The admin/license settings persistence that shared the
comment-preserving editor was removed with its routes.) Sidecar/PDFium packaging and
the production default switch landed with batch 4; batch 5 added the signed
desktop-bundle release matrix, a repo-controlled updater keypair, and the
containerized Linux signed-upgrade e2e proof — macOS/Windows upgrade-proof
legs remain. See
`contracts/desktop-native-startup.md`. The
hardware-signing capability route reports desktop mode
and safely discovers on-disk PKCS#11 libraries without loading them. Windows desktop builds can
also enumerate current-user signing certificates without exporting key material or prompting for a
PIN. Desktop PKCS#11 certificate enumeration now requires a detected/configured canonical driver,
uses a serialized read-only request session and zeroizing PIN, and only returns X.509 certificates
matched to an eligible private signing key. The same provider now signs detached CMS through an
opaque token handle after strict `CKA_ID` selection and mechanism-capability checks; it supports
RSA/SHA-256 and P-256/P-384 ECDSA with safe raw-mechanism fallbacks. Windows-store signing now
selects an exact CurrentUser thumbprint and uses a bounded PowerShell/.NET `SignedCms` bridge over
anonymous pipes, preserving CSP/CNG ownership and native PIN prompts. A live ECC certificate smoke
test passed end-to-end, including independent PDF byte-range/CMS verification. `config/app-config`
reports login and storage capabilities as permanently disabled (`enableLogin: false` is a frozen
compatibility key the SPA still reads); hardware signing remains desktop-loopback gated.

**Removed in batch 7 (historical):** an opt-in secured router used to provide
the full account subsystem — durable local BCrypt identities, persistent
lockout, rotating opaque sessions, digest-only API keys, AES-GCM-protected
TOTP with recovery codes, roles/teams/invitations, user administration, audit
retrieval/export/retention, Supabase JWT verification, and a complete generic
OIDC login flow (discovery, PKCE authorization, SSRF-pinned token exchange,
JWKS-cached ID-token verification, single-use state store, DoS-hardened
public routes, confidential-client support, and the browser-binding
login-CSRF cookie). All of it was deleted by the batch-7 maintainer decision;
the adversarially-reviewed SSRF machinery it pioneered survives where live
consumers remain (`url_to_pdf`'s self-contained resolve-and-pin guard). SAML2
was never built and is out of scope with the rest of the identity layer.


The standalone Rust runtime now performs bounded startup discovery for its optional
command-line dependencies, including Java-compatible QPDF and WeasyPrint minimum
versions. Missing tool groups participate in endpoint alternatives and are reported
as `DEPENDENCY`, separately from administrator `CONFIG` disables. The inactive Java
`print-file` method has no registered route and is therefore not a cutover surface.

The signing migration now has a tested source-independent `SigningKey` boundary
and request-lifetime zeroizing secret wrapper. `/api/v1/security/cert-sign`
supports plain/encrypted PKCS#8 and traditional RSA/P-256/P-384 PEM keys,
strictly parsed in-memory PKCS#12/PFX keystores, and authenticated JKS v1/v2
stores, including password and optional alias selection. These paths create an
invisible incremental CMS signature with a fixed `/ByteRange`/`Contents`
reservation; endpoint tests reconstruct the signed ranges and verify CMS.
The same incremental writer now supports visible page widgets with bounded
signer/date/reason text and an optional vector mark while preserving the CMS
byte range. Desktop-loopback PKCS#11 signing now keeps PIN and key use inside one
serialized login/sign/logout session. Windows-store signing similarly keeps the key in its native
provider and has an opt-in live endpoint fixture. (Managed server signing — the generated
server-held PKCS#12 behind `certType=SERVER` — and the proprietary route entitlement/Keygen tier
derivation were removed with the secured router in batch 7.) Traditional (non-PKCS#8)
EC PEM signing supports P-256 and P-384. Its DEK-Info cipher coverage (AES-128/192/256-CBC,
DES-EDE3-CBC, DES-CBC) already matches everything realistically produced by current tooling — RC2/RC4/
CAMELLIA are deprecated legacy PEM ciphers nobody deliberately picks for a signing workflow and are not
planned. **P-521 signing now works** (2026-07-25): rather than the `x509-certificate` 0.25.0 convenience
signer (which only implements `Secp256r1`/`Secp384r1`), the P-521 path signs the CMS `SignerInfo` directly
with the pure-Rust `p521` crate (ECDSA-P521 + SHA-512 → `ecdsa-with-SHA512` / `secp521r1`), reusing the
existing `/ByteRange`+`/Contents` reservation. Independently verified with OpenSSL 3 (`cms -verify` passes;
tampered content and wrong keys are rejected). **A pre-existing P-384 CMS bug uncovered by that work is
also fixed** (2026-07-25): the P-384 path used to emit a SHA-256 `digestAlgorithm` against an
`ecdsa-with-SHA384` `signatureAlgorithm` — a curve inconsistency strict verifiers (OpenSSL, Adobe) reject.
It now emits SHA-384, verified the same way (OpenSSL `cms -verify` passes where the pre-fix output failed).
Every EC curve is now digest-consistent: P-256/SHA-256, P-384/SHA-384, P-521/SHA-512.
A live SoftHSM/token compatibility matrix and broader Windows smart-card coverage
remain explicit gaps. It also lacks certificate policy validation and
public Java/Acrobat compatibility fixtures, so it is not
full signing or PAdES parity. See `SIGNING_MIGRATION_DESIGN.md`.

(Historical: until batch 7 the binary refused to start when
`DOCKER_ENABLE_SECURITY`/`SECURITY_ENABLELOGIN` was set, pending an
independent security review of an opt-in secured router. Batch 7 deleted the
secured router and the guard: those keys are now ignored with a one-line
startup warning, and `SECURITY_MIGRATION_DESIGN.md` was deleted with them.)

The separate `rustling-ai-engine` crate serves the surviving (stateless) agent
surface: health/auth, classification, PDF comments, both math-audit rounds,
per-request contradiction detection, schema-grounded PDF
edit planning, PDF review, structured PDF creation, saved-agent draft/revision,
the next-action contract (which, matching the Python oracle, is a live stub:
`POST /api/v1/agents/next-action` always returns
`cannot_continue`/"Execution planning is not implemented yet" — see
`contracts/ai-engine-foundation.md`), and the NDJSON orchestrator with
math-audit resume. (Batch 7 removed the Python oracle's stateful arm: the
durable SQLite/pgvector document store, embeddings, PDF question answering,
long-document map/reduce, per-user identity, and the `migrate-sqlite-vec`
binary. The capability manifest advertises the seven surviving capabilities;
`pdf_review`'s contradiction branch requests page content per turn via the
`need_content` protocol instead of reading a store.) The smart and fast model
tiers share the Python-compatible
process-wide `RUSTLING_MODEL_MAX_CONCURRENCY` ceiling, in addition to narrower
per-agent worker limits. Model-selected evidence and comment anchors are mapped back to
trusted local indices, while edit parameters are validated against a generated
snapshot of the Java operation schemas. Deterministic saved-agent steps reuse
that catalog plus the three typed Python agent operations, while `ai_tool`
steps remain restricted to generated processing endpoints; both reject unknown
endpoints, and deterministic steps reject mismatched parameter objects on
inbound requests and model output.

The engine also ports the Python oracle's admin config-push subsystem (Python
PR #7069): `POST /api/v1/config` accepts Java's `AiEngineConfigSync` body (both
camelCase and snake_case field spellings; unknown fields tolerated, matching
the oracle's `TolerantApiModel`), is gated by the Python-compatible
`RUSTLING_ALLOW_CONFIG_PUSH` flag (default on, same as Python; flag-off → 403
naming the flag), rebuilds the live model tiers with a fresh shared semaphore
while in-flight requests keep their runtime snapshot, and persists the pushed
config through an encrypted at-rest cache restored on boot (0600 files;
corrupt/wrong-key cache falls back to environment config, matching
`_restore_cached_config`). One documented divergence: the cache cipher is
AES-GCM rather than Python's Fernet — the cache is engine-private, never read
across languages. The two `process_smoke` timeouts previously written off as
environmental are fixed: `tracing-subscriber` emitted ANSI escapes into piped
output, breaking the handshake parse; smoke tests now capture child stderr and
all five pass in under a second. See `contracts/ai-engine-foundation.md`.

Structured provider inference now includes the Python-compatible native
`ollama:<model>` path for both model tiers: keyless local or optionally
authenticated remote endpoints, normalized OpenAI-compatible URLs, and
schema-constrained native JSON output. A compiled-binary process test proves an
HTTP agent request completes through a fake Ollama server without inventing an
authorization header. The generated operation snapshot no longer passes through
Pydantic: the typed `rustling-operation-catalog` crate translates Java OpenAPI
directly, retains validation/default semantics, and supplies a deterministic
`--check` drift gate while the Python artifact remains an independent oracle.

Environment-backed AI-engine booleans and numeric limits still parse strictly before the listener
binds. Malformed or non-Unicode auth flags terminate startup instead of substituting the permissive
default, and token/concurrency/contradiction bounds are validated at the same fail-closed boundary
(the chunking/pgvector/document-backend settings died with the store; their legacy env vars are
ignored with a startup warning).

The processing service owns the surviving Java-facing AI controller surface.
Its orchestration routes are a real state-machine port rather than a multipart
pass-through: uploads receive stable content IDs; requested pages are
extracted per request; plans run through the same bounded internal dispatcher;
structured reports can resume the engine; every output receives a job file ID;
and engine NDJSON is translated to sync JSON or SSE with disconnect
cancellation. There is no per-user identity anywhere on this path since
batch 7. See `contracts/ai-proxy.md`.

(Removed in batch 7 — historical: the MCP server — JSON-RPC transport, the
`stirling_*` tool set with its generated `mcp_operation_supplement.json`, and
the OAuth/API-key verification modes — plus resource-grant administration,
encrypted S3/MCP/API integration configs, the entire policy subsystem with
its processed-file ledger, folder/S3/webhook sources, triggers, sinks, and
public HMAC webhook receiver, and the classification-label CRUD store. The
operation-catalog crate still carries the unused MCP-supplement generator; it
is inert.)

The `classify-and-label` PDF bridge survives as a pure function: the label
vocabulary arrives in the request body (the SPA owns label storage
client-side), and the bridge reads only a bounded de-duplicated
first-two/last-two page window before writing the focused
`StirlingPDFClassification` Info entry.
See `contracts/ai-engine-foundation.md` and
`contracts/pdf-comment-agent.md`.

The dispatchable `create-pdf-from-html-agent` tool is also owned by
`rustling-processing`. It keeps Java's multipart structured-document contract,
requires the AI feature setting, and renders escaped fields only through a fixed
template. It does not rely on an AI provider at request time. See
`contracts/create-pdf-agent.md`.

The public `math-auditor-agent` orchestration is likewise owned by
`rustling-processing`: PDFium classifies/extracts local evidence, while the
Rust AI engine receives only the two typed protocol messages. See
`contracts/math-auditor-agent.md`.

The self-contained document-classifier route is also live:
it is available at `POST /api/v1/documents/classify` only with a configured
structured-output provider (Anthropic Messages or OpenAI-compatible). It is a
stateless internal primitive (it shares only the `/documents` path prefix
with the deleted store routes) and is deliberately excluded from the engine's
capability manifest, which advertises the seven user-facing capabilities.

### SaaS hosted-cloud layer (`app/saas/`) — REMOVED from scope (batch 7)

This layer was never ported (originally PAUSED as unverifiable in this dev
environment), and the batch-7 no-auth/stateless decision removed it from
scope permanently — the frontend `saas`/`cloud`/`portal` layers were deleted
in the same batch. The controller inventory below is kept as the historical
record of what upstream has and this product will not build:
`stirling.software.proprietary.accountlink` — the
`@Profile("!saas")` self-hosted combined-billing `AccountLinkController`
(`/api/v1/account-link`: `link`/`status`/`unlink`/`usage`/`sync-now`, admin-only,
gated behind `stirling.billing.account-link.enabled`) and its
`InstanceEntitlementInterceptor`, which `AccountLinkWebMvcConfig` registers over
`/api/v1/**` as a request-time 402 entitlement gate with per-request metering,
both calling the same external cloud-billing domain as the SaaS layer
(Supabase auth, payment gateways, cloud billing/entitlement, instance
registry); plus:

- `AiCreateController` / `AiCreateInternalController` — `ai/create/sessions/*`
  (AI document-creation sessions + `JobChargeService` metering).
- `AiProxyController` (SaaS extras) — `ai/{generate_section, generate_all_sections,
  chat/*, edit/sessions/*, pdf-editor/*, intent/check, progressive_render,
  style/{userId}, versions/{userId}, import_template, output, pdf/answer}`.
- `UserRoleWebhookController` — `user-role/*` (Supabase/billing role webhooks).
- `AccountLinkController` (`account-link/*`), `InstanceController`,
  `Payg{Wallet,Invoices,PaymentMethod}Controller`, `PricingPolicyAdminController`,
  `ProcurementController`, `SaasTeamController`, `SaasFleetUsageController` (a `@Profile("saas")`
  team-scoped alternative on the `usage/fleet-stats` path this backend no
  longer serves).
- `DatabaseController`/`DatabaseControllerEnterprise` — H2-only
  (`@Conditional(H2SQLCondition)`) DB backup/restore; N/A — the Rust backend
  keeps no database at all since batch 7.
- `PaygCucumberThrowController` — a `@Profile("payg-cucumber")` hidden test stub
  that forces a 500 for cucumber runs; never registered in production, nothing to port.

`CertSignController`'s base `/api/v1/security` and `PrintFileController`'s
`/api/v1/misc/print-file` show up in naive scans but are false positives — the real
cert-sign routes are ported and the Java print-file mapping is commented out (inactive).

## How to find gaps precisely

Use `docs/contracts/legacy-runtime-baseline.md` (an upstream Stirling-PDF
path — see the repository note at the top) for the cross-surface baseline and
the contract files in `rust/contracts/` for implemented behavior and explicit
gaps. Source-literal counts are not authoritative because Spring composes class
and method mappings while the Rust service composes public and conditional
routers. When diffing Java `@*Mapping` literals against the Rust
route constants, remember two things: (1) batch 7 made upstream's
account/storage/policy/audit/MCP controllers an explicit non-goal, so those
Java routes are *removed-by-decision*, not "unported gaps"; (2) STRIP Java
comments first — several controllers keep inactive
endpoints commented out (e.g. `AuditDashboardController`'s `/stats/range`,
`/principals`, `/latest`; `PrintFileController`'s `/print-file`), and a naive grep
reports those as false "unported" gaps.
