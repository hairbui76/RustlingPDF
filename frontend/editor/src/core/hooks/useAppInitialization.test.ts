import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OpenedFileBatch } from "@app/services/fileOpenService";

/**
 * The startup seam that turns a queued OS launch into files in the workspace
 * (and, for an Explorer context-menu verb, into the mapped tool being opened).
 *
 * `useOpenedFile` is mocked here — its own drain behaviour is covered in
 * useOpenedFile.test.ts — so these tests are about what happens to a batch
 * once it has been drained.
 */

const useOpenedFileMock = vi.fn();
const readOpenedFileMock = vi.fn();
const navigateMock = vi.fn();
const addFilesMock = vi.fn();
// Hoisted so the vi.mock factory below (which vitest lifts to the top of the
// file) can reference it without a temporal-dead-zone error.
const { registeredPaths } = vi.hoisted(() => ({
  registeredPaths: new Map<File, string>(),
}));

vi.mock("@app/hooks/useOpenedFile", () => ({
  useOpenedFile: () => useOpenedFileMock(),
}));

vi.mock("@app/hooks/useSaveShortcut", () => ({
  useSaveShortcut: () => {},
}));

vi.mock("@app/services/fileOpenService", () => ({
  readOpenedFile: (path: string) => readOpenedFileMock(path),
}));

vi.mock("@app/services/toolIntentService", () => ({
  navigateToToolIntent: (intent: unknown) => navigateMock(intent),
}));

vi.mock("@app/services/localFilePathRegistry", () => ({
  rememberLocalFilePath: (file: File, path: string) =>
    registeredPaths.set(file, path),
}));

vi.mock("@app/services/backendHealthMonitor", () => ({
  backendHealthMonitor: { subscribe: () => () => {} },
}));

vi.mock("@app/contexts/file/fileHooks", () => ({
  useFileManagement: () => ({ addFiles: addFilesMock }),
}));

import { useAppInitialization } from "@app/hooks/useAppInitialization";

async function flush() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
  }
}

function stubOpenedBatches(batches: OpenedFileBatch[], loading = false) {
  useOpenedFileMock.mockReturnValue({
    openedFileBatches: batches,
    loading,
    consumeOpenedFileBatches: () => batches,
  });
}

async function renderInitialization() {
  await act(async () => {
    renderHook(() => useAppInitialization());
    await flush();
  });
}

describe("useAppInitialization", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    registeredPaths.clear();
    readOpenedFileMock.mockImplementation(async (path: string) => ({
      fileName: path.split(/[\\/]/).pop() ?? "opened-file.pdf",
      arrayBuffer: new ArrayBuffer(4),
      lastModified: 1_700_000_000_000,
    }));
    addFilesMock.mockResolvedValue([]);
    navigateMock.mockReturnValue(false);
  });

  it("adds an intent batch to the workspace and routes to the mapped tool", async () => {
    stubOpenedBatches([{ paths: ["/x/a.pdf", "/x/b.pdf"], tool: "merge" }]);

    await renderInitialization();

    expect(addFilesMock).toHaveBeenCalledTimes(1);
    const [files, options] = addFilesMock.mock.calls[0];
    expect(files.map((file: File) => file.name)).toEqual(["a.pdf", "b.pdf"]);
    expect(options).toEqual({ selectFiles: true });

    // The on-disk path must be registered BEFORE addFiles runs — that is what
    // attaches localFilePath, and localFilePath is what makes the file
    // saveable in place afterwards. Registered against the File objects that
    // were actually handed to addFiles, so nothing is resolved by metadata.
    expect(registeredPaths.get(files[0])).toBe("/x/a.pdf");
    expect(registeredPaths.get(files[1])).toBe("/x/b.pdf");

    expect(navigateMock).toHaveBeenCalledTimes(1);
    expect(navigateMock).toHaveBeenCalledWith("merge");
    expect(navigateMock.mock.invocationCallOrder[0]).toBeGreaterThan(
      addFilesMock.mock.invocationCallOrder[0],
    );
  });

  it("adds plain-open batches without any tool routing side effects", async () => {
    stubOpenedBatches([{ paths: ["/x/a.pdf"], tool: null }]);

    await renderInitialization();

    expect(addFilesMock).toHaveBeenCalledTimes(1);
    // The intent seam still runs but resolves to "no navigation".
    expect(navigateMock).toHaveBeenCalledWith(null);
  });

  it("skips a batch whose files could not be read", async () => {
    readOpenedFileMock.mockResolvedValue(null);
    stubOpenedBatches([{ paths: ["/gone/a.pdf"], tool: "compress" }]);

    await renderInitialization();

    expect(addFilesMock).not.toHaveBeenCalled();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("keeps the readable files when one path in a batch is gone", async () => {
    readOpenedFileMock.mockImplementation(async (path: string) =>
      path === "/x/gone.pdf"
        ? null
        : {
            fileName: "a.pdf",
            arrayBuffer: new ArrayBuffer(4),
            lastModified: 1_700_000_000_000,
          },
    );
    stubOpenedBatches([{ paths: ["/x/gone.pdf", "/x/a.pdf"], tool: null }]);

    await renderInitialization();

    const [files] = addFilesMock.mock.calls[0];
    expect(files.map((file: File) => file.name)).toEqual(["a.pdf"]);
  });

  it("processes batches in launch order so the last intent wins", async () => {
    stubOpenedBatches([
      { paths: ["/x/a.pdf"], tool: null },
      { paths: ["/x/b.pdf", "/x/c.pdf"], tool: "convert" },
    ]);

    await renderInitialization();

    expect(addFilesMock).toHaveBeenCalledTimes(2);
    expect(navigateMock).toHaveBeenLastCalledWith("convert");
  });

  it("does nothing while the opened-file queue is still loading", async () => {
    stubOpenedBatches([{ paths: ["/x/a.pdf"], tool: "merge" }], true);

    await renderInitialization();

    expect(addFilesMock).not.toHaveBeenCalled();
    expect(navigateMock).not.toHaveBeenCalled();
  });

  it("does nothing when there is nothing queued (the web case)", async () => {
    stubOpenedBatches([]);

    await renderInitialization();

    expect(readOpenedFileMock).not.toHaveBeenCalled();
    expect(addFilesMock).not.toHaveBeenCalled();
  });
});
