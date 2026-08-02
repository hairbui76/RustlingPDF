import { useEffect } from "react";
import { useOpenedFile } from "@app/hooks/useOpenedFile";
import { useSaveShortcut } from "@app/hooks/useSaveShortcut";
import {
  readOpenedFile,
  type OpenedFileBatch,
} from "@app/services/fileOpenService";
import { navigateToToolIntent } from "@app/services/toolIntentService";
import { pendingFilePathMappings } from "@app/services/pendingFilePathMappings";
import { useFileManagement } from "@app/contexts/file/fileHooks";
import { createQuickKey } from "@app/types/fileContext";
import { backendHealthMonitor } from "@app/services/backendHealthMonitor";

/**
 * App-level initialization that needs FileContext. Mounted once by
 * `AppProviders`, inside `FileContextProvider`.
 *
 * Web: everything below no-ops (see `isDesktopRuntime`), so this costs one
 * effect that immediately returns.
 *
 * Desktop:
 * - loads files the OS handed the app into the workspace, with their
 *   on-disk path attached so they can be saved back in place; and
 * - routes an Explorer context-menu launch (`--tool <action>`) to the mapped
 *   tool with the batch's files already selected.
 */
export function useAppInitialization(): void {
  const { addFiles } = useFileManagement();
  const { openedFileBatches, loading, consumeOpenedFileBatches } =
    useOpenedFile();

  useSaveShortcut();

  // Hold one health subscription for the whole session. Each poll re-reads the
  // sidecar's port from Rust, so this is also what keeps the API base URL
  // live: without it the monitor would only run while a tool panel happens to
  // be mounted, and a sidecar that restarted on a different ephemeral port
  // outside that window would leave every request pointed at a dead port.
  // No-ops on web — the monitor never polls there.
  useEffect(() => backendHealthMonitor.subscribe(() => {}), []);

  useEffect(() => {
    if (loading || openedFileBatches.length === 0) {
      return;
    }

    const loadBatch = async (batch: OpenedFileBatch) => {
      const loaded = (
        await Promise.all(
          batch.paths.map(async (filePath) => {
            const fileData = await readOpenedFile(filePath);
            if (!fileData) {
              // readOpenedFile already logged; one unreadable path must not
              // take the rest of the launch down with it.
              return null;
            }
            const file = new File([fileData.arrayBuffer], fileData.fileName, {
              type: "application/pdf",
            });
            return { file, filePath };
          }),
        )
      ).filter((entry): entry is { file: File; filePath: string } =>
        Boolean(entry),
      );

      if (loaded.length === 0) {
        return;
      }

      // Publish the path→file mapping before adding: `addFiles` attaches
      // `localFilePath` from this map as it builds each stub, which is the
      // single mechanism that makes a file "from disk" (and therefore
      // saveable in place) — the same one the native open dialog uses.
      for (const { file, filePath } of loaded) {
        pendingFilePathMappings.set(createQuickKey(file), filePath);
      }

      await addFiles(
        loaded.map((entry) => entry.file),
        { selectFiles: true },
      );

      // Context-menu launches carry a tool intent: once the batch's files are
      // in the workspace and selected, open the mapped tool. Plain opens
      // (tool null / "open") and unknown intents skip navigation.
      navigateToToolIntent(batch.tool);
    };

    const loadBatches = async () => {
      const batches = consumeOpenedFileBatches();
      try {
        // Sequential on purpose: batch order is launch order, and a later
        // intent's navigation must win over an earlier one's.
        for (const batch of batches) {
          await loadBatch(batch);
        }
      } catch (error) {
        console.error("[Desktop] Failed to load opened files:", error);
      }
    };

    void loadBatches();
  }, [openedFileBatches, loading, addFiles, consumeOpenedFileBatches]);
}
