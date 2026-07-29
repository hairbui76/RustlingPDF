# Execution Plan: Pure-Rust Port Programme (eliminate non-Rust runtime surface)

Date: 2026-07-29

## Status

Active

## Outcome

Shrink the product's non-Rust runtime surface to only what genuinely cannot be
written in Rust, endpoint by endpoint, without regressing behaviour. Every
external tool either (a) gets a pure-Rust implementation, (b) is honestly
reported as an optional dependency, or (c) is deliberately retired with the
reason recorded. The UI is explicitly out of scope (maintainer directive).

## Context

- Maintainer directive (2026-07-29): "continue porting everything to Rust
  (except UI)", with Codex spawned as the dev in dev+tester pairs.
- The Java→Rust port itself is complete; batch 7 made the server stateless and
  auth-free. What remains non-Rust is edge I/O: 13 external CLI binaries,
  PDFium (C++), four PDF-JSON fidelity gaps, and build-time scripts.
- Authoritative reconnaissance map: session scratchpad
  `ctxmenu-scout-map.md` (desktop) and the port map produced 2026-07-29
  (external tools / PDFium / fidelity gaps / scripts, with per-item crate
  candidates, effort and risk). Ledger: `rust/PORT_STATUS.md`.
- Codex runs as dev via `codex exec --sandbox workspace-write -C <worktree>`
  (the plugin's own sandbox path is broken on this host); the OS sandbox
  confines writes to the worktree. Claude runs the independent tester.

## Scope

In scope (ordered; see Approach):

- Availability truthfulness for every dependency group.
- Native replacements where a credible pure-Rust route exists:
  `pdftohtml` → Rust; CCITTFax decoding; two Ghostscript call sites
  (colour-space conversion, `removeImagesAfter`); the fixed-template
  `create-pdf-from-html-agent` renderer; PDF→EPUB; an additive `ocrs` OCR
  fallback; an embedded font-program metrics parser.
- Ledger/contract truthfulness for everything touched.

Out of scope (decided, with reasons recorded in the map):

- LibreOffice, WeasyPrint (general HTML), Calibre ebook→PDF, veraPDF, RAR
  read/write, JPEG 2000, true Type3 glyph synthesis, FFmpeg, Ghostscript
  vector output, PDFium rasterisation — no viable pure-Rust route exists, or
  the cost/benefit is indefensible. `convert/pdf/cbr` is a deprecation
  candidate (the RAR compressor is proprietary and unpublishable).
- Frontend build scripts and all UI code.

## Approach

Sequential batches of one or two parallel work-items, each in its own git
worktree, each as Codex-dev + Claude-tester with fix rounds. Order:

1. **WI-1 dependency truthfulness** (+ DeviceN DCT >4 probe) — prerequisite:
   every later removal is measured by "does availability still report the
   truth". Branch `port/dependency-truthfulness`.
2. **WI-2 `pdftohtml` → pure Rust** — first item that actually deletes an
   external binary. Branch `port/pdftohtml-rust`.
3. **WI-3 bundle qpdf + Tesseract into the desktop app** (maintainer-approved
   2026-07-29) — a fresh desktop install must repair PDFs and OCR without the
   user installing anything. Pinned prebuilt binaries + `eng.traineddata`
   staged into `resources/tools/`, wired through the sidecar env with the
   PDFium precedence rule. Prompt prepared:
   scratchpad `codex-wi3-bundle-tools.txt`. Queued behind WI-1/WI-2 because
   two parallel Rust builds already hold ~95 GB and saturate the host.
4. **WI-4 remove Ghostscript entirely** (maintainer 2026-07-29: "I don't need
   pdfa, don't need pdf eps", and confirmed the EPS→PDF import direction goes
   too). Delete `convert/pdf/pdfa` and both vector directions (+ their
   frontend tools); port `misc/replace-invert-pdf` COLOR_SPACE_CONVERSION →
   `moxcms` and OCR `removeImagesAfter` → `lopdf`; drop the `gs` tier from
   compress (level ≥ 6), repair, and ebook optimisation; tear out the
   dependency group, the Dockerfile package, and the CI installs — while
   still ACCEPTING and ignoring the legacy Ghostscript config keys. Prompt
   prepared: scratchpad `codex-wi4-drop-ghostscript.txt`.
   **Open sub-decision inside WI-4:** `general/crop` with
   `removeDataOutsideCrop=true` is a *privacy promise* — `gs` genuinely
   removes content outside the crop box, whereas the existing lopdf fallback
   only sets `/CropBox`, which hides but does not delete it. Codex is
   instructed to either implement real content removal in Rust or refuse the
   flag honestly, never to silently under-deliver.
5. Then, re-prioritised: CCITTFax decoder (`fax` crate); poppler-cMap
   vendoring; fixed-template HTML agent renderer (`krilla`); PDF→EPUB
   (`epub-builder`); embedded font-metrics parser; additive `ocrs` OCR tier.

## Risks And Recovery

- Codex is unsupervised inside its worktree: the independent Claude tester
  grades the committed diff, never the report, and reruns every gate.
- Availability changes are user-visible (the SPA drives tool enablement from
  `endpoints-availability`): WI-1 must add tests pinning the new truth.
- Native replacements must not regress: keep the external tool preferred where
  it exists, ship the Rust path as the fallback first.
- Recovery: each work-item is an unmerged branch; `main` stays releasable
  (v3.1.0). Codex prompts and results are kept in the session scratchpad
  (`codex-wi*-{deps,pdfhtml,result,log}.txt`) so a killed run can be re-issued
  verbatim.

## Progress

- [x] Reconnaissance map complete (external tools, PDFium, fidelity gaps,
      scripts) with per-item feasibility and a not-worth-attempting list.
- [x] Codex-as-dev invocation proven and isolated to a worktree.
- [ ] WI-1 dependency truthfulness: Codex dev running.
- [ ] WI-1 tester sign-off.
- [ ] WI-2 pdftohtml → Rust: Codex dev running.
- [ ] WI-2 tester sign-off.
- [ ] Merge + CI + ledger update.

## Decisions

- 2026-07-29: Codex is the dev, Claude the tester/PM (maintainer directive).
- 2026-07-29: UI and frontend build tooling are permanently out of scope.
- 2026-07-29: PDFium stays as a bundled BSD-3 native runtime — replacing it is
  a Chromium-scale project; only its text-geometry family is worth porting.
- 2026-07-29: **desktop tool bundling — approved for qpdf + Tesseract only**
  (Apache-2.0, ~+25-40 MB per platform). **Ghostscript bundling rejected**:
  it is AGPL-3.0-or-commercial and the maintainer declined the licence
  entanglement ("if it needs a licence, move on to another solution"), so the
  Ghostscript-dependent endpoints stay operator-provisioned and the product
  instead moves its cheap call sites to pure Rust (Approach step 4). Consequence
  recorded honestly: `convert/pdf/pdfa` (PDF/A) and `convert/pdf|vector`
  (EPS/PS) have no cheap Rust route — PDF/A via `krilla` is an L-XL subsystem
  and no pure-Rust PostScript writer exists — so those two remain
  "install Ghostscript yourself", exactly as today.
- 2026-07-29: two shipped-bug suspicions to confirm during WI-1: `pdftohtml`
  is falsely claimed as a `pdf-to-markdown` dependency, and `convert/cbr/pdf`
  may be broken in the Docker image (7-Zip cannot read RAR; non-free `unrar`
  is deliberately excluded).

## Feature-parity target (added 2026-07-29)

Maintainer directive: "in general, I want all functions of iLovePDF."
Measured against iLovePDF's published catalogue (30 tools), RustlingPDF
already covers **27**, plus roughly 28 tools iLovePDF does not have (booklet
imposition, stamps, attachments, table of contents, EML→PDF, extract image
scans, auto-redact, auto-rename, flatten, hardware cert-sign, timestamping,
signature validation, pipeline automation, …).

Gaps and their resolution:

- **PDF→PDF/A** — iLovePDF has it; maintainer chose to **delete it anyway**
  (2026-07-29, re-confirmed after the parity comparison) because it requires
  AGPL Ghostscript. Accepted parity gap of 1 tool.
- **Translate PDF** — the only genuinely missing tool. It needs a translation
  engine, i.e. it would ride on `rustling-ai-engine`. This is also the
  concrete answer to "why do we need the AI engine": it powers Translate PDF
  and AI-Summarizer-style features, both of which iLovePDF ships.
- **AI Summarizer** — reachable today through the chat/orchestrate surface
  when the AI engine is enabled (off by default).
- **Office conversions** (Word/PowerPoint/Excel ↔ PDF, 5 tools) and
  **HTML→PDF** — the endpoints exist but need LibreOffice / WeasyPrint, which
  are not bundled. Maintainer decision 2026-07-29: **"for now, skip that
  function"** — deferred, not deleted. The endpoints stay; desktop users see
  them as dependency-disabled; the Docker image keeps working. Revisit if
  out-of-the-box Office conversion becomes a priority (the only route is
  bundling LibreOffice, ~+450 MB, MPL-2.0 — no pure-Rust option exists).

## Validation

- Focused proof: per-work-item unit + endpoint tests, plus the boot/curl proof
  that availability and behaviour match the claim.
- Integration proof: full `cargo test --workspace --locked` with PDFium bound;
  `task frontend:check` when any TS is touched; CI green after merge.
- Repository-required checks: contracts and `rust/PORT_STATUS.md` updated in
  the same change as the behaviour.

## Result

(Complete after the programme's first batch lands; this plan stays active
across batches and records each one.)
