import axios from "axios";
import { handleHttpError } from "@app/services/httpErrorHandler";
import { setupApiInterceptors } from "@app/services/apiClientSetup";
import { getApiBaseUrl } from "@app/services/apiClientConfig";

// Create axios instance with default config
// `withCredentials` is deliberately left off. The service has no
// authentication, no cookies and no sessions, so credentialed requests buy
// nothing — while on desktop, where the SPA calls the sidecar cross-origin,
// they would force the browser's stricter credentialed-CORS rules, under which
// a wildcard `Access-Control-Allow-Origin` or `Allow-Headers` is rejected
// outright.
const apiClient = axios.create({
  baseURL: getApiBaseUrl(),
  responseType: "json",
});

// Configure headers used by the stateless backend.
setupApiInterceptors(apiClient);

// ---------- Install error interceptor ----------
apiClient.interceptors.response.use(
  (response) => response,
  async (error) => {
    await handleHttpError(error); // Handle error (shows toast unless suppressed)
    return Promise.reject(error);
  },
);

// ---------- Exports ----------
export default apiClient;
