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

/** Write bytes to an absolute path. Returns an error message on failure. */
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
  try {
    const { writeFile } = await import("@tauri-apps/plugin-fs");
    await writeFile(filePath, data);
    return { success: true };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.error(`[desktopFs] Failed to write ${filePath}:`, message);
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
