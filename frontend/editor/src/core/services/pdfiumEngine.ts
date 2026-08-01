import { usePdfiumEngine } from "@embedpdf/engines/react"; // eslint-disable-line no-restricted-imports -- this module is the single sanctioned wrapper; see below
import { pdfiumWasmUrl } from "@app/services/wasmPrecompiler";

/**
 * The ONLY sanctioned way to construct an `@embedpdf` PDFium engine.
 *
 * `@embedpdf/engines` defaults **two** independent options to a public CDN, and
 * both defaults are reached by simply not passing the option:
 *
 * - `wasmUrl` defaults to `cdn.jsdelivr.net/npm/@embedpdf/pdfium@.../pdfium.wasm`;
 * - `fontFallback` resolves as `fontFallback === null ? undefined : fontFallback
 *   ?? cdnFontConfig`, so leaving it `undefined` selects the CDN font config —
 *   only an explicit `null` disables it.
 *
 * The second one shipped: a PDF using a non-embedded CJK font made the worker
 * issue a synchronous XHR to `cdn.jsdelivr.net/npm/@embedpdf/fonts-sc@latest/...`
 * on nothing more than the user opening a file, disclosing their IP, User-Agent
 * and indirectly the script system of the document they were reading.
 *
 * Two CDN defaults have now each been found the same way — by accident, in
 * production — so this wrapper exists to make a third one impossible to reach:
 * it takes no options, and ESLint forbids importing
 * `@embedpdf/engines/react` anywhere else. If the library adds another
 * remote-defaulting option, it gets pinned here, once.
 *
 * `fontFallback: null` means a PDF that references a font it does not embed
 * renders no glyphs for the affected runs. That is the same thing every user
 * behind a firewall, offline, or with jsdelivr down already saw, and the same
 * thing the rest of the app already does — `pdfiumService.ts`, which backs
 * every non-viewer tool, registers no font-fallback manager at all. Shipping
 * the fonts locally instead would cost ~11 MB for Latin (which covers
 * Cyrillic, Greek and Vietnamese), Arabic and Hebrew, and ~141 MB more for
 * CJK; that trade-off is a separate decision, and until it is made, not
 * rendering beats phoning home.
 */
export function useLocalPdfiumEngine() {
  return usePdfiumEngine({
    // Same-origin, emitted next to the app by the build.
    wasmUrl: pdfiumWasmUrl,
    // Explicit null — see above. `undefined` here means "use the CDN".
    fontFallback: null,
  });
}
