# RustlingPDF Roadmap

Living planning document. A fresh working session should read this file plus
`CLAUDE.md` (working rules) and `rust/PORT_STATUS.md` (the authoritative
feature/parity ledger) to get fully oriented. Update this file whenever a batch
lands or the queue changes.

## Snapshot (2026-07-28)

- Repo: standalone product based on Stirling-PDF; **no Java anywhere** — the
  upstream checkout (if present at `../Stirling-PDF`) is a read-only reference
  oracle only.
- `main` — full quality gate green after batch 4:
  `cargo fmt`/`clippy -D warnings` clean, **stirling-processing 1535 passed /
  0 failed** (1 ignored), **stirling-ai-engine 144 / 0**; Tauri desktop-shell
  gate (containerized webkit build: fmt/clippy/tests) **11 / 0**; frontend
  typecheck/eslint clean, **1647 vitest passed**, `vite build` (core) succeeds;
  differential rust-only smoke **13/13**; actionlint clean across all seven
  workflows; `latest.json` composer fixture suite **21/21**; containerized
  Linux signed-upgrade e2e **8/8** (incl. negative-signature tests). Ships
  GitHub CI, single-binary SPA serving, a Docker image, a tag-driven release
  pipeline (GHCR images + signed desktop bundles + updater manifest), and a
  Rust-sidecar desktop shell.
- Open mode is the supported runtime; secured mode is implemented + tested but
  **fail-closed** pending independent human security review.
- Canonical app version lives in `rust/VERSION` (consumed by `build.rs`).
- Verified quick start: `task rust:install && task dev`.

## Landed — Batch 5 (2026-07-28, merged to `main`)

Two dev+tester pairs (desktop release publishing; signed-upgrade e2e proof).
The desktop-release pair signed off round 0; the e2e pair's only major was a
proof-of-run gap (three tester rounds were force-finalized mid-run, never a
harness failure) which the PM discharged by completing the canonical runs.

- **Desktop release publishing**: `desktop-build.yml` (reusable
  `workflow_call` three-OS matrix: linux-x86_64 AppImage+deb, windows-x86_64
  msi, darwin-aarch64 app.tar.gz+dmg; signing via the repo secrets),
  `desktop-release-dryrun.yml` (`workflow_dispatch` proof-run, no
  publishing), `release.yml` gains `publish-desktop` and uploads bundles +
  `.sig` files + `latest.json` to the GitHub release.
  `scripts/compose_latest_json.py` composes the updater manifest — schema
  and per-OS artifact choice derived from the vendored
  `tauri-plugin-updater 2.10.1` source (incl. installer-specific keys like
  `linux-x86_64-deb`; signature = verbatim `.sig` content), unit-tested
  (`compose_latest_json_test.py`, incl. PM-added strict-RFC-3339 and
  stray-file cases). Windows staging: `desktop:stage-sidecar` dispatches to
  `install-pdfium.ps1` on Windows (layout traced to
  `pdfium_runtime.rs` dll resolution); Linux/mac path byte-identical.
- **Signed-upgrade e2e proof (Linux leg)**: containerized harness
  (`run-e2e-container.sh`, only Docker required) proved **8/8** twice plus a
  `--skip-build` determinism rerun: v0.0.1 AppImage with the real Rust
  sidecar detects a signed v99.0.0 update; same-version and downgrade
  manifests refused; wrong-key signature and byte-tampered artifact
  **rejected with the exact expected reasons** (AppImage untouched); the
  good update installs (sha256-asserted on-disk replacement) and the
  relaunched app reports 99.0.0. Contract updated
  (`desktop-native-startup.md`); evidence lands in `.e2e-work/evidence/`.
- **PM pass**: composer validation tightened (strict RFC 3339 `pub_date`,
  stray top-level files rejected), e2e startup probes bounded
  (update-server + Xvfb retries), ledger/contract sync.

**Real-runner proof (PM dry-runs, 2026-07-28)**: the full signed matrix is
green in a single `desktop-release-dryrun` run (actions/runs/30332343674 —
linux-x86_64 + windows-x86_64 + darwin-aarch64, artifacts 237/78/159 MB
incl. `.sig` files; the Windows leg is also the first-ever Windows MSVC
build of the Rust backend). Three real-runner defects were found and fixed
along the way: Git Bash's GNU tar hijacking `tar` in `install-pdfium.ps1`
(drive letter parsed as hostname → now calls `System32\tar.exe`),
linuxdeploy tool deps missing on ubuntu-24.04 (package set mirrored from
the proven e2e container + `--verbose` kept so bundler errors surface),
and appimagetool treating the workflow's `SIGN` env flag as a GPG-signing
request (renamed to `UPDATER_SIGN`).

Known scope limits (recorded in RELEASING.md): macOS is Apple-silicon only
and unnotarized; Windows ships the WiX `.msi` only; macOS/Windows
upgrade-proof legs are runner work still open.

## Landed — Batch 4 (2026-07-28, all tester-signed, merged to `main`)

Three dev+tester pairs (release pipeline, Tauri Rust sidecar, recorded-minors
bundle); release and tauri signed off round 0, minors after one fix round.

- **Release pipeline** (`.github/workflows/release.yml`, `RELEASING.md`,
  `.github/dependabot.yml`): tag-driven (`v<version>`, exact-match guard
  against `rust/VERSION` + `tauri.conf.json` cross-check), publishes both
  Docker targets to GHCR (`ghcr.io/hairbui76/rustlingpdf{,-ai-engine}`,
  `v<version>` + `latest`, OCI version/source/revision/created labels via
  Dockerfile ARGs, buildx `type=gha` cache), `gh release create` with pull
  commands — no new third-party actions beyond three SHA-pinned docker/*
  ones (pins verified against upstream tags by dev and tester
  independently). Dependabot: weekly grouped minor+patch for six ecosystems
  (backend cargo, three src-tauri cargo roots, npm, github-actions).
  VERSION-bump checklist documents all six hard-coded `2.14.2` sites.
- **Tauri desktop → Rust sidecar**: the Rust backend is the packaged
  `externalBin` sidecar and the Java JRE/JAR launch path is deleted;
  `STIRLING_NATIVE_BACKEND_PATH` demoted to dev override; bundled-PDFium
  wiring via `STIRLING_PDFIUM_LIBRARY_PATH` (operator override respected);
  env contract otherwise preserved verbatim; `task desktop:stage-sidecar`
  stages binary + PDFium; new house-style desktop CI gate
  (fmt/clippy/test with webkit deps); dev-update-test scripts reworked to
  the Rust flow (dead jlink/JRE scripts deleted); updater endpoint
  repointed off upstream Stirling-Tools releases to this repo;
  `waitForPort` bounded (120 s). Proven on this host: staged sidecar
  handshake + real PDFium rotate through the bundle layout, and the full
  src-tauri gate compiled/tested in a webkit container.
- **Recorded-minors bundle** (all upstream-verified to bytecode level by
  the tester): flow-styled-root `settings.yml` refused like flow sections
  (file untouched, identity fails open ephemeral); admin/license settings
  persistence migrated off the serde round-trip onto the comment-preserving
  `settings_yaml` editor with a new nested dotted-path upsert + pre-write
  reparse-and-leaf-read-back proof (block-scalar/block-sequence shapes
  refused; CRLF-safe; Java-subtree-replace divergences documented in
  `contracts/admin-settings.md`); OIDC cookie-clear now byte-exact to
  Spring `ResponseCookie#toString` including `Expires=Thu, 01 Jan 1970
  00:00:00 GMT` (verified against disassembled spring-web 7.0.7); inline
  `DecodeParms` Real values truncate with PDFBox `COSNumber.intValue`
  `f2i` semantics (DCT `/ColorTransform` deliberately stays on the PDF.js
  integer-only oracle — divergence documented in `contracts/pdf-json.md`).
- **PM integration pass**: dependabot coverage widened to the standalone
  `provisioner`/`thumbnail-handler` crates; RELEASING.md grep-audit wording
  fixed + old-tag re-dispatch `latest`-regression warning added;
  root-detector doc comments state the `---` document-start limitation
  (callers' reparse proofs cover it — verified no corruption reachable);
  conflicting duplicate-leaf admin batches documented as fail-closed
  divergence in `contracts/admin-settings.md`; stale OIDC-callback-UX
  paragraph in PORT_STATUS corrected.

Recorded follow-ups (see queue item 1): the updater keypair was regenerated
same-day (repo-controlled, upstream pubkey replaced); `install-pdfium.ps1`
is still not wired into `desktop:stage-sidecar` (no Windows bundle until it
is). The license question was resolved by maintainer decision (2026-07-28):
**the product is MIT** — `rust/Cargo.toml`'s workspace `AGPL-3.0-or-later`
was a port-era error and now reads `MIT` (matching the root `LICENSE` and
the Docker image label), and the batch-4 report's claim that the
frontend carve-out LICENSE files are missing was wrong — all seven exist
in-tree (the root LICENSE's `app/*`/`engine/` clauses are conditional on
directories this repo does not have).

## Landed — Batch 3 (2026-07-28, all tester-signed, merged to `main`)

All four work-items delivered by dev+tester pairs and merged; the PM's combined
gate then caught a cross-item defect that a follow-up fix pair closed.

- **GitHub CI** (`.github/workflows/`): backend gate (fmt/clippy/tests + PDFium
  install), frontend gate (typecheck/lint/vitest/build), differential rust-only
  smoke. Action pins byte-for-byte match upstream; actionlint-clean.
- **Single-binary SPA serving**: config-gated static layer behind
  `STIRLING_FRONTEND_DIST` porting `ReactRoutingController` semantics
  (traversal/symlink-safe, `/api` precedence, deep links, cache policy);
  unset ⇒ today's Vite-proxy dev flow unchanged. Contract:
  `rust/contracts/spa-serving.md`.
- **Docker packaging**: multi-stage image (Rust release + frontend dist +
  PDFium + external tools), **~1.41 GB main / 116 MB ai-engine**, compose
  example, `task docker:*`; built and container-smoke-tested locally
  (bit-identical rebuild by the tester).
- **Parity trio**: AI-engine config push (`AiEngineConfigSync` parity, bounded
  off-thread retry), AES-256-GCM at-rest encryption for integration/S3
  credentials with lossless lazy migration, and `AutomaticallyGenerated`
  UUID/key install-identity persistence.
- **Follow-up fix pair** (found by the combined gate): the identity write was
  stripping the settings comment banner and using the wrong version, and — a
  pre-existing bug — an empty `custom_settings.yml` blanked the whole snapshot
  so desktop re-rolled UUID/key every boot. Fixed: empty YAML documents merge
  as a no-op, identity writes are comment-preserving and use the canonical
  `rust/VERSION`, flow-collection sections are refused rather than destroyed,
  and the desktop smoke test now asserts byte-stable settings across two boots.

Both minors recorded here (flow-styled-root settings.yml corruption; admin
license-persist serde round-trip dropping comments) were fixed in batch 4.

## Near-term queue (next batches, in rough priority order)

1. **Desktop release polish** (core path landed in batches 4–5; keypair,
   Windows staging, release matrix, and the Linux upgrade proof are done):
   macOS/Windows upgrade-proof legs (runner work), mac-Intel build
   (`macos-15-intel` leg or cross-compiled/universal target), macOS
   notarization (Apple Developer ID), optional NSIS installer. First real
   tagged release (`v2.14.2`) exercises the whole pipeline end to end.
2. **Coordinated rename** `Stirling` → `Rustling` (crates, `STIRLING_*` env
   vars with back-compat aliases, config keys, UI strings, startup handshake
   line) — one deliberate pass with a compatibility window; do not rename
   piecemeal.
3. **Independent security review** of the secured router + signing subsystem —
   the only gate for enabling `SECURITY_ENABLELOGIN=true` in production. Human
   task; `rust/SECURITY_MIGRATION_DESIGN.md` + `rust/SIGNING_MIGRATION_DESIGN.md`
   are the briefing docs.
4. **ResourceMonitor-style dynamic job-queue capacity** (memory/CPU sampling
   feeding `JobQueueConfig`) — needs a design decision first: is the current
   static budget a deliberate divergence? Upstream reference:
   `ResourceMonitor`/`DynamicJobQueue` in Stirling-PDF `app/common`.
5. **Optional SQLite-backed OIDC pending-login store** — beyond-upstream-parity
   (upstream keeps this state in per-process HTTP sessions); only needed for
   multi-process deployment. Needs wall-clock expiry + at-rest protection for
   `code_verifier`/`client_secret`.
6. **PDF-JSON deep-fidelity program** (multi-session, pick slices):
   - DeviceN DCT > 4 components — **probe first** whether PDFBox itself decodes
     5+-component JPEGs; if not, reclassify as parity-not-a-gap in the ledger.
   - CCITTFax/JBIG2/JPX inline-image decoding (needs new bounded decoders).
   - Type0/Type3 byte-parity + interior-kerning-run rewrite + true Type3 glyph
     synthesis (upstream's own oracle is partially poisoned here — see
     PORT_STATUS "Remaining"; treat as beyond-parity work).

## Explicitly deferred / not planned (with unblock conditions)

- **SAML2 SSO** — blocked on a maintainer decision: native `libxmlsec1`
  dependency vs a from-scratch C14N/XSW implementation. No route-shaped surface
  exists upstream (Spring filter chain), so nothing is route-missing.
- **SaaS/hosted-cloud layer** (upstream `app/saas` + `accountlink`) — depends on
  external Supabase/billing services; out of scope for a self-hosted product
  unless the product direction changes.
- **H2 database routes** (`/api/v1/database/*`, `ui-data/database`) — N/A by
  design: this backend's store is SQLite.
- **`convert/pdf/video`** — implemented but opt-in
  (`STIRLING_PROCESSING_FFMPEG_COMMAND`) while FFmpeg CVE exposure is assessed;
  upstream's own route is commented out.

## Working conventions (summary — full rules in CLAUDE.md)

- Substantial backend work = **dev + independent tester pairs** in per-item git
  worktrees; tester signs off only on clean gate + adversarial pass + upstream
  parity. Trivial fixes are done directly.
- Gates: `task rust:check` (or targeted `cargo test` filters + fmt/clippy),
  `task frontend:check`, `task engine:check`. Storage guard: clean
  `rust/target/debug/deps` when it exceeds 50 GB (`task rust:clean:deps`).
- The PM (orchestrating session) owns `README.md`, `rust/PORT_STATUS.md`, and
  this file; keeps them truthful after every batch.
- Machine quirks (this workstation): port 8080 is Jenkins — run the backend on
  another port; npm needs `npm_config_cache` override; Playwright uses system
  Chrome (`/usr/bin/google-chrome`).
