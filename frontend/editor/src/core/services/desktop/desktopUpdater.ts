/**
 * Desktop auto-update bridge.
 *
 * Wraps @tauri-apps/plugin-updater behind the same shape as the rest of this
 * directory: every entry point returns "nothing to do" on web, and the Tauri
 * modules are only ever reached through dynamic imports so the web bundle
 * never evaluates them.
 *
 * The updater compares the running version against the latest.json manifest
 * that release.yml publishes as a release asset, and verifies every download
 * against the minisign public key pinned in tauri.conf.json before anything
 * is written. Which installs can update in place:
 *
 * - Windows (MSI): supported; the installer runs and the app exits itself.
 * - Linux AppImage: supported; the AppImage is replaced and relaunched.
 * - Linux deb/rpm: NOT supported by the plugin — check() rejects there. That
 *   rejection is deliberately swallowed: package-manager installs update
 *   through their package manager, and a startup check must never surface an
 *   error dialog for an install that is working as designed.
 */
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export interface DesktopUpdateInfo {
  /** Version offered by the manifest, e.g. "0.0.8". */
  version: string;
  /** Version currently running. */
  currentVersion: string;
}

export type DesktopUpdatePhase = "downloading" | "installing";

/**
 * The pending update handle from the last successful check. The plugin's
 * Update object owns a Rust-side resource keyed by rid, so the same instance
 * found by {@link checkForDesktopUpdate} must be the one installed.
 */
let pendingUpdate: import("@tauri-apps/plugin-updater").Update | null = null;

/**
 * Ask the release manifest whether a newer version exists.
 *
 * Returns null on web, when already current, when this install cannot update
 * in place (deb/rpm), and on any network failure — a startup check has no
 * business interrupting startup.
 */
export async function checkForDesktopUpdate(): Promise<DesktopUpdateInfo | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) {
      return null;
    }
    pendingUpdate = update;
    return {
      version: update.version,
      currentVersion: update.currentVersion,
    };
  } catch (error) {
    console.warn("[desktopUpdater] Update check skipped:", error);
    return null;
  }
}

/**
 * Download, verify and install the update found by the last check, then
 * relaunch. Rejects if there is no pending update or the install fails —
 * callers surface that to the user, unlike the silent check.
 *
 * On Windows the MSI takes over and exits the process itself, so the
 * relaunch call below is never reached there; on Linux it is what restarts
 * the replaced AppImage.
 */
export async function installDesktopUpdate(
  onPhase?: (phase: DesktopUpdatePhase) => void,
): Promise<void> {
  if (!pendingUpdate) {
    throw new Error("No pending desktop update to install");
  }
  const update = pendingUpdate;
  onPhase?.("downloading");
  await update.downloadAndInstall((event) => {
    if (event.event === "Finished") {
      onPhase?.("installing");
    }
  });
  pendingUpdate = null;
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
