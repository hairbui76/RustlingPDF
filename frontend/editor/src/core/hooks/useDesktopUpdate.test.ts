import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectConsole } from "@app/tests/failOnConsole";

const checkForDesktopUpdate = vi.hoisted(() => vi.fn());
const installDesktopUpdate = vi.hoisted(() => vi.fn());

vi.mock("@app/services/desktop/desktopUpdater", () => ({
  checkForDesktopUpdate,
  installDesktopUpdate,
}));

import { useDesktopUpdate } from "@app/hooks/useDesktopUpdate";

/**
 * The state machine behind both update surfaces: the startup banner and the
 * manual "check now" button in Settings → General. The manual button is why
 * `upToDate` exists at all — a user who presses it has asked a question, and
 * the startup path's silence would read as a broken button.
 */
describe("useDesktopUpdate", () => {
  beforeEach(() => {
    checkForDesktopUpdate.mockReset();
    installDesktopUpdate.mockReset();
  });

  it("starts idle so nothing is claimed before anyone asks", () => {
    const { result } = renderHook(() => useDesktopUpdate());

    expect(result.current.status).toBe("idle");
    expect(result.current.update).toBeNull();
    expect(result.current.busy).toBe(false);
  });

  it("reports an offered update", async () => {
    checkForDesktopUpdate.mockResolvedValue({
      version: "0.1.4",
      currentVersion: "0.1.3",
    });
    const { result } = renderHook(() => useDesktopUpdate());

    await act(() => result.current.check());

    expect(result.current.status).toBe("available");
    expect(result.current.update?.version).toBe("0.1.4");
  });

  it("distinguishes 'nothing offered' from 'never asked'", async () => {
    checkForDesktopUpdate.mockResolvedValue(null);
    const { result } = renderHook(() => useDesktopUpdate());

    await act(() => result.current.check());

    // Not "idle": the difference is exactly what the manual check must show.
    expect(result.current.status).toBe("upToDate");
    expect(result.current.update).toBeNull();
  });

  it("reports phases while installing", async () => {
    checkForDesktopUpdate.mockResolvedValue({
      version: "0.1.4",
      currentVersion: "0.1.3",
    });
    let reportPhase: ((phase: string) => void) | undefined;
    installDesktopUpdate.mockImplementation((onPhase: (p: string) => void) => {
      reportPhase = onPhase;
      return new Promise(() => {});
    });

    const { result } = renderHook(() => useDesktopUpdate());
    await act(() => result.current.check());
    act(() => result.current.install());

    expect(result.current.phase).toBe("downloading");
    expect(result.current.busy).toBe(true);

    act(() => reportPhase?.("installing"));
    expect(result.current.phase).toBe("installing");
  });

  it("surfaces the failure reason and stops being busy", async () => {
    expectConsole.error("Install failed");
    checkForDesktopUpdate.mockResolvedValue({
      version: "0.1.4",
      currentVersion: "0.1.3",
    });
    installDesktopUpdate.mockRejectedValue(new Error("network error"));

    const { result } = renderHook(() => useDesktopUpdate());
    await act(() => result.current.check());
    act(() => result.current.install());

    await waitFor(() => expect(result.current.failure).toBe("network error"));
    // Busy must clear on failure or the retry button stays spinning forever.
    expect(result.current.busy).toBe(false);
    expect(result.current.phase).toBeNull();
  });

  it("clears a previous failure when the user tries again", async () => {
    expectConsole.error("Install failed");
    checkForDesktopUpdate.mockResolvedValue({
      version: "0.1.4",
      currentVersion: "0.1.3",
    });
    installDesktopUpdate.mockRejectedValueOnce(new Error("first failure"));

    const { result } = renderHook(() => useDesktopUpdate());
    await act(() => result.current.check());
    act(() => result.current.install());
    await waitFor(() => expect(result.current.failure).toBe("first failure"));

    installDesktopUpdate.mockReturnValue(new Promise(() => {}));
    act(() => result.current.install());

    expect(result.current.failure).toBeNull();
    expect(result.current.phase).toBe("downloading");
  });
});
