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
