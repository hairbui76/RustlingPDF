/**
 * Which UI languages the desktop installer carries.
 *
 * All 42 translations cost 8.77 MB raw and 2.21 MB inside the installer — each
 * language is only ~54 KB compressed, so this list is worth about 1.5 MB in
 * total, not a headline saving. It is a desktop-only trim: a web or Docker
 * deployment fetches a translation lazily over HTTP when someone picks it, so
 * an unused language there costs disk on the server and nothing on the wire.
 * The desktop bundle embeds every file in the executable whether or not it is
 * ever read.
 *
 * The set is: the two source locales, the maintainer's own, and the languages
 * with the largest computing populations. It is deliberately a plain list —
 * changing who gets a translation should be one edit here plus a rebuild, not
 * an archaeology exercise.
 *
 * A language left out is genuinely absent from a desktop install, not merely
 * hidden: `supportedLanguages` is filtered through this at runtime so the
 * picker cannot offer one whose file was never shipped, which would otherwise
 * silently fall back to English and look like a broken translation.
 */
export const DESKTOP_SHIPPED_LOCALES = [
  // Source locales — `fallbackLng` resolves here, so they can never be dropped.
  "en-US",
  "en-GB",
  // Maintainer's locale.
  "vi-VN",
  // The rest, chosen by the maintainer for the markets this build targets.
  // Spanish, Brazilian Portuguese, Arabic and Indonesian were dropped on
  // request; they are one line each to restore.
  "zh-CN",
  "zh-TW",
  "ja-JP",
  "ko-KR",
  "fr-FR",
  "de-DE",
  "it-IT",
  "ru-RU",
  "hi-IN",
] as const;

export function isDesktopShippedLocale(code: string): boolean {
  return (DESKTOP_SHIPPED_LOCALES as readonly string[]).includes(code);
}
