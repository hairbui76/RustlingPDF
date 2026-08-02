import {
  basename,
  readDesktopFileWithMeta,
  toArrayBuffer,
} from "@app/services/desktop/desktopFs";
import {
  DESKTOP_COMMANDS,
  desktopInvoke,
  isDesktopRuntime,
  listenOnDesktopWindow,
} from "@app/services/desktop/desktopRuntime";

/**
 * One batch of opened file paths plus the optional tool intent they arrived
 * with (an Explorer context-menu `--tool <action>` launch). Plain opens —
 * double-click, "Open with", drag onto the shortcut, macOS open event — carry
 * `tool: null`.
 *
 * Mirrors `OpenedFileBatch` in `src-tauri/src/commands/files.rs`.
 */
export interface OpenedFileBatch {
  paths: string[];
  tool: string | null;
}

/**
 * Event the Rust side emits at a window whenever that window's opened-file
 * queue gains entries. It carries no payload: the intent rides in the *queue*,
 * not the event, so a cold launch whose webview mounts seconds after the
 * enqueue still sees it.
 */
export const FILES_CHANGED_EVENT = "files-changed";

/**
 * Atomically take the queued batches. Empty on web, and empty on any failure —
 * a launch that cannot be drained must leave an empty workspace, never a
 * crashed startup.
 */
export async function popOpenedFileBatches(): Promise<OpenedFileBatch[]> {
  if (!isDesktopRuntime()) {
    return [];
  }
  try {
    return await desktopInvoke<OpenedFileBatch[]>(
      DESKTOP_COMMANDS.popOpenedBatches,
    );
  } catch (error) {
    console.error("[fileOpen] Failed to pop opened file batches:", error);
    return [];
  }
}

/** Read one queued path into a File-ready buffer, or null if unreadable. */
export async function readOpenedFile(filePath: string): Promise<{
  fileName: string;
  arrayBuffer: ArrayBuffer;
  /**
   * The file's real mtime. Passed to the `File` constructor so `quickKey`
   * (`name|size|lastModified`) identifies the document rather than the instant
   * we read it — otherwise two same-named, same-sized files opened together
   * collide and one is dropped as a duplicate of the other.
   */
  lastModified: number;
} | null> {
  const read = await readDesktopFileWithMeta(filePath);
  if (!read) {
    return null;
  }
  return {
    fileName: basename(filePath, "opened-file.pdf"),
    arrayBuffer: toArrayBuffer(read.bytes),
    lastModified: read.lastModified,
  };
}

/** Subscribe to this window's queue-changed notifications. */
export function onOpenedFilesChanged(handler: () => void): () => void {
  let unlisten: (() => void) | undefined;
  let cancelled = false;

  void listenOnDesktopWindow(FILES_CHANGED_EVENT, handler).then((fn) => {
    // The listener registration is async; if the subscriber unsubscribed while
    // it was in flight, tear the listener down immediately rather than leaking
    // it for the life of the window.
    if (cancelled) {
      fn();
    } else {
      unlisten = fn;
    }
  });

  return () => {
    cancelled = true;
    unlisten?.();
  };
}
