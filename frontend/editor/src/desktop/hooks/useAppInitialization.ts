import { useEffect, useState } from "react";
import { useOpenedFile } from "@app/hooks/useOpenedFile";
import {
  fileOpenService,
  type OpenedFileBatch,
} from "@app/services/fileOpenService";
import { navigateToToolIntent } from "@app/services/toolIntentService";
import { useFileManagement } from "@app/contexts/file/fileHooks";
import { createQuickKey } from "@app/types/fileContext";

/**
 * App initialization hook
 * Desktop version: Handles Tauri-specific file initialization
 * Requires FileContext - must be used inside FileContextProvider
 * - Handles files opened with the app (adds directly to FileContext)
 * - Routes Explorer context-menu launches (`--tool <action>`) to the mapped
 *   tool with the batch's files selected; plain/unknown intents just add files
 */
export function useAppInitialization(): void {
  // Get file management actions
  const { addFiles, updateStirlingFileStub } = useFileManagement();

  // Handle files opened with app (Tauri mode)
  const {
    openedFileBatches,
    loading: openedFileLoading,
    consumeOpenedFileBatches,
  } = useOpenedFile();

  // Load opened files and add directly to FileContext
  useEffect(() => {
    if (openedFileBatches.length === 0 || openedFileLoading) {
      return;
    }

    const loadOpenedBatch = async (batch: OpenedFileBatch) => {
      const loadedFiles = (
        await Promise.all(
          batch.paths.map(async (filePath) => {
            try {
              const fileData =
                await fileOpenService.readFileAsArrayBuffer(filePath);
              if (!fileData) return null;

              const file = new File([fileData.arrayBuffer], fileData.fileName, {
                type: "application/pdf",
              });

              console.log("[Desktop] Loaded file:", fileData.fileName);

              return {
                file,
                filePath,
                quickKey: createQuickKey(file),
              };
            } catch (error) {
              console.error("[Desktop] Failed to load file:", filePath, error);
              return null;
            }
          }),
        )
      ).filter(
        (entry): entry is { file: File; filePath: string; quickKey: string } =>
          Boolean(entry),
      );

      if (loadedFiles.length === 0) {
        return;
      }

      const filesArray = loadedFiles.map((entry) => entry.file);
      const quickKeyToPath = new Map(
        loadedFiles.map((entry) => [entry.quickKey, entry.filePath]),
      );

      const addedFiles = await addFiles(filesArray, { selectFiles: true });
      addedFiles.forEach((file) => {
        const localFilePath = quickKeyToPath.get(file.quickKey);
        if (localFilePath) {
          updateStirlingFileStub(file.fileId, { localFilePath });
        }
      });

      console.log(
        `[Desktop] ${loadedFiles.length} opened file(s) added to FileContext`,
      );

      // Context-menu launches carry a tool intent: after the batch's files
      // are in the workspace (and selected), open the mapped tool. Plain
      // opens (tool null / "open") and unknown intents skip navigation.
      if (navigateToToolIntent(batch.tool)) {
        console.log(`[Desktop] Routed launch intent '${batch.tool}' to tool`);
      }
    };

    const loadOpenedFiles = async () => {
      const batches = consumeOpenedFileBatches();
      try {
        // Sequential on purpose: batch order is launch order, and a later
        // intent's navigation must win over an earlier one's.
        for (const batch of batches) {
          await loadOpenedBatch(batch);
        }
      } catch (error) {
        console.error("[Desktop] Failed to load opened files:", error);
      }
    };

    loadOpenedFiles();
  }, [
    openedFileBatches,
    openedFileLoading,
    addFiles,
    updateStirlingFileStub,
    consumeOpenedFileBatches,
  ]);
}

export function useSetupCompletion(): (completed: boolean) => void {
  const [, setSetupComplete] = useState(false);

  return (completed: boolean) => {
    setSetupComplete(completed);
  };
}
