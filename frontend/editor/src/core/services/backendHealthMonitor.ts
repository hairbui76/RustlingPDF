import { probeDesktopBackend } from "@app/services/desktop/desktopBackend";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import type { BackendHealthState } from "@app/types/backendHealth";

type Listener = (state: BackendHealthState) => void;

const POLL_INTERVAL_MS = 5000;

/**
 * Web builds are served *by* the backend they talk to: if the page loaded, the
 * backend answered. There is nothing to poll and nothing that can go from
 * running to not-running under the app's feet, so the state is a constant.
 */
const WEB_HEALTH: BackendHealthState = {
  status: "healthy",
  message: null,
  error: null,
  isOnline: true,
};

/**
 * Health of the bundled processing sidecar, for the Run button and any other
 * UI that must not offer an action the backend cannot yet serve.
 *
 * Desktop only. The sidecar is a separate process that starts *after* the
 * window is already visible and finishes native tool discovery later still, so
 * "the UI is up" says nothing about whether a tool can run. Reporting a
 * hardcoded `healthy` — which is what the stub this replaces did — enables
 * every Run button from the first frame and turns a not-yet-ready backend into
 * an opaque request failure.
 */
class BackendHealthMonitor {
  private listeners = new Set<Listener>();
  private intervalId: ReturnType<typeof setInterval> | null = null;
  private state: BackendHealthState = isDesktopRuntime()
    ? {
        // The sidecar is launched by the Rust setup hook, so at first paint it
        // is starting, not stopped and not healthy. `message: null` lets the
        // consumer supply its own "checking..." copy, which keeps i18n out of
        // this module's load path — this singleton is constructed on import,
        // and every tool hook transitively imports it.
        status: "starting",
        message: null,
        error: null,
        isOnline: false,
      }
    : WEB_HEALTH;

  private update(next: BackendHealthState): void {
    const changed =
      this.state.status !== next.status ||
      this.state.message !== next.message ||
      this.state.error !== next.error;
    this.state = next;
    if (changed) {
      this.listeners.forEach((listener) => listener(this.state));
    }
  }

  private async pollOnce(): Promise<boolean> {
    if (!isDesktopRuntime()) {
      return true;
    }
    const healthy = await probeDesktopBackend();
    if (healthy) {
      this.update({
        status: "healthy",
        message: null,
        error: null,
        isOnline: true,
      });
    } else {
      // Deferred import for the same reason as the initial state above; this
      // branch only runs inside the desktop shell.
      const { default: i18n } = await import("@app/i18n");
      const offline = i18n.t("backendHealth.offline", "Backend Offline");
      this.update({
        status: "unhealthy",
        message: offline,
        error: offline,
        isOnline: false,
      });
    }
    return healthy;
  }

  private startPolling(): void {
    if (this.intervalId !== null || !isDesktopRuntime()) {
      return;
    }
    void this.pollOnce();
    this.intervalId = setInterval(() => {
      void this.pollOnce();
    }, POLL_INTERVAL_MS);
  }

  private stopPolling(): void {
    if (this.intervalId !== null) {
      clearInterval(this.intervalId);
      this.intervalId = null;
    }
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener);
    listener(this.state);
    if (this.listeners.size === 1) {
      this.startPolling();
    }
    return () => {
      this.listeners.delete(listener);
      if (this.listeners.size === 0) {
        this.stopPolling();
      }
    };
  }

  getSnapshot(): BackendHealthState {
    return this.state;
  }

  /** Force an immediate probe, bypassing the poll interval. */
  async checkNow(): Promise<boolean> {
    return this.pollOnce();
  }
}

export const backendHealthMonitor = new BackendHealthMonitor();
