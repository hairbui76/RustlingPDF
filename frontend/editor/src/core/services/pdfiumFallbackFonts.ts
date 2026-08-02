import { FontCharset } from "@embedpdf/models";
import { fonts as latinFonts } from "@embedpdf/fonts-latin";
import { fonts as arabicFonts } from "@embedpdf/fonts-arabic";
import { fonts as hebrewFonts } from "@embedpdf/fonts-hebrew";
import { BASE_PATH } from "@app/constants/app";
import { isShippedLatinFont } from "@app/constants/fallbackFonts";

/**
 * Same-origin fallback fonts for PDFs that reference a font without embedding
 * it.
 *
 * `@embedpdf`'s built-in config points these at `cdn.jsdelivr.net`, which
 * disclosed the reader's IP, User-Agent and — through the charset requested —
 * the script system of the document being read, on nothing more than opening a
 * file. The font files are therefore copied out of `node_modules` at build time
 * by `viteStaticCopy` (see `vite.config.ts`) and served from our own origin,
 * exactly as `pdfium.wasm` already is.
 *
 * The variant lists come from the font packages' own metadata rather than being
 * retyped here, so this config cannot drift from the files that get copied.
 * Latin is narrowed to the four faces in `constants/fallbackFonts.ts`, which is
 * also what `vite.config.ts` copies — one list, read by both.
 *
 * Coverage is deliberately partial:
 *
 * | charset                      | shipped | size    |
 * | ---------------------------- | ------- | ------- |
 * | Cyrillic, Greek, Vietnamese  | yes     | Latin   |
 * | Arabic                       | yes     | ~0.3 MB |
 * | Hebrew                       | yes     | ~0.05 MB|
 * | Shift-JIS, Hangeul, GB2312, Big5 | NO  | ~141 MB |
 *
 * CJK is not shipped because it is ~141 MB — an order of magnitude more than
 * the whole rest of the application. A CJK PDF that does not embed its fonts
 * renders no glyphs for those runs. That is a real limitation, it is stated in
 * the README and the desktop contract, and it is the deliberate trade: no
 * document, of any script, causes a request to anyone else.
 */

/** Absolute so the worker resolves it against the app, not its own blob URL. */
function fontsBase(): string {
  const path = `${BASE_PATH}/fonts/`;
  return typeof window === "undefined"
    ? path
    : new URL(path, window.location.href).href;
}

interface FontMeta {
  file: string;
  weight?: number;
  italic?: boolean;
}

function variants(fonts: readonly FontMeta[], directory: string) {
  const base = fontsBase();
  return fonts.map((font) => ({
    url: `${base}${directory}/${font.file}`,
    weight: font.weight,
    italic: font.italic,
  }));
}

/**
 * The font config passed to the PDFium engine. Every URL is same-origin; there
 * is no entry that could resolve to a third-party host.
 */
export function localFallbackFontConfig() {
  // Latin ships Cyrillic, Greek and Vietnamese glyphs in the same faces.
  //
  // Filtered to the faces the build actually copies. Advertising the other
  // fourteen would make PDFium request a file that is not there — a 404 per
  // glyph run instead of a substituted stroke — so this filter and
  // vite.config.ts read the same list.
  const latin = variants(
    latinFonts.filter((font) => isShippedLatinFont(font.file)),
    "latin",
  );
  return {
    fonts: {
      [FontCharset.CYRILLIC]: latin,
      [FontCharset.GREEK]: latin,
      [FontCharset.VIETNAMESE]: latin,
      [FontCharset.ANSI]: latin,
      [FontCharset.ARABIC]: variants(arabicFonts, "arabic"),
      [FontCharset.HEBREW]: variants(hebrewFonts, "hebrew"),
    },
  };
}
