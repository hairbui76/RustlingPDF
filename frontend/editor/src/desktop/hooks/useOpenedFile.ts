import { useState, useEffect, useRef, useCallback } from "react";
import {
  fileOpenService,
  type OpenedFileBatch,
} from "@app/services/fileOpenService";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

export function useOpenedFile() {
  const [openedFileBatches, setOpenedFileBatches] = useState<OpenedFileBatch[]>(
    [],
  );
  const [loading, setLoading] = useState(true);
  const openedFileBatchesRef = useRef<OpenedFileBatch[]>([]);

  const clearOpenedFileBatches = useCallback(() => {
    openedFileBatchesRef.current = [];
    setOpenedFileBatches([]);
  }, []);

  const consumeOpenedFileBatches = useCallback(() => {
    const current = openedFileBatchesRef.current;
    openedFileBatchesRef.current = [];
    setOpenedFileBatches([]);
    return current;
  }, []);

  useEffect(() => {
    // Function to read and process file batches from storage
    const readFilesFromStorage = async () => {
      console.log("🔍 Reading file batches from storage...");
      try {
        const batches = await fileOpenService.popOpenedFileBatches();
        console.log(
          "🔍 fileOpenService.popOpenedFileBatches() returned:",
          batches,
        );

        if (batches.length > 0) {
          console.log(`✅ Found ${batches.length} batch(es) in storage`);
          openedFileBatchesRef.current = batches;
          setOpenedFileBatches(batches);
        }
      } catch (error) {
        console.error("❌ Failed to read files from storage:", error);
      } finally {
        setLoading(false);
      }
    };

    // Read files on mount
    readFilesFromStorage();

    // Listen for files-changed events scoped to THIS window only.
    // Rust emits via window.emit(...) / app.emit_to(label, ...) so each
    // Tauri window sees only its own queue updates.
    let unlisten: (() => void) | undefined;
    const currentWindow = getCurrentWebviewWindow();
    currentWindow
      .listen("files-changed", async () => {
        console.log(
          `📂 files-changed event received on window '${currentWindow.label}', re-reading storage...`,
        );
        await readFilesFromStorage();
      })
      .then((unlistenFn) => {
        unlisten = unlistenFn;
      });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  return {
    openedFileBatches,
    loading,
    clearOpenedFileBatches,
    consumeOpenedFileBatches,
  };
}
