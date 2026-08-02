/**
 * Live location of the bundled processing sidecar.
 *
 * The sidecar binds an *ephemeral* loopback port, so its URL is not a build
 * constant and not stable for the lifetime of the window. The Rust launcher
 * seeds `localStorage` and reloads the webview once at startup, but that
 * handshake fires exactly once: if the sidecar is restarted it comes back on a
 * different port and the stored URL is stale, and — because
 * `apiClientConfig.getApiBaseUrl()` is read once at module load — every
 * subsequent request goes to a dead port until the user fully quits the app.
 *
 * This module is the live getter that fixes that lifetime: it asks Rust
 * (`get_backend_port`) for the current port and republishes it, and API
 * requests resolve their base URL through {@link getDesktopBackendUrl} per
 * request rather than once.
 */
import {
  DESKTOP_COMMANDS,
  desktopInvoke,
  isDesktopRuntime,
} from "@app/services/desktop/desktopRuntime";

/**
 * Key the Rust launcher writes at startup (`lib.rs`), read here to seed the
 * port before the first `get_backend_port` round-trip completes.
 */
export const DESKTOP_BACKEND_URL_STORAGE_KEY = "rustlingpdf.desktopBackendUrl";

let cachedPort: number | null = null;
let seeded = false;

/**
 * Use the IPv4 loopback literal, not "localhost": on macOS (and some Linux
 * setups) "localhost" resolves to IPv6 `::1` first, but the sidecar binds the
 * IPv4 wildcard, so a `::1` connection is refused and every tool reports the
 * backend offline while it is in fact up.
 */
function urlForPort(port: number): string {
  return `http://127.0.0.1:${port}`;
}

function seedFromStorage(): void {
  if (seeded || typeof window === "undefined") {
    return;
  }
  seeded = true;
  try {
    const stored = window.localStorage.getItem(DESKTOP_BACKEND_URL_STORAGE_KEY);
    const port = stored ? Number(new URL(stored).port) : NaN;
    if (Number.isInteger(port) && port > 0) {
      cachedPort = port;
    }
  } catch {
    // A malformed or unreadable value is simply no seed; the first
    // refreshDesktopBackendPort() call supplies the real port.
  }
}

/**
 * Current sidecar base URL without a trailing slash, or null when unknown
 * (web, or the sidecar has not reported a port yet). Synchronous, so request
 * interceptors can call it on every request.
 */
export function getDesktopBackendUrl(): string | null {
  if (!isDesktopRuntime()) {
    return null;
  }
  seedFromStorage();
  return cachedPort === null ? null : urlForPort(cachedPort);
}

/**
 * Ask Rust for the sidecar's current port and republish it.
 *
 * Republishing to `localStorage` keeps the launcher's own key truthful, so a
 * later reload (for any reason) starts from the right port instead of the one
 * the sidecar had at first launch.
 */
export async function refreshDesktopBackendPort(): Promise<number | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  try {
    const port = await desktopInvoke<number | null>(
      DESKTOP_COMMANDS.getBackendPort,
    );
    if (typeof port === "number" && port > 0) {
      if (port !== cachedPort) {
        cachedPort = port;
        try {
          window.localStorage.setItem(
            DESKTOP_BACKEND_URL_STORAGE_KEY,
            urlForPort(port),
          );
        } catch {
          // Storage can be unavailable; the in-memory cache is authoritative.
        }
      }
      return port;
    }
  } catch (error) {
    console.error("[desktopBackend] Failed to read backend port:", error);
  }
  return null;
}

/**
 * Probe the sidecar.
 *
 * `dependenciesReady` — not merely a 200 — is the readiness signal: the HTTP
 * listener accepts connections before native tool discovery has finished, and
 * running a tool in that window fails with a confusing dependency error.
 *
 * Uses the browser's own `fetch`. The desktop webview calls the sidecar
 * cross-origin, which the sidecar's `RUSTLING_PDF_TAURI_MODE` allow-list
 * permits (see `rust/contracts/desktop-cors.md`) — the same path every other
 * API call already takes, so a CORS regression surfaces here too rather than
 * being masked by a native HTTP client.
 */
export async function probeDesktopBackend(): Promise<boolean> {
  const port = await refreshDesktopBackendPort();
  const baseUrl = port === null ? getDesktopBackendUrl() : urlForPort(port);
  if (!baseUrl) {
    return false;
  }
  try {
    const response = await fetch(`${baseUrl}/api/v1/config/app-config`, {
      method: "GET",
    });
    if (!response.ok) {
      return false;
    }
    const config = (await response.json()) as { dependenciesReady?: boolean };
    return config.dependenciesReady === true;
  } catch (error) {
    console.debug("[desktopBackend] Health probe failed:", error);
    return false;
  }
}

/** Test seam: drop the cached port and storage seed. */
export function resetDesktopBackendCacheForTests(): void {
  cachedPort = null;
  seeded = false;
}
