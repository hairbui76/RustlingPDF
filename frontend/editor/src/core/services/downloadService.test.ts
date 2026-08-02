import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * The save chain, both ways.
 *
 * Web must stay exactly as it was — an anchor-click download that reports no
 * saved path, because the browser never tells the page where the file went.
 * Desktop must write to the filesystem and report the path it wrote, because
 * that returned path is the only thing that lets a caller clear `isDirty` and
 * stop drawing a saved file as unsaved.
 */

const isDesktopRuntimeMock = vi.fn<() => boolean>();
const saveToLocalPathMock = vi.fn();
const showSaveDialogMock = vi.fn();

vi.mock("@app/services/desktop/desktopRuntime", () => ({
  isDesktopRuntime: () => isDesktopRuntimeMock(),
}));

vi.mock("@app/services/localFileSaveService", () => ({
  saveToLocalPath: (...args: unknown[]) => saveToLocalPathMock(...args),
  showSaveDialog: (...args: unknown[]) => showSaveDialogMock(...args),
}));

import { downloadFile } from "@app/services/downloadService";

describe("downloadFile", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    saveToLocalPathMock.mockResolvedValue({ success: true });
    showSaveDialogMock.mockResolvedValue(null);
  });

  describe("web", () => {
    beforeEach(() => isDesktopRuntimeMock.mockReturnValue(false));

    it("triggers a browser download and never touches the filesystem", async () => {
      const click = vi.spyOn(HTMLAnchorElement.prototype, "click");
      click.mockImplementation(() => {});

      const result = await downloadFile({
        data: new Blob(["x"]),
        filename: "out.pdf",
      });

      expect(click).toHaveBeenCalledTimes(1);
      expect(saveToLocalPathMock).not.toHaveBeenCalled();
      expect(showSaveDialogMock).not.toHaveBeenCalled();
      expect(result).toEqual({ savedPath: undefined });
      click.mockRestore();
    });
  });

  describe("desktop", () => {
    beforeEach(() => isDesktopRuntimeMock.mockReturnValue(true));

    it("overwrites the source file in place when a local path is known", async () => {
      const result = await downloadFile({
        data: new Blob(["x"]),
        filename: "out.pdf",
        localPath: "/home/u/out.pdf",
      });

      // No dialog: "Save" over an already-known path must not ask again.
      expect(showSaveDialogMock).not.toHaveBeenCalled();
      expect(saveToLocalPathMock).toHaveBeenCalledWith(
        expect.anything(),
        "/home/u/out.pdf",
      );
      expect(result).toEqual({ savedPath: "/home/u/out.pdf" });
    });

    it("prompts with a save dialog when there is no local path", async () => {
      showSaveDialogMock.mockResolvedValue("/home/u/chosen.pdf");

      const result = await downloadFile({
        data: new Blob(["x"]),
        filename: "out.pdf",
      });

      expect(showSaveDialogMock).toHaveBeenCalledWith("out.pdf");
      expect(result).toEqual({ savedPath: "/home/u/chosen.pdf" });
    });

    it("reports cancellation rather than a save when the dialog is dismissed", async () => {
      showSaveDialogMock.mockResolvedValue(null);

      const result = await downloadFile({
        data: new Blob(["x"]),
        filename: "out.pdf",
      });

      // Must not be reported as saved — a caller that cleared isDirty here
      // would mark an unsaved file clean.
      expect(result).toEqual({ cancelled: true });
      expect(saveToLocalPathMock).not.toHaveBeenCalled();
    });

    it("throws when the write fails, so no caller marks the file clean", async () => {
      saveToLocalPathMock.mockResolvedValue({
        success: false,
        error: "EACCES",
      });

      await expect(
        downloadFile({
          data: new Blob(["x"]),
          filename: "out.pdf",
          localPath: "/root/out.pdf",
        }),
      ).rejects.toThrow("EACCES");
    });
  });
});
