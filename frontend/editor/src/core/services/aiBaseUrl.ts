import { getApiBaseUrl } from "@app/services/apiClientConfig";
import { getDesktopBackendUrl } from "@app/services/desktop/desktopBackend";

/**
 * Base URL for AI-engine calls (orchestrate stream, AI result-file download).
 *
 * These are raw `fetch` calls, so they bypass the axios interceptor that
 * re-resolves the sidecar's address per request — they must ask for the live
 * desktop backend URL themselves, or they resolve against the webview origin
 * (or a stale port) instead of the sidecar's AI proxy.
 *
 * Web builds talk to whichever backend served the app, so the normal API base
 * is correct there.
 */
export function getAiBaseUrl(): string {
  const desktopBackendUrl = getDesktopBackendUrl();
  if (desktopBackendUrl) {
    return desktopBackendUrl;
  }
  return getApiBaseUrl().replace(/\/$/, ""); // Remove trailing slash
}
