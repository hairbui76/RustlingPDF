# Execution Plan: PDF Pipeline Automation

Date: 2026-07-31

## Status

Completed

## Outcome

Users can create an ordered PDF tool chain, run each operation with the
previous operation's output, keep pipelines locally, and import or export the
pipeline as JSON.

## Context

- Product surface:
  `frontend/editor/src/core/components/tools/automate/AutomationCreation.tsx`
  and `AutomationSelection.tsx`.
- Persistence:
  `frontend/editor/src/core/services/automationStorage.ts`.
- JSON formats and conversion:
  `frontend/editor/src/core/utils/automationConverter.ts`.
- Browser execution:
  `frontend/editor/src/core/utils/automationExecutor.ts`.
- Server execution contract and implementation:
  `rust/contracts/pipeline.md` and
  `rust/crates/rustling-processing/src/pipeline.rs`.

## Scope

In scope:

- Create, configure, save, edit, import, and export ordered automation chains.
- Native Automate JSON and server/folder-scanning pipeline JSON.
- Sequential browser and server execution, including multi-output handoff.
- Executable proof for successful chaining and failure handling.

Out of scope:

- PDF/A conversion.
- Adding new PDF operations solely for pipeline support.
- Cloud synchronization of locally saved automations.

## Approach

Audit the existing UI, storage, JSON conversion, browser executor, and Rust
pipeline endpoint against the requested user outcome. Preserve the existing
implementation where it already satisfies the outcome, add missing browser
execution tests, repair the stubbed app config that allowed an analytics modal
to block automation UI tests, and run focused unit, endpoint, and browser
validation.

## Risks And Recovery

- A tool without an automation operation config cannot participate in a browser
  chain; the builder only exposes supported tools.
- Native Automate JSON uses frontend tool IDs, while folder-scanning JSON uses
  backend endpoints; the converter owns this mapping and reports unresolved
  imported operations.
- The changes in this task are test-only. Recovery is a normal revert of the
  execution tests and the stub app-config default.

## Progress

- [x] Audit product contract, UI builder, local persistence, and both JSON
      formats.
- [x] Audit browser and Rust sequential execution.
- [x] Add browser executor tests for ordered output handoff and fail-fast
      behavior.
- [x] Make stubbed app-config suppress the unrelated analytics startup modal.
- [x] Run focused unit, type, formatting, Rust endpoint, and Playwright proof.

## Decisions

- 2026-07-31: Keep both JSON formats: native JSON is best for UI round-trips;
  folder-scanning JSON is directly compatible with the backend pipeline
  endpoint.
- 2026-07-31: No production implementation change was needed because the
  requested user behavior already existed; close the proof gap instead.
- 2026-07-31: Default `enableAnalytics` to `false` only in the shared stubbed
  app config, matching the fixture contract that startup modals do not block
  unrelated UI tests.

## Validation

- Focused proof: Vitest automation executor and converter suites, 28/28 passed;
  TypeScript no-emit check and Prettier check passed.
- Integration proof: Rust pipeline endpoint suite, 5/5 passed; automation
  builder/import/export Playwright suites, 14/14 passed.
- Repository-required checks: `git diff --check` passed.

## Result

Verified on 2026-07-31. Users can assemble and configure ordered operations,
save them in IndexedDB, export or import `.automate.json`, export or import
backend-compatible `.folder-scan.json`, and run the chain with each step's
outputs feeding the next step. Both browser and Rust execution stop on invalid
or failed operations instead of continuing with stale files. PDF/A remains
outside this task as requested.
