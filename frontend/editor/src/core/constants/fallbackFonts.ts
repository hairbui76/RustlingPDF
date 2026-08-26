/**
 * Which Noto Sans faces ship as PDFium fallback fonts.
 *
 * `@embedpdf/fonts-latin` carries all 18 static faces — nine weights from Thin
 * (100) to Black (900), each with an italic — at roughly 0.61 MB apiece, which
 * is 10.9 MB raw and 4.55 MB inside the installer. That is the single largest
 * thing in the frontend payload, and it exists only to draw documents that
 * reference a font without embedding it.
 *
 * Two faces are enough because the engine does not require an exact match.
 * `findBestFontMatch` in @embedpdf/engines picks the closest weight (biased
 * toward the bolder side at 400 and above) and falls back to a non-italic face
 * when no italic one exists. So a document asking for Light gets Regular, one
 * asking for ExtraBold gets Bold, and one asking for Italic gets the upright
 * face — a different stroke or slant on text that was already being
 * substituted, rather than a missing glyph.
 *
 * The italic faces shipped from 0.0.x to 0.1.6 (four faces, ~1.2 MB raw /
 * ~0.5 MB installer). They were dropped in 0.1.7 as a size call: italic text
 * in a document that failed to embed its own font is rare enough that paying
 * half a megabyte on every download for its slant was judged the wrong trade.
 * Restoring them is adding "NotoSans-Italic.ttf" and "NotoSans-BoldItalic.ttf"
 * back to this list — nothing else knows they exist.
 *
 * This list is the single source of truth: `vite.config.ts` copies exactly
 * these files, and `pdfiumFallbackFonts.ts` filters the package metadata
 * through it. Editing one without the other would either advertise a font that
 * is not on disk (a 404 per glyph run) or ship a file nothing asks for.
 *
 * Arabic and Hebrew are not filtered — together they are under 0.35 MB, and
 * each has few enough faces that dropping any would change coverage rather
 * than weight.
 */
export const SHIPPED_LATIN_FONT_FILES = [
  "NotoSans-Regular.ttf",
  "NotoSans-Bold.ttf",
] as const;

export function isShippedLatinFont(file: string): boolean {
  return (SHIPPED_LATIN_FONT_FILES as readonly string[]).includes(file);
}
