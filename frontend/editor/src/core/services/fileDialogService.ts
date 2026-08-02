import { openDesktopFileDialog } from "@app/services/desktop/desktopDialog";
import {
  basename,
  readDesktopFile,
  toArrayBuffer,
} from "@app/services/desktop/desktopFs";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import { getDocumentFileDialogFilter } from "@app/utils/fileDialogUtils";
import { createQuickKey } from "@app/types/fileContext";

export interface FileWithPath {
  file: File;
  path: string;
  quickKey: string;
}

export interface FileDialogOptions {
  multiple?: boolean;
  filters?: Array<{
    name: string;
    extensions: string[];
  }>;
}

/**
 * Open a native file dialog and read the selected files.
 *
 * This is the only source of `pendingFilePathMappings`, which is the only
 * thing that sets a stub's `localFilePath` — and `localFilePath` is what makes
 * "Save" write back to the file the user opened instead of dropping a copy in
 * Downloads. When this returned `[]` unconditionally (the stub it replaces),
 * every file in the desktop app was path-less, so nothing could be saved in
 * place, nothing could be marked clean after saving, and the Save As button
 * was hidden entirely.
 *
 * Returns `[]` on web, and `[]` when the user cancels. Callers treat that as
 * "fall back to the browser file input" (see `openFilesFromDisk`).
 */
export async function openFileDialog(
  options?: FileDialogOptions,
): Promise<FileWithPath[]> {
  if (!isDesktopRuntime()) {
    return [];
  }

  const paths = await openDesktopFileDialog({
    multiple: options?.multiple ?? true,
    filters: options?.filters ?? getDocumentFileDialogFilter(),
  });

  const filesWithPaths: FileWithPath[] = [];
  for (const filePath of paths) {
    const bytes = await readDesktopFile(filePath);
    if (!bytes) {
      // Already logged. Skip the unreadable one; the rest of a multi-select
      // must still open.
      continue;
    }
    const fileName = basename(filePath, "document");
    const file = new File([toArrayBuffer(bytes)], fileName, {
      type: fileName.toLowerCase().endsWith(".pdf")
        ? "application/pdf"
        : undefined,
    });
    filesWithPaths.push({
      file,
      path: filePath,
      quickKey: createQuickKey(file),
    });
  }

  return filesWithPaths;
}
