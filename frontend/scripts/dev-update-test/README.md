# Auto-Update Testing

Tests the Tauri desktop auto-updater against a locally served, locally signed
update bundle. The desktop app bundles the Rust processing backend as a Tauri
sidecar (`task desktop:stage-sidecar` builds `rust/crates/stirling-processing`
in release mode, installs the pinned PDFium runtime, and stages both into
`src-tauri/`); the flows below build that sidecar automatically.

> **Signing note:** the `updater.pubkey` committed in `tauri.conf.json` is a
> repo-controlled key (minisign id `9ADA2DC8FC4FAF0B`, generated 2026-07-28
> with `npx tauri signer generate`). The private key lives outside the
> repository on the maintainer's machine (`~/.rustlingpdf/updater.key`, no
> password) and in the `TAURI_SIGNING_PRIVATE_KEY` GitHub Actions secret
> (with an empty `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) for the future
> desktop-release signing step. If the key is ever lost, generate a new pair
> and replace the committed pubkey — installed apps only accept updates
> signed by the committed key. The dev flows below are unaffected — they
> generate and use a local throwaway key pair.

## One command (automated)

```bash
# First time only:
npm run tauri:setup-dev-update

# Run tests (builds sidecar + signed bundle, starts server + app, runs checks):
npm run tauri:test-update-e2e

# Full install test (downloads + installs the update):
npm run tauri:test-update-e2e:install

# Skip rebuild if bundle already exists:
bash scripts/dev-update-test/test-update-e2e.sh --skip-build
```

## Containerized (Linux, only Docker required)

`run-e2e-container.sh` runs the whole flow inside a Docker container
(`container/Dockerfile`: rust + webkit2gtk + node 22 + Xvfb), so a Linux host
needs nothing but Docker — no webkit headers, no node, no rust:

```bash
# Full proof: detect + reject bad signatures + install + relaunch:
npm run tauri:test-update-e2e:container

# Check-only (detection asserts, no install):
bash scripts/dev-update-test/run-e2e-container.sh

# Reuse previously built bundles:
bash scripts/dev-update-test/run-e2e-container.sh --install --skip-build

# Debug shell in the container environment:
bash scripts/dev-update-test/run-e2e-container.sh --shell
```

What it proves (the Linux leg of the signed-bundle upgrade proof):

1. builds a **v0.0.1 AppImage** pinned to a throwaway dev pubkey and a
   `localhost:8090` update endpoint, with the real Rust backend sidecar and
   PDFium inside;
2. builds a **signed v99.0.0 updater artifact** + `latest.json` with the dev
   key;
3. launches the v0.0.1 AppImage under Xvfb and drives the real updater IPC
   over the WebKit remote inspector (`e2e-driver.py`):
   - `check_for_update` offers 99.0.0 to the 0.0.1 app,
   - a **same-version manifest** (0.0.1) and a **downgrade manifest** (0.0.0)
     are refused with "No update is currently available" (strict
     `remote > current` version gate),
   - an update signed by a **different valid key is rejected** (pubkey
     pinning) and the AppImage on disk is untouched,
   - a **byte-tampered artifact under the good signature is rejected**
     (content binding) and the AppImage on disk is untouched,
   - the good update downloads, signature-verifies, and **replaces the
     AppImage on disk** (sha256 asserted against the served artifact),
   - the **relaunched AppImage reports version 99.0.0**.

   The negative tests assert the **specific rejection reason** ("created with
   a different key" / "signature verification failed" / "No update is
   currently available"), not merely that an error occurred — an unrelated
   failure (endpoint down, fetch error) fails the test instead of
   false-passing the security property.

Assertion results + served manifests land in `.e2e-work/evidence/`; app and
server logs in `.e2e-work/logs/`.

Caching / rerunning from clean state: the container reuses
`$E2E_CACHE_DIR` (default `~/.cache/rustlingpdf-update-e2e`: cargo registry,
src-tauri target, npm cache, tauri CLI AppImage tooling) plus in-worktree
caches (`rust/target`, `rust/.pdfium`, `frontend/node_modules`, `.keys/`,
`.update-dist/`, `.e2e-work/`). Deleting any or all of these is safe — the
next run rebuilds them. The runner restores worktree file ownership on exit
(the container runs as root over a bind mount).

`--skip-build` reuse is idempotent across install runs — and across runs
killed at any point: the cached v0.0.1 build product is kept as
`.e2e-work/base/RustlingPDF-0.0.1.pristine.AppImage` and every run drives a
fresh working copy of it (the install test replaces the working copy in
place — that is the install proof). Likewise the good manifest is kept as
`.e2e-work/latest-good.pristine.json`, written once per build and never
touched by the driver: the v99 artifact is resolved from it and the served
`latest.json` is restored from it before every run, so a run killed while a
phase manifest (wrong key / tampered url / same-version) was being served
cannot poison the next one. The container toolchain downloads (Node.js,
Task) are version- and sha256-pinned in `container/Dockerfile`.

Container quirks handled for you: AppImage tooling and the built AppImage run
with `APPIMAGE_EXTRACT_AND_RUN=1` (no FUSE in containers), `NO_STRIP=1` for
linuxdeploy, and the app runs with the WebKit bwrap sandbox + DMA-BUF
renderer + compositing disabled (headless X in an unprivileged container).

## Manual testing

```bash
# Terminal 1 - serve the signed v99.0.0 update:
npm run tauri:serve-dev-update

# Terminal 2 - run the app at v0.0.1:
npm run tauri:dev-with-update
```

Go to Settings > General > Software Updates. Click "Check for Updates" then "Install Now".

## Requires

- Rust toolchain (`rustc`, `cargo`) plus the Tauri Linux system libraries on
  Linux hosts (`libwebkit2gtk-4.1-dev`, `libjavascriptcoregtk-4.1-dev`,
  `libgtk-3-dev`, `libsoup-3.0-dev`, `librsvg2-dev`,
  `libayatana-appindicator3-dev`, `libxdo-dev`)
- [Task](https://taskfile.dev) (`task desktop:stage-sidecar` stages the sidecar)
- Node.js, Python 3 (with `pip install websockets`)
- First-time setup generates signing keys + config override
