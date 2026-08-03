import { useEffect, useState } from "react";
import { Group, Text } from "@mantine/core";
import CloseIcon from "@mui/icons-material/Close";
import { useTranslation } from "react-i18next";
import { Button } from "@app/ui/Button";
import { usePreferences } from "@app/contexts/PreferencesContext";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import {
  checkForDesktopUpdate,
  installDesktopUpdate,
  type DesktopUpdateInfo,
  type DesktopUpdatePhase,
} from "@app/services/desktop/desktopUpdater";

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
 * dismissal — the check runs once per app start, never on a timer.
 */
export default function DesktopUpdateBanner() {
  const { t } = useTranslation();
  const { preferences } = usePreferences();
  const [update, setUpdate] = useState<DesktopUpdateInfo | null>(null);
  const [phase, setPhase] = useState<DesktopUpdatePhase | null>(null);
  const [failed, setFailed] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  const checkEnabled =
    isDesktopRuntime() && preferences.checkForUpdatesOnStartup;

  useEffect(() => {
    if (!checkEnabled) {
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      void checkForDesktopUpdate().then((info) => {
        if (!cancelled && info) {
          setUpdate(info);
        }
      });
    }, STARTUP_CHECK_DELAY_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [checkEnabled]);

  if (!update || dismissed) {
    return null;
  }

  const busy = phase !== null && !failed;

  const install = () => {
    setFailed(false);
    setPhase("downloading");
    installDesktopUpdate(setPhase).catch((error) => {
      console.error("[DesktopUpdateBanner] Install failed:", error);
      setFailed(true);
      setPhase(null);
    });
  };

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
      <Text size="sm" style={{ minWidth: 0 }} truncate>
        {failed
          ? t(
              "desktopUpdate.failed",
              "The update could not be installed. Try again, or download it from the releases page.",
            )
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
        leftSection={<CloseIcon fontSize="small" />}
        aria-label={t("desktopUpdate.dismiss", "Not now")}
        onClick={() => setDismissed(true)}
        disabled={busy}
      />
    </Group>
  );
}
