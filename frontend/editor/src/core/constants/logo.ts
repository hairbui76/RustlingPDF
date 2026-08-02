import type { LogoVariant } from "@app/services/preferencesService";

export const LOGO_FOLDER_BY_VARIANT: Record<LogoVariant, string> = {
  modern: "modern-logo",
  classic: "classic-logo",
};

export const ensureLogoVariant = (value?: string | null): LogoVariant => {
  return value === "classic" ? "classic" : "modern";
};

export const getLogoFolder = (variant?: LogoVariant | null): string => {
  return LOGO_FOLDER_BY_VARIANT[ensureLogoVariant(variant)];
};

/**
 * The one variant a desktop installer carries.
 *
 * Both sets used to ship because the two defaults disagree: the backend
 * reports `classic` (`runtime_config.rs`), while `ensureLogoVariant` above
 * answers `modern` for anything that is not literally "classic" — including
 * the `undefined` it sees before the app config has loaded, or if the sidecar
 * is not answering yet. So a desktop launch really did render modern first and
 * classic a moment later, and pruning either one on its own left a broken
 * image on screen; that is how this was caught.
 *
 * `useLogoVariant` pins desktop to this value so the transient state cannot
 * occur, and `vite.config.ts` prunes the other folder. One constant, so the
 * files on disk and the folder the app asks for cannot disagree again.
 */
export const DESKTOP_LOGO_VARIANT: LogoVariant = "classic";
