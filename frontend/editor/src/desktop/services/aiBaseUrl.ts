import { tauriBackendService } from "@app/services/tauriBackendService";

/**
 * Desktop: AI-engine calls go through the bundled local backend's AI proxy,
 * exactly like every other API call. The backend URL is dynamic (the sidecar
 * picks a free port), so resolve it from tauriBackendService.
 *
 * Used for the orchestrate stream (a raw fetch) and the AI result-file
 * download, which would otherwise resolve against the webview origin.
 */
export function getAiBaseUrl(): string {
  return (tauriBackendService.getBackendUrl() ?? "").replace(/\/$/, "");
}
