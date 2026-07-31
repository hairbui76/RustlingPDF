/** Shared state for the My Files view. */

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { FileId } from "@app/types/file";
import { RustlingFileStub } from "@app/types/fileContext";
import { FolderId, FolderRecord, ROOT_FOLDER_ID } from "@app/types/folder";
import { fileStorage } from "@app/services/fileStorage";
import {
  useIndexedDB,
  useIndexedDBRevision,
} from "@app/contexts/IndexedDBContext";
import { useFileActions } from "@app/contexts/file/fileHooks";
import { useFolders } from "@app/contexts/FolderContext";

/** View-toggle modes; tuple keeps the union and iterator in sync. */
export const FILES_PAGE_VIEW_MODES = ["grid", "list"] as const;
export type FilesPageViewMode = (typeof FILES_PAGE_VIEW_MODES)[number];
export type FilesPageSortMode =
  | "name-asc"
  | "name-desc"
  | "modified-desc"
  | "modified-asc"
  | "size-desc"
  | "size-asc";
export type FilesPageTab = "all" | "recent";

export interface FolderNameDialogState {
  mode: "new" | "rename" | null;
  parentId?: FolderId | null;
  folder?: FolderRecord;
}

export interface MoveDialogState {
  open: boolean;
  fileIds?: FileId[];
  folderId?: FolderId;
  initial: FolderId | null;
}

interface FilesPageContextValue {
  // Cached files (leaf-only)
  allFiles: RustlingFileStub[];
  fileMap: Map<FileId, RustlingFileStub>;
  fileCountsByFolder: Map<FolderId | null, number>;
  loading: boolean;
  refresh: () => Promise<void>;

  // Selection
  selectedFileIds: Set<FileId>;
  setSelectedFileIds: React.Dispatch<React.SetStateAction<Set<FileId>>>;
  clearSelection: () => void;

  // View + sort + search + filters
  viewMode: FilesPageViewMode;
  setViewMode: (mode: FilesPageViewMode) => void;
  sortMode: FilesPageSortMode;
  setSortMode: (mode: FilesPageSortMode) => void;
  search: string;
  setSearch: (value: string) => void;
  /** Selected file extensions (uppercased, e.g. ["PDF", "DOCX"]).
   *  Empty array = no type filter applied. */
  typeFilter: string[];
  setTypeFilter: (next: string[]) => void;

  /** Active filter-tab. Drives which files appear and which UI affordances enable. */
  currentTab: FilesPageTab;
  setCurrentTab: (tab: FilesPageTab) => void;

  // Dialog state
  folderNameDialog: FolderNameDialogState;
  openNewFolderDialog: (parentId?: FolderId | null) => void;
  openRenameFolderDialog: (folder: FolderRecord) => void;
  closeFolderNameDialog: () => void;
  submitFolderName: (name: string) => Promise<void>;

  moveDialog: MoveDialogState;
  promptMoveFiles: (fileIds: FileId[]) => void;
  closeMoveDialog: () => void;

  // Action helpers
  moveFilesTo: (fileIds: FileId[], folderId: FolderId | null) => Promise<void>;
  moveFolderTo: (
    folderId: FolderId,
    newParentId: FolderId | null,
  ) => Promise<void>;
  /** Queue files for deletion - opens the DeleteFilesDialog. */
  removeFiles: (fileIds: FileId[]) => Promise<void>;
  /** Files currently queued in the delete dialog (empty when closed). */
  deleteDialogFileIds: FileId[];
  deleteDialogOpen: boolean;
  closeDeleteDialog: () => void;
  /** Confirm deletion from local storage. */
  confirmRemoveFiles: () => Promise<void>;
  /** Open the confirmation dialog; consumer renders DeleteFolderDialog. */
  promptDeleteFolder: (folder: FolderRecord) => void;
  /** Confirmed delete; pass deleteContents=true to also remove files inside. */
  deleteFolder: (
    folder: FolderRecord,
    deleteContents: boolean,
  ) => Promise<void>;
  deleteFolderDialog: {
    folder: FolderRecord | null;
    fileCount: number;
  };
  closeDeleteFolderDialog: () => void;
  setFolderAppearance: (
    folderId: FolderId,
    appearance: { color?: string; icon?: string | null },
  ) => Promise<void>;
}

const FilesPageContext = createContext<FilesPageContextValue | null>(null);

export function FilesPageProvider({ children }: { children: React.ReactNode }) {
  const { t } = useTranslation();
  const indexedDB = useIndexedDB();
  const indexedDBRevision = useIndexedDBRevision();
  const folders = useFolders();
  const { actions: fileActions } = useFileActions();

  const [allFiles, setAllFiles] = useState<RustlingFileStub[]>([]);
  const [loading, setLoading] = useState(true);
  // Generation counter to drop stale reads when a second refresh
  // overlaps the first.
  const refreshGenRef = useRef(0);

  // Narrow dep so refresh isn't recreated on every folders field change.
  const setFoldersError = folders.setError;
  const refresh = useCallback(async () => {
    const gen = ++refreshGenRef.current;
    setLoading(true);
    try {
      const localStubs = await fileStorage.getAllRustlingFileStubs();
      // Bail if a newer refresh started while IDB was reading.
      if (gen !== refreshGenRef.current) return;
      const localLeaf = localStubs.filter((s) => s.isLeaf !== false);
      setAllFiles(localLeaf);
    } catch (err) {
      if (gen !== refreshGenRef.current) return;
      console.error("[FilesPageContext] refresh failed", err);
      setFoldersError(
        err instanceof Error ? err.message : "Failed to load files",
      );
    } finally {
      // Only the latest refresh should clear the loading state.
      if (gen === refreshGenRef.current) setLoading(false);
    }
  }, [setFoldersError]);

  useEffect(() => {
    void refresh();
  }, [refresh, indexedDBRevision]);

  const fileMap = useMemo(() => {
    const map = new Map<FileId, RustlingFileStub>();
    for (const f of allFiles) map.set(f.id, f);
    return map;
  }, [allFiles]);

  const fileCountsByFolder = useMemo(() => {
    const map = new Map<FolderId | null, number>();
    map.set(ROOT_FOLDER_ID, 0);
    for (const f of folders.folders) map.set(f.id, 0);
    for (const file of allFiles) {
      const fid = file.folderId ?? null;
      map.set(fid, (map.get(fid) ?? 0) + 1);
    }
    return map;
  }, [allFiles, folders.folders]);

  // Selection ---------------------------------------------------------------
  const [selectedFileIds, setSelectedFileIds] = useState<Set<FileId>>(
    () => new Set(),
  );
  const clearSelection = useCallback(() => setSelectedFileIds(new Set()), []);

  // Clear selection when folder changes.
  useEffect(() => {
    clearSelection();
  }, [folders.currentFolderId, clearSelection]);

  // View + sort + search + filters ----------------------------------------
  const [viewMode, setViewMode] = useState<FilesPageViewMode>("grid");
  const [sortMode, setSortMode] = useState<FilesPageSortMode>("modified-desc");
  const [search, setSearch] = useState("");
  const [typeFilter, setTypeFilter] = useState<string[]>([]);
  const [currentTab, setCurrentTab] = useState<FilesPageTab>("all");

  // Dialog: folder name -----------------------------------------------------
  const [folderNameDialog, setFolderNameDialog] =
    useState<FolderNameDialogState>({ mode: null });

  const openNewFolderDialog = useCallback(
    (parentId: FolderId | null = folders.currentFolderId) => {
      setFolderNameDialog({ mode: "new", parentId });
    },
    [folders.currentFolderId],
  );

  const openRenameFolderDialog = useCallback((folder: FolderRecord) => {
    setFolderNameDialog({ mode: "rename", folder });
  }, []);

  const closeFolderNameDialog = useCallback(() => {
    setFolderNameDialog({ mode: null });
  }, []);

  const submitFolderName = useCallback(
    async (name: string) => {
      if (folderNameDialog.mode === "new") {
        await folders.createFolder(
          name,
          folderNameDialog.parentId ?? folders.currentFolderId,
        );
      } else if (
        folderNameDialog.mode === "rename" &&
        folderNameDialog.folder
      ) {
        await folders.renameFolder(folderNameDialog.folder.id, name);
      }
    },
    [folderNameDialog, folders],
  );

  // Dialog: move ------------------------------------------------------------
  const [moveDialog, setMoveDialog] = useState<MoveDialogState>({
    open: false,
    initial: ROOT_FOLDER_ID,
  });

  const promptMoveFiles = useCallback(
    (fileIds: FileId[]) => {
      setMoveDialog({ open: true, fileIds, initial: folders.currentFolderId });
    },
    [folders.currentFolderId],
  );

  const closeMoveDialog = useCallback(() => {
    setMoveDialog((m) => ({ ...m, open: false }));
  }, []);

  // Action helpers ----------------------------------------------------------

  const moveFilesTo = useCallback(
    async (fileIds: FileId[], folderId: FolderId | null) => {
      if (fileIds.length === 0) return;
      await indexedDB.moveFilesToFolder(fileIds, folderId);
      await refresh();
    },
    [indexedDB, refresh],
  );

  const moveFolderTo = useCallback(
    async (folderId: FolderId, newParentId: FolderId | null) => {
      // Client-side cycle guard.
      if (newParentId !== null && folders.isDescendant(newParentId, folderId)) {
        folders.setError(
          t(
            "filesPage.cycleBlocked",
            "Can't move a folder into one of its own subfolders.",
          ),
        );
        return;
      }
      await folders.moveFolder(folderId, newParentId);
    },
    [folders, t],
  );

  const [deleteDialogFileIds, setDeleteDialogFileIds] = useState<FileId[]>([]);
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);

  const performDelete = useCallback(
    async (fileIds: FileId[]) => {
      const stubs = fileIds
        .map((id) => fileMap.get(id))
        .filter((s): s is RustlingFileStub => Boolean(s));
      const localIds = stubs.map((stub) => stub.id);
      if (localIds.length > 0) {
        await fileActions.removeFiles(localIds, true);
      }

      const removedIds = new Set(fileIds);
      setSelectedFileIds((prev) => {
        const next = new Set(prev);
        for (const id of removedIds) next.delete(id);
        return next;
      });
      await refresh();
    },
    [fileMap, fileActions, refresh],
  );

  const removeFiles = useCallback(async (fileIds: FileId[]) => {
    if (fileIds.length === 0) return;
    setDeleteDialogFileIds(fileIds);
    setDeleteDialogOpen(true);
  }, []);

  const closeDeleteDialog = useCallback(() => {
    setDeleteDialogOpen(false);
    setDeleteDialogFileIds([]);
  }, []);

  const confirmRemoveFiles = useCallback(async () => {
    await performDelete(deleteDialogFileIds);
    setDeleteDialogOpen(false);
    setDeleteDialogFileIds([]);
  }, [deleteDialogFileIds, performDelete]);

  const setFolderAppearance = useCallback(
    async (
      folderId: FolderId,
      appearance: { color?: string; icon?: string | null },
    ) => {
      await folders.updateFolderAppearance(folderId, appearance);
    },
    [folders],
  );

  const [deleteFolderDialog, setDeleteFolderDialog] = useState<{
    folder: FolderRecord | null;
    fileCount: number;
  }>({ folder: null, fileCount: 0 });
  const closeDeleteFolderDialog = useCallback(
    () => setDeleteFolderDialog({ folder: null, fileCount: 0 }),
    [],
  );

  const filesInSubtree = useCallback(
    (folderId: FolderId): FileId[] => {
      const subtreeIds = new Set<FolderId>([folderId]);
      const stack: FolderId[] = [folderId];
      while (stack.length > 0) {
        const cur = stack.pop()!;
        for (const childId of folders.getChildFolderIds(cur)) {
          if (subtreeIds.has(childId)) continue;
          subtreeIds.add(childId);
          stack.push(childId);
        }
      }
      return allFiles
        .filter((f) => {
          const fid = f.folderId ?? null;
          return fid !== null && subtreeIds.has(fid);
        })
        .map((f) => f.id);
    },
    [allFiles, folders],
  );

  const promptDeleteFolder = useCallback(
    (folder: FolderRecord) => {
      const fileCount = filesInSubtree(folder.id).length;
      setDeleteFolderDialog({ folder, fileCount });
    },
    [filesInSubtree],
  );

  const deleteFolder = useCallback(
    async (folder: FolderRecord, deleteContents: boolean) => {
      if (deleteContents) {
        const fileIds = filesInSubtree(folder.id);
        if (fileIds.length > 0) {
          await fileActions.removeFiles(fileIds, true);
        }
      }
      await folders.deleteFolder(folder.id);
      await refresh();
    },
    [fileActions, filesInSubtree, folders, refresh],
  );

  // Memoise to avoid re-rendering every FileCard on unrelated state churn.
  const value = useMemo<FilesPageContextValue>(
    () => ({
      allFiles,
      fileMap,
      fileCountsByFolder,
      loading,
      refresh,
      selectedFileIds,
      setSelectedFileIds,
      clearSelection,
      viewMode,
      setViewMode,
      sortMode,
      setSortMode,
      search,
      setSearch,
      typeFilter,
      setTypeFilter,
      currentTab,
      setCurrentTab,
      folderNameDialog,
      openNewFolderDialog,
      openRenameFolderDialog,
      closeFolderNameDialog,
      submitFolderName,
      moveDialog,
      promptMoveFiles,
      closeMoveDialog,
      moveFilesTo,
      moveFolderTo,
      removeFiles,
      deleteDialogFileIds,
      deleteDialogOpen,
      closeDeleteDialog,
      confirmRemoveFiles,
      promptDeleteFolder,
      deleteFolder,
      deleteFolderDialog,
      closeDeleteFolderDialog,
      setFolderAppearance,
    }),
    [
      allFiles,
      fileMap,
      fileCountsByFolder,
      loading,
      refresh,
      selectedFileIds,
      clearSelection,
      viewMode,
      sortMode,
      search,
      typeFilter,
      currentTab,
      folderNameDialog,
      openNewFolderDialog,
      openRenameFolderDialog,
      closeFolderNameDialog,
      submitFolderName,
      moveDialog,
      promptMoveFiles,
      closeMoveDialog,
      moveFilesTo,
      moveFolderTo,
      removeFiles,
      deleteDialogFileIds,
      deleteDialogOpen,
      closeDeleteDialog,
      confirmRemoveFiles,
      promptDeleteFolder,
      deleteFolder,
      deleteFolderDialog,
      closeDeleteFolderDialog,
      setFolderAppearance,
    ],
  );

  return (
    <FilesPageContext.Provider value={value}>
      {children}
    </FilesPageContext.Provider>
  );
}

export function useFilesPage(): FilesPageContextValue {
  const ctx = useContext(FilesPageContext);
  if (!ctx) {
    throw new Error("useFilesPage must be used within a FilesPageProvider");
  }
  return ctx;
}
