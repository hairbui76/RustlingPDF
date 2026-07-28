import { ReactNode, useEffect, useRef, useState } from "react";
import { AppProviders as ProprietaryAppProviders } from "@proprietary/components/AppProviders";
import { DesktopConfigSync } from "@app/components/DesktopConfigSync";
import { DesktopBannerInitializer } from "@app/components/DesktopBannerInitializer";
import { SaveShortcutListener } from "@app/components/SaveShortcutListener";
import { DesktopOnboardingModal } from "@app/components/DesktopOnboardingModal";
import { useFirstLaunchCheck } from "@app/hooks/useFirstLaunchCheck";
import { useBackendInitializer } from "@app/hooks/useBackendInitializer";
import { DESKTOP_DEFAULT_APP_CONFIG } from "@app/config/defaultAppConfig";
import { connectionModeService } from "@app/services/connectionModeService";
import { tauriBackendService } from "@app/services/tauriBackendService";
import { endpointAvailabilityService } from "@app/services/endpointAvailabilityService";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@tauri-apps/api/core";
import UpdateModal from "@core/components/shared/UpdateModal";
import { useDesktopUpdatePopup } from "@app/hooks/useDesktopUpdatePopup";

// Common tool endpoints to preload for faster first-use
const COMMON_TOOL_ENDPOINTS = [
  "/api/v1/misc/compress-pdf",
  "/api/v1/general/merge-pdfs",
  "/api/v1/general/split-pages",
  "/api/v1/convert/pdf/img",
  "/api/v1/convert/img/pdf",
  "/api/v1/general/rotate-pdf",
  "/api/v1/misc/add-watermark",
  "/api/v1/security/add-password",
  "/api/v1/security/remove-password",
  "/api/v1/general/extract-pages",
];

/**
 * Desktop application providers
 * Wraps proprietary providers and adds desktop-specific configuration
 * - Enables retry logic for app config (needed for Tauri mode when backend is starting)
 * - Starts the bundled backend on first launch and shows the onboarding modal
 */
export function AppProviders({ children }: { children: ReactNode }) {
  const { isFirstLaunch, setupComplete } = useFirstLaunchCheck();
  const updatePopup = useDesktopUpdatePopup();
  const [bootstrapped, setBootstrapped] = useState(false);
  // Prevent first-launch setup from running twice
  const firstLaunchInitiated = useRef(false);

  useEffect(() => {
    if (!isFirstLaunch && setupComplete) {
      setBootstrapped(true);
    } else if (isFirstLaunch && !setupComplete) {
      // Guard against re-running when a state update re-triggers the effect.
      if (firstLaunchInitiated.current) return;
      firstLaunchInitiated.current = true;
      // First launch: start the bundled backend and mark setup as completed;
      // the onboarding carousel is shown inside the main app.
      tauriBackendService
        .startBackend()
        .then(() => connectionModeService.completeSetup())
        .catch(console.error)
        .finally(() => setBootstrapped(true));
    }
  }, [isFirstLaunch, setupComplete]);

  // Initialize monitoring for the bundled backend (already started in Rust)
  // This sets up port detection and health checks
  useBackendInitializer(bootstrapped);

  // Preload endpoint availability for the local bundled backend.
  useEffect(() => {
    const tryPreload = () => {
      const backendUrl = tauriBackendService.getBackendUrl();
      if (!backendUrl) return;
      if (!tauriBackendService.isOnline) return;
      console.debug(
        "[AppProviders] Preloading common tool endpoints for local backend",
      );
      void endpointAvailabilityService.preloadEndpoints(
        COMMON_TOOL_ENDPOINTS,
        backendUrl,
      );
    };

    const unsubscribe = tauriBackendService.subscribeToStatus(() =>
      tryPreload(),
    );
    tryPreload();
    return unsubscribe;
  }, []);

  useEffect(() => {
    if (!bootstrapped) {
      return;
    }

    if (!isTauri()) {
      return;
    }

    const currentWindow = getCurrentWindow();
    currentWindow
      .show()
      .then(() => currentWindow.unminimize().catch(() => {}))
      .then(() => currentWindow.setFocus().catch(() => {}))
      .then(() => currentWindow.requestUserAttention(1).catch(() => {}))
      .catch(() => {});
  }, [bootstrapped]);

  // Desktop auto-update popup (shown on startup if update available)
  const { state: popupState, actions: popupActions } = updatePopup;
  const updatePopupModal = popupState.updateSummary && (
    <UpdateModal
      opened={popupState.showModal}
      onClose={popupActions.dismissModal}
      onRemindLater={popupActions.remindLater}
      currentVersion={popupState.currentVersion}
      updateSummary={popupState.updateSummary}
      machineInfo={{
        machineType: navigator.platform?.toLowerCase().includes("mac")
          ? "Client-mac"
          : navigator.platform?.toLowerCase().includes("linux")
            ? "Client-unix"
            : "Client-win",
        activeSecurity: false,
        licenseType: "NORMAL",
      }}
      desktopInstall={
        popupState.tauriInstallReady
          ? {
              state: popupState.state,
              progress: popupState.progress,
              errorMessage: popupState.errorMessage,
              canInstall: popupState.canInstall,
              actions: popupActions,
            }
          : undefined
      }
    />
  );

  if (!bootstrapped) {
    return (
      <ProprietaryAppProviders
        appConfigRetryOptions={{
          maxRetries: 5,
          initialDelay: 1000,
        }}
        appConfigProviderProps={{
          initialConfig: DESKTOP_DEFAULT_APP_CONFIG,
          bootstrapMode: "non-blocking",
          autoFetch: false,
        }}
      >
        <div style={{ minHeight: "100vh" }} />
        {updatePopupModal}
      </ProprietaryAppProviders>
    );
  }

  // Normal app flow
  return (
    <ProprietaryAppProviders
      appConfigRetryOptions={{
        maxRetries: 5,
        initialDelay: 1000,
      }}
      appConfigProviderProps={{
        initialConfig: DESKTOP_DEFAULT_APP_CONFIG,
        bootstrapMode: "non-blocking",
        autoFetch: false,
      }}
    >
      <DesktopConfigSync />
      <DesktopBannerInitializer />
      <SaveShortcutListener />
      {children}
      {/* Desktop onboarding modal: welcome slide, shown once on first launch */}
      <DesktopOnboardingModal />
      {/* Desktop auto-update popup */}
      {updatePopupModal}
    </ProprietaryAppProviders>
  );
}
