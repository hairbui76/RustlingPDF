/**
 * Local folder hierarchy for files stored in IndexedDB.
 *
 * Folders and their memberships stay in the browser. Mutations update the
 * in-memory tree and IndexedDB together; there is no account or remote folder
 * service involved.
 */

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { useIndexedDB } from "@app/contexts/IndexedDBContext";
import { folderStorage } from "@app/services/folderStorage";
import {
  FolderBreadcrumbEntry,
  FolderId,
  FolderRecord,
  FolderTreeNode,
  ROOT_FOLDER_ID,
  createFolderId,
  pickFolderColor,
} from "@app/types/folder";

interface FolderContextValue {
  folders: FolderRecord[];
  foldersById: Map<FolderId, FolderRecord>;
  tree: FolderTreeNode[];
  loading: boolean;
  error: string | null;
  setError: (message: string | null) => void;
  currentFolderId: FolderId | null;
  setCurrentFolderId: (id: FolderId | null) => void;
  breadcrumbs: FolderBreadcrumbEntry[];
  refresh: () => Promise<void>;
  createFolder: (
    name: string,
    parentFolderId?: FolderId | null,
  ) => Promise<FolderRecord>;
  renameFolder: (id: FolderId, name: string) => Promise<FolderRecord | null>;
  moveFolder: (
    id: FolderId,
    newParentId: FolderId | null,
  ) => Promise<FolderRecord | null>;
  updateFolderAppearance: (
    id: FolderId,
    appearance: { color?: string; icon?: string | null },
  ) => Promise<FolderRecord | null>;
  deleteFolder: (id: FolderId) => Promise<FolderId[]>;
  getChildFolderIds: (parentId: FolderId | null) => FolderId[];
  isDescendant: (candidateId: FolderId, ancestorId: FolderId | null) => boolean;
}

const FolderContext = createContext<FolderContextValue | null>(null);

function buildTree(folders: FolderRecord[]): FolderTreeNode[] {
  const byParent = new Map<FolderId | null, FolderRecord[]>();
  for (const folder of folders) {
    const siblings = byParent.get(folder.parentFolderId) ?? [];
    siblings.push(folder);
    byParent.set(folder.parentFolderId, siblings);
  }
  for (const siblings of byParent.values()) {
    siblings.sort((a, b) =>
      a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
    );
  }

  const build = (
    parentId: FolderId | null,
    depth: number,
    visited: Set<FolderId>,
  ): FolderTreeNode[] => {
    if (depth >= 50) return [];
    return (byParent.get(parentId) ?? [])
      .filter((folder) => !visited.has(folder.id))
      .map((folder) => {
        const nextVisited = new Set(visited);
        nextVisited.add(folder.id);
        return {
          folder,
          depth,
          children: build(folder.id, depth + 1, nextVisited),
        };
      });
  };

  return build(ROOT_FOLDER_ID, 0, new Set());
}

function collectSubtreeIds(
  rootId: FolderId,
  folders: FolderRecord[],
): FolderId[] {
  const childrenByParent = new Map<FolderId, FolderId[]>();
  for (const folder of folders) {
    if (folder.parentFolderId === null) continue;
    const children = childrenByParent.get(folder.parentFolderId) ?? [];
    children.push(folder.id);
    childrenByParent.set(folder.parentFolderId, children);
  }

  const result = new Set<FolderId>([rootId]);
  const pending = [rootId];
  while (pending.length > 0 && result.size < 10_000) {
    const current = pending.pop()!;
    for (const child of childrenByParent.get(current) ?? []) {
      if (result.has(child)) continue;
      result.add(child);
      pending.push(child);
    }
  }
  return [...result];
}

export function FolderProvider({ children }: { children: React.ReactNode }) {
  const [folders, setFolders] = useState<FolderRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [currentFolderId, setCurrentFolderId] = useState<FolderId | null>(
    ROOT_FOLDER_ID,
  );
  const mountedRef = useRef(true);
  const { clearFolderForFiles } = useIndexedDB();

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const records = await folderStorage.getAllFolders();
      if (!mountedRef.current) return;
      setFolders(records);
      setError(null);
    } catch (cause) {
      console.error("[FolderContext] Failed to read local folders", cause);
      if (mountedRef.current) {
        setError(
          cause instanceof Error ? cause.message : "Failed to load folders",
        );
      }
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const foldersById = useMemo(
    () => new Map(folders.map((folder) => [folder.id, folder])),
    [folders],
  );
  const tree = useMemo(() => buildTree(folders), [folders]);

  const breadcrumbs = useMemo<FolderBreadcrumbEntry[]>(() => {
    const path: FolderBreadcrumbEntry[] = [
      { id: ROOT_FOLDER_ID, name: "All files" },
    ];
    const chain: FolderRecord[] = [];
    const seen = new Set<FolderId>();
    let cursor = currentFolderId;
    while (cursor !== null && !seen.has(cursor)) {
      seen.add(cursor);
      const folder = foldersById.get(cursor);
      if (!folder) break;
      chain.unshift(folder);
      cursor = folder.parentFolderId;
    }
    return path.concat(
      chain.map((folder) => ({ id: folder.id, name: folder.name })),
    );
  }, [currentFolderId, foldersById]);

  const createFolder = useCallback(
    async (name: string, parentFolderId: FolderId | null = currentFolderId) => {
      const now = Date.now();
      const record: FolderRecord = {
        id: createFolderId(),
        name,
        parentFolderId,
        color: pickFolderColor(name),
        createdAt: now,
        updatedAt: now,
      };
      await folderStorage.upsertFolder(record);
      setFolders((current) => [...current, record]);
      setError(null);
      return record;
    },
    [currentFolderId],
  );

  const updateFolder = useCallback(
    async (
      id: FolderId,
      changes: Partial<
        Pick<FolderRecord, "name" | "parentFolderId" | "color" | "icon">
      >,
    ) => {
      const existing = foldersById.get(id);
      if (!existing) return null;
      const record: FolderRecord = {
        ...existing,
        ...changes,
        updatedAt: Date.now(),
      };
      await folderStorage.upsertFolder(record);
      setFolders((current) =>
        current.map((folder) => (folder.id === id ? record : folder)),
      );
      setError(null);
      return record;
    },
    [foldersById],
  );

  const renameFolder = useCallback(
    (id: FolderId, name: string) => updateFolder(id, { name }),
    [updateFolder],
  );

  const moveFolder = useCallback(
    (id: FolderId, parentFolderId: FolderId | null) =>
      updateFolder(id, { parentFolderId }),
    [updateFolder],
  );

  const updateFolderAppearance = useCallback(
    (id: FolderId, appearance: { color?: string; icon?: string | null }) =>
      updateFolder(id, {
        color: appearance.color,
        icon: appearance.icon ?? undefined,
      }),
    [updateFolder],
  );

  const deleteFolder = useCallback(
    async (id: FolderId) => {
      const removed = collectSubtreeIds(id, folders);
      const removedSet = new Set(removed);
      await Promise.all([
        folderStorage.removeFolders(removed),
        clearFolderForFiles(removed),
      ]);
      setFolders((current) =>
        current.filter((folder) => !removedSet.has(folder.id)),
      );
      if (currentFolderId !== null && removedSet.has(currentFolderId)) {
        setCurrentFolderId(ROOT_FOLDER_ID);
      }
      setError(null);
      return removed;
    },
    [clearFolderForFiles, currentFolderId, folders],
  );

  const getChildFolderIds = useCallback(
    (parentId: FolderId | null) =>
      folders
        .filter((folder) => folder.parentFolderId === parentId)
        .map((folder) => folder.id),
    [folders],
  );

  const isDescendant = useCallback(
    (candidateId: FolderId, ancestorId: FolderId | null) => {
      if (ancestorId === null) return true;
      const seen = new Set<FolderId>();
      let cursor: FolderId | null = candidateId;
      while (cursor !== null && !seen.has(cursor)) {
        if (cursor === ancestorId) return true;
        seen.add(cursor);
        cursor = foldersById.get(cursor)?.parentFolderId ?? null;
      }
      return false;
    },
    [foldersById],
  );

  const value = useMemo<FolderContextValue>(
    () => ({
      folders,
      foldersById,
      tree,
      loading,
      error,
      setError,
      currentFolderId,
      setCurrentFolderId,
      breadcrumbs,
      refresh,
      createFolder,
      renameFolder,
      moveFolder,
      updateFolderAppearance,
      deleteFolder,
      getChildFolderIds,
      isDescendant,
    }),
    [
      folders,
      foldersById,
      tree,
      loading,
      error,
      currentFolderId,
      breadcrumbs,
      refresh,
      createFolder,
      renameFolder,
      moveFolder,
      updateFolderAppearance,
      deleteFolder,
      getChildFolderIds,
      isDescendant,
    ],
  );

  return (
    <FolderContext.Provider value={value}>{children}</FolderContext.Provider>
  );
}

export function useFolders(): FolderContextValue {
  const context = useContext(FolderContext);
  if (!context) {
    throw new Error("useFolders must be used within a FolderProvider");
  }
  return context;
}

export function useOptionalFolders(): FolderContextValue | null {
  return useContext(FolderContext);
}
