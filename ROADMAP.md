# RustlingPDF Roadmap

RustlingPDF is a local-first, open-source PDF workbench for desktop, web,
Docker, mobile capture, REST automation, and CLI workflows. The repository
tracks work against current product behavior rather than another
implementation.

## Current priorities

### 1. Release reliability

- Prove signed desktop upgrades on Linux, Windows, and macOS.
- Add macOS notarization and an Intel or universal macOS build.
- Keep installer lifecycle checks and bundled native-tool attribution green.
- Exercise the full tag-to-GitHub-release flow for each release.

### 2. Processing quality

- Expand adversarial fixtures for malformed, encrypted, signed, and unusually
  large documents.
- Improve PDF content-stream fidelity where unsupported encodings still require
  bounded fallbacks.
- Keep optional dependency detection precise and actionable.
- Maintain deterministic CLI and pipeline behavior across HTTP and in-process
  execution.

### 3. User experience

- Improve tool discovery, onboarding, accessibility, keyboard navigation, and
  mobile scanning.
- Make dependency availability and processing failures easier to understand.
- Continue performance and memory work for large-file editing and previews.
- Keep web and desktop behavior aligned through the shared core frontend.

### 4. Automation and integrations

- Expand typed CLI examples and reusable pipeline recipes.
- Improve OpenAPI, operation-catalog, and generated TypeScript coverage.
- Add safe import/export workflows that do not require accounts or persistent
  server state.

### 5. Optional AI

- Keep AI disabled by default, stateless, bounded, and provider-independent.
- Improve cited summaries, schema extraction, translation, and review flows.
- Strengthen timeout, cancellation, concurrency, and malformed-response tests.

## Product boundaries

- No commercial license keys, billing plans, usage credits, or gated open-source
  features.
- No built-in accounts, authentication database, or durable server document
  store.
- No PDF/A roadmap work unless it is explicitly reprioritized.
- External native programs remain optional and are reported through endpoint
  availability.

## Definition of done

A roadmap item is complete only when its behavior contract, executable tests,
generated artifacts, user documentation, and relevant packaging checks agree.
The active execution plans under `docs/plans/active/` contain ordered work;
validated plans move to `docs/plans/completed/`.
