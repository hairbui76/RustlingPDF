import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { expectConsole } from "@app/tests/failOnConsole";

const check = vi.hoisted(() => vi.fn());
const relaunch = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));

type MutableWindow = Window & { isTauri?: boolean };

/**
 * A stand-in for the plugin's Update handle. `downloadAndInstall` is the only
 * member these paths touch; each instance is distinct so a test can prove
 * which handle an attempt used.
 */
function updateHandle(downloadAndInstall: ReturnType<typeof vi.fn>) {
  return { version: "0.1.3", currentVersion: "0.1.2", downloadAndInstall };
}

/**
 * The desktop updater's install path, whose retry exists because of a
 * reproducible field failure: on Windows the first click reported "could not
 * be installed" and the second one, with nothing else changed, installed
 * successfully. The transient fault is not identifiable from the frontend, so
 * what is pinned here is the behaviour — a click that would have failed once
 * must not reach the user as an error.
 */
describe("installDesktopUpdate", () => {
  beforeEach(() => {
    vi.resetModules();
    check.mockReset();
    relaunch.mockReset();
    (window as MutableWindow).isTauri = true;
  });

  afterEach(() => {
    delete (window as MutableWindow).isTauri;
    vi.restoreAllMocks();
  });

  async function loadModule() {
    return import("@app/services/desktop/desktopUpdater");
  }

  it("installs on the first attempt without re-checking", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    check.mockResolvedValue(updateHandle(downloadAndInstall));

    const { checkForDesktopUpdate, installDesktopUpdate } = await loadModule();
    await checkForDesktopUpdate();
    await installDesktopUpdate();

    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    // One check, from the caller — a healthy install must not re-check.
    expect(check).toHaveBeenCalledTimes(1);
    expect(relaunch).toHaveBeenCalledTimes(1);
  });

  it("retries a failed attempt with a freshly checked handle and resolves", async () => {
    expectConsole.warn("Install attempt 1/2 failed");
    const failing = vi.fn().mockRejectedValue(new Error("network error"));
    const succeeding = vi.fn().mockResolvedValue(undefined);
    check
      .mockResolvedValueOnce(updateHandle(failing))
      .mockResolvedValueOnce(updateHandle(succeeding));

    const { checkForDesktopUpdate, installDesktopUpdate } = await loadModule();
    await checkForDesktopUpdate();

    // The user sees no error: this is the whole point of the retry.
    await expect(installDesktopUpdate()).resolves.toBeUndefined();

    expect(failing).toHaveBeenCalledTimes(1);
    expect(succeeding).toHaveBeenCalledTimes(1);
    // The poisoned handle is dropped, so the second attempt re-checks. A
    // spent or unresolvable resource id fails identically every retry.
    expect(check).toHaveBeenCalledTimes(2);
    expect(relaunch).toHaveBeenCalledTimes(1);
  });

  it("gives up after the second attempt and reports the last real error", async () => {
    expectConsole.warn("Install attempt 1/2 failed");
    expectConsole.warn("Install attempt 2/2 failed");
    const firstError = new Error("first failure");
    const secondError = new Error("second failure");
    check
      .mockResolvedValueOnce(
        updateHandle(vi.fn().mockRejectedValue(firstError)),
      )
      .mockResolvedValueOnce(
        updateHandle(vi.fn().mockRejectedValue(secondError)),
      );

    const { checkForDesktopUpdate, installDesktopUpdate } = await loadModule();
    await checkForDesktopUpdate();

    await expect(installDesktopUpdate()).rejects.toThrow("second failure");
    // Exactly two attempts: a retry loop that kept going would hammer a
    // ~50 MB download against a genuinely broken release.
    expect(check).toHaveBeenCalledTimes(2);
    expect(relaunch).not.toHaveBeenCalled();
  });

  it("reports the failure, not the emptiness, when the retry check finds nothing", async () => {
    expectConsole.warn("Install attempt 1/2 failed");
    check
      .mockResolvedValueOnce(
        updateHandle(vi.fn().mockRejectedValue(new Error("download failed"))),
      )
      .mockResolvedValueOnce(null);

    const { checkForDesktopUpdate, installDesktopUpdate } = await loadModule();
    await checkForDesktopUpdate();

    await expect(installDesktopUpdate()).rejects.toThrow("download failed");
  });

  it("checks for itself when asked to install without a prior check", async () => {
    const downloadAndInstall = vi.fn().mockResolvedValue(undefined);
    check.mockResolvedValue(updateHandle(downloadAndInstall));

    const { installDesktopUpdate } = await loadModule();
    await installDesktopUpdate();

    expect(check).toHaveBeenCalledTimes(1);
    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
  });

  it("reports phases so the banner can show downloading then installing", async () => {
    const downloadAndInstall = vi.fn(
      async (onEvent: (event: { event: string }) => void) => {
        onEvent({ event: "Started" });
        onEvent({ event: "Finished" });
      },
    );
    check.mockResolvedValue(updateHandle(downloadAndInstall));

    const { checkForDesktopUpdate, installDesktopUpdate } = await loadModule();
    await checkForDesktopUpdate();
    const phases: string[] = [];
    await installDesktopUpdate((phase) => phases.push(phase));

    expect(phases).toEqual(["downloading", "installing"]);
  });

  it("does not relaunch when the install never succeeded", async () => {
    expectConsole.warn("Install attempt 1/2 failed");
    expectConsole.warn("Install attempt 2/2 failed");
    check.mockResolvedValue(
      updateHandle(vi.fn().mockRejectedValue(new Error("boom"))),
    );

    const { installDesktopUpdate } = await loadModule();
    await expect(installDesktopUpdate()).rejects.toThrow("boom");

    // Relaunching a process whose update never landed would restart the app
    // for no reason and look like a crash.
    expect(relaunch).not.toHaveBeenCalled();
  });
});
