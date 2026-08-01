import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, dirname } from "node:path";
import { fileURLToPath } from "node:url";

/**
 * Regression guard for a leak that shipped.
 *
 * `@embedpdf/engines` defaults BOTH `wasmUrl` and `fontFallback` to
 * `cdn.jsdelivr.net`, and both defaults are selected by *omitting* the option.
 * The worker resolves `fontFallback === null ? undefined : fontFallback ??
 * cdnFontConfig`, so `undefined` means "use the CDN" — the opposite of what it
 * reads like. A viewer call site that passed `wasmUrl` but not `fontFallback`
 * therefore issued a synchronous XHR to
 * `cdn.jsdelivr.net/npm/@embedpdf/fonts-sc@latest/...` whenever a user opened a
 * PDF with a non-embedded CJK font, leaking IP, User-Agent, and indirectly the
 * document's script system. Vietnamese is on the same charset list.
 *
 * These tests fail if that shape can reappear.
 */

const SRC = join(dirname(fileURLToPath(import.meta.url)), "..", ".."); // editor/src/
const ENGINE_WRAPPER = "core/services/pdfiumEngine.ts";

function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...sourceFiles(full));
    } else if (/\.(ts|tsx)$/.test(entry)) {
      out.push(full);
    }
  }
  return out;
}

describe("no remote asset defaults", () => {
  const files = sourceFiles(SRC);

  it("only the sanctioned wrapper may import @embedpdf/engines", () => {
    const offenders = files
      .filter((f) =>
        /from ["']@embedpdf\/engines/.test(readFileSync(f, "utf8")),
      )
      .map((f) => relative(SRC, f))
      .filter((f) => f !== ENGINE_WRAPPER);

    expect(
      offenders,
      "Import useLocalPdfiumEngine from @app/services/pdfiumEngine instead. " +
        "Constructing the engine directly re-opens the CDN defaults.",
    ).toEqual([]);
  });

  it("the wrapper pins every remote-defaulting option to a local value", () => {
    // Strip comments FIRST. The doc comment in that module quotes the option
    // names while explaining the leak, so a naive match against the raw file
    // passes even when the real option has been deleted — this guard was
    // written that way, verified against a reintroduced regression, and found
    // to be useless. Only executable code counts.
    const code = readFileSync(join(SRC, ENGINE_WRAPPER), "utf8")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/.*$/gm, "");

    // `fontFallback` MUST be passed. `undefined` selects the CDN font config,
    // so omitting it is the exact bug this file exists to prevent.
    expect(code, "fontFallback must be passed explicitly").toMatch(
      /fontFallback[,:]/,
    );
    // ...and it must be our local config, not the library's.
    expect(code, "fontFallback must come from localFallbackFontConfig").toMatch(
      /localFallbackFontConfig\(\)/,
    );
    // `wasmUrl` MUST be the locally emitted asset, never a literal URL.
    expect(code, "wasmUrl must be the local asset").toMatch(
      /wasmUrl:\s*pdfiumWasmUrl/,
    );
  });

  it("every fallback font URL is same-origin", async () => {
    // URLs are absolute on purpose: the config is posted into a Web Worker,
    // which would otherwise resolve a relative path against its own blob URL.
    // Absolute is fine; absolute *to another host* is the bug.
    const { localFallbackFontConfig } =
      await import("@app/services/pdfiumFallbackFonts");
    const urls = Object.values(localFallbackFontConfig().fonts)
      .flat()
      .map((variant) => (variant as { url: string }).url);

    expect(urls.length).toBeGreaterThan(0);
    const offOrigin = urls.filter(
      (url) => new URL(url, window.location.href).origin !== window.origin,
    );
    expect(
      offOrigin,
      "fallback fonts must be served from the app's own origin",
    ).toEqual([]);
    expect(urls.every((url) => url.includes("/fonts/"))).toBe(true);
  });

  it("no source file hardcodes a third-party asset host", () => {
    // Hosts the app must never reach on its own initiative. Documentation of a
    // removed leak is fine; a value in code is not, so only non-comment lines
    // are considered.
    const forbidden =
      /(cdn\.jsdelivr\.net|api\.iconify\.design|api\.github\.com|unpkg\.com|cdnjs\.cloudflare\.com)/;
    const offenders: string[] = [];
    for (const file of files) {
      readFileSync(file, "utf8")
        .split("\n")
        .forEach((line, index) => {
          const code = line.trim();
          if (code.startsWith("//") || code.startsWith("*")) return;
          if (forbidden.test(code)) {
            offenders.push(
              `${relative(SRC, file)}:${index + 1} ${code.slice(0, 100)}`,
            );
          }
        });
    }
    expect(offenders).toEqual([]);
  });
});
