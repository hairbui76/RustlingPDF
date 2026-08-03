import React, { useState, useEffect } from "react";
import {
  Paper,
  Stack,
  Switch,
  Text,
  Tooltip,
  NumberInput,
  Select,
} from "@mantine/core";
import { SegmentedControl } from "@app/ui/SegmentedControl";
import { useTranslation } from "react-i18next";
import { usePreferences } from "@app/contexts/PreferencesContext";
import { useAppConfig } from "@app/contexts/AppConfigContext";
import { useTheme } from "@app/components/shared/ThemeProvider";
import LanguageSelector from "@app/components/shared/LanguageSelector";
import { type ThemeMode } from "@app/constants/theme";
import type { ToolPanelMode } from "@app/constants/toolPanel";
import {
  type StartupView,
  type ViewerZoomSetting,
} from "@app/services/preferencesService";
import { Z_INDEX_OVER_CONFIG_MODAL } from "@app/styles/zIndex";
import { useFrontendVersionInfo } from "@app/hooks/useFrontendVersionInfo";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

const DEFAULT_AUTO_UNZIP_FILE_LIMIT = 4;

interface GeneralSectionProps {
  hideTitle?: boolean;
}

const GeneralSection: React.FC<GeneralSectionProps> = ({
  hideTitle = false,
}) => {
  const { t } = useTranslation();
  const { preferences, updatePreference } = usePreferences();
  const { config } = useAppConfig();
  const { setTheme, themeMode } = useTheme();
  const [fileLimitInput, setFileLimitInput] = useState<number | string>(
    preferences.autoUnzipFileLimit,
  );
  const { appVersion, mismatchVersion } = useFrontendVersionInfo(
    config?.appVersion,
  );
  const frontendVersionLabel = appVersion ?? t("common.loading", "Loading..."); // null = loading, shown only when appVersion !== undefined

  // Sync local state with preference changes
  useEffect(() => {
    setFileLimitInput(preferences.autoUnzipFileLimit);
  }, [preferences.autoUnzipFileLimit]);

  return (
    <Stack gap="lg">
      {!hideTitle && (
        <div>
          <Text fw={600} size="lg">
            {t("settings.general.title", "General")}
          </Text>
          <Text size="sm" c="dimmed">
            {t(
              "settings.general.description",
              "Configure general application preferences.",
            )}
          </Text>
        </div>
      )}

      {/* Version info. On web this stays purely local — see README "Privacy
          model". The desktop app has exactly one self-initiated request: the
          startup update check below, and its toggle lives right here. */}
      {(config?.appVersion || appVersion !== undefined) && (
        <Paper withBorder p="md" radius="md">
          <Stack gap="md">
            <div>
              <Text fw={600} size="sm">
                {t("settings.general.version.title", "Version")}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {isDesktopRuntime()
                  ? t(
                      "settings.general.version.descriptionDesktop",
                      "At startup the app asks GitHub once whether a newer version exists and offers it as a banner. Nothing else is ever sent anywhere.",
                    )
                  : t(
                      "settings.general.version.description",
                      "RustlingPDF does not check for updates. Visit the releases page when you want to see whether a newer version exists.",
                    )}
              </Text>
            </div>
            {isDesktopRuntime() && (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                }}
              >
                <div style={{ flex: 1, minWidth: 0 }}>
                  <Text fw={500} size="sm">
                    {t(
                      "settings.general.updates.checkOnStartup",
                      "Check for updates at startup",
                    )}
                  </Text>
                  <Text size="xs" c="dimmed" mt={4}>
                    {t(
                      "settings.general.updates.checkOnStartupDescription",
                      "Turning this off stops the only network request the app makes on its own. Updates are then manual via the releases page.",
                    )}
                  </Text>
                </div>
                <Switch
                  checked={preferences.checkForUpdatesOnStartup}
                  onChange={(event) =>
                    updatePreference(
                      "checkForUpdatesOnStartup",
                      event.currentTarget.checked,
                    )
                  }
                />
              </div>
            )}
            {appVersion !== undefined && (
              <div>
                <Text size="sm" c="dimmed">
                  {t(
                    "settings.general.updates.currentFrontendVersion",
                    "Current Frontend Version",
                  )}
                  :{" "}
                  <Text component="span" fw={500}>
                    {frontendVersionLabel}
                  </Text>
                </Text>
                {mismatchVersion && (
                  <Text size="sm" c="red" mt={4}>
                    {t(
                      "settings.general.updates.versionMismatch",
                      "Warning: A mismatch has been detected between the client version and the AppConfig version. Using different versions can lead to compatibility issues, errors, and security risks. Please ensure that server and client are using the same version.",
                    )}
                  </Text>
                )}
              </div>
            )}
            {config?.appVersion && (
              <Text size="sm" c="dimmed">
                {t(
                  "settings.general.updates.currentBackendVersion",
                  "Current Backend Version",
                )}
                :{" "}
                <Text component="span" fw={500}>
                  {config.appVersion}
                </Text>
              </Text>
            )}
          </Stack>
        </Paper>
      )}

      {/* Appearance */}
      <Paper withBorder p="md" radius="md">
        <Stack gap="md">
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <Text fw={500} size="sm">
                {t("settings.general.theme", "Theme")}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {t(
                  "settings.general.themeDescription",
                  "Choose light, dark, or follow your system so it switches automatically.",
                )}
              </Text>
            </div>
            <SegmentedControl
              value={themeMode}
              onChange={(val) => setTheme(val as ThemeMode)}
              options={[
                {
                  label: t("settings.general.themeLight", "Light"),
                  value: "light",
                },
                {
                  label: t("settings.general.themeDark", "Dark"),
                  value: "dark",
                },
                {
                  label: t("settings.general.themeSystem", "System"),
                  value: "system",
                },
              ]}
            />
          </div>
        </Stack>
      </Paper>

      {/* Language */}
      <Paper withBorder p="md" radius="md">
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <div style={{ flex: 1, minWidth: 0 }}>
            <Text fw={500} size="sm">
              {t("settings.general.language", "Language")}
            </Text>
            <Text size="xs" c="dimmed" mt={4}>
              {t(
                "settings.general.languageDescription",
                "Choose the display language",
              )}
            </Text>
          </div>
          <LanguageSelector position="bottom-end" offset={6} />
        </div>
      </Paper>

      <Paper withBorder p="md" radius="md">
        <Stack gap="md">
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <Text fw={500} size="sm">
                {t(
                  "settings.general.defaultToolPickerMode",
                  "Default tool picker mode",
                )}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {t(
                  "settings.general.defaultToolPickerModeDescription",
                  "Choose whether the tool picker opens in fullscreen or sidebar by default",
                )}
              </Text>
            </div>
            <SegmentedControl
              value={preferences.defaultToolPanelMode}
              onChange={(val: string) =>
                updatePreference("defaultToolPanelMode", val as ToolPanelMode)
              }
              options={[
                {
                  label: t("settings.general.mode.sidebar", "Sidebar"),
                  value: "sidebar",
                },
                {
                  label: t("settings.general.mode.fullscreen", "Fullscreen"),
                  value: "fullscreen",
                },
              ]}
            />
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <Text fw={500} size="sm">
                {t(
                  "settings.general.defaultStartupView",
                  "Default view on launch",
                )}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {t(
                  "settings.general.defaultStartupViewDescription",
                  "Choose which view is active when the app starts",
                )}
              </Text>
            </div>
            <SegmentedControl
              value={preferences.defaultStartupView}
              onChange={(val: string) =>
                updatePreference("defaultStartupView", val as StartupView)
              }
              options={[
                {
                  label: t("settings.general.startupView.tools", "Tools"),
                  value: "tools",
                },
                {
                  label: t("settings.general.startupView.read", "Reader"),
                  value: "read",
                },
                {
                  label: t("settings.general.startupView.automate", "Automate"),
                  value: "automate",
                },
              ]}
            />
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <Text fw={500} size="sm">
                {t("settings.general.defaultViewerZoom", "Default reader zoom")}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {t(
                  "settings.general.defaultViewerZoomDescription",
                  "Set the default zoom level when opening PDFs in the reader",
                )}
              </Text>
            </div>
            <Select
              value={preferences.defaultViewerZoom}
              onChange={(val: string | null) => {
                if (val)
                  updatePreference(
                    "defaultViewerZoom",
                    val as ViewerZoomSetting,
                  );
              }}
              data={[
                {
                  label: t("settings.general.zoomLevel.auto", "Auto"),
                  value: "auto",
                },
                {
                  label: t("settings.general.zoomLevel.fitWidth", "Fit width"),
                  value: "fitWidth",
                },
                {
                  label: t("settings.general.zoomLevel.fitPage", "Fit page"),
                  value: "fitPage",
                },
                { label: "50%", value: "50" },
                { label: "75%", value: "75" },
                { label: "100%", value: "100" },
                { label: "125%", value: "125" },
                { label: "150%", value: "150" },
                { label: "200%", value: "200" },
              ]}
              style={{ width: 140 }}
              allowDeselect={false}
              comboboxProps={{
                withinPortal: true,
                zIndex: Z_INDEX_OVER_CONFIG_MODAL,
              }}
            />
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <Text fw={500} size="sm">
                {t(
                  "settings.general.hideUnavailableTools",
                  "Hide unavailable tools",
                )}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {t(
                  "settings.general.hideUnavailableToolsDescription",
                  "Remove tools that have been disabled by your server instead of showing them greyed out.",
                )}
              </Text>
            </div>
            <Switch
              checked={preferences.hideUnavailableTools}
              onChange={(event) =>
                updatePreference(
                  "hideUnavailableTools",
                  event.currentTarget.checked,
                )
              }
            />
          </div>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <Text fw={500} size="sm">
                {t(
                  "settings.general.hideUnavailableConversions",
                  "Hide unavailable conversions",
                )}
              </Text>
              <Text size="xs" c="dimmed" mt={4}>
                {t(
                  "settings.general.hideUnavailableConversionsDescription",
                  "Remove disabled conversion options in the Convert tool instead of showing them greyed out.",
                )}
              </Text>
            </div>
            <Switch
              checked={preferences.hideUnavailableConversions}
              onChange={(event) =>
                updatePreference(
                  "hideUnavailableConversions",
                  event.currentTarget.checked,
                )
              }
            />
          </div>
          <Tooltip
            label={t(
              "settings.general.autoUnzipTooltip",
              "Automatically extract ZIP files returned from API operations. Disable to keep ZIP files intact. This does not affect automation workflows.",
            )}
            multiline
            w={300}
            withArrow
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                cursor: "help",
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <Text fw={500} size="sm">
                  {t("settings.general.autoUnzip", "Auto-unzip API responses")}
                </Text>
                <Text size="xs" c="dimmed" mt={4}>
                  {t(
                    "settings.general.autoUnzipDescription",
                    "Automatically extract files from ZIP responses",
                  )}
                </Text>
              </div>
              <Switch
                checked={preferences.autoUnzip}
                onChange={(event) =>
                  updatePreference("autoUnzip", event.currentTarget.checked)
                }
              />
            </div>
          </Tooltip>

          <Tooltip
            label={t(
              "settings.general.autoUnzipFileLimitTooltip",
              "Only unzip if the ZIP contains this many files or fewer. Set higher to extract larger ZIPs.",
            )}
            multiline
            w={300}
            withArrow
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                cursor: "help",
              }}
            >
              <div style={{ flex: 1, minWidth: 0 }}>
                <Text fw={500} size="sm">
                  {t(
                    "settings.general.autoUnzipFileLimit",
                    "Auto-unzip file limit",
                  )}
                </Text>
                <Text size="xs" c="dimmed" mt={4}>
                  {t(
                    "settings.general.autoUnzipFileLimitDescription",
                    "Maximum number of files to extract from ZIP",
                  )}
                </Text>
              </div>
              <NumberInput
                value={fileLimitInput}
                onChange={setFileLimitInput}
                onBlur={() => {
                  const numValue = Number(fileLimitInput);
                  const finalValue =
                    !fileLimitInput ||
                    isNaN(numValue) ||
                    numValue < 1 ||
                    numValue > 100
                      ? DEFAULT_AUTO_UNZIP_FILE_LIMIT
                      : numValue;
                  setFileLimitInput(finalValue);
                  updatePreference("autoUnzipFileLimit", finalValue);
                }}
                min={1}
                max={100}
                step={1}
                disabled={!preferences.autoUnzip}
                style={{ width: 90 }}
              />
            </div>
          </Tooltip>
        </Stack>
      </Paper>
    </Stack>
  );
};

export default GeneralSection;
