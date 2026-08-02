import { afterEach, describe, expect, it } from "vitest";
import {
  DesktopRuntimeUnavailableError,
  desktopInvoke,
  DESKTOP_COMMANDS,
  isDesktopRuntime,
  listenOnDesktopWindow,
} from "@app/services/desktop/desktopRuntime";

/**
 * The detection helper is the hinge every restored desktop path turns on: a
 * false negative silently downgrades the desktop app to a web app in a window,
 * and a false positive makes the *web* build try to load Tauri modules that
 * are not there. Both directions are asserted.
 */

type MutableWindow = Window & {
  isTauri?: boolean;
  __TAURI_INTERNALS__?: unknown;
};

function clearTauriGlobals() {
  const w = window as MutableWindow;
  delete w.isTauri;
  delete w.__TAURI_INTERNALS__;
}

afterEach(clearTauriGlobals);

describe("isDesktopRuntime", () => {
  it("is false in a plain browser", () => {
    expect(isDesktopRuntime()).toBe(false);
  });

  it("is true when Tauri injected window.isTauri", () => {
    (window as MutableWindow).isTauri = true;
    expect(isDesktopRuntime()).toBe(true);
  });

  it("is true when only the IPC internals are present", () => {
    // Tauri v2 always injects __TAURI_INTERNALS__; `isTauri` is the newer of
    // the two signals. Detection must not depend on just one of them.
    (window as MutableWindow).__TAURI_INTERNALS__ = { invoke: () => {} };
    expect(isDesktopRuntime()).toBe(true);
  });

  it("is false for a falsy isTauri flag", () => {
    (window as MutableWindow).isTauri = false;
    expect(isDesktopRuntime()).toBe(false);
  });
});

describe("desktopInvoke", () => {
  it("rejects on web without importing @tauri-apps", async () => {
    // If this ever resolves, the web bundle is loading Tauri code — the import
    // is only reached after the guard.
    await expect(
      desktopInvoke(DESKTOP_COMMANDS.getBackendPort),
    ).rejects.toBeInstanceOf(DesktopRuntimeUnavailableError);
  });
});

describe("listenOnDesktopWindow", () => {
  it("resolves to a no-op unlisten on web", async () => {
    const unlisten = await listenOnDesktopWindow("files-changed", () => {
      throw new Error("handler must never fire on web");
    });
    expect(() => unlisten()).not.toThrow();
  });
});
