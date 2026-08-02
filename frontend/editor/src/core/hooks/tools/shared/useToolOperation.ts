import { useCallback, useRef, useEffect, useContext } from "react";
import apiClient from "@app/services/apiClient";
import { useTranslation } from "react-i18next";
import { useFileContext } from "@app/contexts/FileContext";
import { useNavigationActions } from "@app/contexts/NavigationContext";
import { ViewerContext } from "@app/contexts/ViewerContext";
import { useToolState } from "@app/hooks/tools/shared/useToolState";
import {
  useToolApiCalls,
  type ApiCallsConfig,
} from "@app/hooks/tools/shared/useToolApiCalls";
import { useToolResources } from "@app/hooks/tools/shared/useToolResources";
import {
  extractErrorMessage,
  handle422Error,
} from "@app/utils/toolErrorHandler";
import {
  RustlingFile,
  extractFiles,
  FileId,
  RustlingFileStub,
} from "@app/types/fileContext";
import { FILE_EVENTS } from "@app/services/errorUtils";
import { zipFileService } from "@app/services/zipFileService";
import { getFilenameWithoutExtension } from "@app/utils/fileUtils";
import {
  createChildStub,
  generateProcessedFileMetadata,
} from "@app/contexts/file/fileActions";
import { createNewRustlingFileStub } from "@app/types/fileContext";
import { ToolOperation } from "@app/types/file";
import { ensureBackendReady } from "@app/services/backendReadinessGuard";
import { notifyPdfProcessingComplete } from "@app/services/desktopNotificationService";
import {
  buildInputTracking,
  buildOutputPairs,
} from "@app/hooks/tools/shared/toolOperationHelpers";
import { planLocalFilePathCarry } from "@app/hooks/tools/shared/localFilePathCarry";
import {
  ToolType,
  defineSingleFileTool,
  defineMultiFileTool,
  defineCustomTool,
  ToolOperationConfig,
  ToolOperationHook,
  CustomProcessorResult,
  SingleFileToolOperationConfig,
  MultiFileToolOperationConfig,
  CustomToolOperationConfig,
  ProcessingProgress,
  ResponseHandler,
} from "@app/hooks/tools/shared/toolOperationTypes";

export {
  ToolType,
  defineSingleFileTool,
  defineMultiFileTool,
  defineCustomTool,
};
export type {
  ToolOperationConfig,
  ToolOperationHook,
  CustomProcessorResult,
  SingleFileToolOperationConfig,
  MultiFileToolOperationConfig,
  CustomToolOperationConfig,
  ProcessingProgress,
  ResponseHandler,
};

// Re-export for backwards compatibility
export { createStandardErrorHandler } from "@app/utils/toolErrorHandler";

/**
 * Shared hook for tool operations providing consistent error handling, progress tracking,
 * and FileContext integration. Eliminates boilerplate while maintaining flexibility.
 *
 * Supports three tool patterns, selected by the config's toolType:
 * 1. Single-file tools (ToolType.singleFile): processes files individually
 * 2. Multi-file tools (ToolType.multiFile): single API call with all files
 * 3. Complex tools (ToolType.custom): customProcessor takes full control
 *
 * @param config - Tool operation configuration
 * @returns Hook interface with state and execution methods
 */
export const useToolOperation = <TParams>(
  config: ToolOperationConfig<TParams>,
): ToolOperationHook<TParams> => {
  const { t } = useTranslation();
  const { addFiles, consumeFiles, undoConsumeFiles, selectors } =
    useFileContext();
  const { actions: navActions } = useNavigationActions();
  const viewerContext = useContext(ViewerContext);
  const setActiveFileId = viewerContext?.setActiveFileId ?? (() => {});

  // Composed hooks
  const { state, actions } = useToolState();
  const { actions: fileActions } = useFileContext();
  const { processFiles, cancelOperation: cancelApiCalls } =
    useToolApiCalls<TParams>();
  const {
    generateThumbnails,
    createDownloadInfo,
    cleanupBlobUrls,
    extractZipFiles,
  } = useToolResources();

  // Track last operation for undo functionality
  const lastOperationRef = useRef<{
    inputFiles: File[];
    inputRustlingFileStubs: RustlingFileStub[];
    outputFileIds: FileId[];
  } | null>(null);

  const executeOperation = useCallback(
    async (params: TParams, selectedFiles: RustlingFile[]): Promise<void> => {
      // Validation
      if (selectedFiles.length === 0) {
        actions.setError(t("noFileSelected", "No file loaded"));
        return;
      }

      // Handle zero-byte inputs explicitly: mark as error and continue with others
      const zeroByteFiles = selectedFiles.filter((file) => file.size === 0);
      if (zeroByteFiles.length > 0) {
        try {
          for (const f of zeroByteFiles) {
            fileActions.markFileError(f.fileId);
          }
        } catch (e) {
          console.log("markFileError", e);
        }
      }
      const validFiles: RustlingFile[] = selectedFiles.filter(
        (file) => file.size > 0,
      );
      if (validFiles.length === 0) {
        actions.setError(t("noValidFiles", "No valid files to process"));
        return;
      }

      // Block encrypted files from being sent to backend tools
      const encryptedFiles = validFiles.filter((f) => {
        const stub = selectors.getRustlingFileStub(f.fileId);
        return stub?.processedFile?.isEncrypted === true;
      });
      if (encryptedFiles.length > 0) {
        for (const ef of encryptedFiles) {
          fileActions.openEncryptedUnlockPrompt(ef.fileId);
        }
        actions.setError(
          t(
            "encryptedFilesBlocked",
            "{{count}} files are password-protected. Unlock them first.",
            {
              count: encryptedFiles.length,
            },
          ),
        );
        return;
      }

      // Resolve the runtime endpoint from params (static string or function result).
      // Custom processors may omit endpoint entirely — result is undefined in that case.
      const runtimeEndpoint: string | undefined = config.endpoint
        ? typeof config.endpoint === "function"
          ? (config.endpoint(params) ?? undefined)
          : config.endpoint
        : undefined;

      // Check the backend before starting a server-backed operation.
      // Custom processors without an endpoint skip this — they manage their own backend calls.
      const endpointForReadyCheck =
        config.toolType !== ToolType.custom ? runtimeEndpoint : undefined;
      const backendReady = await ensureBackendReady(endpointForReadyCheck);
      if (!backendReady) {
        actions.setError(
          t(
            "backendHealth.offline",
            "Embedded backend is offline. Please try again shortly.",
          ),
        );
        return;
      }

      // Reset state
      actions.setLoading(true);
      actions.setError(null);
      actions.resetResults();
      cleanupBlobUrls();

      // Prepare files with history metadata injection (for PDFs)
      actions.setStatus("Processing files...");

      // Listen for global error file id events from HTTP interceptor during this run
      let externalErrorFileIds: string[] = [];
      const errorListener = (e: Event) => {
        const detail = (e as CustomEvent)?.detail as any;
        if (detail?.fileIds) {
          externalErrorFileIds = Array.isArray(detail.fileIds)
            ? detail.fileIds
            : [];
        }
      };
      window.addEventListener(
        FILE_EVENTS.markError,
        errorListener as EventListener,
      );

      try {
        let processedFiles: File[];
        let successSourceIds: FileId[] = [];
        // Which input each output came from, one entry per output, `null`
        // where the pairing is genuinely unknown. Only used to decide whether
        // an output may overwrite a file on the user's disk, so "unknown" must
        // stay unknown rather than being filled in by index.
        let outputSourceIds: (FileId | null)[] = [];

        // Use original files directly (no PDF metadata injection - history stored in IndexedDB)
        const filesForAPI = extractFiles(validFiles);

        switch (config.toolType) {
          case ToolType.singleFile: {
            // Individual file processing - separate API call per file
            const apiCallsConfig: ApiCallsConfig<TParams> = {
              endpoint: config.endpoint,
              buildFormData: config.buildFormData,
              filePrefix: config.filePrefix,
              responseHandler: config.responseHandler,
              preserveBackendFilename: config.preserveBackendFilename,
            };
            console.debug("[useToolOperation] Multi-file start", {
              count: filesForAPI.length,
            });
            const result = await processFiles(
              params,
              validFiles,
              apiCallsConfig,
              actions.setProgress,
              actions.setStatus,
              fileActions.markFileError,
            );
            processedFiles = result.outputFiles;
            successSourceIds = result.successSourceIds;
            // Exact: each output was produced inside its own input's loop
            // iteration, so this is recorded provenance, not inference.
            outputSourceIds = result.outputSourceIds;
            console.debug("[useToolOperation] Multi-file results", {
              outputFiles: processedFiles.length,
              successSources: result.successSourceIds.length,
            });
            break;
          }
          case ToolType.multiFile: {
            // Multi-file processing - single API call with all files
            actions.setStatus("Processing files...");
            const formData = config.buildFormData(params, filesForAPI);
            const endpoint =
              typeof config.endpoint === "function"
                ? config.endpoint(params)
                : config.endpoint;
            if (!endpoint) {
              throw new Error(
                "This operation has no backend endpoint and cannot be executed directly.",
              );
            }

            const response = await apiClient.post(endpoint, formData, {
              responseType: "blob",
            });

            const responseBlob: Blob = response.data;
            const contentTypeHeader = response.headers?.["content-type"];

            if (config.responseHandler) {
              processedFiles = await config.responseHandler(
                responseBlob,
                filesForAPI,
              );
            } else if (
              await zipFileService.isZipResponse(
                responseBlob,
                typeof contentTypeHeader === "string"
                  ? contentTypeHeader
                  : undefined,
              )
            ) {
              processedFiles = await extractZipFiles(responseBlob);
            } else {
              const filename = `${config.filePrefix}${filesForAPI[0]?.name || "document.pdf"}`;
              processedFiles = [
                new File([responseBlob], filename, { type: "application/pdf" }),
              ];
            }

            if (processedFiles.length === 0) {
              throw new Error(
                "The server processed the request but returned no files.",
              );
            }

            // Assume all inputs succeeded together unless server provided an error earlier
            successSourceIds = validFiles.map((f) => f.fileId);
            // One backend call produced all of these, and a ZIP response
            // carries no ordering guarantee, so which member came from which
            // upload is unknowable here. The single exception is 1→1, where
            // there is only one possible answer. Anything else stays null so
            // no output can claim — and overwrite — an input's file.
            outputSourceIds =
              validFiles.length === 1 && processedFiles.length === 1
                ? [validFiles[0].fileId]
                : processedFiles.map(() => null);
            break;
          }

          case ToolType.custom: {
            actions.setStatus("Processing files...");
            const result = await config.customProcessor(params, filesForAPI);

            processedFiles = result.files;
            const consumedAllInputs = result.consumedAllInputs || false;

            // If consumedAllInputs flag is set, mark all inputs as successful
            // (used for operations that combine N inputs into fewer outputs)
            if (consumedAllInputs) {
              successSourceIds = validFiles.map((f) => f.fileId);
              // N inputs folded into fewer outputs: no output corresponds to
              // exactly one input, so none may claim an input's file.
              outputSourceIds = processedFiles.map(() => null);
            } else {
              // Try to map outputs back to inputs by filename (before extension)
              const inputBaseNames = new Map<string, FileId>();
              for (const f of validFiles) {
                const base = getFilenameWithoutExtension(f.name || "");
                inputBaseNames.set(base, f.fileId);
              }
              const mappedSuccess: FileId[] = [];
              // Per-output provenance from the same name match. Unmatched
              // outputs stay null rather than borrowing a neighbour's source.
              outputSourceIds = processedFiles.map((out) => {
                const base = getFilenameWithoutExtension(out.name || "");
                const id = inputBaseNames.get(base);
                if (id) mappedSuccess.push(id);
                return id ?? null;
              });
              // Fallback to naive alignment if names don't match. This is a
              // guess, and it is confined to which inputs count as consumed —
              // it deliberately does not feed outputSourceIds, because a wrong
              // guess there would overwrite the wrong file on disk.
              if (mappedSuccess.length === 0) {
                successSourceIds = validFiles
                  .slice(0, processedFiles.length)
                  .map((f) => f.fileId);
              } else {
                successSourceIds = mappedSuccess;
              }
            }
            break;
          }
        }

        // Normalize error flags across tool types: mark failures, clear successes
        try {
          const allInputIds = validFiles.map((f) => f.fileId);
          const okSet = new Set(successSourceIds);
          // Clear errors on successes
          for (const okId of okSet) {
            try {
              fileActions.clearFileError(okId);
            } catch (_e) {
              void _e;
            }
          }
          // Mark errors on inputs that didn't succeed
          for (const id of allInputIds) {
            if (!okSet.has(id)) {
              try {
                fileActions.markFileError(id);
              } catch (_e) {
                void _e;
              }
            }
          }
        } catch (_e) {
          void _e;
        }

        if (externalErrorFileIds.length > 0) {
          // If backend told us which sources failed, prefer that mapping
          successSourceIds = validFiles
            .map((f) => f.fileId)
            .filter((id) => !externalErrorFileIds.includes(id));
          // Also mark failed IDs immediately
          try {
            for (const badId of externalErrorFileIds) {
              fileActions.markFileError(badId as FileId);
            }
          } catch (_e) {
            void _e;
          }
        }

        if (processedFiles.length > 0) {
          actions.setFiles(processedFiles);

          // Generate thumbnails and download URL concurrently
          actions.setGeneratingThumbnails(true);
          const [thumbnails, downloadInfo] = await Promise.all([
            generateThumbnails(processedFiles),
            createDownloadInfo(processedFiles, config.operationType),
          ]);
          actions.setGeneratingThumbnails(false);

          actions.setThumbnails(thumbnails);

          // Determine whether outputs are new versions of their inputs or independent artifacts.
          // A version operation produces exactly one output per successful input, all in the same
          // format (e.g. compress, rotate, redact: 1→1 or N→N same extension).
          // Everything else — format conversions (ext change), merges (N→1), splits (1→N) —
          // produces outputs that have no meaningful parent-child relationship with the inputs.
          const isVersionOp =
            processedFiles.length > 0 &&
            successSourceIds.length === processedFiles.length &&
            successSourceIds.every((id, i) => {
              const inputFile = validFiles.find((f) => f.fileId === id);
              const inExt = inputFile?.name.split(".").pop()?.toLowerCase();
              const outExt = processedFiles[i].name
                .split(".")
                .pop()
                ?.toLowerCase();
              return inExt != null && inExt === outExt;
            });

          actions.setStatus("Generating metadata for processed files...");
          const processedFileMetadataArray = await Promise.all(
            processedFiles.map((file) => generateProcessedFileMetadata(file)),
          );

          const { inputFileIds, inputRustlingFileStubs } = buildInputTracking(
            validFiles,
            selectors,
          );

          if (isVersionOp) {
            // Output is a modified version of the input — link it to the input's version chain.
            // The input is removed from the workbench and replaced in-place by the output.
            const newToolOperation: ToolOperation = {
              toolId: config.operationType,
              timestamp: Date.now(),
            };

            const successInputStubs = successSourceIds
              .map((id) => selectors.getRustlingFileStub(id))
              .filter(Boolean) as RustlingFileStub[];

            if (successInputStubs.length !== processedFiles.length) {
              console.warn(
                "[useToolOperation] Mismatch successInputStubs vs outputs",
                {
                  successInputStubs: successInputStubs.length,
                  outputs: processedFiles.length,
                },
              );
            }

            // Prefer the recorded source for each output. The index fallbacks
            // behind it are a guess — `successInputStubs` drops missing stubs
            // and so shifts left if an input disappears mid-run — but they are
            // now only able to misattribute *version history*, never a file on
            // disk: createChildStub no longer inherits `localFilePath`, and the
            // path is attached explicitly below from `outputSourceIds` alone.
            const parentStubForOutput = (index: number) => {
              const sourceId = outputSourceIds[index];
              const recorded = sourceId
                ? selectors.getRustlingFileStub(sourceId)
                : undefined;
              return (
                recorded ||
                successInputStubs[index] ||
                inputRustlingFileStubs[index] ||
                inputRustlingFileStubs[0]
              );
            };

            const { outputRustlingFileStubs, outputRustlingFiles } =
              buildOutputPairs(
                processedFiles,
                thumbnails,
                processedFileMetadataArray,
                (file, thumbnail, metadata, index) =>
                  createChildStub(
                    parentStubForOutput(index),
                    newToolOperation,
                    file,
                    metadata?.thumbnailUrl || thumbnail,
                    metadata,
                  ),
              );

            // Decide which outputs may overwrite a file on disk, from recorded
            // provenance only. Applied after consumeFiles so the stubs exist.
            const pathCarry = planLocalFilePathCarry(
              inputRustlingFileStubs,
              outputRustlingFileStubs.map((stub, index) => ({
                id: stub.id,
                sourceId: outputSourceIds[index] ?? null,
              })),
            );

            // Path for the single-artifact download. Derived from the same
            // vetted pairing, so it is only set when exactly one output owns
            // exactly one file — previously this was whichever input happened
            // to be first, which for an N→N run named an arbitrary document.
            const downloadLocalPath =
              pathCarry.length === 1 ? pathCarry[0].localFilePath : null;

            // Only consume inputs that successfully produced outputs
            const toConsumeInputIds = successSourceIds.filter((id) =>
              inputFileIds.includes(id),
            );
            console.debug("[useToolOperation] Consuming files (version)", {
              inputCount: inputFileIds.length,
              toConsume: toConsumeInputIds.length,
            });
            const outputFileIds = await consumeFiles(
              toConsumeInputIds,
              outputRustlingFiles,
              outputRustlingFileStubs,
            );
            // Tell the viewer to follow the replacement file — consumeFiles prepends the new file
            // to the list, so activeFileIndex would point to the wrong file without this.
            if (outputFileIds.length === 1) setActiveFileId(outputFileIds[0]);

            // Notify on desktop when processing completes
            await notifyPdfProcessingComplete(outputFileIds.length);

            // Carry the desktop save path forward so an output can be saved
            // back over the file it came from. planLocalFilePathCarry has
            // already discarded every pairing that was not provably one input
            // to one output, so no two outputs can target the same file.
            for (const { outputId, localFilePath } of pathCarry) {
              fileActions.updateRustlingFileStub(outputId, { localFilePath });
            }

            actions.setDownloadInfo(
              downloadInfo.url,
              downloadInfo.filename,
              downloadLocalPath,
              outputFileIds,
            );

            lastOperationRef.current = {
              inputFiles: extractFiles(validFiles),
              inputRustlingFileStubs: inputRustlingFileStubs.map((record) => ({
                ...record,
              })),
              outputFileIds,
            };
          } else {
            // Outputs are independent artifacts (format conversion, merge, split).
            // Create fresh root stubs with no parent chain, then swap out only the inputs
            // that successfully produced outputs — other workbench files are untouched.
            const { outputRustlingFileStubs, outputRustlingFiles } =
              buildOutputPairs(
                processedFiles,
                thumbnails,
                processedFileMetadataArray,
                (file, thumbnail, metadata) =>
                  createNewRustlingFileStub(
                    file,
                    undefined,
                    metadata?.thumbnailUrl || thumbnail,
                    metadata,
                  ),
              );

            const toConsumeInputIds = successSourceIds.filter((id) =>
              inputFileIds.includes(id),
            );
            console.debug("[useToolOperation] Consuming files (independent)", {
              inputCount: inputFileIds.length,
              toConsume: toConsumeInputIds.length,
            });
            const outputFileIds = await consumeFiles(
              toConsumeInputIds,
              outputRustlingFiles,
              outputRustlingFileStubs,
            );

            // Notify on desktop when processing completes
            await notifyPdfProcessingComplete(outputFileIds.length);

            actions.setDownloadInfo(
              downloadInfo.url,
              downloadInfo.filename,
              null,
              outputFileIds,
            );

            // Send the user to the viewer for a single PDF output, otherwise the file editor
            const isSinglePdf =
              processedFiles.length === 1 &&
              processedFiles[0].type === "application/pdf";
            navActions.setWorkbench(isSinglePdf ? "viewer" : "fileEditor");

            lastOperationRef.current = {
              inputFiles: extractFiles(validFiles),
              inputRustlingFileStubs: inputRustlingFileStubs.map((record) => ({
                ...record,
              })),
              outputFileIds,
            };
          }
        }
      } catch (error: any) {
        try {
          const handled = await handle422Error(error, (id) =>
            fileActions.markFileError(id as FileId),
          );
          if (handled) {
            actions.setStatus(
              "Process failed due to invalid/corrupted file(s)",
            );
            return;
          }
        } catch (_e) {
          void _e;
        }

        const errorMessage =
          config.getErrorMessage?.(error) || extractErrorMessage(error);
        actions.setError(errorMessage);
        actions.setStatus("");
      } finally {
        window.removeEventListener(
          FILE_EVENTS.markError,
          errorListener as EventListener,
        );
        actions.setLoading(false);
        actions.setProgress(null);
      }
    },
    [
      t,
      config,
      actions,
      addFiles,
      consumeFiles,
      navActions,
      processFiles,
      generateThumbnails,
      createDownloadInfo,
      cleanupBlobUrls,
      extractZipFiles,
    ],
  );

  const cancelOperation = useCallback(() => {
    cancelApiCalls();
    actions.setLoading(false);
    actions.setProgress(null);
    actions.setStatus("Operation cancelled");
  }, [cancelApiCalls, actions]);

  const resetResults = useCallback(() => {
    cleanupBlobUrls();
    actions.resetResults();
    // Clear undo data when results are reset to prevent memory leaks
    lastOperationRef.current = null;
  }, [cleanupBlobUrls, actions]);

  // Cleanup on unmount to prevent memory leaks
  useEffect(() => {
    return () => {
      lastOperationRef.current = null;
    };
  }, []);

  const undoOperation = useCallback(async () => {
    if (!lastOperationRef.current) {
      actions.setError(t("noOperationToUndo", "No operation to undo"));
      return;
    }

    const { inputFiles, inputRustlingFileStubs, outputFileIds } =
      lastOperationRef.current;

    // Validate that we have data to undo
    if (inputFiles.length === 0 || inputRustlingFileStubs.length === 0) {
      actions.setError(
        t("invalidUndoData", "Cannot undo: invalid operation data"),
      );
      return;
    }

    if (outputFileIds.length === 0) {
      actions.setError(
        t(
          "noFilesToUndo",
          "Cannot undo: no files were processed in the last operation",
        ),
      );
      return;
    }

    try {
      // Undo the consume operation
      await undoConsumeFiles(inputFiles, inputRustlingFileStubs, outputFileIds);

      // Clear results and operation tracking
      resetResults();
      lastOperationRef.current = null;

      // Show success message
      actions.setStatus(t("undoSuccess", "Operation undone successfully"));
    } catch (error: any) {
      let errorMessage = extractErrorMessage(error);

      // Provide more specific error messages based on error type
      if (error.message?.includes("Mismatch between input files")) {
        errorMessage = t(
          "undoDataMismatch",
          "Cannot undo: operation data is corrupted",
        );
      } else if (error.message?.includes("IndexedDB")) {
        errorMessage = t(
          "undoStorageError",
          "Undo completed but some files could not be saved to storage",
        );
      } else if (error.name === "QuotaExceededError") {
        errorMessage = t(
          "undoQuotaError",
          "Cannot undo: insufficient storage space",
        );
      }

      actions.setError(
        `${t("undoFailed", "Failed to undo operation")}: ${errorMessage}`,
      );

      // Don't clear the operation data if undo failed - user might want to try again
    }
  }, [undoConsumeFiles, resetResults, actions, t]);

  return {
    // State
    files: state.files,
    thumbnails: state.thumbnails,
    isGeneratingThumbnails: state.isGeneratingThumbnails,
    downloadUrl: state.downloadUrl,
    downloadFilename: state.downloadFilename,
    downloadLocalPath: state.downloadLocalPath,
    outputFileIds: state.outputFileIds,
    isLoading: state.isLoading,
    status: state.status,
    errorMessage: state.errorMessage,
    progress: state.progress,
    // Actions
    executeOperation,
    resetResults,
    clearError: actions.clearError,
    cancelOperation,
    undoOperation,
  };
};
