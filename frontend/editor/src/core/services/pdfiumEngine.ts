import { usePdfiumEngine } from "@embedpdf/engines/react"; // eslint-disable-line no-restricted-imports -- this module is the single sanctioned wrapper; see below
import { useMemo } from "react";
import { pdfiumWasmUrl } from "@app/services/wasmPrecompiler";
import { localFallbackFontConfig } from "@app/services/pdfiumFallbackFonts";

/**
 * The ONLY sanctioned way to construct an `@embedpdf` PDFium engine.
 *
 * `@embedpdf/engines` defaults **two** independent options to a public CDN, and
 * both defaults are reached by simply not passing the option:
 *
 * - `wasmUrl` defaults to `cdn.jsdelivr.net/npm/@embedpdf/pdfium@.../pdfium.wasm`;
 * - `fontFallback` resolves as `fontFallback === null ? undefined : fontFallback
 *   ?? cdnFontConfig`, so leaving it `undefined` selects the CDN font config.
 *
 * The second one shipped: a PDF using a non-embedded CJK font made the worker
 * issue a synchronous XHR to `cdn.jsdelivr.net/npm/@embedpdf/fonts-sc@latest/...`
 * on nothing more than the user opening a file, disclosing their IP, User-Agent
 * and indirectly the script system of the document they were reading.
 *
 * Two CDN defaults have now each been found the same way — by accident, in
 * production — so this wrapper exists to make a third one impossible to reach:
 * it takes no options, and ESLint forbids importing `@embedpdf/engines`
 * anywhere else. If the library adds another remote-defaulting option, it gets
 * pinned here, once.
 *
 * Both pinned values are same-origin: the WASM is emitted next to the app, and
 * the fallback fonts are copied out of `node_modules` at build time (see
 * `pdfiumFallbackFonts`). No code path here can fall through to
 * `cdnFontConfig`.
 */
export function useLocalPdfiumEngine() {
  // Stable identity: `usePdfiumEngine` lists `fontFallback` in its effect deps,
  // so a fresh object every render would tear the engine down and rebuild it.
  const fontFallback = useMemo(() => localFallbackFontConfig(), []);

  return usePdfiumEngine({
    // Same-origin, emitted next to the app by the build.
    wasmUrl: pdfiumWasmUrl,
    // Same-origin font set. NEVER leave this undefined — that selects the CDN.
    fontFallback,
  });
}
