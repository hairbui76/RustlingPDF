import { backendHealthMonitor } from "@app/services/backendHealthMonitor";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

const BACKEND_TOAST_COOLDOWN_MS = 4000;
let lastBackendToast = 0;

/**
 * Gate a tool run on the backend actually being able to serve it.
 *
 * Web builds are served by their backend, so there is nothing to wait for and
 * this is always true. Desktop builds talk to a sidecar that starts after the
 * window appears: without this gate, a run started in the first seconds fails
 * inside the HTTP layer with an error that says nothing about *why*.
 *
 * @param _endpoint - Reserved for per-endpoint readiness; the sidecar reports
 *   readiness as a single `dependenciesReady` flag, not per endpoint.
 */
export async function ensureBackendReady(_endpoint?: string): Promise<boolean> {
  if (!isDesktopRuntime()) {
    return true;
  }

  if (backendHealthMonitor.getSnapshot().isOnline) {
    return true;
  }

  // The cached snapshot can be up to one poll interval stale, and a user who
  // clicks Run the instant the backend comes up should not be told to wait.
  if (await backendHealthMonitor.checkNow()) {
    return true;
  }

  // The caller also surfaces this as inline tool error text; the toast is
  // rate-limited so a burst of runs does not stack duplicates.
  const now = Date.now();
  if (now - lastBackendToast > BACKEND_TOAST_COOLDOWN_MS) {
    lastBackendToast = now;
    // Imported here rather than at module scope: this module is pulled in by
    // every tool hook, and eagerly loading i18n and the toast tree makes those
    // hooks impossible to unit-test without also standing both of them up.
    const [{ default: i18n }, { alert }] = await Promise.all([
      import("@app/i18n"),
      import("@app/components/toast"),
    ]);
    alert({
      alertType: "error",
      title: i18n.t("backendHealth.offline", "Backend Offline"),
      body: i18n.t("backendHealth.checking", "Checking backend status..."),
      isPersistentPopup: false,
    });
  }

  return false;
}
