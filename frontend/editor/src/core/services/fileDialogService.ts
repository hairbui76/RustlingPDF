import { openDesktopFileDialog } from "@app/services/desktop/desktopDialog";
import {
  basename,
  readDesktopFileWithMeta,
  toArrayBuffer,
} from "@app/services/desktop/desktopFs";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import { getDocumentFileDialogFilter } from "@app/utils/fileDialogUtils";

export interface FileWithPath {
  file: File;
  path: string;
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
 * Each returned `File` is paired with the path it was read from by object
 * identity — `openFilesFromDisk` hands that pairing to
 * `localFilePathRegistry`, and `localFilePath` is what makes "Save" write back
 * to the file the user opened instead of dropping a copy in Downloads. When
 * this returned `[]` unconditionally (the stub it replaces), every file in the
 * desktop app was path-less, so nothing could be saved in place, nothing could
 * be marked clean after saving, and the Save As button was hidden entirely.
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
    const read = await readDesktopFileWithMeta(filePath);
    if (!read) {
      // Already logged. Skip the unreadable one; the rest of a multi-select
      // must still open.
      continue;
    }
    const fileName = basename(filePath, "document");
    const file = new File([toArrayBuffer(read.bytes)], fileName, {
      type: fileName.toLowerCase().endsWith(".pdf")
        ? "application/pdf"
        : undefined,
      // The file's real mtime, not the moment we read it. Without this the
      // constructor defaults to Date.now(), and two distinct documents with
      // the same name and size read in one tick get identical `quickKey`s —
      // so one is discarded as a duplicate of the other.
      lastModified: read.lastModified,
    });
    filesWithPaths.push({ file, path: filePath });
  }

  return filesWithPaths;
}
