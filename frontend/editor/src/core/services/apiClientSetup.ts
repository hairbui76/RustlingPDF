import type { AxiosInstance } from "axios";
import { getBrowserId } from "@app/utils/browserIdentifier";
import { getDesktopBackendUrl } from "@app/services/desktop/desktopBackend";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export function setupApiInterceptors(client: AxiosInstance): void {
  // Add browser ID header for WAU tracking
  client.interceptors.request.use(
    (config) => {
      const browserId = getBrowserId();
      config.headers["X-Browser-Id"] = browserId;
      return config;
    },
    (error) => Promise.reject(error),
  );

  // Desktop: resolve the sidecar's address per request, not once at module
  // load. The sidecar binds an ephemeral port; if it restarts it comes back on
  // a different one, and a baseURL captured when `apiClient` was constructed
  // would send every subsequent request to a dead port for the rest of the
  // window's life. `getDesktopBackendUrl()` is the live getter.
  if (isDesktopRuntime()) {
    client.interceptors.request.use(
      (config) => {
        const backendUrl = getDesktopBackendUrl();
        if (backendUrl && config.url && !/^https?:\/\//i.test(config.url)) {
          const separator = config.url.startsWith("/") ? "" : "/";
          config.url = `${backendUrl}${separator}${config.url}`;
        }
        // The bundled sidecar has no authentication and no sessions; sending
        // credentials would only force the browser's stricter
        // credentialed-CORS rules onto these cross-origin requests.
        config.withCredentials = false;
        return config;
      },
      (error) => Promise.reject(error),
    );
  }
}
