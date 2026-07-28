import type { AxiosInstance, InternalAxiosRequestConfig } from "axios";
import { alert } from "@app/components/toast";
import { setupApiInterceptors as coreSetup } from "@core/services/apiClientSetup";
import { tauriBackendService } from "@app/services/tauriBackendService";
import { createBackendNotReadyError } from "@app/constants/backendErrors";
import i18n from "@app/i18n";

/**
 * Headers for raw fetch() calls (the AI SSE stream) — desktop variant.
 * There is no authentication; the local bundled backend takes plain requests.
 */
export async function getAuthHeaders(): Promise<Record<string, string>> {
  return {};
}

const BACKEND_TOAST_COOLDOWN_MS = 4000;
let lastBackendToast = 0;

// Extended config for custom properties
interface ExtendedRequestConfig extends InternalAxiosRequestConfig {
  operationName?: string;
  skipBackendReadyCheck?: boolean;
  _retry?: boolean;
}

/**
 * Desktop-specific API interceptors
 * - Reuses the core interceptors (X-Browser-Id)
 * - Prefixes relative URLs with the bundled backend's dynamic base URL
 * - Blocks mutating API calls while the bundled backend is still starting
 */
export function setupApiInterceptors(client: AxiosInstance): void {
  coreSetup(client);

  client.interceptors.request.use(
    async (config: InternalAxiosRequestConfig) => {
      const extendedConfig = config as ExtendedRequestConfig;
      const skipCheck = extendedConfig.skipBackendReadyCheck === true;

      const backendUrl = tauriBackendService.getBackendUrl();
      if (
        backendUrl &&
        extendedConfig.url &&
        !extendedConfig.url.startsWith("http")
      ) {
        extendedConfig.url = `${backendUrl.replace(/\/$/, "")}${extendedConfig.url}`;
      }
      // The bundled backend runs without authentication; never send credentials.
      extendedConfig.withCredentials = false;

      // Backend readiness check for the local bundled backend
      if (!skipCheck && !tauriBackendService.isOnline) {
        const method = (extendedConfig.method || "get").toLowerCase();
        if (method !== "get") {
          const now = Date.now();
          if (now - lastBackendToast > BACKEND_TOAST_COOLDOWN_MS) {
            lastBackendToast = now;
            alert({
              alertType: "error",
              title: i18n.t("backendHealth.offline", "Backend Offline"),
              body: i18n.t(
                "backendHealth.wait",
                "Please wait for the backend to finish launching and try again.",
              ),
              isPersistentPopup: false,
            });
          }
          return Promise.reject(createBackendNotReadyError());
        }
      }

      return extendedConfig;
    },
    (error) => Promise.reject(error),
  );
}
