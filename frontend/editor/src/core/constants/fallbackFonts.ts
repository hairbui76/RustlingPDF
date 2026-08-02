/**
 * Which Noto Sans faces ship as PDFium fallback fonts.
 *
 * `@embedpdf/fonts-latin` carries all 18 static faces — nine weights from Thin
 * (100) to Black (900), each with an italic — at roughly 0.61 MB apiece, which
 * is 10.9 MB raw and 4.55 MB inside the installer. That is the single largest
 * thing in the frontend payload, and it exists only to draw documents that
 * reference a font without embedding it.
 *
 * Four faces are enough because the engine does not require an exact match.
 * `findBestFontMatch` in @embedpdf/engines picks the closest weight (biased
 * toward the bolder side at 400 and above) and falls back to a non-italic face
 * when no italic one exists. So a document asking for Light gets Regular and
 * one asking for ExtraBold gets Bold — a slightly different stroke on text
 * that was already being substituted, rather than a missing glyph.
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
  "NotoSans-Italic.ttf",
  "NotoSans-Bold.ttf",
  "NotoSans-BoldItalic.ttf",
] as const;

export function isShippedLatinFont(file: string): boolean {
  return (SHIPPED_LATIN_FONT_FILES as readonly string[]).includes(file);
}
