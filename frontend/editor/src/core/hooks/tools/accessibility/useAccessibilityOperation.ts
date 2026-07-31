import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { RustlingFile } from "@app/types/fileContext";
import type { ToolOperationHook } from "@app/hooks/tools/shared/useToolOperation";
import type { AccessibilityParameters } from "@app/hooks/tools/accessibility/useAccessibilityParameters";
import {
  checkPdfAccessibility,
  remediatePdfAccessibility,
} from "@app/tools/accessibility/accessibilityApi";
import type {
  AccessibilityRepairs,
  AccessibilityReport,
} from "@app/tools/accessibility/types";
import { extractErrorMessage } from "@app/utils/toolErrorHandler";

export interface AccessibilityOperationHook extends ToolOperationHook<AccessibilityParameters> {
  report: AccessibilityReport | null;
  checkedFile: File | null;
  applyRepairs: (file: File, repairs: AccessibilityRepairs) => Promise<File>;
}

function accessibleFilename(filename: string): string {
  const stem = filename.replace(/\.pdf$/i, "");
  return `${stem}_accessible.pdf`;
}

export const useAccessibilityOperation = (): AccessibilityOperationHook => {
  const { t } = useTranslation();
  const [report, setReport] = useState<AccessibilityReport | null>(null);
  const [checkedFile, setCheckedFile] = useState<File | null>(null);
  const [files, setFiles] = useState<File[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [status, setStatus] = useState("");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const resetResults = useCallback(() => {
    setReport(null);
    setCheckedFile(null);
    setFiles([]);
    setStatus("");
    setErrorMessage(null);
  }, []);

  const clearError = useCallback(() => setErrorMessage(null), []);

  const executeOperation = useCallback(
    async (
      _parameters: AccessibilityParameters,
      selectedFiles: RustlingFile[],
    ) => {
      if (selectedFiles.length === 0) {
        setErrorMessage(t("noFileSelected", "No file loaded"));
        return;
      }
      const file = selectedFiles[0];
      setIsLoading(true);
      setStatus(
        t(
          "accessibility.processing",
          "Checking document structure and accessible names...",
        ),
      );
      setErrorMessage(null);
      setReport(null);
      setFiles([]);
      try {
        setReport(await checkPdfAccessibility(file));
        setCheckedFile(file);
        setStatus(t("accessibility.checked", "Accessibility check complete"));
      } catch (error) {
        setErrorMessage(extractErrorMessage(error));
        throw error;
      } finally {
        setIsLoading(false);
      }
    },
    [t],
  );

  const applyRepairs = useCallback(
    async (file: File, repairs: AccessibilityRepairs) => {
      setIsLoading(true);
      setStatus(
        t("accessibility.remediating", "Applying repairs and re-checking..."),
      );
      setErrorMessage(null);
      try {
        const blob = await remediatePdfAccessibility(file, repairs);
        const output = new File([blob], accessibleFilename(file.name), {
          type: "application/pdf",
          lastModified: Date.now(),
        });
        const updatedReport = await checkPdfAccessibility(output);
        setCheckedFile(output);
        setReport(updatedReport);
        setFiles([output]);
        setStatus(
          t(
            "accessibility.remediated",
            "Repairs applied and the result was checked again",
          ),
        );
        return output;
      } catch (error) {
        setErrorMessage(extractErrorMessage(error));
        throw error;
      } finally {
        setIsLoading(false);
      }
    },
    [t],
  );

  const cancelOperation = useCallback(() => undefined, []);
  const undoOperation = useCallback(async () => resetResults(), [resetResults]);

  return useMemo(
    () => ({
      files,
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
      report,
      checkedFile,
      applyRepairs,
    }),
    [
      applyRepairs,
      cancelOperation,
      checkedFile,
      clearError,
      errorMessage,
      executeOperation,
      files,
      isLoading,
      report,
      resetResults,
      status,
      undoOperation,
    ],
  );
};
