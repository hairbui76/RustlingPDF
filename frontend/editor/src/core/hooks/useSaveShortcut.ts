import { useEffect } from "react";
import { useFileActions, useFileState } from "@app/contexts/file/fileHooks";
import { downloadFile } from "@app/services/downloadService";
import { isDesktopRuntime } from "@app/services/desktop/desktopRuntime";

/**
 * Ctrl/Cmd+S saves the selected files (or all of them when nothing is
 * selected), matching the WorkbenchBar save button.
 *
 * Desktop only, deliberately. In a browser Ctrl+S belongs to the browser, and
 * hijacking it to fire a download per selected file would be a worse outcome
 * than the page-save the user asked for.
 *
 * Files with a `localFilePath` are overwritten in place; files without one get
 * a Save As dialog (inside `downloadFile`). Either way a successful save
 * records the path and clears `isDirty`, which is what stops a just-saved file
 * still being drawn as unsaved.
 */
export function useSaveShortcut(): void {
  const { selectors, state } = useFileState();
  const { actions } = useFileActions();
  const selectedFileIds = state.ui.selectedFileIds;

  useEffect(() => {
    if (!isDesktopRuntime()) {
      return;
    }

    const saveAll = async () => {
      // Iterate ids, not two parallel arrays: `getFiles` and
      // `getRustlingFileStubs` both drop misses, so a single file present in
      // one and absent from the other would shift every later pairing and save
      // one file's bytes over another file's path.
      const fileIds =
        selectedFileIds.length > 0
          ? selectedFileIds
          : selectors.getAllFileIds();

      for (const fileId of fileIds) {
        const file = selectors.getFile(fileId);
        const stub = selectors.getRustlingFileStub(fileId);
        if (!file || !stub) continue;

        try {
          const result = await downloadFile({
            data: file,
            filename: file.name,
            localPath: stub.localFilePath,
            fileId: stub.id,
          });
          if (result.savedPath) {
            actions.updateRustlingFileStub(stub.id, {
              localFilePath: stub.localFilePath ?? result.savedPath,
              isDirty: false,
            });
          }
        } catch (error) {
          console.error(`[Desktop] Failed to save ${file.name}:`, error);
        }
      }
    };

    // Key repeat, or an impatient second press while a native Save As dialog
    // is open, would otherwise stack dialogs and re-save the same files.
    let saving = false;

    const handleKeyDown = async (event: KeyboardEvent) => {
      // Case-insensitive because Caps Lock reports "S", but shift-free so
      // Ctrl+Shift+S stays available to anything that wants it.
      if (
        !(event.ctrlKey || event.metaKey) ||
        event.shiftKey ||
        event.key.toLowerCase() !== "s"
      ) {
        return;
      }
      event.preventDefault();
      if (saving) {
        return;
      }
      saving = true;
      try {
        await saveAll();
      } finally {
        saving = false;
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [selectors, selectedFileIds, actions]);
}
