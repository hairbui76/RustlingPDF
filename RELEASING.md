# Releasing RustlingPDF

Releases are tag-driven. A tag named `v<version>` runs
`.github/workflows/release.yml`, publishes the runtime and optional AI images,
builds signed desktop bundles, and attaches the artifacts to a GitHub release.

## Version contract

`rust/VERSION` is the canonical version. A release tag must be exactly `v`
followed by that value. Suffixes such as `-rc1` are intentionally rejected
because the workflow moves the `latest` container tags.

Update these product-version sites together:

| File | Value |
|---|---|
| `rust/VERSION` | Canonical version |
| `frontend/editor/src-tauri/tauri.conf.json` | Tauri `version` |
| `rust/crates/rustling-processing/src/runtime_metrics.rs` | Version assertion |
| `rust/crates/rustling-processing/tests/info_endpoints.rs` | Status endpoint assertion |

Audit the old version before committing:

```bash
rg 'OLD\\.VERSION' rust frontend \
  --glob '!target/**' \
  --glob '!node_modules/**'
```

Review matches in dependency lockfiles separately; a dependency that happens
to use the same version is not a product version.

## Pre-release checks

From a clean checkout:

```bash
task rust:check
task frontend:check
task engine:check
task desktop:test
git diff --check
```

Also verify:

- generated operation catalogs and frontend API types are current;
- frontend and backend third-party dependency notices are current;
- `rust/VERSION` and the Tauri version match;
- the release commit is on `main`;
- release notes describe user-visible changes and known limitations.

For a packaging proof without publishing, dispatch
`.github/workflows/desktop-release-dryrun.yml`. Use `linux-only` for a fast
smoke run or `all` for the complete desktop matrix.

## Publish

After the release commit is merged:

```bash
git switch main
git pull --ff-only
git tag vX.Y.Z
git push origin vX.Y.Z
```

The workflow performs these stages:

1. `verify-version` checks the tag, `rust/VERSION`, and Tauri configuration.
2. `publish-images` pushes:
   - `ghcr.io/hairbui76/rustlingpdf:vX.Y.Z`;
   - `ghcr.io/hairbui76/rustlingpdf:latest`;
   - `ghcr.io/hairbui76/rustlingpdf-ai-engine:vX.Y.Z`;
   - `ghcr.io/hairbui76/rustlingpdf-ai-engine:latest`.
3. `publish-desktop` runs the reusable three-platform build matrix with bundle
   signing enabled.
4. `github-release` creates the GitHub release and uploads the installers and
   their `.sig` signature files.

The release workflow is idempotent for the same tag. Re-run only from the
newest release tag; running an older tag would move the mutable container
`latest` tags backward.

## Desktop matrix

| Platform | Runner | Bundles | Artifact |
|---|---|---|---|
| Linux x86-64 | `ubuntu-latest` | AppImage and deb | `desktop-linux-x86_64` |
| Windows x86-64 | `windows-latest` | WiX MSI | `desktop-windows-x86_64` |
| macOS arm64 | `macos-latest` | app archive and DMG | `desktop-darwin-aarch64` |

Every leg:

- builds the release `rustling-processing` sidecar;
- installs the pinned PDFium runtime;
- stages qpdf, Tesseract, and English OCR data;
- builds the core frontend;
- runs `npx tauri build`;
- signs the bundles when signing is enabled; and
- uploads a `desktop-<os>-<arch>` artifact that `github-release` attaches to
  the release.

The Windows leg also performs a real MSI install/assert/uninstall/assert
lifecycle check.

Required repository secrets:

- `TAURI_SIGNING_PRIVATE_KEY`;
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

The build fails if a bundle comes out unsigned.

## Signatures and updates

Every signed bundle is published with a `.sig` file next to it, so anyone who
downloads an installer can verify it before running it. That is all the
signatures are for.

A `.sig` file with no reachable public key verifies nothing, so the minisign
public counterpart of `TAURI_SIGNING_PRIVATE_KEY` is published here. Its key id
is `9ADA2DC8FC4FAF0B`:

```
untrusted comment: minisign public key: 9ADA2DC8FC4FAF0B
RWQLr0/8yC3amuV5GDR8m9AVltIe6+Czr0uGloq+3q4aLflmHXvKF9Jd
```

Verify a download with
`minisign -Vm <artifact> -P RWQLr0/8yC3amuV5GDR8m9AVltIe6+Czr0uGloq+3q4aLflmHXvKF9Jd`.

### Why `bundle.createUpdaterArtifacts` is still `true`

Despite the name, that flag is not an auto-update switch — it is the only path
in `tauri-cli` that runs minisign over the bundles (`sign_updaters` in
`bundle.rs`). Turning it off produces **no `.sig` files at all**, which is why
`desktop-build.yml`'s collect step fails the build when signatures stop
appearing. It is a build-output flag with no runtime effect: the updater
plugin, its endpoints, its capabilities and its in-app pubkey are all gone, so
nothing in a packaged app can poll or install anything. The
`task desktop:build:dev:*` targets override it to `false` on purpose — a local
dev build has no signing key.

The application does not check for updates. It contacts no update server and
publishes no update manifest, so nothing about an install — not even its
existence — is reported anywhere. To move to a newer version, check the
releases page yourself and download the installer:

`https://github.com/hairbui76/RustlingPDF/releases`

## Bundled native tools

Desktop packages redistribute PDFium, qpdf, Tesseract, OCR data, and their
runtime closures. Version pins, checksums, provenance, source offers, notices,
and full license texts live under `rust/scripts/desktop-tools/`.

When changing a bundled tool:

1. Update its version and checksums in the platform installers.
2. Rebuild any repository-produced native artifact and pin the published hash.
3. Update `SOURCES.md`, `THIRD-PARTY-NOTICES.txt`, and generated inventories.
4. Run staging smoke checks and every affected desktop matrix leg.
5. Update the relevant desktop contract.

Third-party notices are dependency attribution under the open-source
distribution; they are not a commercial product-license mechanism.

## Current packaging limits

- macOS publishes an Apple-silicon build only.
- macOS bundles are not notarized while `signingIdentity` is unset.
- Windows publishes WiX MSI; NSIS is not part of the release matrix.
- Mutable `latest` image tags are for convenience. Reproducible deployments
  should pin a version tag or image digest.
