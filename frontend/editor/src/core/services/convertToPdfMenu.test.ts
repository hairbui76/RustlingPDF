import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { act, renderHook } from "@testing-library/react";
import { useConvertParameters } from "@app/hooks/tools/convert/useConvertParameters";
import {
  TOOL_INTENT_ACTIONS,
  resolveToolIntent,
} from "@app/services/toolIntentService";

/**
 * Keeps the Windows Explorer "Convert to PDF with RustlingPDF" verb honest.
 *
 * The MSI registers that verb for every extension in the
 * `ConvertToPdfExtensions` define of `provisioning.wxs`, and each verb
 * launches the app with `--tool convert`. The launch lands on the Convert
 * tool with the files selected; nothing tells the tool the user asked for
 * PDF. That only works because every registered source format has PDF as its
 * sole conversion target, so auto-detection preselects "<ext> → PDF" and the
 * Convert button is ready. An extension with no PDF route, or with several
 * targets, would open onto an empty or wrong setting — this test fails first.
 *
 * The NSIS installer registers the same menu from `windows/nsis/hooks.nsh`;
 * the last block pins that file to the MSI so the two installers can never
 * offer different menus.
 */

const HERE = dirname(fileURLToPath(import.meta.url));
const EDITOR_ROOT = join(HERE, "..", "..", "..");
const PROVISIONING_WXS = join(
  EDITOR_ROOT,
  "src-tauri",
  "windows",
  "wix",
  "provisioning.wxs",
);

const NSIS_HOOKS = join(
  EDITOR_ROOT,
  "src-tauri",
  "windows",
  "nsis",
  "hooks.nsh",
);

const wxs = readFileSync(PROVISIONING_WXS, "utf8");
const nsh = readFileSync(NSIS_HOOKS, "utf8");

/** `[key, label, action]` for each .pdf cascade verb the MSI authors. */
function msiPdfCascadeVerbs(): string[][] {
  const labels = new Map(
    [
      ...wxs.matchAll(
        /\\\.pdf\\shell\\RustlingPDF\\shell\\(\w+)" ForceDeleteOnUninstall="yes">\s*<RegistryValue Type="string" Name="MUIVerb" Value="([^"]+)"/g,
      ),
    ].map((m) => [m[1], m[2]]),
  );
  return [
    ...wxs.matchAll(
      /\\\.pdf\\shell\\RustlingPDF\\shell\\(\w+)\\command">\s*<RegistryValue Type="string" Value="&quot;\[!Path\]&quot; --tool (\w+) &quot;%1&quot;"/g,
    ),
  ].map((m) => [m[1], labels.get(m[1]) ?? "", m[2]]);
}

function registeredExtensions(): string[] {
  const match = wxs.match(
    /<\?define\s+ConvertToPdfExtensions\s*=\s*"([^"]+)"\s*\?>/,
  );
  if (!match) {
    throw new Error(
      "ConvertToPdfExtensions define not found in provisioning.wxs",
    );
  }
  return match[1].split(";").map((ext) => ext.trim());
}

function analyze(names: string[]) {
  const { result } = renderHook(() => useConvertParameters());
  act(() => {
    result.current.analyzeFileTypes(names.map((name) => ({ name })));
  });
  return result.current;
}

describe("Explorer Convert to PDF verb", () => {
  const extensions = registeredExtensions();

  it("registers a non-empty set of distinct, WiX-identifier-safe extensions", () => {
    expect(extensions.length).toBeGreaterThan(0);
    expect(new Set(extensions).size).toBe(extensions.length);
    for (const ext of extensions) {
      // The extension is spliced into a Component Id and a registry path.
      expect(ext).toMatch(/^[a-z0-9]+$/);
      expect(ext).not.toBe("pdf");
    }
  });

  it("covers the headline office formats", () => {
    expect(extensions).toEqual(
      expect.arrayContaining(["doc", "docx", "xlsx", "pptx"]),
    );
  });

  it.each(registeredExtensions())(
    "preselects a valid .%s → PDF conversion",
    (ext) => {
      const params = analyze([`report.${ext}`]);
      // A recognised source format, not the "file-<ext>"/"any" fallback that
      // accepts anything — that would let an unsupported extension pass.
      expect(params.parameters.fromExtension).toBe(
        ext === "jpeg" ? "jpg" : ext,
      );
      expect(params.parameters.toExtension).toBe("pdf");
      expect(params.validateParameters()).toBe(true);
      expect(params.getEndpointName()).not.toBe("");
    },
  );

  it("preselects PDF for a mixed multi-select of registered formats", () => {
    const params = analyze(["a.docx", "b.xlsx", "c.png"]);
    expect(params.parameters.toExtension).toBe("pdf");
    expect(params.validateParameters()).toBe(true);
  });

  it("launches every verb with the convert intent, which routes to the Convert tool", () => {
    // Both the per-extension components and their ComponentRefs are generated
    // from the same define, so neither can drift from the other.
    expect(
      wxs.match(/<\?foreach Ext in \$\(var\.ConvertToPdfExtensions\)\?>/g),
    ).toHaveLength(2);
    expect(wxs).toContain('<ComponentRef Id="ConvertToPdfMenu_$(var.Ext)" />');
    const verbBlock = wxs.match(
      /<\?foreach Ext in \$\(var\.ConvertToPdfExtensions\)\?>([\s\S]*?)<\?endforeach\?>/,
    )?.[1];
    expect(verbBlock).toContain(
      'Value="&quot;[!Path]&quot; --tool convert &quot;%1&quot;"',
    );
    expect(TOOL_INTENT_ACTIONS).toContain("convert");
    expect(resolveToolIntent("convert")).toBe("convert");
  });
});

describe("NSIS installer Explorer menu", () => {
  it("registers the Convert to PDF verb for the same extensions, in the same order", () => {
    const nsisExtensions = [
      ...nsh.matchAll(/!insertmacro \$\{_MACRO\} "([^"]+)"/g),
    ].map((m) => m[1]);
    expect(nsisExtensions).toEqual(registeredExtensions());
  });

  it("uses the MSI's Convert to PDF key, label and command", () => {
    expect(nsh).toContain(
      '!define RUSTLING_CONVERT_VERB "RustlingPDF.ConvertToPdf"',
    );
    expect(wxs).toContain("\\shell\\RustlingPDF.ConvertToPdf");
    expect(nsh).toContain('"MUIVerb" "Convert to PDF with RustlingPDF"');
    expect(wxs).toContain('Value="Convert to PDF with RustlingPDF"');
    expect(nsh).toContain(
      '\\command" "" "$\\"$INSTDIR\\${MAINBINARYNAME}.exe$\\" --tool convert $\\"%1$\\""',
    );
  });

  it("mirrors every .pdf cascade verb of the MSI", () => {
    const msiVerbs = msiPdfCascadeVerbs();
    const nsisVerbs = [
      ...nsh.matchAll(
        /!insertmacro RUSTLING_WRITE_PDF_CASCADE_VERB "(\w+)" "([^"]+)" "(\w+)"/g,
      ),
    ].map((m) => [m[1], m[2], m[3]]);
    expect(msiVerbs.length).toBeGreaterThan(0);
    expect(nsisVerbs).toEqual(msiVerbs);
    // Every cascade action is a launch intent the app understands.
    expect(msiVerbs.map(([, , action]) => action)).toEqual([
      ...TOOL_INTENT_ACTIONS,
    ]);
  });

  it("defines both hooks the template inserts", () => {
    expect(nsh).toMatch(/^!macro NSIS_HOOK_POSTINSTALL\r?$/m);
    expect(nsh).toMatch(/^!macro NSIS_HOOK_POSTUNINSTALL\r?$/m);
  });
});
