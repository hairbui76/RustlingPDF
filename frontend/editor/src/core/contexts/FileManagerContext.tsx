import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { useFileManagement } from "@app/contexts/FileContext";
import { downloadFiles } from "@app/utils/downloadUtils";
import { fileStorage } from "@app/services/fileStorage";
import { openFilesFromDisk } from "@app/services/openFilesFromDisk";
import { zipFileService } from "@app/services/zipFileService";
import type { FileId } from "@app/types/file";
import type { RustlingFileStub } from "@app/types/fileContext";

interface FileManagerContextValue {
  activeSource: "recent" | "local" | "drive";
  selectedFileIds: FileId[];
  searchTerm: string;
  selectedFiles: RustlingFileStub[];
  filteredFiles: RustlingFileStub[];
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  selectedFilesSet: Set<FileId>;
  expandedFileIds: Set<FileId>;
  loadedHistoryFiles: Map<FileId, RustlingFileStub[]>;
  isLoading: boolean;
  activeFileIds: FileId[];

  onSourceChange: (source: "recent" | "local" | "drive") => void;
  onLocalFileClick: () => void;
  onFileSelect: (
    file: RustlingFileStub,
    index: number,
    shiftKey?: boolean,
  ) => void;
  onFileRemove: (index: number) => void;
  onHistoryFileRemove: (file: RustlingFileStub) => void;
  onFileDoubleClick: (file: RustlingFileStub) => void;
  onOpenFiles: () => void;
  onSearchChange: (value: string) => void;
  onFileInputChange: (event: React.ChangeEvent<HTMLInputElement>) => void;
  onSelectAll: () => void;
  onDeleteSelected: () => void;
  onDownloadSelected: () => void;
  onDownloadSingle: (file: RustlingFileStub) => void;
  onToggleExpansion: (fileId: FileId) => void;
  onAddToRecents: (file: RustlingFileStub) => void;
  onUnzipFile: (file: RustlingFileStub) => Promise<void>;
  onNewFilesSelect: (files: File[]) => void;
  onGoogleDriveSelect: (files: File[]) => void;
  refreshRecentFiles: () => Promise<void>;

  recentFiles: RustlingFileStub[];
  isFileSupported: (fileName: string) => boolean;
  modalHeight: string;
}

const FileManagerContext = createContext<FileManagerContextValue | null>(null);

interface FileManagerProviderProps {
  children: React.ReactNode;
  recentFiles: RustlingFileStub[];
  onRecentFilesSelected: (files: RustlingFileStub[]) => void;
  onNewFilesSelect: (files: File[]) => void;
  onClose: () => void;
  isFileSupported: (fileName: string) => boolean;
  isOpen: boolean;
  onBulkRemove?: (fileIds: FileId[]) => void;
  modalHeight: string;
  refreshRecentFiles: () => Promise<void>;
  isLoading: boolean;
  activeFileIds: FileId[];
  maxSelectable?: number | null;
}

export const FileManagerProvider: React.FC<FileManagerProviderProps> = ({
  children,
  recentFiles,
  onRecentFilesSelected,
  onNewFilesSelect,
  onClose,
  isFileSupported,
  isOpen,
  onBulkRemove,
  modalHeight,
  refreshRecentFiles,
  isLoading,
  activeFileIds,
  maxSelectable = null,
}) => {
  const [activeSource, setActiveSource] = useState<
    "recent" | "local" | "drive"
  >("recent");
  const [selectedFileIds, setSelectedFileIds] = useState<FileId[]>(
    () => activeFileIds,
  );
  const [searchTerm, setSearchTerm] = useState("");
  const [lastClickedIndex, setLastClickedIndex] = useState<number | null>(null);
  const [expandedFileIds, setExpandedFileIds] = useState<Set<FileId>>(
    new Set(),
  );
  const [loadedHistoryFiles, setLoadedHistoryFiles] = useState<
    Map<FileId, RustlingFileStub[]>
  >(new Map());
  const fileInputRef = useRef<HTMLInputElement>(null);
  const { removeFiles } = useFileManagement();

  useEffect(() => {
    if (isOpen) {
      setSelectedFileIds(activeFileIds);
    }
  }, [isOpen, activeFileIds]);

  const selectedFilesSet = useMemo(
    () => new Set(selectedFileIds),
    [selectedFileIds],
  );
  const selectedFiles = useMemo(
    () => recentFiles.filter((file) => selectedFilesSet.has(file.id)),
    [recentFiles, selectedFilesSet],
  );
  const filteredFiles = useMemo(() => {
    const query = searchTerm.trim().toLowerCase();
    return query
      ? recentFiles.filter((file) => file.name.toLowerCase().includes(query))
      : recentFiles;
  }, [recentFiles, searchTerm]);

  const handleSourceChange = useCallback(
    (source: "recent" | "local" | "drive") => {
      setActiveSource(source);
      if (source !== "recent") {
        setSelectedFileIds([]);
        setSearchTerm("");
        setLastClickedIndex(null);
      }
    },
    [],
  );

  const handleLocalFileClick = useCallback(async () => {
    const files = await openFilesFromDisk({
      multiple: true,
      onFallbackOpen: () => fileInputRef.current?.click(),
    });
    if (files.length === 0) return;
    onNewFilesSelect(files);
    await refreshRecentFiles();
    onClose();
  }, [onNewFilesSelect, refreshRecentFiles, onClose]);

  const handleFileSelect = useCallback(
    (file: RustlingFileStub, currentIndex: number, shiftKey?: boolean) => {
      if (shiftKey && lastClickedIndex !== null) {
        const startIndex = Math.min(lastClickedIndex, currentIndex);
        const endIndex = Math.max(lastClickedIndex, currentIndex);
        setSelectedFileIds((previous) => {
          const next = new Set(previous);
          for (let index = startIndex; index <= endIndex; index += 1) {
            const id = filteredFiles[index]?.id;
            if (id) next.add(id);
          }
          const ids = Array.from(next);
          return maxSelectable == null ? ids : ids.slice(-maxSelectable);
        });
        return;
      }

      setSelectedFileIds((previous) => {
        const next = new Set(previous);
        if (next.has(file.id)) {
          next.delete(file.id);
        } else {
          next.add(file.id);
          if (maxSelectable != null && next.size > maxSelectable) {
            const oldest = next.values().next().value as FileId | undefined;
            if (oldest) next.delete(oldest);
          }
        }
        return Array.from(next);
      });
      setLastClickedIndex(currentIndex);
    },
    [filteredFiles, lastClickedIndex, maxSelectable],
  );

  const getSafeFilesToDelete = useCallback(
    (fileIds: FileId[], allFiles: RustlingFileStub[]): FileId[] => {
      const fileMap = new Map(allFiles.map((file) => [file.id, file]));
      const filesToDelete = new Set<FileId>();
      const filesToPreserve = new Set<FileId>();

      for (const leafId of fileIds) {
        const currentFile = fileMap.get(leafId);
        if (!currentFile) continue;
        filesToDelete.add(leafId);

        if ((currentFile.versionNumber ?? 1) > 1) {
          const originalId = currentFile.originalFileId || currentFile.id;
          allFiles
            .filter((file) => (file.originalFileId || file.id) === originalId)
            .forEach((file) => filesToDelete.add(file.id));
        }
      }

      for (const file of allFiles) {
        if (file.isLeaf === false || fileIds.includes(file.id)) continue;
        const originalId = file.originalFileId || file.id;
        allFiles
          .filter(
            (candidate) =>
              (candidate.originalFileId || candidate.id) === originalId,
          )
          .forEach((candidate) => filesToPreserve.add(candidate.id));
      }

      const safeIds = Array.from(filesToDelete).filter(
        (id) => !filesToPreserve.has(id),
      );
      const remainingFiles = allFiles.filter(
        (file) => !safeIds.includes(file.id),
      );

      for (const file of remainingFiles) {
        if (file.isLeaf !== false) continue;
        const originalId = file.originalFileId || file.id;
        const hasDescendant = remainingFiles.some(
          (candidate) =>
            candidate.parentFileId === file.id ||
            ((candidate.originalFileId || candidate.id) === originalId &&
              candidate.id !== file.id),
        );
        if (!hasDescendant) safeIds.push(file.id);
      }

      return safeIds;
    },
    [],
  );

  const removeLocalFiles = useCallback(
    async (ids: FileId[]) => {
      if (ids.length === 0) return;
      const allFiles = await fileStorage.getAllRustlingFileStubs();
      const safeIds = getSafeFilesToDelete(ids, allFiles);
      const safeIdSet = new Set(safeIds);

      setSelectedFileIds((previous) =>
        previous.filter((id) => !safeIdSet.has(id)),
      );
      setExpandedFileIds((previous) => {
        const next = new Set(previous);
        safeIds.forEach((id) => next.delete(id));
        return next;
      });
      setLoadedHistoryFiles((previous) => {
        const next = new Map(previous);
        safeIds.forEach((id) => next.delete(id));
        for (const [mainId, history] of next.entries()) {
          const remaining = history.filter((file) => !safeIdSet.has(file.id));
          if (remaining.length !== history.length) next.set(mainId, remaining);
        }
        return next;
      });
      onBulkRemove?.(safeIds);
      removeFiles(safeIds, false);

      try {
        await fileStorage.deleteMultipleRustlingFiles(safeIds);
      } finally {
        await refreshRecentFiles();
      }
    },
    [getSafeFilesToDelete, onBulkRemove, refreshRecentFiles, removeFiles],
  );

  const handleFileRemove = useCallback(
    (index: number) => {
      const file = filteredFiles[index];
      if (file) void removeLocalFiles([file.id]);
    },
    [filteredFiles, removeLocalFiles],
  );

  const handleHistoryFileRemove = useCallback(
    async (file: RustlingFileStub) => {
      setLoadedHistoryFiles((previous) => {
        const next = new Map(previous);
        next.delete(file.id);
        for (const [mainId, history] of next.entries()) {
          const remaining = history.filter((entry) => entry.id !== file.id);
          if (remaining.length !== history.length) next.set(mainId, remaining);
        }
        return next;
      });
      await fileStorage.deleteRustlingFile(file.id);
      await refreshRecentFiles();
    },
    [refreshRecentFiles],
  );

  const handleFileDoubleClick = useCallback(
    (file: RustlingFileStub) => {
      if (!isFileSupported(file.name)) return;
      onRecentFilesSelected([file]);
      onClose();
    },
    [isFileSupported, onRecentFilesSelected, onClose],
  );

  const handleOpenFiles = useCallback(() => {
    const uncheckedActiveIds = activeFileIds.filter((id) => {
      if (selectedFilesSet.has(id)) return false;
      const stub = filteredFiles.find((file) => file.id === id);
      return !stub?.isDirty;
    });
    if (uncheckedActiveIds.length > 0) {
      removeFiles(uncheckedActiveIds, false);
    }

    const newlySelected = selectedFiles.filter(
      (file) => !activeFileIds.includes(file.id),
    );
    if (newlySelected.length > 0) onRecentFilesSelected(newlySelected);
    onClose();
  }, [
    activeFileIds,
    filteredFiles,
    onClose,
    onRecentFilesSelected,
    removeFiles,
    selectedFiles,
    selectedFilesSet,
  ]);

  const handleFileInputChange = useCallback(
    async (event: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(event.target.files ?? []);
      event.target.value = "";
      if (files.length === 0) return;
      onNewFilesSelect(files);
      await refreshRecentFiles();
      onClose();
    },
    [onNewFilesSelect, refreshRecentFiles, onClose],
  );

  const handleSelectAll = useCallback(() => {
    const allSelected =
      filteredFiles.length > 0 &&
      filteredFiles.every((file) => selectedFilesSet.has(file.id));
    setSelectedFileIds(
      allSelected
        ? []
        : filteredFiles
            .map((file) => file.id)
            .slice(0, maxSelectable ?? undefined),
    );
    setLastClickedIndex(null);
  }, [filteredFiles, maxSelectable, selectedFilesSet]);

  const handleDownloadSelected = useCallback(async () => {
    if (selectedFiles.length === 0) return;
    await downloadFiles(selectedFiles, {
      zipFilename: `selected-files-${new Date()
        .toISOString()
        .slice(0, 19)
        .replace(/[:-]/g, "")}.zip`,
    });
  }, [selectedFiles]);

  const handleDownloadSingle = useCallback(async (file: RustlingFileStub) => {
    await downloadFiles([file]);
  }, []);

  const handleToggleExpansion = useCallback(
    async (fileId: FileId) => {
      const isExpanded = expandedFileIds.has(fileId);
      setExpandedFileIds((previous) => {
        const next = new Set(previous);
        if (isExpanded) next.delete(fileId);
        else next.add(fileId);
        return next;
      });

      if (isExpanded) {
        setLoadedHistoryFiles((previous) => {
          const next = new Map(previous);
          next.delete(fileId);
          return next;
        });
        return;
      }

      const currentFile = recentFiles.find((file) => file.id === fileId);
      if (!currentFile || (currentFile.versionNumber ?? 1) <= 1) return;

      const allFiles = await fileStorage.getAllRustlingFileStubs();
      const fileMap = new Map(allFiles.map((file) => [file.id, file]));
      const history: RustlingFileStub[] = [];
      let current = fileMap.get(fileId);
      while (current?.parentFileId) {
        const parent = fileMap.get(current.parentFileId);
        if (!parent) break;
        history.push(parent);
        current = parent;
      }
      history.sort(
        (left, right) => (left.versionNumber ?? 1) - (right.versionNumber ?? 1),
      );
      setLoadedHistoryFiles((previous) => {
        const next = new Map(previous);
        next.set(fileId, history);
        return next;
      });
    },
    [expandedFileIds, recentFiles],
  );

  const handleAddToRecents = useCallback(
    async (file: RustlingFileStub) => {
      await fileStorage.markFileAsLeaf(file.id);
      await refreshRecentFiles();
    },
    [refreshRecentFiles],
  );

  const handleGoogleDriveSelect = useCallback(
    async (files: File[]) => {
      if (files.length === 0) return;
      onNewFilesSelect(files);
      await refreshRecentFiles();
      onClose();
    },
    [onNewFilesSelect, refreshRecentFiles, onClose],
  );

  const handleUnzipFile = useCallback(
    async (file: RustlingFileStub) => {
      const storedFile = await fileStorage.getRustlingFile(file.id);
      if (!storedFile) return;
      const result = await zipFileService.extractAndStoreFilesWithHistory(
        storedFile,
        file,
      );
      if (result.errors.length > 0) {
        console.error("Errors during unzip:", result.errors);
      }
      if (result.success) await refreshRecentFiles();
    },
    [refreshRecentFiles],
  );

  useEffect(() => {
    if (isOpen) return;
    setActiveSource("recent");
    setSelectedFileIds([]);
    setSearchTerm("");
    setLastClickedIndex(null);
  }, [isOpen]);

  const contextValue = useMemo<FileManagerContextValue>(
    () => ({
      activeSource,
      selectedFileIds,
      searchTerm,
      selectedFiles,
      filteredFiles,
      fileInputRef,
      selectedFilesSet,
      expandedFileIds,
      loadedHistoryFiles,
      isLoading,
      activeFileIds,
      onSourceChange: handleSourceChange,
      onLocalFileClick: handleLocalFileClick,
      onFileSelect: handleFileSelect,
      onFileRemove: handleFileRemove,
      onHistoryFileRemove: handleHistoryFileRemove,
      onFileDoubleClick: handleFileDoubleClick,
      onOpenFiles: handleOpenFiles,
      onSearchChange: setSearchTerm,
      onFileInputChange: handleFileInputChange,
      onSelectAll: handleSelectAll,
      onDeleteSelected: () => void removeLocalFiles(selectedFileIds),
      onDownloadSelected: () => void handleDownloadSelected(),
      onDownloadSingle: (file) => void handleDownloadSingle(file),
      onToggleExpansion: (fileId) => void handleToggleExpansion(fileId),
      onAddToRecents: (file) => void handleAddToRecents(file),
      onUnzipFile: handleUnzipFile,
      onNewFilesSelect,
      onGoogleDriveSelect: handleGoogleDriveSelect,
      refreshRecentFiles,
      recentFiles,
      isFileSupported,
      modalHeight,
    }),
    [
      activeFileIds,
      activeSource,
      expandedFileIds,
      filteredFiles,
      handleAddToRecents,
      handleDownloadSelected,
      handleDownloadSingle,
      handleFileDoubleClick,
      handleFileInputChange,
      handleFileRemove,
      handleFileSelect,
      handleGoogleDriveSelect,
      handleHistoryFileRemove,
      handleLocalFileClick,
      handleOpenFiles,
      handleSelectAll,
      handleSourceChange,
      handleToggleExpansion,
      handleUnzipFile,
      isFileSupported,
      isLoading,
      loadedHistoryFiles,
      modalHeight,
      onNewFilesSelect,
      recentFiles,
      refreshRecentFiles,
      removeLocalFiles,
      searchTerm,
      selectedFileIds,
      selectedFiles,
      selectedFilesSet,
    ],
  );

  return (
    <FileManagerContext.Provider value={contextValue}>
      {children}
    </FileManagerContext.Provider>
  );
};

export const useFileManagerContext = (): FileManagerContextValue => {
  const context = useContext(FileManagerContext);
  if (!context) {
    throw new Error(
      "useFileManagerContext must be used within a FileManagerProvider",
    );
  }
  return context;
};
