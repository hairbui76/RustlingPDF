# Execution Plan: Independent Product Cleanup

Date: 2026-07-31

## Status

Completed

## Outcome

Remove predecessor branding, runtime identifiers, compatibility aliases,
historical parity material, and inherited commercial-only product surfaces.
RustlingPDF must build and operate as an independent MIT-licensed product.

Completion requires a repository-wide audit with no predecessor-name occurrence
outside legally required attribution in the root license.

## Authority

- The user explicitly requested complete removal of predecessor-related
  material.
- The root `LICENSE` is the authority for retaining mandatory MIT attribution.
- `README.md`, `docs/product/features.md`, `rust/RUNNING_WITH_RUST.md`, and
  `rust/contracts/` describe the current product and runtime.
- The user does not want a commercial license-key system or PDF/A work in this
  change.

## Scope

In scope:

- Product names, assets, package/service identifiers, URLs, browser storage,
  application-data paths, environment variables, and generated metadata.
- Compatibility aliases and historical implementation/parity documentation.
- Commercial license, billing, usage-credit, account, and gated-feature
  surfaces that are not part of the open-source product.
- Restrictively licensed frontend overlays and their build modes.
- Generated catalogs, API types, dependency notices, snapshots, and ignored
  build artifacts that preserve the removed identity.

Out of scope:

- Removing attribution required by the license of retained source.
- Publishing a release or migrating data outside this repository.
- Adding PDF/A support.

## Sequence

1. Remove commercial and restrictive source boundaries; make web and desktop
   builds use the open-source core.
2. Replace runtime and package identity, deleting compatibility aliases rather
   than carrying them forward.
3. Remove predecessor-derived internal names, assets, URLs, fixtures, and
   generated output.
4. Rewrite current documentation and contracts without lineage or parity
   language.
5. Regenerate catalogs, types, metadata, and third-party dependency notices.
6. Run frontend, Rust, desktop, generator, and repository integrity checks.
7. Audit tracked content, tracked filenames, and scoped ignored artifacts;
   manually review the single legal-attribution exception.

## Risks And Recovery

- Identity and path changes intentionally break old upgrade/configuration
  continuity. Recovery is a coherent commit revert, not a hidden alias.
- Removing commercial overlays can expose missing core imports. Type checks,
  builds, and desktop tests provide the recovery signal.
- Generated files may reintroduce deleted surfaces. Generators are updated
  before their outputs are accepted.
- Required attribution must remain. The final audit excludes only the reviewed
  root-license notice.

## Progress

- [x] Repository-wide content and filename inventory completed.
- [x] Restrictive frontend overlay trees removed.
- [x] Web and desktop builds switched to the core frontend.
- [x] Commercial license validation, license endpoints, and feature gates
  removed.
- [x] Runtime/package identity and predecessor compatibility aliases removed.
- [x] Internal identifiers and branded assets renamed.
- [x] Dead billing, plan, and usage-credit configuration removed.
- [x] Current documentation and contracts rewritten.
- [x] Generated outputs refreshed and checked.
- [x] Complete validation gates pass.
- [x] Final forbidden-string and legal-attribution audit passes.

## Validation

- Rust formatting, workspace check, strict Clippy, locked tests, and CLI smoke.
- Frontend TypeScript, lint, formatting, unit tests, and production build.
- Tauri/provisioner/thumbnail-handler formatting, checks, and tests where the
  host supports them.
- OpenAPI reference validation plus operation catalog, frontend API types,
  dependency notices, and OG metadata regeneration.
- `git diff --check`, case-insensitive content scan, tracked filename scan, and
  scoped ignored-artifact scan.

## Result

RustlingPDF now ships only its independent open-source core. Predecessor
branding, product identifiers, compatibility aliases, commercial licensing,
account/billing surfaces, restrictive frontend overlays, and stale generated
assets were removed. The sample and test PDFs were regenerated or normalized so
their visible content and metadata use the current identity.

Validation completed:

- Rust workspace formatting, strict Clippy, locked all-target tests, targeted
  fixture tests, and CLI operation-catalog smoke passed.
- Frontend type checking, linting, formatting, dependency-cycle analysis, 845
  unit tests, production build, and Storybook build passed. Storybook was built
  with Node 22.14 because the installed Node 20.14 is below Vite's supported
  floor.
- Provisioner checks/tests and the thumbnail handler's Windows cross-check
  passed. The root Tauri native Linux check remains host-limited by missing
  `glib-2.0.pc`; a full Windows root build additionally needs the MSVC linker
  toolchain.
- OpenAPI references, operation catalogs, generated frontend API types,
  dependency notices, OG metadata, and PDF syntax/metadata checks passed.
- Case-insensitive tracked-content, filename, worktree, production-build,
  Storybook, gzip, and PDF-metadata scans found no predecessor-name occurrence.
  The sole reviewed exception is the mandatory copyright attribution in the
  root MIT `LICENSE`.
