/**
 * Filesystem access for the desktop shell.
 *
 * Every function here is a no-op (or a failure) on web, so callers never have
 * to branch on the runtime themselves — see `desktopRuntime.ts` for why the
 * `@tauri-apps/*` imports are dynamic and confined to this directory.
 */
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

/**
 * Read a file from an absolute path.
 *
 * Returns null on web, and null when the read fails (a queued path can have
 * been moved or deleted between the launch and the webview mounting — that
 * must degrade to "this one file did not open", never to a thrown startup).
 */
export async function readDesktopFile(
  filePath: string,
): Promise<Uint8Array | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  try {
    const { readFile } = await import("@tauri-apps/plugin-fs");
    return await readFile(filePath);
  } catch (error) {
    console.error(`[desktopFs] Failed to read ${filePath}:`, error);
    return null;
  }
}

/**
 * Last-modified time of a file, in epoch milliseconds, or null when unknown.
 *
 * Worth the extra IPC: it is what a `File` built from disk must carry as its
 * `lastModified`. Omitting it makes the `File` constructor default to
 * `Date.now()`, which turns `quickKey` (`name|size|lastModified`) into "the
 * millisecond we happened to read this", so two genuinely different documents
 * with the same name and size — read in the same tick, as they are inside a
 * `Promise.all` — become indistinguishable and one is dropped as a duplicate.
 */
export async function statDesktopFileMtime(
  filePath: string,
): Promise<number | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  try {
    const { stat } = await import("@tauri-apps/plugin-fs");
    const info = await stat(filePath);
    return info.mtime ? info.mtime.getTime() : null;
  } catch (error) {
    console.debug(`[desktopFs] Could not stat ${filePath}:`, error);
    return null;
  }
}

/**
 * Read a file together with the metadata a `File` needs to describe it
 * faithfully. Null when unreadable — see {@link readDesktopFile}.
 */
export async function readDesktopFileWithMeta(
  filePath: string,
): Promise<{ bytes: Uint8Array; lastModified: number } | null> {
  const bytes = await readDesktopFile(filePath);
  if (!bytes) {
    return null;
  }
  const mtime = await statDesktopFileMtime(filePath);
  return { bytes, lastModified: mtime ?? Date.now() };
}

/**
 * Write bytes to an absolute path, without destroying what is already there
 * until the new content is safely on disk.
 *
 * Writing straight to the target truncates it first, so a crash, a power
 * loss, or a full disk part-way through leaves the user with a truncated
 * file and no original. That risk was tolerable when in-place save barely
 * worked; it is not now that this is the primary save path and the target is
 * usually the user's own document.
 *
 * So: write a sibling temp file, then rename it over the target. `rename` is
 * atomic within a filesystem on POSIX, and Rust's `fs::rename` — which
 * plugin-fs calls — uses `MOVEFILE_REPLACE_EXISTING` on Windows, so it
 * replaces an existing destination on both. Any failure before the rename
 * leaves the original untouched; the temp file is then cleaned up.
 *
 * The temp name carries a random suffix so two concurrent saves to the same
 * target cannot clobber each other's staging file.
 */
export async function writeDesktopFile(
  filePath: string,
  data: Uint8Array,
): Promise<{ success: boolean; error?: string }> {
  if (!isDesktopRuntime()) {
    return {
      success: false,
      error: "Local file save is not available in this runtime",
    };
  }

  const suffix = Math.random().toString(36).slice(2, 8);
  const tempPath = `${filePath}.${suffix}.rustling-tmp`;

  try {
    const { writeFile, rename } = await import("@tauri-apps/plugin-fs");
    await writeFile(tempPath, data);
    await rename(tempPath, filePath);
    return { success: true };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`[desktopFs] Failed to write ${filePath}:`, message);
    // Best-effort: a leftover temp file next to the user's document is
    // confusing, but failing to remove it must not mask the write error.
    try {
      const { remove } = await import("@tauri-apps/plugin-fs");
      await remove(tempPath);
    } catch {
      // Nothing was staged, or it cannot be removed. Either way, report the
      // original failure.
    }
    return { success: false, error: message };
  }
}

/** Join path segments using the host platform's separator. */
export async function joinDesktopPath(...parts: string[]): Promise<string> {
  const { join } = await import("@tauri-apps/api/path");
  return join(...parts);
}

/**
 * Convert a `readFile` result to an ArrayBuffer without copying when possible.
 *
 * `readFile` usually returns a tightly packed buffer; handing it over directly
 * avoids a full copy, which for a large PDF is a transient 2x memory spike.
 * Only slice when the view is a window over a larger ArrayBuffer.
 */
export function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  if (bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength) {
    return bytes.buffer as ArrayBuffer;
  }
  return bytes.buffer.slice(
    bytes.byteOffset,
    bytes.byteOffset + bytes.byteLength,
  ) as ArrayBuffer;
}

/** Last path segment of an absolute path, on either separator convention. */
export function basename(filePath: string, fallback: string): string {
  return filePath.split(/[\\/]/).pop() || fallback;
}
