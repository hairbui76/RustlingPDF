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
 * Prompt for a folder and write several files into it.
 *
 * Desktop only; web builds report failure so callers fall back to per-file
 * downloads.
 */
export async function saveMultipleFilesWithPrompt(
  files: (Blob | File)[],
  defaultDirectory?: string,
): Promise<MultiFileSaveResult> {
  if (!isDesktopRuntime()) {
    return {
      success: false,
      savedCount: 0,
      error: "Multi-file save not available in web mode",
    };
  }

  const folder = await openDesktopDirectoryDialog({
    defaultPath: defaultDirectory,
    title: `Save ${files.length} file${files.length > 1 ? "s" : ""}`,
  });
  if (!folder) {
    return { success: false, savedCount: 0, cancelledByUser: true };
  }

  let savedCount = 0;
  const errors: string[] = [];

  for (const file of files) {
    const fileName =
      file instanceof File ? file.name : `output_${savedCount + 1}.pdf`;
    try {
      const filePath = await joinDesktopPath(folder, fileName);
      const arrayBuffer = await file.arrayBuffer();
      const result = await writeDesktopFile(
        filePath,
        new Uint8Array(arrayBuffer),
      );
      if (result.success) {
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
    return { success: true, savedCount };
  }
  if (savedCount > 0) {
    return {
      success: false,
      savedCount,
      error: `Saved ${savedCount}/${files.length} files. Errors: ${errors.join(", ")}`,
    };
  }
  return {
    success: false,
    savedCount: 0,
    error: `Failed to save files: ${errors.join(", ")}`,
  };
}
