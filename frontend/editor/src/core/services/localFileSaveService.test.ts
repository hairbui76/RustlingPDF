import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * Writing a group of outputs into a folder the user picked.
 *
 * The property that matters: no output may silently destroy another. Two
 * results of one run sharing a filename is ordinary — a split whose parts are
 * all `page.pdf` — and writing both by name would leave one file where the
 * caller believes there are two.
 */

const isDesktopRuntimeMock = vi.fn<() => boolean>();
const openDirectoryMock = vi.fn();
const writeMock = vi.fn();

vi.mock("@app/services/desktop/desktopRuntime", () => ({
  isDesktopRuntime: () => isDesktopRuntimeMock(),
}));

vi.mock("@app/services/desktop/desktopDialog", () => ({
  openDesktopDirectoryDialog: (...args: unknown[]) =>
    openDirectoryMock(...args),
  saveDesktopFileDialog: vi.fn(),
  filtersForFilename: () => [],
}));

vi.mock("@app/services/desktop/desktopFs", () => ({
  writeDesktopFile: (...args: unknown[]) => writeMock(...args),
  joinDesktopPath: async (...parts: string[]) => parts.join("/"),
}));

import { saveMultipleFilesWithPrompt } from "@app/services/localFileSaveService";

describe("saveMultipleFilesWithPrompt", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    isDesktopRuntimeMock.mockReturnValue(true);
    openDirectoryMock.mockResolvedValue("/out");
    writeMock.mockResolvedValue({ success: true });
  });

  it("gives same-named outputs distinct filenames", async () => {
    const result = await saveMultipleFilesWithPrompt([
      new File(["a"], "page.pdf"),
      new File(["b"], "page.pdf"),
      new File(["c"], "page.pdf"),
    ]);

    expect(result.success).toBe(true);
    expect(result.savedPaths).toEqual([
      "/out/page.pdf",
      "/out/page (2).pdf",
      "/out/page (3).pdf",
    ]);
    // Three distinct destinations, so three files actually exist afterwards.
    expect(new Set(result.savedPaths).size).toBe(3);
  });

  it("keeps the extension when disambiguating", async () => {
    const result = await saveMultipleFilesWithPrompt([
      new File(["a"], "report.docx"),
      new File(["b"], "report.docx"),
    ]);
    expect(result.savedPaths[1]).toBe("/out/report (2).docx");
  });

  it("reports the path of each file, and null for one that failed", async () => {
    writeMock
      .mockResolvedValueOnce({ success: true })
      .mockResolvedValueOnce({ success: false, error: "EACCES" });

    const result = await saveMultipleFilesWithPrompt([
      new File(["a"], "a.pdf"),
      new File(["b"], "b.pdf"),
    ]);

    expect(result.success).toBe(false);
    expect(result.savedCount).toBe(1);
    expect(result.savedPaths).toEqual(["/out/a.pdf", null]);
  });

  it("reports cancellation without writing anything", async () => {
    openDirectoryMock.mockResolvedValue(null);

    const result = await saveMultipleFilesWithPrompt([
      new File(["a"], "a.pdf"),
    ]);

    expect(result.cancelledByUser).toBe(true);
    expect(result.savedPaths).toEqual([null]);
    expect(writeMock).not.toHaveBeenCalled();
  });

  it("does nothing on web", async () => {
    isDesktopRuntimeMock.mockReturnValue(false);

    const result = await saveMultipleFilesWithPrompt([
      new File(["a"], "a.pdf"),
    ]);

    expect(result.success).toBe(false);
    expect(openDirectoryMock).not.toHaveBeenCalled();
  });
});
