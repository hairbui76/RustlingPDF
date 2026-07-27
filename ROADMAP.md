# RustlingPDF Roadmap

Living planning document. A fresh working session should read this file plus
`CLAUDE.md` (working rules) and `rust/PORT_STATUS.md` (the authoritative
feature/parity ledger) to get fully oriented. Update this file whenever a batch
lands or the queue changes.

## Snapshot (2026-07-27)

- Repo: standalone product based on Stirling-PDF; **no Java anywhere** — the
  upstream checkout (if present at `../Stirling-PDF`) is a read-only reference
  oracle only.
- `main` @ `b08a550` — full quality gate green:
  `cargo fmt`/`clippy -D warnings` clean, **stirling-processing 1455 passed /
  0 failed**, **stirling-ai-engine 144 / 0**; frontend typecheck/eslint clean,
  **1647 vitest passed**, `vite build` (core) succeeds.
- Open mode is the supported runtime; secured mode is implemented + tested but
  **fail-closed** pending independent human security review.
- Canonical app version lives in `rust/VERSION` (consumed by `build.rs`).
- Verified quick start: `task rust:install && task dev`.

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

One recorded minor (follow-up candidate): a hand-authored **flow-styled root**
settings.yml (`{a: 1}`) would get an appended block section, producing a
two-document YAML that fails to reparse — refuse it like flow sections. Also
noted: the admin license-persist path still uses serde round-trip (drops
comments) — migrate onto `settings_yaml`.

## Near-term queue (next batches, in rough priority order)

1. **Release pipeline** (CI base landed in batch 3): release workflow
   (tagged builds, `rust/VERSION` bump discipline), Docker image publish
   (GHCR), dependabot/renovate for Cargo + npm.
2. **Tauri desktop → Rust sidecar**: `src-tauri` still declares JRE/JAR
   resources and launches a Java bundle by default
   (`frontend/editor/src-tauri/tauri.conf.json`, `src/commands/backend.rs`);
   port to spawning the Rust binary (the backend already implements the
   ephemeral-port handshake + parent-death watchdog — see
   `rust/contracts/desktop-native-startup.md`). Also rework
   `frontend/scripts/dev-update-test/*.sh` (annotated non-functional).
3. **Coordinated rename** `Stirling` → `Rustling` (crates, `STIRLING_*` env
   vars with back-compat aliases, config keys, UI strings, startup handshake
   line) — one deliberate pass with a compatibility window; do not rename
   piecemeal.
4. **Independent security review** of the secured router + signing subsystem —
   the only gate for enabling `SECURITY_ENABLELOGIN=true` in production. Human
   task; `rust/SECURITY_MIGRATION_DESIGN.md` + `rust/SIGNING_MIGRATION_DESIGN.md`
   are the briefing docs.
5. **ResourceMonitor-style dynamic job-queue capacity** (memory/CPU sampling
   feeding `JobQueueConfig`) — needs a design decision first: is the current
   static budget a deliberate divergence? Upstream reference:
   `ResourceMonitor`/`DynamicJobQueue` in Stirling-PDF `app/common`.
6. **Optional SQLite-backed OIDC pending-login store** — beyond-upstream-parity
   (upstream keeps this state in per-process HTTP sessions); only needed for
   multi-process deployment. Needs wall-clock expiry + at-rest protection for
   `code_verifier`/`client_secret`.
7. **PDF-JSON deep-fidelity program** (multi-session, pick slices):
   - DeviceN DCT > 4 components — **probe first** whether PDFBox itself decodes
     5+-component JPEGs; if not, reclassify as parity-not-a-gap in the ledger.
   - CCITTFax/JBIG2/JPX inline-image decoding (needs new bounded decoders).
   - Type0/Type3 byte-parity + interior-kerning-run rewrite + true Type3 glyph
     synthesis (upstream's own oracle is partially poisoned here — see
     PORT_STATUS "Remaining"; treat as beyond-parity work).
   - Recorded minor: Real-valued inline `DecodeParms` (`/Predictor 2.0`) are
     treated as absent where PDFBox truncates to int — cheap fix.
8. **Recorded minor from batch 2**: OIDC cookie-clear `Set-Cookie` is not
   byte-identical to Spring's `Expires` spelling — cosmetic, fix opportunistically.

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
