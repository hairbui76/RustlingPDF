import { useAppConfig } from "@app/contexts/AppConfigContext";
import { useIsMobile } from "@app/hooks/useIsMobile";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

/**
 * Whether the "upload from phone via QR code" entry points render.
 *
 * One hook rather than a predicate copy-pasted per button, because the two
 * copies had already drifted into different variable names and the desktop
 * clause below must hold at every entry point or none.
 *
 * Desktop is excluded deliberately, not provisionally. The QR flow needs the
 * phone to reach this machine: load the scanner page and call the session
 * API. A desktop install can serve neither — the sidecar binds loopback and
 * serves no static files, and the QR URL falls back to `tauri://localhost`,
 * which no phone can resolve — so the button produced a QR code that could
 * never work. Making it work would mean opening an unauthenticated LAN
 * listener from an app whose privacy story is "no network surface"; that was
 * prototyped and the maintainer decided against the feature. Web and Docker
 * deployments keep it: there the server is already reachable by the phone,
 * and the flow works as designed.
 */
export function useMobileUploadAvailability(): boolean {
  const { config } = useAppConfig();
  const isMobile = useIsMobile();
  return (
    Boolean(config?.enableMobileScanner) && !isMobile && !isDesktopRuntime()
  );
}
