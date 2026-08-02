import {
  filtersForFilename,
  openDesktopDirectoryDialog,
  saveDesktopFileDialog,
} from "@app/services/desktop/desktopDialog";
import {
  joinDesktopPath,
  writeDesktopFile,
} from "@app/services/desktop/desktopFs";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export interface SaveResult {
  success: boolean;
  error?: string;
}

export interface MultiFileSaveResult {
  success: boolean;
  savedCount: number;
  cancelledByUser?: boolean;
  error?: string;
  /**
   * Where each input file ended up, parallel to the `files` argument; null for
   * one that failed. Callers need the actual paths to record what is now on
   * disk — a count alone cannot tell them which file was written where.
   */
  savedPaths: (string | null)[];
}

/**
 * Write file data to an absolute filesystem path.
 *
 * Desktop only. Web builds have no such capability — a browser can offer a
 * download, but it cannot overwrite the file the user opened — so this reports
 * failure there and callers fall back to the download path.
 */
export async function saveToLocalPath(
  data: Blob | File,
  filePath: string,
): Promise<SaveResult> {
  if (!isDesktopRuntime()) {
    return {
      success: false,
      error: "Local file save not available in web mode",
    };
  }
  const arrayBuffer = await data.arrayBuffer();
  return writeDesktopFile(filePath, new Uint8Array(arrayBuffer));
}

/**
 * Show a native save dialog and return the chosen path, or null if cancelled
 * (or on web, where there is no native dialog).
 */
export async function showSaveDialog(
  defaultFilename: string,
  defaultDirectory?: string,
): Promise<string | null> {
  if (!isDesktopRuntime()) {
    return null;
  }
  return saveDesktopFileDialog({
    defaultPath: defaultDirectory
      ? `${defaultDirectory}/${defaultFilename}`
      : defaultFilename,
    // Derived from the filename, not hardcoded to PDF: a conversion result is
    // a .docx/.zip/.png and a PDF-only filter would mislabel it.
    filters: filtersForFilename(defaultFilename),
    title: "Save As",
  });
}

/**
 * Make `name` unique within `used`, suffixing before the extension.
 *
 * Two outputs of one run can share a name — a split whose parts are all
 * `page.pdf`, or two tools producing `document.pdf`. Writing both into the
 * chosen directory by name means the second silently destroys the first while
 * `savedPaths` still reports two successful saves, so the caller marks both
 * clean and the user is told everything was written.
 */
function uniqueName(name: string, used: Set<string>): string {
  if (!used.has(name)) {
    used.add(name);
    return name;
  }
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const extension = dot > 0 ? name.slice(dot) : "";
  for (let counter = 2; ; counter++) {
    const candidate = `${stem} (${counter})${extension}`;
    if (!used.has(candidate)) {
      used.add(candidate);
      return candidate;
    }
  }
}

/**
 * Prompt for a folder and write several files into it.
 *
 * Desktop only; web builds report failure so callers fall back to per-file
 * downloads.
 */
export async function saveMultipleFilesWithPrompt(
  files: (Blob | File)[],
  defaultDirectory?: string,
): Promise<MultiFileSaveResult> {
  const savedPaths: (string | null)[] = files.map(() => null);

  if (!isDesktopRuntime()) {
    return {
      success: false,
      savedCount: 0,
      error: "Multi-file save not available in web mode",
      savedPaths,
    };
  }

  const folder = await openDesktopDirectoryDialog({
    defaultPath: defaultDirectory,
    title: `Save ${files.length} file${files.length > 1 ? "s" : ""}`,
  });
  if (!folder) {
    return {
      success: false,
      savedCount: 0,
      cancelledByUser: true,
      savedPaths,
    };
  }

  let savedCount = 0;
  const errors: string[] = [];
  const usedNames = new Set<string>();

  for (let index = 0; index < files.length; index++) {
    const file = files[index];
    const fileName = uniqueName(
      file instanceof File ? file.name : `output_${index + 1}.pdf`,
      usedNames,
    );
    try {
      const filePath = await joinDesktopPath(folder, fileName);
      const arrayBuffer = await file.arrayBuffer();
      const result = await writeDesktopFile(
        filePath,
        new Uint8Array(arrayBuffer),
      );
      if (result.success) {
        savedPaths[index] = filePath;
        savedCount++;
      } else {
        errors.push(`${fileName}: ${result.error}`);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      errors.push(`${fileName}: ${message}`);
    }
  }

  if (savedCount === files.length) {
    return { success: true, savedCount, savedPaths };
  }
  if (savedCount > 0) {
    return {
      success: false,
      savedCount,
      error: `Saved ${savedCount}/${files.length} files. Errors: ${errors.join(", ")}`,
      savedPaths,
    };
  }
  return {
    success: false,
    savedCount: 0,
    savedPaths,
    error: `Failed to save files: ${errors.join(", ")}`,
  };
}
