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
    // Strip comments FIRST. The doc comment in that module quotes
    // `fontFallback: null` while explaining the leak, so a naive match against
    // the raw file passes even when the real option has been deleted — this
    // guard was written that way, verified against a reintroduced regression,
    // and found to be useless. Only executable code counts.
    const code = readFileSync(join(SRC, ENGINE_WRAPPER), "utf8")
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/.*$/gm, "");

    // `fontFallback` MUST be an explicit null: undefined selects the CDN.
    expect(code, "fontFallback must be explicitly null").toMatch(
      /fontFallback:\s*null/,
    );
    // `wasmUrl` MUST be the locally emitted asset, never a literal URL.
    expect(code, "wasmUrl must be the local asset").toMatch(
      /wasmUrl:\s*pdfiumWasmUrl/,
    );
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
