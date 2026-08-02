import { getDesktopBackendUrl } from "@app/services/desktop/desktopBackend";

/**
 * Get the base URL for API requests.
 *
 * Priority:
 * 1. window.RUSTLING_PDF_API_BASE_URL (runtime override - fixes hardcoded localhost
 *    issues)
 * 2. the bundled sidecar's current URL, on desktop
 * 3. import.meta.env.VITE_API_BASE_URL (build-time env var)
 * 4. '/' (relative path - works for same-origin deployments)
 *
 * Note: Runtime override is needed because VITE_API_BASE_URL gets baked into the build.
 * If someone builds with VITE_API_BASE_URL=http://localhost:8080, it breaks production deployments.
 *
 * IMPORTANT — this is read once, when `apiClient` is constructed at module
 * load. On desktop the sidecar's port is *not* stable for the window's
 * lifetime, so this value alone is not enough: `apiClientSetup` re-resolves
 * the desktop base URL on every request. Treat what this returns on desktop as
 * a seed, not as the address.
 */
export function getApiBaseUrl(): string {
  if (typeof window !== "undefined") {
    const runtimeOverride = window.RUSTLING_PDF_API_BASE_URL;
    if (runtimeOverride) {
      return runtimeOverride;
    }
  }

  const desktopBackendUrl = getDesktopBackendUrl();
  if (desktopBackendUrl) {
    return desktopBackendUrl;
  }

  return import.meta.env.VITE_API_BASE_URL || "/";
}
