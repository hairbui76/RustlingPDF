import { beforeEach, describe, expect, it, vi } from "vitest";
import { peekLocalFilePath } from "@app/services/localFilePathRegistry";

/**
 * End of the chain that arms the cross-write: the native dialog hands back
 * files, and whatever pairs them to their paths decides which file a later
 * "Save" overwrites.
 */

const openFileDialogMock = vi.fn();

vi.mock("@app/services/fileDialogService", () => ({
  openFileDialog: (...args: unknown[]) => openFileDialogMock(...args),
}));

import { openFilesFromDisk } from "@app/services/openFilesFromDisk";

describe("openFilesFromDisk", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("gives each file its own path even when they are metadata-identical", async () => {
    // Two different documents that a metadata key cannot tell apart: same
    // name, same size, same timestamp. This is the "merge two copies of the
    // same document from different folders" case that shipped broken.
    const lastModified = 1_700_000_000_000;
    const a = new File([new Uint8Array(16)], "report.pdf", { lastModified });
    const b = new File([new Uint8Array(16)], "report.pdf", { lastModified });

    openFileDialogMock.mockResolvedValue([
      { file: a, path: "/work/report.pdf" },
      { file: b, path: "/backup/report.pdf" },
    ]);

    const files = await openFilesFromDisk();

    expect(files).toEqual([a, b]);
    expect(peekLocalFilePath(a)).toBe("/work/report.pdf");
    expect(peekLocalFilePath(b)).toBe("/backup/report.pdf");
  });

  it("falls back when the dialog returns nothing", async () => {
    openFileDialogMock.mockResolvedValue([]);
    const onFallbackOpen = vi.fn();

    const files = await openFilesFromDisk({ onFallbackOpen });

    expect(files).toEqual([]);
    expect(onFallbackOpen).toHaveBeenCalledTimes(1);
  });
});
