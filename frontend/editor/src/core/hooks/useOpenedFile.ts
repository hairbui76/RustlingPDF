import { useCallback, useEffect, useRef, useState } from "react";
import {
  onOpenedFilesChanged,
  popOpenedFileBatches,
  type OpenedFileBatch,
} from "@app/services/fileOpenService";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export interface UseOpenedFileResult {
  openedFileBatches: OpenedFileBatch[];
  loading: boolean;
  /** Take the pending batches and clear them, so they load exactly once. */
  consumeOpenedFileBatches: () => OpenedFileBatch[];
}

/**
 * Drain the desktop shell's opened-file queue.
 *
 * The Rust side queues every file the OS hands the app — double-click, "Open
 * with", drag onto the shortcut, macOS open event, Explorer context-menu verb
 * — and emits `files-changed` at the owning window. Nothing else drains that
 * queue: without this hook the queue fills and the app opens an empty
 * workspace, which is exactly what shipped in v0.0.1.
 *
 * Both halves are needed. The initial pop covers a cold launch, where the
 * batch was enqueued before the webview existed and its event fired into the
 * void; the event listener covers a warm launch, where the app is already
 * running and the single-instance callback enqueues into a live window.
 *
 * No-op on web: `popOpenedFileBatches` returns `[]` and the listener is never
 * registered.
 */
export function useOpenedFile(): UseOpenedFileResult {
  const [openedFileBatches, setOpenedFileBatches] = useState<OpenedFileBatch[]>(
    [],
  );
  const [loading, setLoading] = useState(isDesktopRuntime());
  const pendingRef = useRef<OpenedFileBatch[]>([]);

  const consumeOpenedFileBatches = useCallback(() => {
    const pending = pendingRef.current;
    pendingRef.current = [];
    setOpenedFileBatches([]);
    return pending;
  }, []);

  useEffect(() => {
    if (!isDesktopRuntime()) {
      return;
    }

    let active = true;

    const drainQueue = async () => {
      const batches = await popOpenedFileBatches();
      if (!active) {
        return;
      }
      if (batches.length > 0) {
        // Append rather than replace: a `files-changed` event can land while a
        // previous batch is still being read off disk, and replacing here
        // would drop the earlier launch's files.
        pendingRef.current = [...pendingRef.current, ...batches];
        setOpenedFileBatches(pendingRef.current);
      }
      setLoading(false);
    };

    void drainQueue();
    const unsubscribe = onOpenedFilesChanged(() => {
      void drainQueue();
    });

    return () => {
      active = false;
      unsubscribe();
    };
  }, []);

  return { openedFileBatches, loading, consumeOpenedFileBatches };
}
