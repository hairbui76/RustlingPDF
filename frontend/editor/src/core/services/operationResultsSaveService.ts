import type { FileId, RustlingFileStub } from "@app/types/fileContext";
import {
  downloadFile,
  downloadFromUrl,
  DownloadResult,
} from "@app/services/downloadService";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

export interface OperationSaveContext {
  downloadUrl: string | null;
  downloadFilename: string;
  downloadLocalPath?: string | null;
  outputFileIds?: string[] | null;
  getFile: (fileId: FileId) => File | undefined;
  getStub: (fileId: FileId) => RustlingFileStub | undefined;
  markSaved: (fileId: FileId, savedPath?: string) => void;
}

/**
 * Persist the results of a tool run.
 *
 * Web takes the aggregate: one `downloadUrl` (a ZIP when the run produced
 * several outputs), because a browser cannot write back to the files the user
 * dragged in, and firing N downloads at once is worse than one archive.
 *
 * Desktop takes the per-file path instead. Each output already knows the
 * on-disk file it came from, so writing each result to its own source path is
 * what "run a tool over my files" means there — the aggregate ZIP would force
 * the user to unpack it and copy the results back by hand.
 */
export async function saveOperationResults(
  context: OperationSaveContext,
): Promise<DownloadResult | null> {
  if (!context.downloadUrl) return null;

  if (
    isDesktopRuntime() &&
    context.outputFileIds &&
    context.outputFileIds.length > 0
  ) {
    for (const fileId of context.outputFileIds) {
      const file = context.getFile(fileId as FileId);
      if (!file) continue;
      const stub = context.getStub(fileId as FileId);

      // No localFilePath (a result with no on-disk origin) falls through to a
      // Save As dialog inside downloadFile, so nothing is silently dropped.
      const result = await downloadFile({
        data: file,
        filename: file.name,
        localPath: stub?.localFilePath,
        fileId,
      });

      if (result.savedPath) {
        context.markSaved(fileId as FileId, result.savedPath);
      }
    }
    return null;
  }

  const result = await downloadFromUrl(
    context.downloadUrl,
    context.downloadFilename || "download",
    context.downloadLocalPath || undefined,
  );

  if (context.outputFileIds && result.savedPath) {
    for (const fileId of context.outputFileIds) {
      context.markSaved(fileId as FileId, result.savedPath);
    }
  }

  return result;
}
