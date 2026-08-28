# Execution Plan: Optional PaddleOCR backend

Date: 2026-08-28

## Status

Active — implemented, tested, and documented, and scoped to developer-only
builds by maintainer decision. Remaining work is the model-backed smoke test,
which needs operator-supplied artifacts.

## Outcome

RustlingPDF can create searchable OCR PDFs with the pinned `PaddleOCR-Rust`
port when an operator explicitly selects that engine and supplies all local
runtime, model, dictionary, and text-layer font paths. Existing OCRmyPDF and
Tesseract behavior remains the default, and no Paddle model or ONNX Runtime
binary enters an installer.

## Context

- `rust/contracts/ocr-pdf.md` owns the observable endpoint behavior.
- `rust/contracts/runtime-config.md` owns operator-visible configuration.
- `rust/crates/rustling-processing/src/ocr_pdf.rs` owns engine selection and
  searchable-PDF assembly.
- `rust/crates/rustling-processing/src/pdfium_backend.rs` owns bounded PDFium
  rendering and page construction.
- `PaddleOCR-Rust` is the maintainer-provided local inspection symlink; shipped
  code must depend on the exact public Git revision rather than that absolute
  path.

## Scope

In scope:

- Add a pinned, feature-gated `PaddleOCR-Rust` dependency to the sidecar.
- Add explicit opt-in configuration with local paths only.
- Reuse one serialized Paddle engine session across requests.
- Render selected PDF pages, recognize them, and create raster-plus-invisible-
  text searchable pages before ordered merge.
- Preserve default OCRmyPDF/Tesseract behavior and endpoint response shapes.
- Add configuration, geometry, searchable-layer, and unchanged-default tests.

Out of scope:

- Bundling or downloading ONNX Runtime, models, dictionaries, or fonts.
- Replacing OCRmyPDF or Tesseract as the default engine.
- Claiming languages or platforms beyond the pinned port evidence.
- Implementing Paddle equivalents for OCRmyPDF-only cleanup switches.

## Approach

1. Introduce a `paddle-ocr` processing feature enabled by the shipped CLI and
   pin the Git dependency to revision `74359e2ba04bbd6695d923b4529bf4aa67249e70`.
2. Parse `ocr.engine: paddle` plus five explicit artifact/font paths. Default
   selection stays `auto`, which runs the existing path unchanged.
3. Lazily load and serialize one `OcrEngine`, verify the selected model and
   dictionary identities, and reuse sessions across requests.
4. Reuse PDFium's bounded page rendering. For OCR-selected pages, create a new
   page with the rendered scan as its background and CID-font invisible text
   positioned from Paddle quadrilaterals; retain skipped source pages unchanged.
5. Update contracts and execute focused checks.

## Risks And Recovery

- PDF pixel coordinates use a top-left origin while PDF points use bottom-left;
  pure geometry tests and a PDFium extraction test pin the transform.
- `OcrEngine` is not `Sync`; a mutex owns one reusable engine and serializes
  inference rather than sharing sessions concurrently.
- Font coverage controls whether PDFium can encode recognized Unicode. The font
  path is therefore explicit and required instead of silently choosing a host
  font.
- Rollback is one coherent revert: remove the feature/dependency, Paddle config,
  backend branch, PDFium page writer, and contract additions. Existing default
  behavior has no migration or persisted state.

## Progress

- [x] Confirm product choice: optional external artifacts, existing default kept.
- [x] Inspect Paddle API, artifact policy, resource evidence, and Rustling OCR contract.
- [x] Add the pinned optional dependency and the first configuration implementation.
- [x] Add the first reusable-engine and searchable-page implementation.
- [x] Resolve the compiler warnings and the invalid-engine availability
      edge case.
- [x] Add focused feature-on and feature-off configuration/selection tests.
- [x] Prove the searchable text layer with a PDFium extraction test.
- [x] Add OCR orchestration tests for the default path, Paddle selection,
      request shape, page selection, and no-silent-fallback.
- [x] Update `rust/contracts/ocr-pdf.md`,
      `rust/contracts/runtime-config.md`, and the settings template.
- [x] Run formatting, both feature states, and the shipped CLI check.
- [ ] **Blocked on a maintainer decision:** the feature reaches no shipped
      artifact (see "Shipped-artifact gap"). Decide that before release-impact
      measurement, because the measurement only has a subject if the answer is
      to ship it.
- [ ] Model-backed smoke test (needs the five operator paths).

## Handoff Snapshot

The working tree is deliberately uncommitted. It is on `main` at base commit
`6324ae8cad4019ddab8f90e4a4631d2752b4daf5`.

Do not add or modify the untracked `PaddleOCR-Rust` symlink. It points to
`/mnt/ssdvolumes/repo/PaddleOCR-Rust`, is maintainer-owned inspection state, and
must never enter the RustlingPDF commit.

Current implementation surfaces:

- `rust/crates/rustling-processing/Cargo.toml` adds the optional `paddle-ocr`
  feature and pins `paddleocr-rust` to public revision
  `74359e2ba04bbd6695d923b4529bf4aa67249e70` with its `onnxruntime` feature.
- `rust/crates/rustling-cli/Cargo.toml` enables that feature. **Correction:**
  an earlier revision of this plan called `rustling-cli` "the shipped sidecar".
  It is not; no release path builds it, so the feature reaches no shipped
  artifact — which is now the recorded intent. See "Distribution Scope".
  Runtime activation is opt-in either way, so the existing engine remains the
  default.
- `rust/Cargo.lock` contains the pinned Paddle dependency plus `ort`,
  `ort-sys`, `ndarray`, and `jpeg-decoder`. Review the lock diff; do not update
  unrelated packages.
- `rust/crates/rustling-processing/src/paddle_ocr.rs` is new and untracked. It
  parses the five artifact paths into a service state, validates the pinned
  detector/recognizer/dictionary identities, lazily creates one `OcrEngine`,
  serializes inference through a mutex, and maps recognized quadrilaterals.
- `rust/crates/rustling-processing/src/runtime_config.rs` parses
  `ocr.engine: auto|paddle`, five YAML path fields, and matching environment
  overrides. Explicit incomplete Paddle configuration fails instead of falling
  back.
- `rust/crates/rustling-processing/src/lib.rs` creates and shares the Paddle
  service with the OCR handler and exposes the OCR endpoint when Paddle is the
  selected usable path.
- `rust/crates/rustling-processing/src/ocr_pdf.rs` selects Paddle only when
  explicitly configured, reuses PDFium page rendering and skip-text selection,
  recognizes rendered pages, writes searchable pages, merges them in order,
  and emits recognized lines to the optional sidecar text file.
- `rust/crates/rustling-processing/src/pdfium_backend.rs` now returns rendered
  page dimensions and can create a raster page with invisible CID-font text
  objects positioned from Paddle quadrilaterals. The geometry conversion has a
  pure unit test, and PDF text extraction is now proven against the pinned
  PDFium runtime.

Observed dependency facts that constrain the implementation:

- The Paddle port returns text plus quadrilaterals; RustlingPDF must create the
  searchable PDF layer itself.
- The pinned detector and recognizer total about 132 MiB, before ONNX Runtime
  (about 30 MiB on the inspected Linux checkout). None may be bundled or
  downloaded by the application or release workflow.
- A user-provided text-layer font is required. Do not silently select a host
  font because broad Unicode/CJK coverage is not guaranteed.
- `OcrEngine` is reused serially because its session type is not safely shared
  for concurrent inference under the inspected API.

## Distribution Scope

**Decision (2026-08-28, maintainer): Paddle stays developer-only for now.** It
is available in a build that enables the `paddle-ocr` feature, and in no
shipped artifact. This is intent, not an oversight.

The earlier handoff recorded that `rust/crates/rustling-cli/Cargo.toml`
"enables that feature in the shipped sidecar". That description was wrong, and
correcting it is what prompted the decision. Verified on 2026-08-28:

- The desktop sidecar is built by `.taskfiles/desktop.yml:26` and
  `.github/workflows/desktop-build.yml:238`, both
  `cargo build --release --locked -p rustling-processing`.
- The Docker runtime image builds `-p rustling-processing --bin
  rustling-processing` and `-p rustling-ai-engine` (`docker/Dockerfile:53`).
- `rustling-cli` is a workspace member run locally through `task rust:cli`
  (`.taskfiles/rust.yml:57`). No release, packaging, or installer path builds
  it.
- `cargo tree -p rustling-processing -i paddleocr-rust` reports
  `did not match any packages`; the same command with `--features paddle-ocr`
  resolves it. Cargo only unifies features across packages that are in the
  build, so a feature enabled by `rustling-cli` does nothing for a
  `-p rustling-processing` build.

So `ocr.engine: paddle` on the desktop app or the Docker image returns "this
RustlingPDF build does not include the paddle-ocr feature", and that is the
correct answer for those builds today.

Why this is the right default for now: the engine has never run against a real
model, so there is nothing yet worth paying for. Shipping it would pull `ort`,
`ndarray`, and `jpeg-decoder` into a binary that is compiled three times per
release — the Windows leg, the Linux leg, and the Docker image — which cuts
directly against the release budget in
`docs/plans/active/build-speed-and-app-size.md`.

### If this is revisited

Enable `paddle-ocr` for the sidecar, either as a `default` feature of
`rustling-processing` or with `--features paddle-ocr` in the three build
commands above. Then measure rather than infer: release sidecar size,
**compressed** installer size, and release wall clock, each with and without
the feature. Read the compressed figure — the raw byte delta overstates the
download by roughly eightfold, as that plan records. Do this only after a
model-backed test proves the engine works end to end.

## Known Gaps And Issues

Resolved since the previous handoff:

- All three compiler warnings are fixed. `PaddleOcrError` is now `pub`, matching
  how `PdfiumOcrError` is exposed through the same private module;
  `validated_options` is deleted and its tests call the two split validators;
  `NotCompiled` is `#[cfg(not(feature = "paddle-ocr"))]`. Three further
  feature-off dead-code warnings (the loader-only constants) were found and
  gated the same way. Clippy is clean on `--all-targets` in both feature states.
- Invalid explicit engine selection is observable. Any non-`auto` `ocr.engine`
  keeps `ocr-pdf` advertised, so a typo surfaces as its own configuration error
  instead of `501 Not Implemented` blaming an absent OCRmyPDF.
- An invalid or incomplete Paddle configuration is now resolved at the top of
  `run_paddle_fallback`, before rendering. Previously a host without PDFium
  reported `PdfiumUnavailable` for what was really a configuration mistake.
- `load_true_type_from_bytes(..., true)` has executable proof:
  `paddle_searchable_page_extracts_its_invisible_text` builds a raster page with
  a `Hello` line using the repository DejaVu font, reopens it with PDFium,
  extracts `Hello`, and asserts render mode `Invisible`. It passed against the
  pinned PDFium runtime.
- The `languages`, `ocrRenderType`, cleanup-switch, sidecar, and rasterization
  semantics are now stated in `rust/contracts/ocr-pdf.md`.

Still open, and honestly bounded:

- Paddle page ordering under `skip-text` is proven only at the selection rule
  (`ocr_page_selection_mode`) and by sharing `try_prepare_tesseract_pages` and
  `merge_pdf_paths_to_file` with the Tesseract fallback, whose ordering has an
  end-to-end test. An end-to-end Paddle ordering test needs the operator models
  and cannot run in CI. Do not claim more than this.
- CJK coverage is unproven. DejaVu is an English fixture. The dictionary
  identity check pins which characters may be recognised, but nothing verifies
  that an operator's chosen font can encode them; a font without the glyphs
  produces a page whose text does not extract.
- The five configured paths are not checked for existence at startup. A wrong
  path fails on the first OCR request, not at boot.
- ONNX Runtime is a caller-supplied platform artifact and is not digest pinned.
  Do not claim the external bundle is distributable or supply-chain closed.
- No model-backed recognition has ever run. Every Paddle test stops at or
  before artifact loading.

## Next Agent Runbook

Steps 1 to 6 of the original runbook are complete, and distribution scope is
settled; see "Validation" and "Distribution Scope". What is left:

1. Re-read `AGENTS.md`, `docs/WORKFLOW.md`, this plan, the two OCR contracts,
   and the current diff. Stay on `main` and preserve unrelated/untracked state.
   Do not add, modify, or commit the untracked `PaddleOCR-Rust` symlink.
2. If the maintainer provides the five external paths, run one model-backed
   smoke test on a small fixture. Do not download, copy, package, or commit the
   external artifacts merely to make this test run. A fixture covering the
   recognised characters, with a CJK-capable font, would also close the
   coverage gap; the maintainer's `PaddleOCR-Rust` checkout has such fonts, and
   they must stay outside this repository.
3. Do not enable the feature for the sidecar without a fresh maintainer
   decision. "Distribution Scope" records why, and what to measure if it is
   ever revisited.
4. Review `git status`, ensure the symlink is still untracked, update `Result`,
   and move this plan to `docs/plans/completed/` once the smoke test has run or
   the maintainer accepts it as permanently out of CI scope.
5. Commit or push only when the maintainer explicitly authorizes it. Nothing in
   this working tree has been committed.

## Decisions

- 2026-08-28: The maintainer selected the optional external-artifact approach;
  bundling/replacing Tesseract is excluded.
- 2026-08-28: Activation is explicit (`ocr.engine: paddle`). A partially
  configured Paddle engine fails clearly and never silently falls back.
- 2026-08-28: A local TrueType/OpenType font path is required because the
  selected dictionary contains Unicode not covered by PDF Standard 14 fonts.
- 2026-08-28: Any non-`auto` `ocr.engine` keeps the `ocr-pdf` endpoint
  advertised. An invalid value must fail as its own configuration error;
  answering `501 Not Implemented` would blame a missing OCRmyPDF for a typo.
  Recorded in both contracts.
- 2026-08-28: Paddle configuration is resolved before any rendering work, so a
  misconfigured engine reports the configuration error rather than whatever the
  renderer says first. This is why `run_paddle_fallback` takes the font path up
  front.
- 2026-08-28: Paddle is developer-only. It ships in no installer or image; the
  feature must be enabled at build time. Chosen because the engine has never
  run against a real model, and shipping it would add `ort`/`ndarray` to a
  sidecar compiled three times per release. See "Distribution Scope".
- 2026-08-28: `ocr.paddle.*` is documented in
  `rust/crates/rustling-processing/resources/settings.yml.template`, which is
  the operator-facing sample config and already carries `tessdataDir` and the
  OCR process limits. Defaults are inert (`engine: auto`, empty paths).

## Validation

- Focused proof: runtime-config unit tests; OCR selection/validation tests;
  coordinate transform tests; PDFium searchable-page extraction test.
- Integration or end-to-end proof: model-backed OCR is conditional on the
  operator-provisioned artifacts and cannot run in ordinary CI.
- Repository-required checks: `cargo fmt --all --check`, focused `cargo test`,
  and `cargo check -p rustling-cli` from `rust/`.

Use the system compiler path explicitly in this environment. The default
`/home/hairbui76/.local/bin/cc` is another CLI, not a C compiler:

```sh
cd rust
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin cargo fmt --all --check
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin cargo check -p rustling-processing
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin cargo test -p rustling-processing
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin \
  cargo clippy -p rustling-processing --all-targets
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin \
  cargo check -p rustling-processing --features paddle-ocr
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin \
  cargo test -p rustling-processing --features paddle-ocr
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin \
  cargo clippy -p rustling-processing --all-targets --features paddle-ocr
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin cargo test -p rustling-processing --bins
PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin cargo check -p rustling-cli
```

The PDFium proof needs the pinned runtime. Without it the test skips, which is
why it fails loudly instead when the variable *is* set:

```sh
RUSTLING_PDFIUM_LIBRARY_PATH=/mnt/ssdvolumes/repo/RustlingPDF/rust/.pdfium/current/libpdfium.so \
  PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin \
  cargo test -p rustling-processing --lib \
  paddle_searchable_page_extracts_its_invisible_text
```

Validation observed on 2026-08-28, all from `rust/` with
`PATH=/usr/bin:/bin:/home/hairbui76/.cargo/bin`:

| Command | Result |
|---|---|
| `cargo fmt --all --check` | pass, no diff |
| `cargo clippy -p rustling-processing --all-targets` | pass, zero warnings |
| `cargo clippy -p rustling-processing --all-targets --features paddle-ocr` | pass, zero warnings |
| `cargo test -p rustling-processing --lib` | 561 passed, 0 failed |
| `cargo test -p rustling-processing --lib --features paddle-ocr` | 561 passed, 0 failed |
| `cargo test -p rustling-processing --bins` | 15 passed, 0 failed (settings-template merge) |
| `cargo test -p rustling-processing --test desktop_startup_smoke` | 3 passed, 1 ignored |
| `cargo test -p rustling-processing --test convert_from_pdf --test service` | 32 + 30 passed, 0 failed (OCR endpoint availability contract) |
| `cargo check -p rustling-cli` | pass |

The PDFium proof ran against the pinned runtime with
`RUSTLING_PDFIUM_LIBRARY_PATH=<repo>/rust/.pdfium/current/libpdfium.so`:

```
test pdfium_backend::tests::paddle_searchable_page_extracts_its_invisible_text ... ok
```

Paddle-related tests, all passing in both feature states:

- `paddle_ocr::tests::disabled_service_does_not_select_paddle`
- `paddle_ocr::tests::invalid_service_reports_configuration_before_loading`
- `paddle_ocr::tests::configured_service_exposes_the_operator_supplied_font`
- `paddle_ocr::tests::feature_off_build_reports_the_missing_feature_for_complete_configuration`
  (feature off) / `..::feature_on_build_reaches_artifact_loading_for_complete_configuration`
  (feature on)
- `runtime_config::tests::paddle_ocr_is_disabled_by_default_and_requires_every_explicit_path`
- `runtime_config::tests::paddle_ocr_yaml_keeps_all_paths_explicit_and_enables_ocr_availability`
- `runtime_config::tests::invalid_ocr_engine_is_reported_instead_of_falling_back`
- `ocr_pdf::tests::default_engine_still_rejects_languages_without_installed_tessdata`
- `ocr_pdf::tests::explicit_paddle_does_not_require_installed_tessdata`
- `ocr_pdf::tests::explicit_paddle_keeps_request_shape_validation`
- `ocr_pdf::tests::invalid_paddle_configuration_never_falls_back`
- `ocr_pdf::tests::paddle_selects_pages_with_the_tesseract_fallback_rule`
- `pdfium_backend::tests::paddle_geometry_maps_top_left_pixels_to_bottom_left_pdf_points`
- `pdfium_backend::tests::paddle_searchable_page_extracts_its_invisible_text`

Not run, and not claimed:

- Model-backed OCR. It needs the operator's ONNX Runtime, models, and
  dictionary, which are deliberately absent from the repository and from CI.
- Release-artifact size and CI-time measurement. Deliberately not attempted,
  and not needed: the feature is in no shipped artifact by decision (see
  "Distribution Scope"), so the installer and release-time delta are zero. The
  supporting fact is a dependency-graph one, not a size inference:
  `cargo tree -p rustling-processing -i paddleocr-rust` does not resolve.

## Acceptance Criteria

- Default `auto` behavior is observably unchanged.
- Explicit valid Paddle configuration creates an ordered searchable PDF and
  never invokes OCRmyPDF/Tesseract.
- Explicit invalid/partial Paddle configuration fails clearly and never falls
  back.
- The processing crate builds and tests with and without `paddle-ocr`; the
  shipped CLI builds with the feature enabled.
- PDFium extraction proves the produced text layer is searchable and rendering
  proof shows the invisible text does not visibly duplicate the raster.
- All operator-visible configuration and limitations are documented in the
  repository contracts.
- No model, dictionary, font, ONNX Runtime library, local symlink target, or
  local absolute path is included in the commit or release resources.
- Installer-size and CI-time statements are backed by measurements rather than
  inferred from dependency structure.

## Result

The implementation is complete, warning-free, tested in both feature states,
and documented in the contracts. It remains uncommitted on `main`.

What is proven: configuration parsing and its failure modes; that `auto` is
untouched; that explicit Paddle skips the tessdata gate while keeping request
validation; that a misconfigured engine fails as a configuration error before
any rendering and never falls back; the pixel-to-point transform; and — against
the real PDFium runtime — that the produced text layer extracts and is
invisible.

What is not: any model-backed recognition, CJK font coverage, end-to-end page
ordering on the Paddle path, and release impact.

Distribution scope is settled: Paddle is developer-only, reaching no installer
or image, so the release impact is zero and there is nothing to measure. The
plan stays active only for the model-backed smoke test, which needs artifacts
the maintainer has not supplied and which cannot run in CI.

Nothing has been committed or pushed. The untracked `PaddleOCR-Rust` symlink
is untouched and no external artifact entered the repository.
