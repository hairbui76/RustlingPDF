import type { FileId, RustlingFileStub } from "@app/types/fileContext";
import {
  downloadFile,
  downloadFromUrl,
  DownloadResult,
} from "@app/services/downloadService";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";
import { saveMultipleFilesWithPrompt } from "@app/services/localFileSaveService";

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
    const outputs = context.outputFileIds
      .map((fileId) => ({
        fileId: fileId as FileId,
        file: context.getFile(fileId as FileId),
        localPath: context.getStub(fileId as FileId)?.localFilePath,
      }))
      .filter((entry): entry is typeof entry & { file: File } =>
        Boolean(entry.file),
      );

    // Outputs that own a file on disk are written straight back to it. That
    // ownership was established by planLocalFilePathCarry, which only grants a
    // path where exactly one output descends from exactly one input — so no
    // two entries here can target the same file.
    const inPlace = outputs.filter((entry) => entry.localPath);
    for (const entry of inPlace) {
      const result = await downloadFile({
        data: entry.file,
        filename: entry.file.name,
        localPath: entry.localPath,
        fileId: entry.fileId,
      });
      if (result.savedPath) {
        context.markSaved(entry.fileId, result.savedPath);
      }
    }

    // Everything else has no on-disk origin — the parts of a split, the
    // product of a merge. Asking for a destination once and writing them all
    // there beats a modal per file, which for a 40-page split is 40 dialogs.
    const needsDestination = outputs.filter((entry) => !entry.localPath);
    if (needsDestination.length === 1) {
      const only = needsDestination[0];
      const result = await downloadFile({
        data: only.file,
        filename: only.file.name,
        fileId: only.fileId,
      });
      if (result.savedPath) {
        context.markSaved(only.fileId, result.savedPath);
      }
    } else if (needsDestination.length > 1) {
      const result = await saveMultipleFilesWithPrompt(
        needsDestination.map((entry) => entry.file),
      );
      result.savedPaths.forEach((savedPath, index) => {
        if (savedPath) {
          context.markSaved(needsDestination[index].fileId, savedPath);
        }
      });
      if (!result.success && !result.cancelledByUser && result.error) {
        // Partial or total failure must not read as success: the files are
        // still dirty and the user needs to know which ones did not land.
        throw new Error(result.error);
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
