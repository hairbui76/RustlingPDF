import { AxiosInstance } from "axios";
import { getBrowserId } from "@app/utils/browserIdentifier";

/**
 * Headers for raw fetch() calls (SSE streams). There is no authentication;
 * only the anonymous per-browser id rides along. Async to keep the call
 * signature shared with layers that resolve headers lazily.
 */
export async function getAuthHeaders(): Promise<Record<string, string>> {
  return {};
}

export function setupApiInterceptors(client: AxiosInstance): void {
  // Tag every request with the anonymous per-browser id (used for WAU
  // estimation server-side; carries no identity).
  client.interceptors.request.use(
    (config) => {
      config.headers["X-Browser-Id"] = getBrowserId();
      return config;
    },
    (error) => {
      return Promise.reject(error);
    },
  );
}
