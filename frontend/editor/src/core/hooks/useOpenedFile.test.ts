import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { expectConsole } from "@app/tests/failOnConsole";
import type { OpenedFileBatch } from "@app/services/fileOpenService";

/**
 * The queue drain, exercised against a faked `files-changed` event.
 *
 * Nothing here starts a webview, so what is proven is the drain *logic*: that
 * a cold launch is picked up on mount, that a warm launch delivered as an
 * event is picked up too, that batches accumulate rather than overwrite, that
 * consuming empties the queue, and that web mode never touches any of it.
 * Whether Rust actually emits that event to this window is not testable here.
 */

const isDesktopRuntimeMock = vi.fn<() => boolean>();
const popOpenedFileBatchesMock = vi.fn<() => Promise<OpenedFileBatch[]>>();
const unsubscribeMock = vi.fn();
let changeHandler: (() => void) | null = null;

vi.mock("@app/services/desktop/desktopRuntime", () => ({
  isDesktopRuntime: () => isDesktopRuntimeMock(),
}));

vi.mock("@app/services/fileOpenService", () => ({
  popOpenedFileBatches: () => popOpenedFileBatchesMock(),
  onOpenedFilesChanged: (handler: () => void) => {
    changeHandler = handler;
    return unsubscribeMock;
  },
}));

import { useOpenedFile } from "@app/hooks/useOpenedFile";

/** Flush pending microtasks so awaited promises settle. */
async function flush() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
  }
}

describe("useOpenedFile", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset, not just clear: `mockResolvedValueOnce` queues survive
    // clearAllMocks, so an unconsumed one would leak into the next test.
    popOpenedFileBatchesMock.mockReset();
    changeHandler = null;
    isDesktopRuntimeMock.mockReturnValue(true);
    popOpenedFileBatchesMock.mockResolvedValue([]);
  });

  it("drains the queue on mount (cold launch)", async () => {
    const batch: OpenedFileBatch = { paths: ["/x/a.pdf"], tool: "merge" };
    popOpenedFileBatchesMock.mockResolvedValueOnce([batch]);

    const { result } = renderHook(() => useOpenedFile());
    await act(flush);

    expect(result.current.loading).toBe(false);
    expect(result.current.openedFileBatches).toEqual([batch]);
  });

  it("drains again when the window reports files-changed (warm launch)", async () => {
    const { result } = renderHook(() => useOpenedFile());
    await act(flush);
    expect(result.current.openedFileBatches).toEqual([]);

    const late: OpenedFileBatch = { paths: ["/x/late.pdf"], tool: null };
    popOpenedFileBatchesMock.mockResolvedValueOnce([late]);

    expect(changeHandler).toBeTypeOf("function");
    await act(async () => {
      changeHandler?.();
      await flush();
    });

    expect(result.current.openedFileBatches).toEqual([late]);
  });

  it("accumulates a second batch rather than replacing the first", async () => {
    const first: OpenedFileBatch = { paths: ["/x/a.pdf"], tool: null };
    popOpenedFileBatchesMock.mockResolvedValueOnce([first]);

    const { result } = renderHook(() => useOpenedFile());
    await act(flush);

    const second: OpenedFileBatch = { paths: ["/x/b.pdf"], tool: "compress" };
    popOpenedFileBatchesMock.mockResolvedValueOnce([second]);
    await act(async () => {
      changeHandler?.();
      await flush();
    });

    // Replacing here would silently drop the first launch's files.
    expect(result.current.openedFileBatches).toEqual([first, second]);
  });

  it("consuming returns the batches once and empties the queue", async () => {
    const batch: OpenedFileBatch = { paths: ["/x/a.pdf"], tool: null };
    popOpenedFileBatchesMock.mockResolvedValueOnce([batch]);

    const { result } = renderHook(() => useOpenedFile());
    await act(flush);

    let consumed: OpenedFileBatch[] = [];
    act(() => {
      consumed = result.current.consumeOpenedFileBatches();
    });
    expect(consumed).toEqual([batch]);
    expect(result.current.openedFileBatches).toEqual([]);

    act(() => {
      consumed = result.current.consumeOpenedFileBatches();
    });
    expect(consumed).toEqual([]);
  });

  it("keeps launch order when a slow pop overlaps a fast one", async () => {
    // The first pop resolves last. Appending in resolve order would put the
    // second launch's batch first, and since batches are loaded in order, the
    // earlier launch's tool intent would win over the later one's.
    const first: OpenedFileBatch = { paths: ["/x/first.pdf"], tool: "merge" };
    const second: OpenedFileBatch = {
      paths: ["/x/second.pdf"],
      tool: "compress",
    };

    let releaseFirst: () => void = () => {};
    popOpenedFileBatchesMock
      .mockImplementationOnce(
        () =>
          new Promise<OpenedFileBatch[]>((resolve) => {
            releaseFirst = () => resolve([first]);
          }),
      )
      .mockResolvedValueOnce([second]);

    const { result } = renderHook(() => useOpenedFile());
    // Let the mount drain actually start, so it is genuinely in flight (and
    // `releaseFirst` is wired) before the second one is requested.
    await act(flush);

    await act(async () => {
      // Second drain requested while the first is still in flight.
      changeHandler?.();
      releaseFirst();
      await flush();
    });

    expect(result.current.openedFileBatches).toEqual([first, second]);
  });

  it("survives a failing pop and keeps draining afterwards", async () => {
    popOpenedFileBatchesMock.mockRejectedValueOnce(new Error("ipc down"));
    expectConsole.error("Failed to drain the opened-file queue");

    const { result } = renderHook(() => useOpenedFile());
    await act(flush);

    const later: OpenedFileBatch = { paths: ["/x/later.pdf"], tool: null };
    popOpenedFileBatchesMock.mockResolvedValueOnce([later]);
    await act(async () => {
      changeHandler?.();
      await flush();
    });

    expect(result.current.openedFileBatches).toEqual([later]);
  });

  it("unsubscribes on unmount", async () => {
    const { unmount } = renderHook(() => useOpenedFile());
    await act(flush);
    unmount();
    expect(unsubscribeMock).toHaveBeenCalled();
  });

  it("does nothing at all on web", async () => {
    isDesktopRuntimeMock.mockReturnValue(false);

    const { result } = renderHook(() => useOpenedFile());
    await act(flush);

    expect(popOpenedFileBatchesMock).not.toHaveBeenCalled();
    expect(changeHandler).toBeNull();
    // Never reports "still loading" in a runtime that has nothing to load.
    expect(result.current.loading).toBe(false);
    expect(result.current.openedFileBatches).toEqual([]);
  });
});
