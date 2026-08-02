import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectConsole } from "@app/tests/failOnConsole";

/**
 * Ctrl/Cmd+S. The important assertions are that it is bound on desktop and
 * *not* bound on web (where the key belongs to the browser), and that a file
 * is only marked clean when a write actually happened.
 */

const isDesktopRuntimeMock = vi.fn<() => boolean>();
const downloadFileMock = vi.fn();
const updateStubMock = vi.fn();

vi.mock("@app/services/desktop/desktopRuntime", () => ({
  isDesktopRuntime: () => isDesktopRuntimeMock(),
}));

vi.mock("@app/services/downloadService", () => ({
  downloadFile: (...args: unknown[]) => downloadFileMock(...args),
}));

const files = new Map<string, File>();
const stubs = new Map<string, Record<string, unknown>>();
let selectedFileIds: string[] = [];

vi.mock("@app/contexts/file/fileHooks", () => ({
  useFileState: () => ({
    state: { ui: { selectedFileIds } },
    selectors: {
      getAllFileIds: () => [...files.keys()],
      getFile: (id: string) => files.get(id),
      getRustlingFileStub: (id: string) => stubs.get(id),
    },
  }),
  useFileActions: () => ({
    actions: { updateRustlingFileStub: updateStubMock },
  }),
}));

import { useSaveShortcut } from "@app/hooks/useSaveShortcut";

async function pressSave(init: Partial<KeyboardEventInit> = {}) {
  await act(async () => {
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "s", ctrlKey: true, ...init }),
    );
    for (let i = 0; i < 20; i++) await Promise.resolve();
  });
}

function addFile(id: string, name: string, localFilePath?: string) {
  files.set(id, new File(["x"], name));
  stubs.set(id, { id, localFilePath });
}

describe("useSaveShortcut", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    files.clear();
    stubs.clear();
    selectedFileIds = [];
    isDesktopRuntimeMock.mockReturnValue(true);
    downloadFileMock.mockResolvedValue({ savedPath: "/home/u/a.pdf" });
  });

  it("is not bound on web", async () => {
    isDesktopRuntimeMock.mockReturnValue(false);
    addFile("f1", "a.pdf", "/home/u/a.pdf");

    renderHook(() => useSaveShortcut());
    await pressSave();

    expect(downloadFileMock).not.toHaveBeenCalled();
  });

  it("saves every file back to its own path when nothing is selected", async () => {
    addFile("f1", "a.pdf", "/home/u/a.pdf");
    addFile("f2", "b.pdf", "/home/u/b.pdf");

    renderHook(() => useSaveShortcut());
    await pressSave();

    expect(downloadFileMock).toHaveBeenCalledTimes(2);
    expect(downloadFileMock.mock.calls[0][0]).toMatchObject({
      filename: "a.pdf",
      localPath: "/home/u/a.pdf",
    });
    expect(downloadFileMock.mock.calls[1][0]).toMatchObject({
      filename: "b.pdf",
      localPath: "/home/u/b.pdf",
    });
  });

  it("saves only the selection when there is one", async () => {
    addFile("f1", "a.pdf", "/home/u/a.pdf");
    addFile("f2", "b.pdf", "/home/u/b.pdf");
    selectedFileIds = ["f2"];

    renderHook(() => useSaveShortcut());
    await pressSave();

    expect(downloadFileMock).toHaveBeenCalledTimes(1);
    expect(downloadFileMock.mock.calls[0][0]).toMatchObject({
      filename: "b.pdf",
    });
  });

  it("marks a saved file clean and records the path it was written to", async () => {
    addFile("f1", "a.pdf");
    downloadFileMock.mockResolvedValue({ savedPath: "/home/u/picked.pdf" });

    renderHook(() => useSaveShortcut());
    await pressSave();

    expect(updateStubMock).toHaveBeenCalledWith("f1", {
      localFilePath: "/home/u/picked.pdf",
      isDirty: false,
    });
  });

  it("leaves the file dirty when the save dialog was cancelled", async () => {
    addFile("f1", "a.pdf");
    downloadFileMock.mockResolvedValue({ cancelled: true });

    renderHook(() => useSaveShortcut());
    await pressSave();

    expect(updateStubMock).not.toHaveBeenCalled();
  });

  it("keeps going when one file fails to save", async () => {
    // The failure is the point of the test — logging it is contract, not noise.
    expectConsole.error("[Desktop] Failed to save a.pdf");
    addFile("f1", "a.pdf", "/root/a.pdf");
    addFile("f2", "b.pdf", "/home/u/b.pdf");
    downloadFileMock
      .mockRejectedValueOnce(new Error("EACCES"))
      .mockResolvedValueOnce({ savedPath: "/home/u/b.pdf" });

    renderHook(() => useSaveShortcut());
    await pressSave();

    expect(downloadFileMock).toHaveBeenCalledTimes(2);
    expect(updateStubMock).toHaveBeenCalledTimes(1);
    expect(updateStubMock).toHaveBeenCalledWith("f2", expect.anything());
  });

  it("ignores Ctrl+Shift+S and plain S", async () => {
    addFile("f1", "a.pdf", "/home/u/a.pdf");

    renderHook(() => useSaveShortcut());
    await pressSave({ shiftKey: true });
    await pressSave({ ctrlKey: false });

    expect(downloadFileMock).not.toHaveBeenCalled();
  });

  it("handles Caps Lock (key reported as 'S')", async () => {
    addFile("f1", "a.pdf", "/home/u/a.pdf");

    renderHook(() => useSaveShortcut());
    await pressSave({ key: "S" });

    expect(downloadFileMock).toHaveBeenCalledTimes(1);
  });
});
