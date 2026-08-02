import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FileId } from "@app/types/fileContext";

/**
 * How tool results reach the disk.
 *
 * The two properties that matter: an output only overwrites a file it owns,
 * and a group of outputs that own nothing is saved through one destination
 * prompt rather than a modal each.
 */

const isDesktopRuntimeMock = vi.fn<() => boolean>();
const downloadFileMock = vi.fn();
const downloadFromUrlMock = vi.fn();
const saveMultipleMock = vi.fn();

vi.mock("@app/services/desktop/desktopRuntime", () => ({
  isDesktopRuntime: () => isDesktopRuntimeMock(),
}));

vi.mock("@app/services/downloadService", () => ({
  downloadFile: (...args: unknown[]) => downloadFileMock(...args),
  downloadFromUrl: (...args: unknown[]) => downloadFromUrlMock(...args),
}));

vi.mock("@app/services/localFileSaveService", () => ({
  saveMultipleFilesWithPrompt: (...args: unknown[]) =>
    saveMultipleMock(...args),
}));

import { saveOperationResults } from "@app/services/operationResultsSaveService";

const files = new Map<string, File>();
const stubs = new Map<string, { localFilePath?: string }>();
const markSaved = vi.fn();

function context(outputFileIds: string[]) {
  return {
    downloadUrl: "blob:results",
    downloadFilename: "results.zip",
    outputFileIds,
    getFile: (id: FileId) => files.get(id),
    getStub: (id: FileId) => stubs.get(id) as never,
    markSaved,
  };
}

describe("saveOperationResults", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    files.clear();
    stubs.clear();
    isDesktopRuntimeMock.mockReturnValue(true);
    downloadFileMock.mockResolvedValue({ savedPath: "/picked.pdf" });
    downloadFromUrlMock.mockResolvedValue({ savedPath: undefined });
    saveMultipleMock.mockResolvedValue({
      success: true,
      savedCount: 2,
      savedPaths: ["/out/part1.pdf", "/out/part2.pdf"],
    });
  });

  it("web takes the aggregate download, not per-file writes", async () => {
    isDesktopRuntimeMock.mockReturnValue(false);
    files.set("o1", new File(["x"], "a.pdf"));

    await saveOperationResults(context(["o1"]));

    expect(downloadFromUrlMock).toHaveBeenCalledTimes(1);
    expect(downloadFileMock).not.toHaveBeenCalled();
  });

  it("writes each owning output back to its own file", async () => {
    files.set("o1", new File(["x"], "a.pdf"));
    files.set("o2", new File(["y"], "b.pdf"));
    stubs.set("o1", { localFilePath: "/home/u/a.pdf" });
    stubs.set("o2", { localFilePath: "/home/u/b.pdf" });
    downloadFileMock
      .mockResolvedValueOnce({ savedPath: "/home/u/a.pdf" })
      .mockResolvedValueOnce({ savedPath: "/home/u/b.pdf" });

    await saveOperationResults(context(["o1", "o2"]));

    expect(downloadFileMock).toHaveBeenCalledTimes(2);
    expect(downloadFileMock.mock.calls[0][0]).toMatchObject({
      localPath: "/home/u/a.pdf",
    });
    expect(downloadFileMock.mock.calls[1][0]).toMatchObject({
      localPath: "/home/u/b.pdf",
    });
    expect(saveMultipleMock).not.toHaveBeenCalled();
    expect(markSaved).toHaveBeenCalledWith("o1", "/home/u/a.pdf");
    expect(markSaved).toHaveBeenCalledWith("o2", "/home/u/b.pdf");
  });

  it("asks once for a destination when several outputs own nothing", async () => {
    // The parts of a split. One dialog for the group, not one per part.
    files.set("o1", new File(["x"], "part1.pdf"));
    files.set("o2", new File(["y"], "part2.pdf"));

    await saveOperationResults(context(["o1", "o2"]));

    expect(saveMultipleMock).toHaveBeenCalledTimes(1);
    expect(downloadFileMock).not.toHaveBeenCalled();
    expect(markSaved).toHaveBeenCalledWith("o1", "/out/part1.pdf");
    expect(markSaved).toHaveBeenCalledWith("o2", "/out/part2.pdf");
  });

  it("uses a single Save As for a lone path-less output", async () => {
    files.set("o1", new File(["x"], "merged.pdf"));

    await saveOperationResults(context(["o1"]));

    expect(downloadFileMock).toHaveBeenCalledTimes(1);
    expect(downloadFileMock.mock.calls[0][0].localPath).toBeUndefined();
    expect(saveMultipleMock).not.toHaveBeenCalled();
  });

  it("does not report a partial group save as success", async () => {
    files.set("o1", new File(["x"], "part1.pdf"));
    files.set("o2", new File(["y"], "part2.pdf"));
    saveMultipleMock.mockResolvedValue({
      success: false,
      savedCount: 1,
      savedPaths: ["/out/part1.pdf", null],
      error: "Saved 1/2 files. Errors: part2.pdf: EACCES",
    });

    await expect(saveOperationResults(context(["o1", "o2"]))).rejects.toThrow(
      "Saved 1/2",
    );
    // The one that landed is still recorded; only the failure is surfaced.
    expect(markSaved).toHaveBeenCalledWith("o1", "/out/part1.pdf");
    expect(markSaved).not.toHaveBeenCalledWith("o2", expect.anything());
  });

  it("stays quiet when the user cancels the destination prompt", async () => {
    files.set("o1", new File(["x"], "part1.pdf"));
    files.set("o2", new File(["y"], "part2.pdf"));
    saveMultipleMock.mockResolvedValue({
      success: false,
      savedCount: 0,
      cancelledByUser: true,
      savedPaths: [null, null],
    });

    await expect(
      saveOperationResults(context(["o1", "o2"])),
    ).resolves.toBeNull();
    expect(markSaved).not.toHaveBeenCalled();
  });
});
