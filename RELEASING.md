# Releasing RustlingPDF

Releases are tag-driven. Pushing a tag `v<version>` runs
`.github/workflows/release.yml`, which verifies version discipline, publishes
both Docker images to GHCR, and creates the GitHub release. Nothing publishes
until the tag lands, and the workflow refuses tags that do not match the
repository's canonical version.

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
   - `github-release` — creates the GitHub release with auto-generated
     notes plus the exact `docker pull` commands.

   A failed run can be retried by re-running the workflow (or dispatching it
   from the tag ref): image pushes overwrite the same tags and the release
   step is idempotent when the release already exists. **Only re-dispatch
   from the newest release tag**: dispatching from an older tag passes the
   version guard (the old tree matches the old tag) and would move both
   images' `latest` tags backwards to that older release.

## Notes

- **CI flavor vs. image flavor**: the regular Frontend CI gate builds and
  tests the **core** (OSS) flavor, while the published Docker image ships the
  **proprietary** flavor SPA (matching upstream Stirling-PDF's self-hosted
  embedded image; it degrades to core behavior at runtime in open mode). A
  release therefore exercises the proprietary build path inside the Docker
  build itself — if that build breaks, it surfaces in `publish-images`, not
  in the earlier CI gates.
- **Desktop updater signing (follow-up)**: the Tauri desktop shell currently
  has no release artifact in this pipeline. The updater keypair already
  exists: the committed `updater.pubkey` (minisign id `9ADA2DC8FC4FAF0B`) is
  repo-controlled, and the private key is available to workflows as the
  `TAURI_SIGNING_PRIVATE_KEY` secret (empty
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). When the desktop-bundle release job
  lands, it signs update artifacts with that secret and uploads the
  `.sig` files + `latest.json` to the GitHub release — extend `release.yml`
  and this document together at that point.
- **Version tag `latest`**: both image tags are moved on every release;
  consumers who need reproducibility should pin `vX.Y.Z` (or the image
  digest) instead.
