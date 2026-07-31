# Execution Plan: Pipeline Drag-and-Drop Release

Date: 2026-07-31

## Status

Completed

## Outcome

RustlingPDF v3.1.3 is published from `main` with the visual pipeline builder,
JSON persistence, and accessible drag-and-drop step reordering, with signed
desktop artifacts and versioned container images produced by the repository's
tag-driven release workflow.

## Context

- Release authority: `RELEASING.md` and `.github/workflows/release.yml`.
- Canonical product version: `rust/VERSION`.
- Pipeline UI:
  `frontend/editor/src/core/components/tools/automate/ToolList.tsx`.
- Reorder rule: `frontend/editor/src/core/utils/automationReorder.ts`.
- Completed pipeline audit:
  `docs/plans/completed/pdf-pipeline-automation.md`.
- Previous release: v3.1.2, published successfully on 2026-07-30.

## Scope

In scope:

- Commit the completed pipeline UI, proof, documentation, and release-version
  changes directly to `main`.
- Run the documented local pre-release gates and full hosted desktop dry-run.
- Publish v3.1.3 through the tag-driven workflow and verify its release assets.

Out of scope:

- New pipeline features beyond reorder.
- PDF/A conversion.
- Changes to updater signing keys or bundled native-tool versions.

## Approach

Use a patch bump from 3.1.2 to 3.1.3 because the change is backward-compatible
UI functionality. Update the four version sites required by `RELEASING.md`,
run local release gates, commit the feature and release bump, and push directly
to `origin/main`. Wait for main CI and the all-platform desktop release dry-run
before pushing `v3.1.3`. Monitor the tag workflow through GitHub release
creation and verify the expected image, desktop, signature, and updater assets.

## Risks And Recovery

- A pushed release tag starts image publication and signed desktop builds; do
  not push it before source CI and the dry-run are green.
- A transient tag-workflow failure can be rerun from the same newest tag. A
  source or version defect consumes the release version; fix on `main` and use
  a later patch tag instead of rewriting the published tag.
- `.codex/` is user-owned and must remain untracked and unstaged.

## Progress

- [x] Confirm local `main` equals `origin/main` and v3.1.2 is the latest release.
- [x] Bump all required version sites to 3.1.3.
- [x] Run local pre-release gates.
- [x] Commit and push `main`.
- [x] Verify main CI and all-platform desktop dry-run.
- [x] Push v3.1.3 and verify the published release.

## Decisions

- 2026-07-31: Use v3.1.3, a patch release, because pipeline reorder is an
  additive backward-compatible feature.
- 2026-07-31: Require the full hosted desktop dry-run before tagging because
  the release publishes signed installers on three operating systems.
- 2026-07-31: Rust, frontend, AI engine, generated-model, updater-manifest,
  version-alignment, and diff checks passed locally. `task desktop:test`
  built and staged the release sidecar plus pinned native tools, then stopped
  while compiling Tauri because this workstation lacks the system development
  package that provides `glib-2.0.pc`. The hosted Desktop CI and all-platform
  dry-run install the repository-declared Linux dependencies and remain the
  required packaging proof before tagging.
- 2026-07-31: The first all-platform dry-run built, signed, installed, and
  uninstalled the Windows MSI, but its identity assertion still expected the
  predecessor-era UpgradeCode instead of the independent-product UpgradeCode
  pinned in `tauri.conf.json`. Align the lifecycle proof and current installer
  contract with the authoritative configuration, then rerun the complete
  matrix before tagging.
- 2026-07-31: Tag v3.1.3 points to `fba9f95`, the exact commit proven by the
  successful main CI and all-platform desktop dry-run. `main` received later,
  unrelated document-conversion commits while the tag workflow ran; those
  changes are intentionally not part of v3.1.3.

## Validation

- Automation proof: 31 focused Vitest assertions and 16 Playwright scenarios,
  including persisted pointer drag-and-drop and accessible keyboard reorder.
- Full local proof: Rust formatting, strict Clippy and workspace tests;
  frontend typecheck, lint, formatting and 850 tests; AI engine checks and 117
  tests; generated frontend/engine operation models; 21 updater-manifest tests;
  version alignment and `git diff --check`.
- Main CI: Backend CI run `30624769288`, Frontend CI run `30624769338`, and
  Desktop CI run `30624769284` all succeeded for `fba9f95`.
- Packaging proof: desktop dry-run `30624773472` succeeded for Linux, Windows,
  and macOS. It produced all three desktop artifacts plus Windows MSI lifecycle
  logs; install, uninstall, identity, provisioning, and cleanup checks passed.
- Release proof: tag workflow `30627304567` succeeded, publishing the runtime
  images and signed desktop artifacts before creating the GitHub release.
- Public release proof: v3.1.3 is the latest non-draft, non-prerelease release
  with ten uploaded assets. `latest.json` reports version `3.1.3` and signed
  updater URLs for macOS, Linux AppImage/DEB, and Windows MSI.

## Result

RustlingPDF v3.1.3 was published successfully from tag `v3.1.3` at commit
`fba9f95`. The release includes the visual pipeline builder with persisted JSON
pipelines, pointer drag-and-drop and keyboard step reordering, signed desktop
downloads for Linux, Windows, and macOS, updater metadata, and versioned
container images.

Release: https://github.com/hairbui76/RustlingPDF/releases/tag/v3.1.3
