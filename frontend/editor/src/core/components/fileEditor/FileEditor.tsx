import { useState, useCallback, useMemo, useEffect, useRef } from "react";
import { flushSync } from "react-dom";
import { Center, Box, LoadingOverlay } from "@mantine/core";
import { Dropzone } from "@mantine/dropzone";
import {
  useFileSelection,
  useFileState,
  useFileManagement,
  useFileActions,
} from "@app/contexts/FileContext";
import { useNavigationActions } from "@app/contexts/NavigationContext";
import { useViewer } from "@app/contexts/ViewerContext";
import { zipFileService } from "@app/services/zipFileService";
import { detectFileExtension } from "@app/utils/fileUtils";
import FileEditorThumbnail from "@app/components/fileEditor/FileEditorThumbnail";
import AddFileCard from "@app/components/fileEditor/AddFileCard";
import FilePickerModal from "@app/components/shared/FilePickerModal";
import { FileId, RustlingFile } from "@app/types/fileContext";
import { alert } from "@app/components/toast";
import { downloadFile } from "@app/services/downloadService";
import { useToolWorkflow } from "@app/contexts/ToolWorkflowContext";

interface FileEditorProps {
  onOpenPageEditor?: () => void;
  onMergeFiles?: (files: RustlingFile[]) => void;
  toolMode?: boolean;
  supportedExtensions?: string[];
}

const FileEditor = ({
  toolMode = false,
  supportedExtensions = ["pdf"],
}: FileEditorProps) => {
  // Utility function to check if a file extension is supported
  const isFileSupported = useCallback(
    (fileName: string): boolean => {
      const extension = detectFileExtension(fileName);
      return extension ? supportedExtensions.includes(extension) : false;
    },
    [supportedExtensions],
  );

  // Use optimized FileContext hooks
  const { state, selectors } = useFileState();
  const { addFiles, removeFiles, reorderFiles } = useFileManagement();
  const { actions: fileActions } = useFileActions();
  const { selectedFileIds, setSelectedFiles } = useFileSelection();

  // Extract needed values from state (memoized to prevent infinite loops)
  const activeRustlingFileStubs = useMemo(
    () => selectors.getRustlingFileStubs(),
    [state.files.byId, state.files.ids],
  );

  // Always-current refs so callbacks can read the latest stubs/selection without
  // closing over them as deps — prevents every callback from regenerating whenever
  // any stub changes (e.g. thumbnail load), which would bust React.memo on every thumbnail.
  const stubsRef = useRef(activeRustlingFileStubs);
  stubsRef.current = activeRustlingFileStubs;
  const selectedFileIdsRef = useRef(selectedFileIds);
  selectedFileIdsRef.current = selectedFileIds;

  // Get navigation actions
  const { actions: navActions } = useNavigationActions();

  // Get viewer context for setting active file index and ID
  const { setActiveFileIndex, setActiveFileId } = useViewer();

  const [_status, _setStatus] = useState<string | null>(null);
  const [_error, _setError] = useState<string | null>(null);

  // Toast helpers
  const showStatus = useCallback(
    (
      message: string,
      type: "neutral" | "success" | "warning" | "error" = "neutral",
    ) => {
      alert({
        alertType: type,
        title: message,
        expandable: false,
        durationMs: 4000,
      });
    },
    [],
  );
  const showError = useCallback((message: string) => {
    alert({
      alertType: "error",
      title: "Error",
      body: message,
      expandable: true,
    });
  }, []);

  // Current tool (for enforcing maxFiles limits)
  const { selectedTool } = useToolWorkflow();

  // Compute effective max allowed files based on the active tool and mode
  const maxAllowed = useMemo<number>(() => {
    const rawMax = selectedTool?.maxFiles;
    return !toolMode || rawMax == null || rawMax < 0 ? Infinity : rawMax;
  }, [selectedTool?.maxFiles, toolMode]);

  const [showFilePickerModal, setShowFilePickerModal] = useState(false);

  // Process uploaded files using context
  // ZIP extraction is now handled automatically in FileContext based on user preferences
  const handleFileUpload = useCallback(
    async (uploadedFiles: File[]) => {
      _setError(null);

      try {
        if (uploadedFiles.length > 0) {
          // FileContext will automatically handle ZIP extraction based on user preferences
          // - Respects autoUnzip setting
          // - Respects autoUnzipFileLimit
          // - HTML ZIPs stay intact
          // - Non-ZIP files pass through unchanged
          await addFiles(uploadedFiles, { selectFiles: true });
          // After auto-selection, enforce maxAllowed if needed
          if (Number.isFinite(maxAllowed)) {
            const nowSelectedIds = selectors
              .getSelectedRustlingFileStubs()
              .map((r) => r.id);
            if (nowSelectedIds.length > maxAllowed) {
              setSelectedFiles(nowSelectedIds.slice(-maxAllowed));
            }
          }
          showStatus(`Added ${uploadedFiles.length} file(s)`, "success");
        }
      } catch (err) {
        const errorMessage =
          err instanceof Error ? err.message : "Failed to process files";
        showError(errorMessage);
        console.error("File processing error:", err);
      }
    },
    [addFiles, showStatus, showError, selectors, maxAllowed, setSelectedFiles],
  );

  // Enforce maxAllowed when tool changes or when an external action sets too many selected files
  useEffect(() => {
    if (Number.isFinite(maxAllowed) && selectedFileIds.length > maxAllowed) {
      setSelectedFiles(selectedFileIds.slice(-maxAllowed));
    }
  }, [maxAllowed, selectedFileIds, setSelectedFiles]);

  // File reordering handler for drag and drop
  const handleReorderFiles = useCallback(
    (sourceFileId: FileId, targetFileId: FileId, selectedFileIds: FileId[]) => {
      const currentIds = stubsRef.current.map((r) => r.id);

      // Find indices
      const sourceIndex = currentIds.findIndex((id) => id === sourceFileId);
      const targetIndex = currentIds.findIndex((id) => id === targetFileId);

      if (sourceIndex === -1 || targetIndex === -1) {
        console.warn("Could not find source or target file for reordering");
        return;
      }

      // Handle multi-file selection reordering
      const filesToMove =
        selectedFileIds.length > 1
          ? selectedFileIds.filter((id) => currentIds.includes(id))
          : [sourceFileId];

      // Create new order
      const newOrder = [...currentIds];

      // Remove files to move from their current positions (in reverse order to maintain indices)
      const sourceIndices = filesToMove
        .map((id) => newOrder.findIndex((nId) => nId === id))
        .sort((a, b) => b - a); // Sort descending

      sourceIndices.forEach((index) => {
        newOrder.splice(index, 1);
      });

      // Calculate insertion index after removals
      let insertIndex = newOrder.findIndex((id) => id === targetFileId);
      if (insertIndex !== -1) {
        // Determine if moving forward or backward
        const isMovingForward = sourceIndex < targetIndex;
        if (isMovingForward) {
          // Moving forward: insert after target
          insertIndex += 1;
        } else {
          // Moving backward: insert before target (insertIndex already correct)
        }
      } else {
        // Target was moved, insert at end
        insertIndex = newOrder.length;
      }

      // Insert files at the calculated position
      newOrder.splice(insertIndex, 0, ...filesToMove);

      // Animate the reorder using the View Transitions API where available.
      // Each FileEditorThumbnail carries a stable `view-transition-name`, so
      // the browser snapshots each card before and after the DOM reorder and
      // interpolates the positions automatically. `flushSync` forces React to
      // apply the reorderFiles dispatch synchronously inside the transition
      // callback so the BEFORE/AFTER snapshots capture the correct frames.
      const applyReorder = () => reorderFiles(newOrder);
      const docWithViewTransition = document as Document & {
        startViewTransition?: (cb: () => void) => unknown;
      };
      if (typeof docWithViewTransition.startViewTransition === "function") {
        docWithViewTransition.startViewTransition(() => {
          flushSync(applyReorder);
        });
      } else {
        applyReorder();
      }

      // Update status
      const moveCount = filesToMove.length;
      showStatus(`${moveCount > 1 ? `${moveCount} files` : "File"} reordered`);
    },
    [reorderFiles, showStatus],
  );

  // File operations using context
  const handleCloseFile = useCallback(
    (fileId: FileId) => {
      const record = stubsRef.current.find((r) => r.id === fileId);
      const file = record ? selectors.getFile(record.id) : null;
      if (record && file) {
        removeFiles([record.id], false);
        setSelectedFiles(
          selectedFileIdsRef.current.filter((id) => id !== record.id),
        );
      }
    },
    [selectors, removeFiles, setSelectedFiles],
  );

  const handleDownloadFile = useCallback(
    async (fileId: FileId) => {
      const record = stubsRef.current.find((r) => r.id === fileId);
      const file = record ? selectors.getFile(record.id) : null;
      console.log("[FileEditor] handleDownloadFile called:", {
        fileId,
        hasRecord: !!record,
        hasFile: !!file,
        localFilePath: record?.localFilePath,
        isDirty: record?.isDirty,
      });
      if (record && file) {
        const result = await downloadFile({
          data: file,
          filename: file.name,
          localPath: record.localFilePath,
          fileId,
        });
        console.log("[FileEditor] Download complete, checking dirty state:", {
          localFilePath: record.localFilePath,
          isDirty: record.isDirty,
          savedPath: result.savedPath,
        });
        // Mark file as clean after successful save to disk
        if (result.savedPath) {
          console.log("[FileEditor] Marking file as clean:", fileId);
          fileActions.updateRustlingFileStub(fileId, {
            localFilePath: record.localFilePath ?? result.savedPath,
            isDirty: false,
          });
        } else {
          console.log("[FileEditor] Skipping clean mark:", {
            savedPath: result.savedPath,
            isDirty: record.isDirty,
          });
        }
      }
    },
    [selectors, fileActions],
  );

  const handleUnzipFile = useCallback(
    async (fileId: FileId) => {
      const record = stubsRef.current.find((r) => r.id === fileId);
      const file = record ? selectors.getFile(record.id) : null;
      if (record && file) {
        try {
          // Extract and store files using shared service method
          const result = await zipFileService.extractAndStoreFilesWithHistory(
            file,
            record,
          );

          if (result.success && result.extractedStubs.length > 0) {
            // Add extracted file stubs to FileContext
            await fileActions.addRustlingFileStubs(result.extractedStubs);

            // Remove the original ZIP file
            removeFiles([fileId], false);

            alert({
              alertType: "success",
              title: `Extracted ${result.extractedStubs.length} file(s) from ${file.name}`,
              expandable: false,
              durationMs: 3500,
            });
          } else {
            alert({
              alertType: "error",
              title: `Failed to extract files from ${file.name}`,
              body: result.errors.join("\n"),
              expandable: true,
              durationMs: 3500,
            });
          }
        } catch (error) {
          console.error("Failed to unzip file:", error);
          alert({
            alertType: "error",
            title: `Error unzipping ${file.name}`,
            expandable: false,
            durationMs: 3500,
          });
        }
      }
    },
    [selectors, fileActions, removeFiles],
  );

  // Anchor for Shift-ranges: the last card clicked without Shift. Kept in a ref
  // because it is only ever read inside the next click, never rendered.
  const selectionAnchorRef = useRef<FileId | null>(null);

  // Every selection made from the grid goes through here so the active tool's
  // `maxFiles` is honoured. Without it a Shift-range could hand a
  // single-file tool twelve documents; the effect below would then trim it,
  // but only after the selection had already been published.
  const applySelection = useCallback(
    (ids: FileId[]) => {
      setSelectedFiles(
        Number.isFinite(maxAllowed) && ids.length > maxAllowed
          ? ids.slice(-maxAllowed)
          : ids,
      );
    },
    [maxAllowed, setSelectedFiles],
  );

  const handleCardSelect = useCallback(
    (fileId: FileId, modifiers: { shift: boolean; toggle: boolean }) => {
      const ids = stubsRef.current.map((stub) => stub.id);
      const current = selectedFileIdsRef.current;

      if (modifiers.shift && selectionAnchorRef.current) {
        const from = ids.indexOf(selectionAnchorRef.current);
        const to = ids.indexOf(fileId);
        if (from !== -1 && to !== -1) {
          const [lo, hi] = from <= to ? [from, to] : [to, from];
          // The range replaces the selection rather than adding to it, so a
          // stray Shift-click cannot silently grow a selection a destructive
          // tool is about to run on.
          applySelection(ids.slice(lo, hi + 1));
          return;
        }
      }

      selectionAnchorRef.current = fileId;

      if (modifiers.toggle) {
        applySelection(
          current.includes(fileId)
            ? current.filter((id) => id !== fileId)
            : [...current, fileId],
        );
        return;
      }

      // A plain click on the only selected card clears it, so there is a way
      // back to "nothing selected" without reaching for the sidebar.
      applySelection(
        current.length === 1 && current[0] === fileId ? [] : [fileId],
      );
    },
    [applySelection],
  );

  const handleViewFile = useCallback(
    (fileId: FileId) => {
      const index = stubsRef.current.findIndex((r) => r.id === fileId);
      if (index !== -1) {
        setActiveFileId(fileId as string);
        setActiveFileIndex(index);
        navActions.setWorkbench("viewer");
      }
    },
    [setActiveFileId, setActiveFileIndex, navActions.setWorkbench],
  );

  const handleLoadFromStorage = useCallback(async (selectedFiles: File[]) => {
    if (selectedFiles.length === 0) return;

    try {
      // Use FileContext to handle loading stored files
      // The files are already in FileContext, just need to add them to active files
      showStatus(`Loaded ${selectedFiles.length} files from storage`);
    } catch (err) {
      console.error("Error loading files from storage:", err);
      showError("Failed to load some files from storage");
    }
  }, []);

  return (
    <Dropzone
      onDrop={handleFileUpload}
      multiple={true}
      maxSize={2 * 1024 * 1024 * 1024}
      style={{
        border: "none",
        borderRadius: 0,
        backgroundColor: "transparent",
      }}
      activateOnClick={false}
      activateOnDrag={true}
    >
      <Box pos="relative" style={{ overflow: "auto" }}>
        <LoadingOverlay visible={state.ui.isProcessing} />

        <Box p="md">
          {activeRustlingFileStubs.length === 0 ? (
            <Center h="60vh">
              <AddFileCard onFileSelect={handleFileUpload} />
            </Center>
          ) : (
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))",
                rowGap: "1.5rem",
                padding: "1rem",
                pointerEvents: "auto",
              }}
            >
              {/* Add File Card - only show when files exist */}
              {activeRustlingFileStubs.length > 0 && (
                <AddFileCard
                  key="add-file-card"
                  onFileSelect={handleFileUpload}
                />
              )}

              {activeRustlingFileStubs.map((record, index) => {
                return (
                  <FileEditorThumbnail
                    key={record.id}
                    file={record}
                    index={index}
                    totalFiles={activeRustlingFileStubs.length}
                    onCloseFile={handleCloseFile}
                    onViewFile={handleViewFile}
                    onReorderFiles={handleReorderFiles}
                    onDownloadFile={handleDownloadFile}
                    onUnzipFile={handleUnzipFile}
                    toolMode={toolMode}
                    isSupported={isFileSupported(record.name)}
                    isSelected={selectedFileIds.includes(record.id)}
                    onCardSelect={handleCardSelect}
                  />
                );
              })}
            </div>
          )}
        </Box>

        {/* File Picker Modal */}
        <FilePickerModal
          opened={showFilePickerModal}
          onClose={() => setShowFilePickerModal(false)}
          storedFiles={[]} // FileEditor doesn't have access to stored files, needs to be passed from parent
          onSelectFiles={handleLoadFromStorage}
        />
      </Box>
    </Dropzone>
  );
};

export default FileEditor;
