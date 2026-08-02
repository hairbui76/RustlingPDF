import { useMemo } from "react";
import { usePreferences } from "@app/contexts/PreferencesContext";
import { useAppConfig } from "@app/contexts/AppConfigContext";
import type { LogoVariant } from "@app/services/preferencesService";
import { DESKTOP_LOGO_VARIANT, ensureLogoVariant } from "@app/constants/logo";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export function useLogoVariant(): LogoVariant {
  const { preferences } = usePreferences();
  const { config } = useAppConfig();

  return useMemo(() => {
    // A desktop installer carries one variant, so there is nothing to resolve
    // and resolving anyway would render a broken image: with no config yet,
    // `ensureLogoVariant(undefined)` answers "modern", whose files that build
    // does not ship. See DESKTOP_LOGO_VARIANT.
    if (isDesktopRuntime()) return DESKTOP_LOGO_VARIANT;

    // Check local storage first, then fall back to server config
    const preferenceVariant = preferences.logoVariant;
    const configVariant = config?.logoStyle;
    return ensureLogoVariant(preferenceVariant ?? configVariant);
  }, [config?.logoStyle, preferences.logoVariant]);
}
