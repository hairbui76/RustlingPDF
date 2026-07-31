import { useCallback, useState } from "react";

import { useIndexedDB } from "@app/contexts/IndexedDBContext";
import { fileStorage } from "@app/services/fileStorage";
import type { RustlingFileStub } from "@app/types/fileContext";

export const useFileManager = () => {
  const [loading, setLoading] = useState(false);
  const indexedDB = useIndexedDB();

  const loadRecentFiles = useCallback(async (): Promise<RustlingFileStub[]> => {
    setLoading(true);
    try {
      if (!indexedDB) return [];
      const files = await fileStorage.getLeafRustlingFileStubs();
      return files.sort(
        (left, right) => (right.lastModified ?? 0) - (left.lastModified ?? 0),
      );
    } catch (error) {
      console.error("Failed to load recent files:", error);
      return [];
    } finally {
      setLoading(false);
    }
  }, [indexedDB]);

  return { loading, loadRecentFiles };
};
