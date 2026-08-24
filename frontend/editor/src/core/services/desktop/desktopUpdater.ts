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
 * Attempts one click of "Update and restart" is worth.
 *
 * The first attempt failing and the second succeeding was reproducible in the
 * field on Windows, which is the signature of a transient fault rather than a
 * broken release: the ~50 MB download crossing a cold CDN edge or a proxy, an
 * antivirus scanner holding the freshly written installer in the temp
 * directory, or a handle whose Rust-side resource no longer resolves. None of
 * those are worth showing a user an error for when simply doing it again
 * works, and none of them can be told apart from here.
 *
 * Retrying cannot install twice: on Windows the plugin calls ShellExecute and
 * then `process::exit(0)`, so a first attempt that reached the installer never
 * returns here at all. Reaching the catch block proves nothing was installed.
 */
const INSTALL_ATTEMPTS = 2;

/**
 * The handle to install with, re-checking when there is none.
 *
 * A failed attempt drops its handle rather than reusing it, because one
 * failure mode is the handle itself: the plugin's Update owns a resource id in
 * the webview's resource table, and a spent or unresolvable id fails the same
 * way every time it is retried. Only a fresh check produces a new one.
 */
async function resolveUpdateHandle(): Promise<
  import("@tauri-apps/plugin-updater").Update | null
> {
  if (pendingUpdate) {
    return pendingUpdate;
  }
  const { check } = await import("@tauri-apps/plugin-updater");
  pendingUpdate = await check();
  return pendingUpdate;
}

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
 * relaunch. Retries once before rejecting; callers surface the final failure
 * to the user, unlike the silent check.
 *
 * On Windows the installer takes over and exits the process itself, so the
 * relaunch call below is never reached there; on Linux it is what restarts
 * the replaced AppImage.
 */
export async function installDesktopUpdate(
  onPhase?: (phase: DesktopUpdatePhase) => void,
): Promise<void> {
  let lastError: unknown;
  let installed = false;

  for (
    let attempt = 1;
    attempt <= INSTALL_ATTEMPTS && !installed;
    attempt += 1
  ) {
    const update = await resolveUpdateHandle();
    if (!update) {
      // A retry whose re-check finds nothing reports what actually went wrong
      // on the attempt before it, not the emptiness that followed.
      throw lastError ?? new Error("No pending desktop update to install");
    }

    onPhase?.("downloading");
    try {
      await update.downloadAndInstall((event) => {
        if (event.event === "Finished") {
          onPhase?.("installing");
        }
      });
      pendingUpdate = null;
      installed = true;
    } catch (error) {
      lastError = error;
      pendingUpdate = null;
      console.warn(
        `[desktopUpdater] Install attempt ${attempt}/${INSTALL_ATTEMPTS} failed:`,
        error,
      );
    }
  }

  if (!installed) {
    throw lastError;
  }

  // Outside the retry loop on purpose: reaching here means the update is
  // already applied, so a relaunch failure must never re-enter the download.
  // Only platforms where the installer leaves this process alive get this far
  // (Linux AppImage); on Windows the process is gone by now.
  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
}
