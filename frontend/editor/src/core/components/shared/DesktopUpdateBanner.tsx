import { useEffect, useState } from "react";
import { LocalIcon } from "@app/components/shared/LocalIcon";
import { Group, Text } from "@mantine/core";
import { useTranslation } from "react-i18next";
import { Button } from "@app/ui/Button";
import { usePreferences } from "@app/contexts/PreferencesContext";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import { useDesktopUpdate } from "@app/hooks/useDesktopUpdate";

/**
 * Wait this long after mount before asking GitHub for a newer version. The
 * first seconds of a desktop launch are the busy ones — the sidecar boots and
 * lib.rs reloads the webview once the backend port is known, which would
 * throw away an earlier check's result anyway.
 */
const STARTUP_CHECK_DELAY_MS = 10_000;

/**
 * One-line banner above the app content when a newer desktop version exists.
 *
 * Renders nothing on web, when up to date, when the user turned the startup
 * check off, when the check fails (offline, deb/rpm installs), and after a
 * dismissal — the check runs once per app start, never on a timer. Settings →
 * General has the button for asking again on purpose.
 */
export default function DesktopUpdateBanner() {
  const { t } = useTranslation();
  const { preferences } = usePreferences();
  const { update, phase, failure, busy, check, install } = useDesktopUpdate();
  const [dismissed, setDismissed] = useState(false);

  const checkEnabled =
    isDesktopRuntime() && preferences.checkForUpdatesOnStartup;

  useEffect(() => {
    if (!checkEnabled) {
      return;
    }
    const timer = setTimeout(() => {
      void check();
    }, STARTUP_CHECK_DELAY_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [checkEnabled, check]);

  if (!update || dismissed) {
    return null;
  }

  return (
    <Group
      justify="center"
      gap="sm"
      px="md"
      py={6}
      wrap="nowrap"
      bg="var(--mantine-color-blue-light)"
      data-testid="desktop-update-banner"
    >
      <Text
        size="sm"
        style={{ minWidth: 0 }}
        truncate
        // The line truncates on a narrow window, so the reason stays reachable
        // on hover instead of being cut off.
        title={failure ?? undefined}
      >
        {failure !== null
          ? `${t(
              "desktopUpdate.failed",
              "The update could not be installed. Try again, or download it from the releases page.",
            )} (${failure})`
          : phase === "installing"
            ? t("desktopUpdate.installing", "Installing update…")
            : phase === "downloading"
              ? t("desktopUpdate.downloading", "Downloading update…")
              : t("desktopUpdate.available", {
                  defaultValue: "RustlingPDF {{version}} is available.",
                  version: update.version,
                })}
      </Text>
      <Button
        variant="primary"
        size="sm"
        onClick={install}
        loading={busy}
        data-testid="desktop-update-install"
      >
        {t("desktopUpdate.installButton", "Update and restart")}
      </Button>
      <Button
        variant="quiet"
        size="sm"
        leftSection={
          <LocalIcon icon="close-rounded" width="1.25rem" height="1.25rem" />
        }
        aria-label={t("desktopUpdate.dismiss", "Not now")}
        onClick={() => setDismissed(true)}
        disabled={busy}
      />
    </Group>
  );
}
