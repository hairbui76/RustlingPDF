/**
 * Native file dialogs for the desktop shell.
 *
 * Every function returns "the user chose nothing" on web, which is the same
 * shape callers already handle for a cancelled dialog — so the web build falls
 * through to its own `<input type="file">` path unchanged.
 */
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export interface DesktopDialogFilter {
  name: string;
  extensions: string[];
}

/** Show a native open dialog. Returns the selected absolute paths. */
export async function openDesktopFileDialog(options: {
  multiple?: boolean;
  filters?: DesktopDialogFilter[];
}): Promise<string[]> {
  if (!isDesktopRuntime()) {
    return [];
  }
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: options.multiple ?? true,
      filters: options.filters,
    });
    if (!selected) {
      return [];
    }
    return Array.isArray(selected) ? selected : [selected];
  } catch (error) {
    console.error("[desktopDialog] Open dialog failed:", error);
    return [];
  }
}

/** Show a native directory picker. Returns the chosen path, or null. */
export async function openDesktopDirectoryDialog(options: {
  defaultPath?: string;
  title?: string;
}): Promise<string | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: options.defaultPath,
      title: options.title,
    });
    return typeof selected === "string" ? selected : null;
  } catch (error) {
    console.error("[desktopDialog] Directory dialog failed:", error);
    return null;
  }
}

/** Show a native save dialog. Returns the chosen path, or null if cancelled. */
export async function saveDesktopFileDialog(options: {
  defaultPath: string;
  filters?: DesktopDialogFilter[];
  title?: string;
}): Promise<string | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  try {
    const { save } = await import("@tauri-apps/plugin-dialog");
    return await save({
      defaultPath: options.defaultPath,
      filters: options.filters,
      title: options.title,
    });
  } catch (error) {
    console.error("[desktopDialog] Save dialog failed:", error);
    return null;
  }
}

/**
 * Derive a dialog filter from a filename's extension.
 *
 * Without this the dialog forces `.pdf` onto every output, so saving a
 * conversion result (`.docx`, `.zip`, `.png`) writes a mislabelled file.
 */
export function filtersForFilename(filename: string): DesktopDialogFilter[] {
  const extension = filename.split(".").pop()?.toLowerCase() ?? "";
  if (!extension || extension === filename.toLowerCase()) {
    return [];
  }
  return [{ name: extension.toUpperCase(), extensions: [extension] }];
}
