import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OpenedFileBatch } from "@app/services/fileOpenService";

// ── Mocks (hoisted by vi.mock) ──────────────────────────────────────────────
const useOpenedFileMock = vi.fn();
const readFileMock = vi.fn();
const navigateMock = vi.fn();
const addFilesMock = vi.fn();
const updateStubMock = vi.fn();

vi.mock("@app/hooks/useOpenedFile", () => ({
  useOpenedFile: () => useOpenedFileMock(),
}));

vi.mock("@app/services/fileOpenService", () => ({
  fileOpenService: {
    readFileAsArrayBuffer: (path: string) => readFileMock(path),
  },
}));

vi.mock("@app/services/toolIntentService", () => ({
  navigateToToolIntent: (intent: unknown) => navigateMock(intent),
}));

vi.mock("@app/contexts/file/fileHooks", () => ({
  useFileManagement: () => ({
    addFiles: addFilesMock,
    updateStirlingFileStub: updateStubMock,
  }),
}));

vi.mock("@app/types/fileContext", () => ({
  createQuickKey: (file: File) => file.name,
}));

import { useAppInitialization } from "@app/hooks/useAppInitialization";

/** Flush pending microtasks so awaited promises settle. */
async function flushMicrotasks() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
  }
}

function stubOpenedBatches(batches: OpenedFileBatch[]) {
  useOpenedFileMock.mockReturnValue({
    openedFileBatches: batches,
    loading: false,
    clearOpenedFileBatches: vi.fn(),
    consumeOpenedFileBatches: () => batches,
  });
}

async function renderInitialization() {
  await act(async () => {
    renderHook(() => useAppInitialization());
    await flushMicrotasks();
  });
}

describe("useAppInitialization (desktop launch-intent seam)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    readFileMock.mockImplementation(async (path: string) => ({
      fileName: path.split(/[\\/]/).pop() ?? "opened-file.pdf",
      arrayBuffer: new ArrayBuffer(4),
    }));
    addFilesMock.mockImplementation(async (files: File[]) =>
      files.map((file) => ({
        quickKey: file.name,
        fileId: `id-${file.name}`,
      })),
    );
    navigateMock.mockReturnValue(false);
  });

  it("adds an intent batch to the workspace and routes to the mapped tool", async () => {
    stubOpenedBatches([{ paths: ["/x/a.pdf", "/x/b.pdf"], tool: "merge" }]);

    await renderInitialization();

    // One addFiles call for the whole batch, files selected.
    expect(addFilesMock).toHaveBeenCalledTimes(1);
    const [files, options] = addFilesMock.mock.calls[0];
    expect(files.map((f: File) => f.name)).toEqual(["a.pdf", "b.pdf"]);
    expect(options).toEqual({ selectFiles: true });

    // Local paths are attached to the added stubs.
    expect(updateStubMock).toHaveBeenCalledWith("id-a.pdf", {
      localFilePath: "/x/a.pdf",
    });
    expect(updateStubMock).toHaveBeenCalledWith("id-b.pdf", {
      localFilePath: "/x/b.pdf",
    });

    // The intent is routed once, after the files were added.
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

  it("skips routing when no file in the intent batch could be read", async () => {
    readFileMock.mockResolvedValue(null);
    stubOpenedBatches([{ paths: ["/gone/a.pdf"], tool: "compress" }]);

    await renderInitialization();

    expect(addFilesMock).not.toHaveBeenCalled();
    expect(navigateMock).not.toHaveBeenCalled();
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
    useOpenedFileMock.mockReturnValue({
      openedFileBatches: [{ paths: ["/x/a.pdf"], tool: "merge" }],
      loading: true,
      clearOpenedFileBatches: vi.fn(),
      consumeOpenedFileBatches: vi.fn(() => []),
    });

    await renderInitialization();

    expect(addFilesMock).not.toHaveBeenCalled();
    expect(navigateMock).not.toHaveBeenCalled();
  });
});
