# Execution Plan: Windows Explorer Right-Click Menu (Cascade Quick Actions)

Date: 2026-07-29

## Status

Completed

## Outcome

Right-clicking one or more PDF files in Windows Explorer shows a "RustlingPDF"
cascade submenu (on Windows 11 under "Show more options" — documented v1
limitation) with quick actions: Open, plus tool actions (e.g. Merge, Compress,
Convert) that launch/focus the app with the selected files preloaded into the
chosen tool. Registered by the MSI, cleanly removed on uninstall, MSI upgrade
continuity preserved (pinned UpgradeCode untouched).

## Context

- Maintainer decisions: cascade submenu with quick actions; Windows first
  (Linux/macOS keep their existing file-association "Open With").
- Scout map (authoritative spec): session scratchpad — task output of the
  explorer-context-menu scout; key facts inline below.
- Existing infra: WiX fragment `windows/wix/provisioning.wxs` +
  `ProvisioningComponentGroup` (thumbnail-handler precedent: HKLM registry
  components, Guid="*", KeyPath rules); staging `build-provisioner.mjs`;
  file-open flow `src-tauri/src/lib.rs` (parse_launch_files :19-25,
  single-instance :44-66, macOS Opened :163-196) → `commands/files.rs` queue →
  `files-changed` → `desktop/hooks/useOpenedFile.ts` →
  `useAppInitialization.ts`.
- Constraints: fragments are Handlebars-rendered (literal `{{` is a template);
  fragment paths resolve against the `tauri build` cwd (`editor/`); MSI is
  perMachine (HKLM legal); plain registry verbs fire once per selected file
  (single-instance coalescing + in-app aggregation needed); NSIS dev build
  ignores WiX fragments (not a bug); upstream oracle has nothing to port.

## Scope

In scope:

- WiX registry cascade under `HKLM\SOFTWARE\Classes\SystemFileAssociations\
  .pdf\shell\RustlingPDF` (MUIVerb + SubCommands pattern, per-action subverbs
  invoking `"[INSTALLDIR]RustlingPDF.exe" --tool <action> "%1"`), new
  components in `ProvisioningComponentGroup` following the Guid="*"/KeyPath
  rules.
- App-side launch intent: `--tool <name>` argument parsing (back-compat with
  bare paths), intent+files forwarding through the opened-files queue and
  single-instance callback, aggregation/debounce so an N-file multi-select
  (N processes) lands as one action with N files.
- Frontend: route the intent to the matching tool with the files preloaded;
  action set limited to tools that exist and accept preloaded files.
- Docs: contract update, Win11 "Show more options" caveat, uninstall behavior.

Out of scope:

- Windows 11 top-level menu (MSIX/sparse-package IExplorerCommand) — follow-up.
- DropTarget/IExplorerCommand COM batching — the debounce approach ships v1.
- Linux file-manager service menus / macOS Finder extension — follow-up.

## Approach

One dev+tester pair in a worktree. Dev: WXS + Rust arg/intent plumbing (+unit
tests) + frontend routing + docs. Tester: container src-tauri gate, arg/intent
unit tests, WXS static verification (XML validity, Handlebars safety, WiX
component rules, registry semantics), frontend gate. PM: merge, push, prove
the MSI actually builds with the new fragment via the desktop dry-run
(windows-latest leg), then release when the maintainer asks.

## Risks And Recovery

- WXS cannot be compiled locally (no WiX on Linux) — the windows-latest
  dry-run is the compile proof before any release; a broken fragment fails
  that leg, not users.
- MSI upgrade safety: never touch upgradeCode; new components additive with
  fresh GUIDs; old-product-removed-first scheme makes additions safe.
- Multi-select storm: bounded debounce with tests; worst case degrades to the
  same files arriving in several batches (open still correct).
- Rollback: branch unmerged until sign-off; menu is registry-only and removed
  by uninstall.

## Progress

- [x] Scout map complete; maintainer scope decisions recorded.
- [x] Dev implementation + local gates (16 new Rust unit tests, 14 new
      vitest; container gate green; WXS rendered+parsed; action-set triple
      pinned by tests in WXS/Rust/TS).
- [x] Tester sign-off round 0 (independent WXS re-derivation + render pass;
      3 minors, all documented residuals).
- [x] PM merge fcc100c + CI 3/3 green + windows-latest MSI build proof
      (dry-run 30421938113, all three legs success).
- [x] Result recorded; plan moved to completed.

## Decisions

- 2026-07-29: Cascade submenu with quick actions (not just Open); Windows
  first. Win11 legacy-submenu placement accepted for v1.
- 2026-07-29: Multi-select handled by in-app aggregation (debounce), not a
  COM DropTarget — smallest correct v1.

## Validation

- Focused proof: unit tests for `--tool` parsing, intent queue, debounce
  aggregation; container src-tauri gate (fmt/clippy/tests).
- Integration proof: windows-latest dry-run builds the MSI with the new
  fragment (WiX compile is the gate); frontend gate for the routing.
- Repository-required checks: CI green on merge.

## Result

Verified outcome (2026-07-29): the MSI registers a registry-only Explorer
cascade menu for .pdf (Open/Merge/Compress/Convert) launching the app with a
--tool intent; N-file multi-select aggregates into a single debounced batch;
plain opens, macOS open-file, and drag-drop unchanged. Merged as fcc100c with
CI green and the WiX fragment compile-proven by the windows-latest dry-run
(run 30421938113). Contract: rust/contracts/desktop-explorer-context-menu.md.

Limitations / follow-ups: Windows 11 places the menu under "Show more
options" (top-level needs a packaged IExplorerCommand — follow-up); NSIS dev
builds carry no menu (MSI-only); runtime click-through validation on a real
Windows machine remains a manual step for the maintainer after the next
release. Ships with the next tagged release.
