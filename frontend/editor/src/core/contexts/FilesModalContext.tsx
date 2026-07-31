import React, {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
} from "react";

import { useFileActions } from "@app/contexts/FileContext";
import { useFileContext } from "@app/contexts/file/fileHooks";
import {
  useNavigationActions,
  useNavigationState,
} from "@app/contexts/NavigationContext";
import { useFileHandler } from "@app/hooks/useFileHandler";
import { fileStorage } from "@app/services/fileStorage";
import type { RustlingFileStub } from "@app/types/fileContext";
import type { FileId } from "@app/types/file";

interface FilesModalContextType {
  isFilesModalOpen: boolean;
  openFilesModal: (options?: {
    insertAfterPage?: number;
    customHandler?: (files: File[], insertAfterPage?: number) => void;
    maxSelectable?: number | null;
  }) => void;
  closeFilesModal: () => void;
  maxSelectable: number | null;
  onFileUpload: (files: File[]) => void;
  onRecentFileSelect: (files: RustlingFileStub[]) => void;
  onModalClose?: () => void;
  setOnModalClose: (callback: () => void) => void;
}

const FilesModalContext = createContext<FilesModalContextType | null>(null);

export const FilesModalProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const { addFiles } = useFileHandler();
  const { actions } = useFileActions();
  const fileContext = useFileContext();
  const { actions: navigationActions } = useNavigationActions();
  const { workbench, selectedTool } = useNavigationState();
  const isMultiTool =
    workbench === "pageEditor" && selectedTool === "multiTool";

  const [isFilesModalOpen, setIsFilesModalOpen] = useState(false);
  const [onModalClose, setOnModalClose] = useState<(() => void) | undefined>();
  const [insertAfterPage, setInsertAfterPage] = useState<number | undefined>();
  const [customHandler, setCustomHandler] = useState<
    ((files: File[], insertAfterPage?: number) => void) | undefined
  >();
  const [maxSelectable, setMaxSelectable] = useState<number | null>(null);

  const openFilesModal = useCallback(
    (options?: {
      insertAfterPage?: number;
      customHandler?: (files: File[], insertAfterPage?: number) => void;
      maxSelectable?: number | null;
    }) => {
      setInsertAfterPage(options?.insertAfterPage);
      setCustomHandler(() => options?.customHandler);
      setMaxSelectable(options?.maxSelectable ?? null);
      setIsFilesModalOpen(true);
    },
    [],
  );

  const closeFilesModal = useCallback(() => {
    setIsFilesModalOpen(false);
    setInsertAfterPage(undefined);
    setCustomHandler(undefined);
    onModalClose?.();
  }, [onModalClose]);

  const handleFileUpload = useCallback(
    async (files: File[]) => {
      if (customHandler) {
        customHandler(files, insertAfterPage);
      } else {
        await addFiles(files);
        const ids = files
          .map((file) => fileContext.findFileId(file) as FileId | undefined)
          .filter((id): id is FileId => Boolean(id));
        if (ids.length > 0) {
          const selectedIds = fileContext.selectors
            .getSelectedRustlingFileStubs()
            .map((file) => file.id);
          actions.setSelectedFiles(
            Array.from(new Set([...selectedIds, ...ids])),
          );
        }
        if (!isMultiTool) {
          navigationActions.setWorkbench(
            files.length === 1 ? "viewer" : "fileEditor",
          );
        }
      }
      closeFilesModal();
    },
    [
      actions,
      addFiles,
      closeFilesModal,
      customHandler,
      fileContext,
      insertAfterPage,
      isMultiTool,
      navigationActions,
    ],
  );

  const handleRecentFileSelect = useCallback(
    async (stubs: RustlingFileStub[]) => {
      if (customHandler) {
        const files = (
          await Promise.all(
            stubs.map((stub) => fileStorage.getRustlingFile(stub.id)),
          )
        ).filter((file): file is NonNullable<typeof file> => Boolean(file));
        if (files.length > 0) customHandler(files, insertAfterPage);
        closeFilesModal();
        return;
      }

      if (!actions.addRustlingFileStubs) {
        console.error("addRustlingFileStubs action not available");
        closeFilesModal();
        return;
      }

      await actions.addRustlingFileStubs(stubs, { selectFiles: false });
      const selectedIds = fileContext.selectors
        .getSelectedRustlingFileStubs()
        .map((file) => file.id);
      actions.setSelectedFiles(
        Array.from(new Set([...selectedIds, ...stubs.map((file) => file.id)])),
      );

      if (!isMultiTool) {
        navigationActions.setWorkbench(
          stubs.length === 1 ? "viewer" : "fileEditor",
        );
      }
      closeFilesModal();
    },
    [
      actions,
      closeFilesModal,
      customHandler,
      fileContext.selectors,
      insertAfterPage,
      isMultiTool,
      navigationActions,
    ],
  );

  const setModalCloseCallback = useCallback((callback: () => void) => {
    setOnModalClose(() => callback);
  }, []);

  const contextValue = useMemo<FilesModalContextType>(
    () => ({
      isFilesModalOpen,
      openFilesModal,
      closeFilesModal,
      maxSelectable,
      onFileUpload: handleFileUpload,
      onRecentFileSelect: handleRecentFileSelect,
      onModalClose,
      setOnModalClose: setModalCloseCallback,
    }),
    [
      closeFilesModal,
      handleFileUpload,
      handleRecentFileSelect,
      isFilesModalOpen,
      maxSelectable,
      onModalClose,
      openFilesModal,
      setModalCloseCallback,
    ],
  );

  return (
    <FilesModalContext.Provider value={contextValue}>
      {children}
    </FilesModalContext.Provider>
  );
};

export const useFilesModalContext = (): FilesModalContextType => {
  const context = useContext(FilesModalContext);
  if (!context) {
    throw new Error(
      "useFilesModalContext must be used within a FilesModalProvider",
    );
  }
  return context;
};
