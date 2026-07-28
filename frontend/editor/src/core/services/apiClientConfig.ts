/**
 * Get the base URL for API requests.
 *
 * Priority:
 * 1. window.RUSTLING_PDF_API_BASE_URL (runtime override - fixes hardcoded localhost
 *    issues; the pre-rename STIRLING_PDF_API_BASE_URL spelling injected by older
 *    backends is honoured as a fallback)
 * 2. import.meta.env.VITE_API_BASE_URL (build-time env var)
 * 3. '/' (relative path - works for same-origin deployments)
 *
 * Note: Runtime override is needed because VITE_API_BASE_URL gets baked into the build.
 * If someone builds with VITE_API_BASE_URL=http://localhost:8080, it breaks production deployments.
 */
export function getApiBaseUrl(): string {
  // Runtime override to fix hardcoded localhost in builds
  if (typeof window !== "undefined") {
    const runtimeOverride =
      (window as any).RUSTLING_PDF_API_BASE_URL ||
      (window as any).STIRLING_PDF_API_BASE_URL;
    if (runtimeOverride) {
      return runtimeOverride;
    }
  }

  return import.meta.env.VITE_API_BASE_URL;
}
