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
- `rust/VERSION` is the single source of truth: `rustling-processing`'s
  `build.rs` derives the served application version from it at compile time.

## Version-bump checklist

The version is hard-coded in several places that must move **in lockstep**.
When bumping, update every one of these (line numbers are as of 2.14.2 and
may drift; the content anchors are stable):

| # | File | What to change |
|---|------|----------------|
| 1 | `rust/VERSION` | The canonical version (single line, e.g. `2.14.2`). |
| 2 | `frontend/editor/src-tauri/tauri.conf.json` | The `"version"` field (line 5). The release workflow cross-checks this against `rust/VERSION` and fails on mismatch. |
| 3 | `rust/crates/rustling-processing/src/runtime_metrics.rs` | Test literal in `preserves_java_version_and_metric_filters`: `assert_eq!(application_version(), "2.14.2")` (~line 289). |
| 4 | `rust/crates/rustling-processing/tests/info_endpoints.rs` | Test literal: `assert_eq!(status_json["version"], "2.14.2")` (~line 28). |
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
3. **Merge to `main`** through the normal PR flow (backend/frontend/desktop
   CI must be green).
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
backend sidecar + pinned PDFium + qpdf 12.3.2 + Tesseract 5.5.3 with English
tessdata 4.1.0 (`task desktop:stage-sidecar`; Unix and Windows dispatch to
their matching checksum-verified installers), prepares the desktop frontend,
and runs `npx tauri build`:

| Leg | Runner | Bundles | Updater platform key(s) |
|-----|--------|---------|--------------------------|
| linux-x86_64 | ubuntu-latest | `.AppImage`, `.deb` | `linux-x86_64` (AppImage), `linux-x86_64-deb` |
| windows-x86_64 | windows-latest | `.msi` (WiX) | `windows-x86_64`, `windows-x86_64-msi` |
| darwin-aarch64 | macos-latest (Apple silicon) | `.app.tar.gz` (updater), `.dmg` | `darwin-aarch64` |

The staging diagnostic prints both KiB and human-readable sizes for
`resources/tools` on every runner. Linux's measured static-musl tools tree is
25.12 MiB. Windows must be re-measured per release after the JBIG-free libtiff
swap; macOS remains unmeasured until a real runner build. Do not infer bundle
size from an unpacked upstream installer.

qpdf, Tesseract, and tessdata are Apache-2.0, but their runtime closures also
redistribute LGPL components and the GPL-3.0-with-exception GCC runtime.
The Windows Tesseract closure replaces upstream's libtiff with the repository's
JBIG-free build, so `libjbig-0.dll` and GPL-2.0 are not shipped. Full license
texts, qpdf's NOTICE, the LGPL relinking notice, provenance, and measured
dependency closures live under `rust/scripts/desktop-tools/` and are bundled
under `resources/tools/licenses`.

### Bumping the bundled Tesseract

1. Update the version and source/installer checksums in both
   `install-desktop-tools` scripts and
   `rust/scripts/desktop-tools/build-tesseract-musl.sh`.
2. If the Windows libtiff version changes, set its hash to
   `PENDING_CI_BUILD`, run the JBIG-free libtiff workflow with publication
   enabled, and pin the reported SHA-256.
3. Set the Linux artifact hash to `PENDING_CI_BUILD`, run the Linux Tesseract
   workflow with publication enabled, and pin the reported SHA-256.
4. Update `SOURCES.md`, `THIRD-PARTY-NOTICES.txt`, the generated desktop
   license inventory, and `rust/contracts/desktop-native-startup.md`.
5. Re-run the installer, staging smoke checks, and each native release leg.

Published Linux artifacts can be checked with `gh attestation verify` against
this repository. The full release procedure and residual Windows ABI risk are
documented in `rust/scripts/desktop-tools/SOURCES.md`.

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
always match the asset names. (Since the product rename to `RustlingPDF`
— no space — this is a no-op safeguard; v2.14.2 assets were the
`Stirling.PDF_*` spelling.)

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
- **Product rename continuity (`Stirling PDF` → `RustlingPDF`, post-v2.14.2)**:
  release assets are now named `RustlingPDF_<version>_*` /
  `rustling-pdf_<version>_*.deb` / `RustlingPDF.app.tar.gz`. Update
  continuity for installs of the v2.14.2 (`Stirling.PDF_*`) assets:
  - **Windows**: `bundle.windows.wix.upgradeCode` is pinned to
    `3305fba9-7e5e-5c09-bc71-eca0a65f4fee` — the value every shipped MSI
    (v2.14.2 included) carries. tauri-bundler would otherwise derive it as
    `uuid5(NAMESPACE_DNS, "{productName}.exe.app.x64")`, so an unpinned
    rename would change it and make renamed MSIs install side-by-side
    instead of upgrading. **Never change this GUID.** (The pinned value is
    the derivation for the historical name `Stirling-PDF`, inherited from
    upstream.)
  - **macOS**: the updater swaps the `.app` contents in place, keeping the
    existing on-disk folder name — an upgraded v2.14.2 install stays at
    `Stirling PDF.app` with RustlingPDF contents until reinstalled.
  - **Linux AppImage**: the updater byte-replaces the existing AppImage
    file, keeping the user's file name.
  - **Linux deb/rpm**: the package name changed (`stirling-pdf` →
    `rustling-pdf`), so a deb-key update installs the new package alongside
    the old one (no file conflicts — paths are product-name-scoped); users
    should `apt remove stirling-pdf` afterwards.
  - The bundle `identifier` (`stirling.pdf.dev`), updater endpoint, and
    pubkey are unchanged, so update polling, deep links, single-instance,
    and app-data paths (`Stirling-PDF` dirs) all carry over.

## Notes

- **CI flavor vs. image flavor**: the regular Frontend CI gate builds and
  tests the **core** (OSS) flavor, while the published Docker image ships the
  **proprietary** flavor SPA (the repo's default dev/build mode, matching
  upstream Stirling-PDF's self-hosted embedded image; since the auth/SaaS
  removal the flavor cascade is desktop → proprietary → core and proprietary
  adds only client-side extras over core). A release therefore exercises the
  proprietary build path inside the Docker build itself — if that build
  breaks, it surfaces in `publish-images`, not in the earlier CI gates.
- **Desktop upgrade e2e proof (follow-up)**: the desktop bundles, updater
  signatures, and `latest.json` are published by this pipeline (see
  "Desktop artifacts"). The Linux signed-upgrade e2e proof passed on
  2026-07-28 (recorded in git history; the harness that produced it was
  since removed by maintainer decision pending a rebuilt test harness);
  macOS/Windows upgrade-proof legs remain outstanding.
- **Version tag `latest`**: both image tags are moved on every release;
  consumers who need reproducibility should pin `vX.Y.Z` (or the image
  digest) instead.
