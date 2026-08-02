import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import {
  saveToLocalPath,
  showSaveDialog,
} from "@app/services/localFileSaveService";

export interface DownloadRequest {
  data: Blob | File;
  filename: string;
  localPath?: string;
  /** Workspace fileId associated with the download, when known. */
  fileId?: string;
}

export interface DownloadResult {
  savedPath?: string;
  cancelled?: boolean;
}

/**
 * Hand a file to the user.
 *
 * Web: a browser download. `savedPath` stays undefined, because the browser
 * never tells the page where (or whether) the file landed.
 *
 * Desktop: a real filesystem write. With `localPath` it overwrites that file
 * in place — this is "Save". Without one it opens a native Save As dialog.
 * Either way it returns the path it wrote, and that returned path is what lets
 * callers clear `isDirty` and stop showing a saved file as unsaved.
 */
export async function downloadFile(
  request: DownloadRequest,
): Promise<DownloadResult> {
  if (isDesktopRuntime()) {
    const targetPath =
      request.localPath ?? (await showSaveDialog(request.filename));
    if (!targetPath) {
      return { cancelled: true };
    }
    const result = await saveToLocalPath(request.data, targetPath);
    if (!result.success) {
      throw new Error(result.error || "Failed to save file");
    }
    return { savedPath: targetPath };
  }

  const url = URL.createObjectURL(request.data);

  const link = document.createElement("a");
  link.href = url;
  link.download = request.filename;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);

  URL.revokeObjectURL(url);

  return { savedPath: request.localPath };
}

export async function downloadFromUrl(
  url: string,
  filename: string,
  localPath?: string,
): Promise<DownloadResult> {
  if (isDesktopRuntime()) {
    // Materialise the bytes first: an <a download> in the webview would either
    // be ignored or land in the OS download folder, neither of which can
    // honour `localPath` or report where the file went.
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`Download failed (${response.status})`);
    }
    return downloadFile({ data: await response.blob(), filename, localPath });
  }

  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);

  return { savedPath: localPath };
}
