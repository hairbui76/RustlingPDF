// @ts-check

import eslint from "@eslint/js";
import globals from "globals";
import { defineConfig } from "eslint/config";
import tseslint from "typescript-eslint";

const srcGlobs = ["editor/src/**/*.{js,mjs,jsx,ts,tsx}"];
const nodeGlobs = [
  "scripts/**/*.{js,ts,mjs,mts}",
  "editor/scripts/**/*.{js,ts,mjs,mts}",
  // Covers editor/vite.config.ts and editor/vitest.config.ts.
  "editor/*.config.{js,ts,mjs}",
  "*.config.{js,ts,mjs}",
  ".storybook/*.{js,ts,mjs,mts,tsx}",
];

const baseRestrictedImportPatterns = [
  {
    regex: "^\\.",
    message: "Use a workspace alias (@app/*) instead of relative imports.",
  },
  {
    regex: "^src/",
    message: "Use a workspace alias instead of absolute src/ imports.",
  },
];

// The single directory allowed to name a Tauri module. See the config block
// near the bottom of this file for why, and `core/services/desktop/
// desktopRuntime.ts` for what it exports to the rest of the app.
const desktopBridgeGlob = "editor/src/core/services/desktop/**/*.{ts,tsx}";

const tauriImportRestriction = {
  regex: "^@tauri-apps/",
  message:
    "Tauri APIs may only be imported from editor/src/core/services/desktop/**. Everywhere else, use the helpers that directory exports (isDesktopRuntime, desktopInvoke, ...) so the web bundle never loads a Tauri module.",
};

const embedpdfEnginesImportRestriction = {
  regex: "^@embedpdf/engines",
  message:
    "Import useLocalPdfiumEngine from @app/services/pdfiumEngine instead. @embedpdf/engines defaults wasmUrl AND fontFallback to a public CDN, so constructing the engine directly risks a silent third-party request on document open.",
};

// Button/SegmentedControl/Chip must come from the shared DS (@app/ui), not Mantine.
// If no variant fits, extend @app/ui — that layer (editor/src/core/ui) is exempt below.
const mantineComponentImportRestrictions = [
  {
    selector:
      "ImportDeclaration[source.value='@mantine/core'] > ImportSpecifier[imported.name=/^(Button|ActionIcon|UnstyledButton|CloseButton|FileButton)$/]",
    message:
      'Use the shared Button (@app/ui/Button) instead of the Mantine button family. variant=primary|secondary|tertiary, accent=default|neutral|brand|ai|highlight|danger|success|warning; an icon-only button is `<Button leftSection={…} aria-label="…" />`. If no variant fits, extend the shared Button rather than importing Mantine.',
  },
  {
    selector:
      "ImportDeclaration[source.value='@mantine/core'] > ImportSpecifier[imported.name='SegmentedControl']",
    message:
      "Use the shared SegmentedControl (@app/ui/SegmentedControl) instead of Mantine's.",
  },
  {
    selector:
      "ImportDeclaration[source.value='@mantine/core'] > ImportSpecifier[imported.name=/^(Chip|Pill)$/]",
    message:
      "Use the shared Chip (@app/ui/Chip) instead of Mantine's Chip/Pill.",
  },
];

// Raw <button> should be a shared Button too — but bespoke CSS-styled controls
// (tabs, nav rows, preset chips) can be exempted from this selector alone.
const rawButtonSyntaxRestriction = {
  selector: "JSXOpeningElement[name.name='button']",
  message:
    "Use the shared Button (@app/ui/Button) instead of a raw <button> element. If no variant fits, extend the shared Button.",
};

const sharedComponentSyntaxRestrictions = [
  ...mantineComponentImportRestrictions,
  rawButtonSyntaxRestriction,
];

export default defineConfig(
  {
    // Everything that contains 3rd party code that we don't want to lint
    ignores: [
      "dist",
      "node_modules",
      "playwright-report",
      "storybook-static",
      "test-results",
      "editor/dist",
      "editor/public",
      "editor/src-tauri",
      "editor/playwright-report",
      "editor/test-results",
    ],
  },
  eslint.configs.recommended,
  tseslint.configs.recommended,
  {
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: baseRestrictedImportPatterns,
        },
      ],
      "@typescript-eslint/no-empty-object-type": [
        "error",
        {
          // Allow empty extending interfaces because there's no real reason not to, and it makes it obvious where to put extra attributes in the future
          allowInterfaces: "with-single-extends",
        },
      ],
      "@typescript-eslint/no-explicit-any": "off", // Temporarily disabled until codebase conformant
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          args: "all", // All function args must be used (or explicitly ignored)
          argsIgnorePattern: "^_", // Allow unused variables beginning with an underscore
          caughtErrors: "all", // Caught errors must be used (or explicitly ignored)
          caughtErrorsIgnorePattern: "^_", // Allow unused variables beginning with an underscore
          destructuredArrayIgnorePattern: "^_", // Allow unused variables beginning with an underscore
          varsIgnorePattern: "^_", // Allow unused variables beginning with an underscore
          ignoreRestSiblings: true, // Allow unused variables when removing attributes from objects (otherwise this requires explicit renaming like `({ x: _x, ...y }) => y`, which is clunky)
        },
      ],
    },
  },
  // The web application must not import native Tauri APIs.
  {
    files: srcGlobs,
    ignores: [desktopBridgeGlob],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [...baseRestrictedImportPatterns, tauriImportRestriction],
        },
      ],
    },
  },
  // `@embedpdf/engines` defaults BOTH `wasmUrl` and `fontFallback` to
  // cdn.jsdelivr.net, and both defaults are reached by simply omitting the
  // option — `fontFallback: undefined` selects the CDN font config, only an
  // explicit `null` disables it. Two of these have already shipped and leaked.
  // `@app/services/pdfiumEngine` pins every remote-defaulting option in one
  // place; nothing else may construct an engine.
  {
    files: ["editor/src/**/*.{js,mjs,jsx,ts,tsx}"],
    ignores: [desktopBridgeGlob],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            ...baseRestrictedImportPatterns,
            tauriImportRestriction,
            embedpdfEnginesImportRestriction,
          ],
        },
      ],
    },
  },
  // The one exception to the Tauri ban. Desktop behaviour lives in `core`
  // behind a runtime check, and every `@tauri-apps/*` module it needs is
  // reached by *dynamic* import from this directory alone — so the web bundle
  // never evaluates one, and there is exactly one place to look when asking
  // what the desktop shell is allowed to do. Everything else in the app goes
  // through the helpers this directory exports.
  {
    files: [desktopBridgeGlob],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            ...baseRestrictedImportPatterns,
            embedpdfEnginesImportRestriction,
          ],
        },
      ],
    },
  },
  // app code must use shared DS Button/SegmentedControl/Chip.
  {
    files: ["editor/src/**/*.{js,mjs,jsx,ts,tsx}"],
    ignores: [
      "editor/src/core/ui/**/*.{js,mjs,jsx,ts,tsx}", // the shared DS itself — wraps Mantine/raw elements
      "**/*.stories.{js,mjs,jsx,ts,tsx}", // stories may demo Mantine directly
      "**/*.test.{js,mjs,jsx,ts,tsx}", // tests may use raw elements as fixtures
    ],
    rules: {
      "no-restricted-syntax": ["error", ...sharedComponentSyntaxRestrictions],
    },
  },
  // Intentional exceptions: ARIA tablist tabs and sub-26px segmented header —
  // semantically not buttons; shared Button sizing can't represent them.
  // Do NOT add ordinary buttons here.
  {
    files: [
      "editor/src/core/components/shared/FileSelectorPicker.tsx",
      "editor/src/core/components/filesPage/FileManagerView.tsx",
      "editor/src/core/pages/HomePage.tsx",
    ],
    rules: {
      "no-restricted-syntax": "off",
    },
  },
  // Stricter rules that not all sub-folders are conformant to yet.
  {
    files: srcGlobs,
    ignores: [
      "editor/src/core/components/annotation/**/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/pageEditor/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/pageEditor/commands/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/pageEditor/hooks/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/shared/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/shared/config/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/shared/config/configSections/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/shared/pageEditor/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/tools/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/tools/addStamp/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/tools/automate/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/tools/bookletImposition/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/tools/certSign/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/tools/pdfTextEditor/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/tools/shared/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/components/viewer/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/contexts/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/contexts/file/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/contexts/viewer/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/hooks/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/hooks/signing/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/hooks/tools/adjustContrast/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/hooks/tools/convert/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/hooks/tools/removePassword/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/hooks/tools/shared/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/services/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/tools/annotate/useAnnotationSelection.ts",
      "editor/src/core/types/*.{js,mjs,jsx,ts,tsx}",
      "editor/src/core/utils/*.{js,mjs,jsx,ts,tsx}",
    ],
    rules: {
      "@typescript-eslint/no-explicit-any": "error",
    },
  },
  // Config for browser scripts
  {
    files: srcGlobs,
    languageOptions: {
      globals: {
        ...globals.browser,
      },
    },
  },
  // Config for node scripts
  {
    files: nodeGlobs,
    languageOptions: {
      globals: {
        ...globals.node,
      },
    },
    rules: {
      // The `@app/*` alias is created by vite-tsconfig-paths for application
      // code. These files are the build tooling itself — vite.config.ts is
      // loaded by Vite before any of that exists — so a relative path is the
      // only thing that resolves here, not a stylistic shortcut. The `src/`
      // half of the restriction still applies.
      "no-restricted-imports": [
        "error",
        {
          patterns: baseRestrictedImportPatterns.filter(
            (pattern) => pattern.regex !== "^\\.",
          ),
        },
      ],
    },
  },
);
