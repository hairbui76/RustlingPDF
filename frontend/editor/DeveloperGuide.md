# Frontend Developer Guide

RustlingPDF has one open-source frontend source tree:
`frontend/editor/src/core`. Web, Docker, and Tauri desktop builds all compile
that tree in Vite `core` mode.

## Imports

Use the `@app/*` alias for application modules:

```ts
import { useFileContext } from "@app/contexts/FileContext";
```

The alias resolves to `src/core`. Do not add commercial, proprietary, cloud,
SaaS, desktop-overlay, or prototype source layers. Native desktop behavior is
provided by the Tauri bridge and generic extension modules in the core tree.

## Application structure

- `components/` contains shared UI and tool-specific controls.
- `contexts/` owns application state, including file lifecycle.
- `hooks/tools/` composes tool execution and result handling.
- `services/` contains API, storage, update, and platform-facing services.
- `tools/` contains complete tool screens.
- `types/` contains shared TypeScript contracts.
- `data/` contains tool taxonomy and generated metadata.
- `tests/` contains stubbed and live Playwright scenarios.

## File lifecycle

All document operations go through `FileContext`. Use its actions and selectors
instead of maintaining a second file store. PDF.js documents, object URLs, and
large binary buffers must be released when a file, preview, or operation result
is replaced.

New processing tools should use `useToolOperation` and the shared operation
types. Endpoint request shapes come from the generated
`src/core/api/types/toolApiTypes.ts`; regenerate them when `SwaggerDoc.json`
changes.

## Desktop integration

Core code may call generic bridge modules for behavior such as opening a path
or selecting a folder. Those modules must provide a browser-safe implementation
and may use Tauri APIs when the runtime exposes them. Keep product logic in the
shared core; keep OS lifecycle and installer behavior under `src-tauri`.

## Styling

Read `src/core/theme/README.md` before changing colors. Components use semantic
tokens rather than literal color values. Run the theme and contrast checks with
the normal frontend gate.

## Translations

User-visible copy belongs in the locale TOML files under `public/locales/`.
Keep English source keys coherent and avoid adding commercial plan, billing,
usage-credit, or license-activation copy.

## Generated files

These files are committed and must match their generators:

- `src/core/api/types/toolApiTypes.ts`;
- `src/core/data/ogImageMap.json`;
- `public/og-metadata.json`;
- frontend third-party dependency notices.

Use the Task targets rather than editing generated files by hand:

```bash
task frontend:tool-models
task frontend:prepare:og
task frontend:licenses:generate
```

## Validation

```bash
task frontend:typecheck
task frontend:lint
task frontend:format:check
task frontend:test
task frontend:build
task frontend:check
```

Use focused unit or Playwright tests while iterating, then run the complete
frontend gate before handing off a change.
