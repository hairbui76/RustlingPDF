import { Menu, Tooltip } from "@mantine/core";
import { useTranslation } from "react-i18next";
import LightModeRoundedIcon from "@mui/icons-material/LightModeRounded";
import DarkModeRoundedIcon from "@mui/icons-material/DarkModeRounded";
import SettingsBrightnessRoundedIcon from "@mui/icons-material/SettingsBrightnessRounded";
import CheckRoundedIcon from "@mui/icons-material/CheckRounded";

import { Button } from "@app/ui/Button";
import { useTheme } from "@app/components/shared/ThemeProvider";
import type { ThemeMode } from "@app/constants/theme";
import "@app/components/shared/ThemeModeControl.css";

const ICONS: Record<ThemeMode, typeof LightModeRoundedIcon> = {
  light: LightModeRoundedIcon,
  dark: DarkModeRoundedIcon,
  system: SettingsBrightnessRoundedIcon,
};

/**
 * Theme switcher for the app chrome — the same `preferences.theme` the
 * Settings → General → Appearance control writes, surfaced where it can be
 * found without opening Settings. One preference, two entry points.
 *
 * Form: an icon+label trigger opening a three-option radio menu.
 *  - A segmented control shows all three at once but is far too wide for the
 *    3.5rem collapsed rail, and a permanent three-cell widget in the footer
 *    would out-shout the bottom group bar.
 *  - A button that cycles the modes is compact but never reveals what the
 *    other options are.
 *  - This shows the current mode at rest — as an icon *and*, when the sidebar
 *    is expanded, as text — and reveals the full set on demand.
 *
 * The icon tracks the chosen *mode*, not the resolved light/dark scheme, so
 * "System" stays distinguishable from "Light" while the OS is in light mode.
 */
export function ThemeModeControl({ collapsed }: { collapsed: boolean }) {
  const { t } = useTranslation();
  const { themeMode, setTheme } = useTheme();

  const labels: Record<ThemeMode, string> = {
    light: t("settings.general.themeLight", "Light"),
    dark: t("settings.general.themeDark", "Dark"),
    system: t("settings.general.themeSystem", "System"),
  };
  const modes: ThemeMode[] = ["light", "dark", "system"];
  const CurrentIcon = ICONS[themeMode];
  // Names the control *and* states the value, so a screen reader announces the
  // current mode without the user having to open the menu.
  const triggerLabel = t("fileSidebar.themeCurrent", "Theme: {{mode}}", {
    mode: labels[themeMode],
  });

  return (
    <Menu position="right-end" withinPortal shadow="md" width={180}>
      <Menu.Target>
        <Tooltip
          label={triggerLabel}
          position="right"
          withinPortal
          disabled={!collapsed}
        >
          <Button
            variant="quiet"
            size="sm"
            justify={collapsed ? "center" : "start"}
            className="theme-mode-control__trigger"
            data-collapsed={collapsed}
            aria-label={triggerLabel}
            leftSection={<CurrentIcon sx={{ fontSize: "1.1rem" }} />}
          >
            {collapsed ? undefined : labels[themeMode]}
          </Button>
        </Tooltip>
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Label>{t("settings.general.theme", "Theme")}</Menu.Label>
        {modes.map((mode) => {
          const Icon = ICONS[mode];
          const checked = mode === themeMode;
          return (
            <Menu.Item
              key={mode}
              role="menuitemradio"
              aria-checked={checked}
              onClick={() => setTheme(mode)}
              leftSection={<Icon sx={{ fontSize: "1rem" }} />}
              rightSection={
                checked ? (
                  <CheckRoundedIcon
                    sx={{ fontSize: "1rem" }}
                    aria-hidden="true"
                  />
                ) : undefined
              }
            >
              {labels[mode]}
            </Menu.Item>
          );
        })}
      </Menu.Dropdown>
    </Menu>
  );
}

export default ThemeModeControl;
