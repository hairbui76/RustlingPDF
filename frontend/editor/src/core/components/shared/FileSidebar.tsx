import React, {
  useState,
  useCallback,
  useMemo,
  useRef,
  useEffect,
  forwardRef,
} from "react";
import {
  Checkbox,
  Loader,
  Menu,
  Modal,
  TextInput,
  Tooltip,
} from "@mantine/core";
import { ActionIcon } from "@app/ui/ActionIcon";
import { Button } from "@app/ui/Button";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { useFileState, useFileActions } from "@app/contexts/file/fileHooks";
import { useAppConfig } from "@app/contexts/AppConfigContext";
import { useGoogleDrivePicker } from "@app/hooks/useGoogleDrivePicker";
import {
  useNavigationState,
  useNavigationActions,
  useNavigationGuard,
} from "@app/contexts/NavigationContext";
import { useViewer } from "@app/contexts/ViewerContext";
import { useFileHandler } from "@app/hooks/useFileHandler";
import {
  useIndexedDB,
  useIndexedDBRevision,
} from "@app/contexts/IndexedDBContext";
import { GoogleDriveIcon } from "@app/components/shared/CloudStorageIcons";
import ThemeModeControl from "@app/components/shared/ThemeModeControl";
import { useLogoAssets } from "@app/hooks/useLogoAssets";
import { useFileActionTerminology } from "@app/hooks/useFileActionTerminology";
import type { RustlingFileStub } from "@app/types/fileContext";
import { useFolders } from "@app/contexts/FolderContext";
import type { FolderId } from "@app/types/folder";
import type { FileId } from "@app/types/file";
import { FileItem } from "@app/components/shared/FileSidebarFileItem";
import { VersionHistoryModal } from "@app/components/filesPage/VersionHistoryModal";
import { useBulkAddProgress } from "@app/services/bulkAddProgress";
import "@app/components/shared/FileSidebar.css";
import { LocalIcon } from "@app/components/shared/LocalIcon";

const COLLAPSED_WIDTH = "3.5rem";
const EXPANDED_WIDTH = "16.25rem"; // ~260px

/** Only surface the "Adding files…" progress row for drops big enough that the
 *  pre-dispatch scan is user-visible; small adds finish before it would paint. */
const BULK_ADD_INDICATOR_MIN = 8;

export interface FileSidebarProps {
  collapsed?: boolean;
  onToggleCollapse?: () => void;
  onOpenSettings?: () => void;
  /** Accessible name override for the toggle button. */
  toggleAriaLabel?: string;
  /** Icon override for the toggle button (e.g. back-arrow on /files). */
  toggleIcon?: React.ReactNode;
  /** Override the Open-from-computer handler (e.g. upload to /files folder). */
  onUploadFiles?: (files: File[]) => void | Promise<void>;
  /** Override the Google Drive handler. */
  onPickGoogleDriveFiles?: (files: File[]) => void | Promise<void>;
  /** Override the Search row click (e.g. focus the /files search input). */
  onSearchClick?: () => void;
  /** Extra action row inserted under Open-from-computer (e.g. New folder). */
  extraAction?: {
    icon: React.ReactNode;
    label: string;
    onClick: () => void;
    disabled?: boolean;
    disabledTooltip?: string;
    testId?: string;
  };
}

/**
 * Slim "Adding files… X/Y" indicator during a bulk drop's pre-dispatch scan, so
 * a 200-file folder never looks frozen between drop and rows appearing. A
 * SEPARATE component so the per-file progress emissions re-render only this row
 * — not the whole sidebar (and its file list) hundreds of times per drop.
 */
function BulkAddProgressRow() {
  const { t } = useTranslation();
  const bulkAdd = useBulkAddProgress();
  if (bulkAdd.total < BULK_ADD_INDICATOR_MIN || bulkAdd.done >= bulkAdd.total) {
    return null;
  }
  return (
    <div className="file-sidebar-bulk-add" role="status" aria-live="polite">
      <div className="file-sidebar-bulk-add-label">
        <span>{t("fileSidebar.addingFiles", "Adding files…")}</span>
        <span className="file-sidebar-bulk-add-count">
          {bulkAdd.done}/{bulkAdd.total}
        </span>
      </div>
      <div className="file-sidebar-bulk-add-track">
        <div
          className="file-sidebar-bulk-add-bar"
          style={{
            width: `${Math.round((bulkAdd.done / bulkAdd.total) * 100)}%`,
          }}
        />
      </div>
    </div>
  );
}

const FileSidebar = forwardRef<HTMLDivElement, FileSidebarProps>(
  function FileSidebar(
    {
      collapsed = false,
      onToggleCollapse,
      onOpenSettings,
      toggleAriaLabel,
      toggleIcon,
      onUploadFiles,
      onPickGoogleDriveFiles,
      onSearchClick,
      extraAction,
    },
    ref,
  ) {
    const { t } = useTranslation();
    const logoAssets = useLogoAssets();
    // Same vocabulary source as the empty state's primary button. Hardcoding
    // "Open from computer" here left the web build calling one action two
    // names on the same screen — this row said "Open", the button said
    // "Upload". The hook says "Open"/"Save" on desktop, where files come from
    // and go back to the user's disk, and "Upload"/"Download" on the web.
    const terminology = useFileActionTerminology();
    const [searchActive, setSearchActive] = useState(false);
    const [searchQuery, setSearchQuery] = useState("");
    const searchInputRef = useRef<HTMLInputElement>(null);
    const nativeFileInputRef = useRef<HTMLInputElement>(null);
    // State (not ref) so setting it triggers a re-render - avoids racing addFiles state updates.
    const [pendingViewFileId, setPendingViewFileId] = useState<string | null>(
      null,
    );

    const navigate = useNavigate();
    const { config } = useAppConfig();
    const {
      isEnabled: isGoogleDriveEnabled,
      openPicker: openGoogleDrivePicker,
    } = useGoogleDrivePicker();
    const { state } = useFileState();
    const { actions: fileActions } = useFileActions();
    const { actions: navActions } = useNavigationActions();
    const { workbench: currentWorkbench, selectedTool } = useNavigationState();
    const isMultiTool =
      currentWorkbench === "pageEditor" && selectedTool === "multiTool";
    const { requestNavigation } = useNavigationGuard();
    const { activeFileId, setActiveFileId } = useViewer();
    const { addFiles } = useFileHandler();
    const indexedDB = useIndexedDB();

    const displayName = "RustlingPDF";

    // Leaf files = user-visible files (excludes intermediate tool outputs)
    const [allFileStubs, setAllFileStubs] = useState<RustlingFileStub[]>([]);
    const [stubsLoaded, setStubsLoaded] = useState(false);
    // Kebab "Version history" target; drives VersionHistoryModal.
    const [versionHistoryTarget, setVersionHistoryTarget] =
      useState<RustlingFileStub | null>(null);

    const refreshStubs = useCallback(async () => {
      // Leaf files from IDB - same source as the file selection modal.
      const stubs = await indexedDB.loadLeafMetadata();
      const idbIds = new Set(stubs.map((s) => s.id as string));

      // Also include workbench files not yet flushed to IDB.
      const pendingStubs = state.files.ids
        .map((id) => state.files.byId[id])
        .filter(
          (stub): stub is NonNullable<typeof stub> =>
            !!stub && stub.isLeaf !== false && !idbIds.has(stub.id as string),
        );

      const allStubs = [...stubs, ...pendingStubs];
      // A version swap briefly lists both the old leaf (IDB) and its replacement (workbench); two stubs for one lineage collide on the row key and corrupt React reconciliation, so drop any stub another names as its parent.
      const superseded = new Set(
        allStubs.map((s) => s.parentFileId as string | undefined),
      );
      const currentStubs = allStubs.filter(
        (s) => !superseded.has(s.id as string),
      );
      setAllFileStubs(
        currentStubs.sort(
          (a, b) => (b.lastModified ?? 0) - (a.lastModified ?? 0),
        ),
      );
      setStubsLoaded(true);
    }, [indexedDB, state.files.ids, state.files.byId]);

    // Refresh on mount, workbench changes, or external IndexedDB writes —
    // COALESCED. refreshStubs is a full IDB metadata scan, and it's re-created on
    // every workspace change: during a big folder drop that's hundreds of
    // triggers (per-file thumbnail hydrations and versioned deliveries),
    // which uncoalesced means O(files²) IDB reads and a re-render storm. A short
    // trailing throttle turns a burst into one scan per window; the first run
    // fires immediately so mount/load isn't delayed.
    const indexedDBRevision = useIndexedDBRevision();
    const lastRefreshAt = useRef(0);
    useEffect(() => {
      const REFRESH_COALESCE_MS = 300;
      const wait = Math.max(
        0,
        lastRefreshAt.current + REFRESH_COALESCE_MS - Date.now(),
      );
      const timer = window.setTimeout(() => {
        lastRefreshAt.current = Date.now();
        void refreshStubs();
      }, wait);
      return () => window.clearTimeout(timer);
    }, [refreshStubs, indexedDBRevision]);

    const handleSidebarDelete = useCallback(
      async (fileId: FileId) => {
        await fileActions.removeFiles([fileId], true);
        await refreshStubs();
      },
      [fileActions, refreshStubs],
    );

    // Kebab: open the version-history modal for this one file.
    const handleVersionHistory = useCallback(
      (fileId: FileId) => {
        const stub = allFileStubs.find((s) => s.id === fileId);
        if (stub) setVersionHistoryTarget(stub);
      },
      [allFileStubs],
    );

    // Once a pending file lands in state, open it in the viewer.
    useEffect(() => {
      if (!pendingViewFileId) return;
      const isInWorkbench = state.files.ids.some(
        (id) => (id as string) === pendingViewFileId,
      );
      if (isInWorkbench) {
        setPendingViewFileId(null);
        setActiveFileId(pendingViewFileId);
        navActions.setWorkbench("viewer");
      }
    }, [pendingViewFileId, state.files.ids, setActiveFileId, navActions]);

    // Memoized so unrelated state changes keep a stable array identity.
    const filteredFileStubs = useMemo(() => {
      const q = searchQuery.trim().toLowerCase();
      return q
        ? allFileStubs.filter((stub) => stub.name.toLowerCase().includes(q))
        : allFileStubs;
    }, [allFileStubs, searchQuery]);

    // Workbench membership as a Set for O(1) per-row lookups (see renderFileRow).
    const workbenchIds = useMemo(
      () => new Set(state.files.ids.map((id) => id as string)),
      [state.files.ids],
    );
    // How many rendered stubs share each lineage — >1 means split siblings, which
    // must key by their unique leaf id rather than the shared lineage (see renderFileRow).
    const lineageCounts = useMemo(() => {
      const counts = new Map<string, number>();
      for (const s of filteredFileStubs) {
        const k = (s.originalFileId ?? s.id) as string;
        counts.set(k, (counts.get(k) ?? 0) + 1);
      }
      return counts;
    }, [filteredFileStubs]);

    // Handle search activation
    const handleSearchClick = useCallback(() => {
      if (onSearchClick) {
        onSearchClick();
        return;
      }
      if (collapsed && onToggleCollapse) {
        onToggleCollapse();
      }
      setSearchActive(true);
    }, [collapsed, onToggleCollapse, onSearchClick]);

    const handleSearchClose = useCallback(() => {
      setSearchActive(false);
      setSearchQuery("");
    }, []);

    useEffect(() => {
      if (searchActive && searchInputRef.current) {
        searchInputRef.current.focus();
      }
    }, [searchActive]);

    // Handle Google Drive
    const handleGoogleDriveClick = useCallback(async () => {
      if (!isGoogleDriveEnabled) return;
      const files = await openGoogleDrivePicker({ multiple: true });
      if (files.length === 0) return;
      if (onPickGoogleDriveFiles) {
        await onPickGoogleDriveFiles(files);
        return;
      }
      await addFiles(files);
      if (!isMultiTool) {
        navActions.setWorkbench(files.length === 1 ? "viewer" : "fileEditor");
      }
    }, [
      isGoogleDriveEnabled,
      openGoogleDrivePicker,
      addFiles,
      navActions,
      isMultiTool,
      onPickGoogleDriveFiles,
    ]);

    // Toggle file in/out of workbench
    const handleFileClick = useCallback(
      async (fileId: FileId) => {
        const stub = allFileStubs.find((s) => s.id === fileId);
        if (!stub) return;

        const workbenchFileId = state.files.ids.find(
          (id) => (id as string) === (stub.id as string),
        );

        if (workbenchFileId) {
          // If this is the file currently open in the viewer, route through the
          // navigation guard so the save modal fires when there are unsaved changes.
          const isCurrentlyViewed = workbenchFileId === viewedWorkbenchId;
          if (isCurrentlyViewed) {
            requestNavigation(() => {
              void fileActions.removeFiles([workbenchFileId], false);
            });
            return;
          }
          await fileActions.removeFiles([workbenchFileId], false);
        } else {
          // Re-add by stub to preserve its ID - addFiles() would create a new UUID + IDB entry.
          const workbenchCount = state.files.ids.length;

          if (workbenchCount > 0 && currentWorkbench === "viewer") {
            navActions.setWorkbench("fileEditor");
          }

          await fileActions.addRustlingFileStubs([stub]);

          if (isMultiTool) {
            fileActions.setSelectedFiles([
              ...state.ui.selectedFileIds,
              stub.id,
            ]);
          } else {
            if (workbenchCount === 0) {
              navActions.setWorkbench("viewer");
            } else {
              navActions.setWorkbench("fileEditor");
            }
          }
        }
      },
      [
        allFileStubs,
        state.files.ids,
        state.ui.selectedFileIds,
        fileActions,
        navActions,
        currentWorkbench,
        activeFileId,
        requestNavigation,
        isMultiTool,
      ],
    );

    // Which file is currently open in the viewer - stable ID, never index-derived.
    const viewedWorkbenchId =
      currentWorkbench === "viewer" ? activeFileId : null;

    /**
     * The collection checkbox: put every file in this collection into the
     * workspace, or take them all back out.
     *
     * Batched rather than looping handleFileClick — that would re-enter the
     * add mutex and switch workbench once per file, which for a 53-file
     * collection is 53 renders and a visible stutter. One add call feeds the
     * existing bulk-add progress row instead.
     */
    const handleCollectionToggle = useCallback(
      async (stubs: RustlingFileStub[]) => {
        const missing = stubs.filter(
          (stub) => !workbenchIds.has(stub.id as string),
        );

        if (missing.length === 0) {
          const ids = stubs.map((stub) => stub.id as FileId);
          const remove = () => {
            void fileActions.removeFiles(ids, false);
          };
          // Removing the file the viewer is showing has to go through the
          // navigation guard, or unsaved changes are dropped silently.
          if (
            viewedWorkbenchId &&
            stubs.some((stub) => (stub.id as string) === viewedWorkbenchId)
          ) {
            requestNavigation(remove);
          } else {
            remove();
          }
          return;
        }

        const workbenchCount = state.files.ids.length;
        // Leave the viewer before mutating a non-empty workbench, same as the
        // single-file path does.
        if (workbenchCount > 0 && currentWorkbench === "viewer") {
          navActions.setWorkbench("fileEditor");
        }

        await fileActions.addRustlingFileStubs(missing);

        if (isMultiTool) {
          fileActions.setSelectedFiles([
            ...state.ui.selectedFileIds,
            ...missing.map((stub) => stub.id),
          ]);
        } else {
          // A collection always lands in the file editor: even a one-file
          // collection is a "these files" action, and the viewer shows one.
          navActions.setWorkbench(
            workbenchCount === 0 && missing.length === 1
              ? "viewer"
              : "fileEditor",
          );
        }
      },
      [
        workbenchIds,
        fileActions,
        navActions,
        state.files.ids,
        state.ui.selectedFileIds,
        currentWorkbench,
        isMultiTool,
        viewedWorkbenchId,
        requestNavigation,
      ],
    );

    const handleEyeClick = useCallback(
      async (fileId: FileId, _e: React.MouseEvent) => {
        const stub = allFileStubs.find((s) => s.id === fileId);
        if (!stub) return;

        const isCurrentlyViewed = !!(
          viewedWorkbenchId &&
          (viewedWorkbenchId as string) === (stub.id as string)
        );

        if (isCurrentlyViewed) {
          // Closing the currently-viewed file - guard against unsaved changes.
          navActions.setWorkbench("fileEditor");
          return;
        }

        // Switching to a different file while viewer is open - guard against unsaved changes.
        const performSwitch = async () => {
          const alreadyInWorkbench = state.files.ids.some(
            (id) => (id as string) === (stub.id as string),
          );

          if (!alreadyInWorkbench) {
            // Leave viewer before mutating workbench (prevents PSPDFKit crash).
            if (state.files.ids.length > 0 && currentWorkbench === "viewer") {
              navActions.setWorkbench("fileEditor");
            }
            await fileActions.addRustlingFileStubs([stub]);
          }

          // Route through pendingViewFileId so both setActiveFileIndex + setWorkbench fire together.
          setPendingViewFileId(stub.id as string);
        };

        if (currentWorkbench === "viewer" && viewedWorkbenchId) {
          requestNavigation(() => {
            void performSwitch();
          });
        } else {
          await performSwitch();
        }
      },
      [
        allFileStubs,
        viewedWorkbenchId,
        state.files.ids,
        fileActions,
        navActions,
        currentWorkbench,
        setPendingViewFileId,
        requestNavigation,
      ],
    );

    // Shared ingest path for both the native picker and drag-and-drop.
    // Per-tool validation happens downstream.
    const ingestFiles = useCallback(
      async (files: File[]) => {
        if (files.length === 0) return;
        if (onUploadFiles) {
          await onUploadFiles(files);
        } else {
          await addFiles(files);
          if (!isMultiTool) {
            navActions.setWorkbench(
              files.length === 1 ? "viewer" : "fileEditor",
            );
          }
        }
      },
      [addFiles, navActions, isMultiTool, onUploadFiles],
    );

    const handleNativeFilePick = useCallback(
      async (e: React.ChangeEvent<HTMLInputElement>) => {
        await ingestFiles(Array.from(e.target.files ?? []));
        e.target.value = "";
      },
      [ingestFiles],
    );

    // Native OS file drop onto the sidebar - mirrors the workbench drop zone.
    // Only react to OS file drags ("Files" type); internal element drags (e.g.
    // watched-folder file moves) set their own dataTransfer keys and must pass
    // through untouched.
    const [isFileDragOver, setIsFileDragOver] = useState(false);
    const dragDepth = useRef(0);

    const isNativeFileDrag = (e: React.DragEvent) =>
      Array.from(e.dataTransfer.types).includes("Files");

    const handleDragEnter = useCallback((e: React.DragEvent) => {
      if (!isNativeFileDrag(e)) return;
      e.preventDefault();
      dragDepth.current += 1;
      setIsFileDragOver(true);
    }, []);

    const handleDragOver = useCallback((e: React.DragEvent) => {
      if (!isNativeFileDrag(e)) return;
      // Required so the browser fires `drop` rather than opening the file.
      e.preventDefault();
      e.dataTransfer.dropEffect = "copy";
    }, []);

    const handleDragLeave = useCallback((e: React.DragEvent) => {
      if (!isNativeFileDrag(e)) return;
      // dragenter/leave fire per child element; the counter keeps the overlay
      // stable until the cursor genuinely leaves the sidebar.
      dragDepth.current -= 1;
      if (dragDepth.current <= 0) {
        dragDepth.current = 0;
        setIsFileDragOver(false);
      }
    }, []);

    const handleDrop = useCallback(
      async (e: React.DragEvent) => {
        if (!isNativeFileDrag(e)) return;
        e.preventDefault();
        dragDepth.current = 0;
        setIsFileDragOver(false);
        await ingestFiles(Array.from(e.dataTransfer.files ?? []));
      },
      [ingestFiles],
    );

    const shouldHideGoogleDrive =
      !isGoogleDriveEnabled && config?.hideDisabledToolsGoogleDrive;

    const width = collapsed ? COLLAPSED_WIDTH : EXPANDED_WIDTH;

    // ── Collections ────────────────────────────────────────────────────────
    // These are the same folders the My Files page manages, not a second
    // grouping concept: same store, same records, same ids. Surfacing them here
    // means a collection made in either place shows up in both.
    const { folders, createFolder } = useFolders();
    const { moveFilesToFolder } = useIndexedDB();
    const [closedCollections, setClosedCollections] = useState<Set<string>>(
      new Set(),
    );
    const toggleCollection = useCallback((id: string) => {
      setClosedCollections((prev) => {
        const next = new Set(prev);
        if (!next.delete(id)) next.add(id);
        return next;
      });
    }, []);

    const [creatingCollection, setCreatingCollection] = useState(false);
    const [newCollectionName, setNewCollectionName] = useState("");

    const submitNewCollection = useCallback(async () => {
      const name = newCollectionName.trim();
      if (!name) return;
      await createFolder(name, null);
      setNewCollectionName("");
      setCreatingCollection(false);
    }, [createFolder, newCollectionName]);

    const collectionOptions = useMemo(
      () => folders.map((f) => ({ id: f.id as string, name: f.name })),
      [folders],
    );

    const handleMoveToCollection = useCallback(
      async (fileId: FileId, collectionId: string | null) => {
        await moveFilesToFolder([fileId], collectionId as FolderId | null);
        await refreshStubs();
      },
      [moveFilesToFolder, refreshStubs],
    );

    // Only collections that currently hold a file get a header. An empty
    // collection in the sidebar is a row that does nothing; My Files is where
    // collections are managed, this is where they are used.
    const groupedFileStubs = useMemo(() => {
      const byCollection = new Map<string, RustlingFileStub[]>();
      const unfiled: RustlingFileStub[] = [];
      for (const stub of filteredFileStubs) {
        const id = (stub.folderId as string | null) ?? null;
        if (!id) {
          unfiled.push(stub);
          continue;
        }
        const bucket = byCollection.get(id);
        if (bucket) bucket.push(stub);
        else byCollection.set(id, [stub]);
      }
      const groups: {
        collection: { id: string; name: string } | null;
        stubs: RustlingFileStub[];
      }[] = [];
      for (const folder of folders) {
        const stubs = byCollection.get(folder.id as string);
        if (stubs?.length) {
          groups.push({
            collection: { id: folder.id as string, name: folder.name },
            stubs,
          });
        }
      }
      // A file whose collection was deleted elsewhere still has to appear.
      for (const [id, stubs] of byCollection) {
        if (!folders.some((f) => (f.id as string) === id))
          unfiled.push(...stubs);
      }
      if (unfiled.length) groups.push({ collection: null, stubs: unfiled });
      return groups;
    }, [filteredFileStubs, folders]);

    // Render one file row (shared by the flat list and the grouped legacy web build layout).
    const renderFileRow = (stub: RustlingFileStub) => {
      // O(1) membership instead of a per-row linear scan of the workbench ids.
      const isInWorkbench = workbenchIds.has(stub.id as string);
      const workbenchFileId = isInWorkbench ? (stub.id as FileId) : undefined;
      const isViewedInViewer = !!(
        viewedWorkbenchId && viewedWorkbenchId === (stub.id as string)
      );
      const isActive = isViewedInViewer;
      const isEncryptedFile = stub.processedFile?.isEncrypted === true;
      const thumbnailUrl = isEncryptedFile
        ? undefined
        : (workbenchFileId
            ? state.files.byId[workbenchFileId]?.thumbnailUrl
            : undefined) || stub.thumbnailUrl;
      // Key by lineage (originalFileId) so a version swap updates the row in place instead of
      // remounting. But a 1-input→many-output op (split) yields sibling leaves that share one
      // originalFileId; those would collide on the key, so fall back to the unique leaf id when a
      // lineage is present more than once.
      const lineageKey = (stub.originalFileId ?? stub.id) as string;
      const rowKey =
        (lineageCounts.get(lineageKey) ?? 0) > 1
          ? (stub.id as string)
          : lineageKey;
      return (
        <FileItem
          key={rowKey}
          fileId={stub.id}
          name={stub.name}
          size={stub.size}
          lastModified={stub.lastModified}
          isSelected={isInWorkbench}
          isActive={isActive}
          isViewedInViewer={isViewedInViewer}
          thumbnailUrl={thumbnailUrl}
          onClick={handleFileClick}
          onEyeClick={handleEyeClick}
          onDelete={handleSidebarDelete}
          onVersionHistory={handleVersionHistory}
          hasVersionHistory={(stub.versionNumber ?? 1) > 1}
          collections={collectionOptions}
          currentCollectionId={(stub.folderId as string | null) ?? null}
          onMoveToCollection={handleMoveToCollection}
        />
      );
    };

    return (
      <div
        ref={ref}
        className="file-sidebar"
        style={{ width, minWidth: width, maxWidth: width }}
        data-collapsed={collapsed}
        data-sidebar="file-sidebar"
        data-tour="quick-access-bar"
        data-file-drag-over={isFileDragOver || undefined}
        onDragEnter={handleDragEnter}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        {isFileDragOver && (
          <div className="file-sidebar-drop-overlay" aria-hidden="true">
            <LocalIcon
              icon="upload-file-rounded"
              className="file-sidebar-drop-overlay-icon"
            />
            {!collapsed && (
              <span className="file-sidebar-drop-overlay-text">
                {t("fileSidebar.dropToAdd", "Drop files to add")}
              </span>
            )}
          </div>
        )}
        <div className="file-sidebar-inner">
          {/* Header: hamburger + branding */}
          <Tooltip
            label={toggleAriaLabel ?? t("fileSidebar.expand", "Expand sidebar")}
            position="right"
            withinPortal
            disabled={!collapsed}
          >
            <div
              className="file-sidebar-header"
              onClick={() => onToggleCollapse?.()}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onToggleCollapse?.();
                }
              }}
              aria-label={
                toggleAriaLabel ??
                (collapsed
                  ? t("fileSidebar.expand", "Expand sidebar")
                  : t("fileSidebar.collapse", "Collapse sidebar"))
              }
            >
              {/* Wrapper carries sizing; data-toggle-flip-rtl flips icon in RTL. */}
              <span
                className="file-sidebar-menu-icon"
                data-toggle-flip-rtl={toggleIcon ? "true" : undefined}
              >
                {toggleIcon ?? <LocalIcon icon="menu-rounded" />}
              </span>
              {/* Expanded: the full horizontal lockup (mark + wordmark).
                  Collapsed: nothing but the toggle — the lockup is 3.53:1 and
                  cannot fit a 3.5rem rail, and the burger is already the
                  affordance there. */}
              {!collapsed && (
                <img
                  src={logoAssets.horizontalLockup}
                  alt={t("fileSidebar.brandLockupAlt", "RustlingPDF")}
                  className="file-sidebar-brand-lockup sidebar-content-fade"
                />
              )}
            </div>
          </Tooltip>

          {/* Search row */}
          <Tooltip
            label={t("fileSidebar.search", "Search")}
            position="right"
            withinPortal
            disabled={!collapsed}
          >
            <div
              className={`file-sidebar-search-row${searchActive && !collapsed ? " active" : ""}`}
              onClick={!searchActive ? handleSearchClick : undefined}
              role={!searchActive ? "button" : undefined}
              tabIndex={!searchActive ? 0 : undefined}
              onKeyDown={
                !searchActive
                  ? (e) => e.key === "Enter" && handleSearchClick()
                  : undefined
              }
            >
              {searchActive && !collapsed ? (
                <LocalIcon
                  icon="close-rounded"
                  className="file-sidebar-search-icon"
                  onClick={(e) => {
                    e.stopPropagation();
                    handleSearchClose();
                  }}
                />
              ) : (
                <LocalIcon
                  icon="search-rounded"
                  className="file-sidebar-search-icon"
                />
              )}
              {!collapsed &&
                (searchActive ? (
                  <input
                    ref={searchInputRef}
                    className="file-sidebar-search-input sidebar-content-fade"
                    value={searchQuery}
                    onChange={(e) => setSearchQuery(e.target.value)}
                    placeholder={t(
                      "fileSidebar.searchPlaceholder",
                      "Search files...",
                    )}
                    onClick={(e) => e.stopPropagation()}
                  />
                ) : (
                  <span className="file-sidebar-search-label sidebar-content-fade">
                    {t("fileSidebar.search", "Search")}
                  </span>
                ))}
            </div>
          </Tooltip>

          {/* Scrollable content */}
          <div className="file-sidebar-scroll">
            {/* Hidden native file input - kept outside the !collapsed gate so
                the "Open from computer" row below (always rendered) can fire
                it in either sidebar state without a silent no-op. */}
            <input
              ref={nativeFileInputRef}
              type="file"
              multiple
              // No `accept` filter - this picker feeds the global workspace,
              // not a specific tool, so users may legitimately upload PNGs,
              // ZIPs, etc. for the convert/merge/extract tools to handle.
              style={{ display: "none" }}
              onChange={handleNativeFilePick}
              data-testid="file-input"
            />
            {/* Open from Computer + My Files + Google Drive */}
            {/* Tooltips only fire when collapsed - when expanded the visible
                text label below already identifies each row, so a tooltip
                would just flash a duplicate. Distinct icons (UploadFile for
                "Open from computer" vs FolderOpen for "My Files") so the
                collapsed rail isn't two identical folder icons either. */}
            <Tooltip
              label={terminology.uploadFromComputer}
              position="right"
              withinPortal
              disabled={!collapsed}
            >
              <div
                className="file-sidebar-action-row"
                // `files-button` is the long-standing upload entry-point
                // testid: click + setInputFiles on `file-input` above. Tour
                // anchor lives here too - the tour now spotlights the native
                // picker shortcut rather than the old modal.
                data-testid="files-button"
                data-tour="files-button"
                onClick={() => {
                  // "Open from computer" goes straight to the native OS file
                  // picker. The full file manager (recent + drives + folders)
                  // is reachable via "My Files" below.
                  nativeFileInputRef.current?.click();
                }}
                role="button"
                tabIndex={0}
                aria-label={terminology.uploadFromComputer}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    nativeFileInputRef.current?.click();
                  }
                }}
              >
                <LocalIcon
                  icon="upload-file-rounded"
                  className="file-sidebar-action-icon"
                />
                {!collapsed && (
                  <span className="file-sidebar-action-label sidebar-content-fade">
                    {terminology.uploadFromComputer}
                  </span>
                )}
              </div>
            </Tooltip>

            {extraAction && (
              <Tooltip
                label={extraAction.disabledTooltip ?? extraAction.label}
                position="right"
                withinPortal
                // Only force a wide multiline box when the long disabled
                // reason is shown; the short label fits one line.
                multiline={Boolean(
                  extraAction.disabled && extraAction.disabledTooltip,
                )}
                w={
                  extraAction.disabled && extraAction.disabledTooltip
                    ? 220
                    : undefined
                }
                disabled={
                  !collapsed &&
                  !(extraAction.disabled && extraAction.disabledTooltip)
                }
              >
                <div
                  className={`file-sidebar-action-row${extraAction.disabled ? " disabled" : ""}`}
                  data-testid={extraAction.testId}
                  onClick={() => {
                    if (extraAction.disabled) return;
                    extraAction.onClick();
                  }}
                  role="button"
                  tabIndex={extraAction.disabled ? -1 : 0}
                  aria-disabled={extraAction.disabled}
                  aria-label={extraAction.label}
                  onKeyDown={(e) => {
                    if (extraAction.disabled) return;
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      extraAction.onClick();
                    }
                  }}
                >
                  <span className="file-sidebar-action-icon">
                    {extraAction.icon}
                  </span>
                  {!collapsed && (
                    <span className="file-sidebar-action-label sidebar-content-fade">
                      {extraAction.label}
                    </span>
                  )}
                </div>
              </Tooltip>
            )}

            <Tooltip
              label={t("fileSidebar.myFiles", "My Files")}
              position="right"
              withinPortal
              disabled={!collapsed}
            >
              <div
                className="file-sidebar-action-row"
                data-testid="my-files-button"
                onClick={() => {
                  if (collapsed && onToggleCollapse) onToggleCollapse();
                  navigate("/files");
                }}
                role="button"
                tabIndex={0}
                aria-label={t("fileSidebar.myFiles", "My Files")}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    navigate("/files");
                  }
                }}
              >
                <LocalIcon
                  icon="folder-open-rounded"
                  className="file-sidebar-action-icon"
                />
                {!collapsed && (
                  <span className="file-sidebar-action-label sidebar-content-fade">
                    {t("fileSidebar.myFiles", "My Files")}
                  </span>
                )}
              </div>
            </Tooltip>

            {!shouldHideGoogleDrive && (
              <Tooltip
                label={
                  !isGoogleDriveEnabled
                    ? t(
                        "fileSidebar.googleDriveDisabled",
                        "Google Drive is not configured",
                      )
                    : t("fileSidebar.googleDrive", "Open from Google Drive")
                }
                position="right"
                withinPortal
                disabled={!collapsed}
              >
                <div
                  className={`file-sidebar-cloud-row${!isGoogleDriveEnabled ? " disabled" : ""}`}
                  onClick={handleGoogleDriveClick}
                  role="button"
                  tabIndex={isGoogleDriveEnabled ? 0 : -1}
                  aria-disabled={!isGoogleDriveEnabled}
                  aria-label={
                    !isGoogleDriveEnabled
                      ? t(
                          "fileSidebar.googleDriveDisabled",
                          "Google Drive is not configured",
                        )
                      : t("fileSidebar.googleDrive", "Open from Google Drive")
                  }
                >
                  <div className="file-sidebar-cloud-icon-wrapper">
                    <GoogleDriveIcon
                      className="file-sidebar-cloud-icon-gray"
                      style={{ color: "var(--c-text-muted)" }}
                    />
                    {isGoogleDriveEnabled && (
                      <GoogleDriveIcon
                        colored
                        className="file-sidebar-cloud-icon-color"
                      />
                    )}
                  </div>
                  {!collapsed && (
                    <span className="file-sidebar-action-label sidebar-content-fade">
                      {t("fileSidebar.googleDrive", "Google Drive")}
                    </span>
                  )}
                </div>
              </Tooltip>
            )}

            {/* Files section - always visible when expanded */}
            {!collapsed && (
              <div className="file-sidebar-files-section sidebar-content-fade">
                <div className="file-sidebar-section-header">
                  <span className="file-sidebar-section-label">
                    {t("fileSidebar.files", "Files")}
                  </span>
                  <ActionIcon
                    variant="quiet"
                    className="file-sidebar-section-btn file-sidebar-section-btn-external"
                    onClick={() => navigate("/files")}
                    title={t(
                      "fileSidebar.openFileManager",
                      "Browse all files & folders",
                    )}
                    aria-label={t(
                      "fileSidebar.openFileManager",
                      "Browse all files & folders",
                    )}
                    data-testid="open-files-page"
                  >
                    <LocalIcon
                      icon="open-in-new-rounded"
                      width="1rem"
                      height="1rem"
                    />
                  </ActionIcon>
                  <Menu position="bottom-end" withinPortal shadow="md">
                    <Menu.Target>
                      <ActionIcon
                        variant="quiet"
                        className="file-sidebar-section-btn file-sidebar-section-btn-add"
                        title={t("fileSidebar.addFiles", "Add files")}
                        aria-label={t("fileSidebar.addFiles", "Add files")}
                      >
                        <LocalIcon
                          icon="add-rounded"
                          width="1rem"
                          height="1rem"
                        />
                      </ActionIcon>
                    </Menu.Target>
                    <Menu.Dropdown>
                      <Menu.Item
                        leftSection={
                          <LocalIcon
                            icon="add-rounded"
                            width={16}
                            height={16}
                          />
                        }
                        onClick={() => nativeFileInputRef.current?.click()}
                      >
                        {t("fileSidebar.addFiles", "Add files")}
                      </Menu.Item>
                      <Menu.Item
                        leftSection={
                          <LocalIcon
                            icon="create-new-folder-outline-rounded"
                            width={16}
                            height={16}
                          />
                        }
                        onClick={() => setCreatingCollection(true)}
                      >
                        {t("fileSidebar.newCollection", "New collection")}
                      </Menu.Item>
                    </Menu.Dropdown>
                  </Menu>
                </div>

                <BulkAddProgressRow />

                {!stubsLoaded ? (
                  <div className="file-sidebar-loading">
                    <Loader size="sm" color="var(--c-text-subtle)" />
                  </div>
                ) : filteredFileStubs.length > 0 ? (
                  <div className="file-sidebar-file-list">
                    {groupedFileStubs.map(({ collection, stubs }) => {
                      // Files with no collection render bare, exactly as the
                      // flat list always did — a collection header for "the
                      // ones you have not filed" would be noise for the many
                      // users who never make a collection at all.
                      if (!collection) return stubs.map(renderFileRow);
                      const isOpen = !closedCollections.has(collection.id);
                      const openedCount = stubs.filter((stub) =>
                        workbenchIds.has(stub.id as string),
                      ).length;
                      const allOpened = openedCount === stubs.length;
                      return (
                        <div
                          key={collection.id}
                          className="file-sidebar-collection"
                        >
                          {/* The checkbox sits beside the header button, not
                              inside it: a checkbox nested in a button is
                              invalid markup and the collapse toggle would
                              swallow its clicks. */}
                          <div className="file-sidebar-collection-header-row">
                            <Button
                              type="button"
                              variant="quiet"
                              hover={false}
                              className="file-sidebar-collection-header"
                              aria-expanded={isOpen}
                              onClick={() => toggleCollection(collection.id)}
                            >
                              <LocalIcon
                                icon="chevron-right-rounded"
                                width="1rem"
                                height="1rem"
                                className="file-sidebar-collection-chevron"
                                data-open={isOpen}
                              />
                              <LocalIcon
                                icon="folder-outline-rounded"
                                width="0.95rem"
                                height="0.95rem"
                              />
                              <span className="file-sidebar-collection-name">
                                {collection.name}
                              </span>
                              <span className="file-sidebar-collection-count">
                                {stubs.length}
                              </span>
                            </Button>
                            <Checkbox
                              size="xs"
                              className="file-sidebar-collection-checkbox"
                              checked={allOpened}
                              indeterminate={!allOpened && openedCount > 0}
                              onChange={() => {
                                void handleCollectionToggle(stubs);
                              }}
                              aria-label={
                                allOpened
                                  ? t("fileSidebar.deselectCollection", {
                                      defaultValue:
                                        "Remove all files in {{name}} from the workspace",
                                      name: collection.name,
                                    })
                                  : t("fileSidebar.selectCollection", {
                                      defaultValue:
                                        "Add all files in {{name}} to the workspace",
                                      name: collection.name,
                                    })
                              }
                            />
                          </div>
                          {isOpen && stubs.map(renderFileRow)}
                        </div>
                      );
                    })}
                  </div>
                ) : (
                  !searchActive && (
                    <div className="file-sidebar-empty">
                      <p className="file-sidebar-empty-text">
                        {t("fileSidebar.noFiles", "No files yet")}
                      </p>
                      <p className="file-sidebar-empty-hint">
                        {t("fileSidebar.dropHint", "Open files to get started")}
                      </p>
                    </div>
                  )
                )}
              </div>
            )}
          </div>
        </div>

        {/* Naming a new collection. A dialog rather than an inline row: the
            sidebar list is virtual-feeling and a text field wedged into it
            fights the scroll position the moment the list re-sorts. */}
        <Modal
          opened={creatingCollection}
          onClose={() => setCreatingCollection(false)}
          title={t("fileSidebar.newCollection", "New collection")}
          centered
          size="sm"
        >
          <TextInput
            data-autofocus
            value={newCollectionName}
            onChange={(e) => setNewCollectionName(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void submitNewCollection();
            }}
            placeholder={t("fileSidebar.collectionName", "Collection name")}
            aria-label={t("fileSidebar.collectionName", "Collection name")}
          />
          <div className="file-sidebar-collection-actions">
            <Button
              variant="secondary"
              onClick={() => setCreatingCollection(false)}
            >
              {t("common.cancel", "Cancel")}
            </Button>
            <Button
              disabled={!newCollectionName.trim()}
              onClick={() => void submitNewCollection()}
            >
              {t("common.create", "Create")}
            </Button>
          </div>
        </Modal>

        {/* Kebab "Version history" modal. */}
        <VersionHistoryModal
          opened={Boolean(versionHistoryTarget)}
          onClose={() => setVersionHistoryTarget(null)}
          file={versionHistoryTarget}
          onChanged={refreshStubs}
        />

        {/* Theme switcher — same preference as Settings → General, surfaced
            here so it can be found without opening Settings.

            Expanded, it sits beside the settings gear in the row below. It
            keeps a row of its own only in the 3.5rem collapsed rail, which has
            no room to put two icons side by side. */}
        {collapsed && (
          <div className="file-sidebar-theme-row" data-collapsed={collapsed}>
            <ThemeModeControl collapsed={collapsed} />
          </div>
        )}

        {/* Bottom bar: user name + settings */}
        <Tooltip
          label={
            onOpenSettings
              ? `${displayName} - ${t("fileSidebar.openSettings", "Open settings")}`
              : displayName
          }
          position="right"
          withinPortal
          disabled={!collapsed}
        >
          <div
            className="file-sidebar-bottom-bar"
            onClick={onOpenSettings}
            role={onOpenSettings ? "button" : undefined}
            tabIndex={onOpenSettings ? 0 : undefined}
            onKeyDown={
              onOpenSettings
                ? (e) => e.key === "Enter" && onOpenSettings()
                : undefined
            }
            data-testid={onOpenSettings ? "config-button" : undefined}
            data-tour={onOpenSettings ? "config-button" : undefined}
            aria-label={
              onOpenSettings
                ? t("fileSidebar.openSettings", "Open settings")
                : displayName
            }
            style={onOpenSettings ? { cursor: "pointer" } : undefined}
          >
            <div
              className="file-sidebar-bottom-avatar"
              aria-label={displayName}
            >
              {displayName.charAt(0).toUpperCase()}
            </div>
            {!collapsed && (
              <span className="file-sidebar-bottom-identity sidebar-content-fade">
                <span className="file-sidebar-bottom-name">{displayName}</span>
                {/* The installed version, right where a screenshot of a bug
                    report crops to. Settings has the long-form copy; this is
                    the at-a-glance answer to "which build am I on?". Absent
                    until the backend reports in — showing a placeholder would
                    just be a second unknown. */}
                {config?.appVersion && (
                  <span className="file-sidebar-bottom-version">
                    v{config.appVersion}
                  </span>
                )}
              </span>
            )}
            {!collapsed && (
              /* The whole row is the "open settings" target, so the theme
                 trigger inside it has to swallow its own click and keys —
                 otherwise picking a theme would also open Settings behind the
                 menu. `collapsed` is passed as true to get the icon-only form:
                 here it means "no room for a text label", not "the sidebar is
                 collapsed". */
              <span
                className="file-sidebar-bottom-theme"
                onClick={(e) => e.stopPropagation()}
                onKeyDown={(e) => e.stopPropagation()}
                role="presentation"
              >
                <ThemeModeControl collapsed />
              </span>
            )}
            {onOpenSettings && !collapsed && (
              <div className="file-sidebar-bottom-settings">
                <LocalIcon
                  icon="settings-rounded"
                  width="1.1rem"
                  height="1.1rem"
                />
              </div>
            )}
          </div>
        </Tooltip>
      </div>
    );
  },
);

export default FileSidebar;
