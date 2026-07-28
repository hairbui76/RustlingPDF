# Releasing RustlingPDF

Releases are tag-driven. Pushing a tag `v<version>` runs
`.github/workflows/release.yml`, which verifies version discipline, publishes
both Docker images to GHCR, builds and signs the desktop bundles on a
three-OS runner matrix, and creates the GitHub release with the desktop
artifacts and the updater manifest attached. Nothing publishes until the tag
lands, and the workflow refuses tags that do not match the repository's
canonical version.

## Tag scheme

- Format: `v<version>`, where `<version>` is exactly the contents of
  `rust/VERSION` (e.g. `rust/VERSION` = `2.14.2` → tag `v2.14.2`).
- Exact match only. Suffixed tags (`v2.14.2-rc1`, `v2.14.2-hotfix`) **fail**
  the release workflow's guard by design — the pipeline publishes exact
  releases only, and pre-release tags must never overwrite the `latest`
  image tags.
- `rust/VERSION` is the single source of truth: `stirling-processing`'s
  `build.rs` derives the served application version from it at compile time.

## Version-bump checklist

The version is hard-coded in several places that must move **in lockstep**.
When bumping, update every one of these (line numbers are as of 2.14.2 and
may drift; the content anchors are stable):

| # | File | What to change |
|---|------|----------------|
| 1 | `rust/VERSION` | The canonical version (single line, e.g. `2.14.2`). |
| 2 | `frontend/editor/src-tauri/tauri.conf.json` | The `"version"` field (line 5). The release workflow cross-checks this against `rust/VERSION` and fails on mismatch. |
| 3 | `rust/crates/stirling-processing/src/runtime_metrics.rs` | Test literal in `preserves_java_version_and_metric_filters`: `assert_eq!(application_version(), "2.14.2")` (~line 289). |
| 4 | `rust/crates/stirling-processing/tests/info_endpoints.rs` | Test literal: `assert_eq!(status_json["version"], "2.14.2")` (~line 28). |
| 5 | `frontend/editor/src/core/testing/serverExperienceSimulations.ts` | Fixture: `appVersion: "2.14.2"` in `BASE_NO_LOGIN_CONFIG` (~line 41). |
| 6 | `frontend/editor/src/proprietary/testing/serverExperienceSimulations.ts` | Fixture: `appVersion: "2.14.2"` in `BASE_NO_LOGIN_CONFIG` (~line 51). |

Quick audit for stragglers before committing the bump (should list exactly
sites 2–6 and nothing else; site 1, `rust/VERSION`, is extensionless and
invisible to the `--include` globs — check it by hand):

```bash
grep -rn "2\.14\.2" --include="*.rs" --include="*.ts" --include="*.json" \
     --include="*.toml" rust frontend | grep -v node_modules | grep -v target
```

(Replace `2\.14\.2` with the old version.)

## Release flow

1. **Bump** every site in the checklist above on a branch.
2. **Run the gates** locally:
   - `task rust:check` (sites 3 and 4 are test assertions — they fail until
     the bump is consistent),
   - `task frontend:check` (sites 5 and 6 are typed fixtures used by the
     simulation suites).
3. **Merge to `main`** through the normal PR flow (backend/frontend/
   differential-smoke CI must be green).
4. **Tag and push the tag** from the merged commit on `main`:

   ```bash
   git switch main && git pull
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

5. **The release workflow does the rest** (`.github/workflows/release.yml`):
   - `verify-version` — asserts tag == `v` + `rust/VERSION` and that
     `tauri.conf.json` agrees; anything else fails with a pointer back to
     this file.
   - `publish-images` — builds and pushes both `docker/Dockerfile` targets
     to GHCR, each tagged `vX.Y.Z` and `latest`, with OCI
     `version`/`source`/`revision`/`created` labels baked in:
     - `ghcr.io/hairbui76/rustlingpdf` (default `runtime` target),
     - `ghcr.io/hairbui76/rustlingpdf-ai-engine` (optional sidecar).
   - `publish-desktop` — calls the reusable
     `.github/workflows/desktop-build.yml` matrix (see "Desktop artifacts"
     below) with signing enabled.
   - `github-release` — creates the GitHub release with auto-generated
     notes plus the exact `docker pull` commands, then composes
     `latest.json` from the desktop artifacts
     (`scripts/compose_latest_json.py`) and uploads the desktop bundles,
     their `.sig` files, and `latest.json` as release assets.

   A failed run can be retried by re-running the workflow (or dispatching it
   from the tag ref): image pushes overwrite the same tags, the release
   step keeps an existing release's notes, and asset uploads use
   `--clobber` so a partially-uploaded release is completed idempotently.
   **Only re-dispatch from the newest release tag**: dispatching from an
   older tag passes the version guard (the old tree matches the old tag)
   and would move both images' `latest` tags backwards to that older
   release.

## Desktop artifacts

`publish-desktop` runs `.github/workflows/desktop-build.yml` (reusable,
`workflow_call`) on three hosted runners; each leg stages the release Rust
backend sidecar + pinned PDFium (`task desktop:stage-sidecar` — Windows
dispatches to `rust/scripts/install-pdfium.ps1`), prepares the desktop
frontend, and runs `npx tauri build`:

| Leg | Runner | Bundles | Updater platform key(s) |
|-----|--------|---------|--------------------------|
| linux-x86_64 | ubuntu-latest | `.AppImage`, `.deb` | `linux-x86_64` (AppImage), `linux-x86_64-deb` |
| windows-x86_64 | windows-latest | `.msi` (WiX) | `windows-x86_64`, `windows-x86_64-msi` |
| darwin-aarch64 | macos-latest (Apple silicon) | `.app.tar.gz` (updater), `.dmg` | `darwin-aarch64` |

What gets uploaded to the GitHub release:

- the installers themselves (`.AppImage`, `.deb`, `.msi`, `.dmg`),
- one `.sig` per updater artifact — base64 over the minisign signature box,
  produced by `tauri build` from the `TAURI_SIGNING_PRIVATE_KEY` secret
  (counterpart of the committed `src-tauri` `updater.pubkey`, minisign id
  `9ADA2DC8FC4FAF0B`; the password secret is the empty string),
- `latest.json` — the static manifest `tauri-plugin-updater` polls at
  `https://github.com/hairbui76/RustlingPDF/releases/latest/download/latest.json`
  (the endpoint committed in `tauri.conf.json`). Schema and platform-key
  rules are documented in `scripts/compose_latest_json.py`, which is
  unit-tested by `scripts/compose_latest_json_test.py` against a fixture
  artifact tree (`python3 scripts/compose_latest_json_test.py`).

Updater flow: a packaged app polls `latest.json`, picks its
`{os}-{arch}[-{installer}]` platform key, downloads the URL, verifies the
download against the `signature` field with the bundled pubkey, and
installs (AppImage self-replace, `dpkg -i`, MSI passive install, macOS
`.app` swap). Bundle file names are renamed space→dot before upload —
GitHub applies the same rename to release assets, so the manifest URLs
always match the asset names.

Dry-run (proving the matrix without tagging a release): dispatch
**Desktop release dry-run** (`.github/workflows/desktop-release-dryrun.yml`)
from the Actions tab — `platforms: all` for the full matrix or
`linux-only` for a cheap smoke leg. It calls the same reusable workflow
with signing enabled and uploads workflow artifacts only; nothing is
published.

Known scope limits:

- macOS is Apple-silicon only (`darwin-aarch64`). An Intel build needs
  either a `macos-15-intel`-style runner leg or a cross-compiled
  `x86_64-apple-darwin`/`universal-apple-darwin` target — follow-up.
- macOS bundles are not notarized (`signingIdentity: null`): Gatekeeper
  shows the unidentified-developer warning until Apple Developer ID signing
  is added — follow-up.
- Windows ships the WiX `.msi` only (no NSIS installer is configured in
  `tauri.conf.json`).

## Notes

- **CI flavor vs. image flavor**: the regular Frontend CI gate builds and
  tests the **core** (OSS) flavor, while the published Docker image ships the
  **proprietary** flavor SPA (matching upstream Stirling-PDF's self-hosted
  embedded image; it degrades to core behavior at runtime in open mode). A
  release therefore exercises the proprietary build path inside the Docker
  build itself — if that build breaks, it surfaces in `publish-images`, not
  in the earlier CI gates.
- **Desktop upgrade e2e proof (follow-up)**: the desktop bundles, updater
  signatures, and `latest.json` are published by this pipeline (see
  "Desktop artifacts"), but the cross-platform signed-bundle **upgrade**
  proof — a packaged old version updating itself to a newly published one
  via the reworked `frontend/scripts/dev-update-test` flow on a
  webkit-capable host — is still outstanding.
- **Version tag `latest`**: both image tags are moved on every release;
  consumers who need reproducibility should pin `vX.Y.Z` (or the image
  digest) instead.
