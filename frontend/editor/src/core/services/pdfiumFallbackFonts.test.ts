import { describe, it, expect } from "vitest";
import { FontCharset } from "@embedpdf/models";
import { fonts as latinFonts } from "@embedpdf/fonts-latin";

import { localFallbackFontConfig } from "@app/services/pdfiumFallbackFonts";
import { SHIPPED_LATIN_FONT_FILES } from "@app/constants/fallbackFonts";

/**
 * The font config and `vite.config.ts` read the same list, and these tests are
 * what keeps that true. Advertising a face the build does not copy costs a 404
 * per glyph run at render time — visible only when a PDF happens to reference
 * an unembedded font, which is exactly the case nobody exercises by hand.
 */
describe("pdfiumFallbackFonts", () => {
  const config = localFallbackFontConfig();
  const latin = config.fonts[FontCharset.ANSI];

  it("advertises only the faces the build copies", () => {
    const advertised = latin.map((v) => v.url.split("/").pop());
    expect(new Set(advertised)).toEqual(new Set(SHIPPED_LATIN_FONT_FILES));
  });

  it("still covers regular and bold uprights, and nothing italic", () => {
    // The engine picks the closest weight, so these two are what every other
    // weight degrades onto; italics degrade onto them too, by design since
    // 0.1.7 (see fallbackFonts.ts). Losing one silently widens the
    // substitution; regaining an italic silently re-adds half a megabyte.
    const shape = latin
      .map((v) => `${v.weight ?? 400}${v.italic ? "i" : ""}`)
      .sort();
    expect(shape).toEqual(["400", "700"]);
  });

  it("drops the majority of the package rather than a token few", () => {
    // Guards the saving itself: if a future dependency bump reintroduces the
    // full set, this fails instead of quietly adding 3.5 MB to the installer.
    expect(latinFonts.length).toBeGreaterThan(latin.length * 3);
  });

  it("keeps every URL same-origin", () => {
    // The reason these files are vendored at all: the package default points
    // at a CDN, which discloses the reader's IP and the document's script
    // system on open.
    for (const charset of Object.values(config.fonts)) {
      for (const variant of charset) {
        expect(variant.url).not.toMatch(/^https?:\/\/(?!localhost)/);
      }
    }
  });
});
