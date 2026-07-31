import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RustlingFile } from "@app/types/fileContext";
import type { ToolOperationHook } from "@app/hooks/tools/shared/useToolOperation";
import type { DocumentUnderstandingParameters } from "@app/hooks/tools/documentUnderstanding/useDocumentUnderstandingParameters";
import { understandDocument } from "@app/tools/documentUnderstanding/documentUnderstandingApi";
import type { DocumentUnderstandingResponse } from "@app/tools/documentUnderstanding/types";
import { extractErrorMessage } from "@app/utils/toolErrorHandler";

export interface DocumentUnderstandingOperationHook extends ToolOperationHook<DocumentUnderstandingParameters> {
  response: DocumentUnderstandingResponse | null;
  sourceFile: File | null;
}

export const useDocumentUnderstandingOperation =
  (): DocumentUnderstandingOperationHook => {
    const { t } = useTranslation();
    const [response, setResponse] =
      useState<DocumentUnderstandingResponse | null>(null);
    const [sourceFile, setSourceFile] = useState<File | null>(null);
    const [isLoading, setIsLoading] = useState(false);
    const [status, setStatus] = useState("");
    const [errorMessage, setErrorMessage] = useState<string | null>(null);

    const resetResults = useCallback(() => {
      setResponse(null);
      setSourceFile(null);
      setStatus("");
      setErrorMessage(null);
    }, []);

    const executeOperation = useCallback(
      async (
        parameters: DocumentUnderstandingParameters,
        selectedFiles: RustlingFile[],
      ) => {
        const file = selectedFiles[0];
        if (!file) {
          setErrorMessage(t("noFileSelected", "No file loaded"));
          return;
        }
        setIsLoading(true);
        setResponse(null);
        setErrorMessage(null);
        setStatus(
          t(
            "documentUnderstanding.processing",
            "Extracting bounded page text and contacting the configured AI provider...",
          ),
        );
        try {
          setResponse(await understandDocument(file, parameters));
          setSourceFile(file);
          setStatus(
            t(
              "documentUnderstanding.complete",
              "Document understanding complete",
            ),
          );
        } catch (error) {
          setErrorMessage(extractErrorMessage(error));
          throw error;
        } finally {
          setIsLoading(false);
        }
      },
      [t],
    );

    const clearError = useCallback(() => setErrorMessage(null), []);
    const cancelOperation = useCallback(() => undefined, []);
    const undoOperation = useCallback(
      async () => resetResults(),
      [resetResults],
    );

    return useMemo(
      () => ({
        files: [],
        thumbnails: [],
        isGeneratingThumbnails: false,
        downloadUrl: null,
        downloadFilename: "",
        isLoading,
        status,
        errorMessage,
        progress: null,
        executeOperation,
        resetResults,
        clearError,
        cancelOperation,
        undoOperation,
        response,
        sourceFile,
      }),
      [
        cancelOperation,
        clearError,
        errorMessage,
        executeOperation,
        isLoading,
        resetResults,
        response,
        sourceFile,
        status,
        undoOperation,
      ],
    );
  };
