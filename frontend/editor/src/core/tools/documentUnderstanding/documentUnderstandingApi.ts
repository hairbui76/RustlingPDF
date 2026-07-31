import apiClient from "@app/services/apiClient";
import type { DocumentUnderstandingParameters } from "@app/hooks/tools/documentUnderstanding/useDocumentUnderstandingParameters";
import type { DocumentUnderstandingResponse } from "@app/tools/documentUnderstanding/types";

const ROUTES = {
  summary: "/api/v1/ai/tools/document-summary",
  extraction: "/api/v1/ai/tools/document-extraction",
  translation: "/api/v1/ai/tools/document-translation",
} as const;

export async function understandDocument(
  file: File | Blob,
  parameters: DocumentUnderstandingParameters,
): Promise<DocumentUnderstandingResponse> {
  const formData = new FormData();
  formData.append("fileInput", file);
  switch (parameters.mode) {
    case "summary":
      formData.append("detail", parameters.summaryDetail);
      if (parameters.instructions.trim()) {
        formData.append("instructions", parameters.instructions.trim());
      }
      break;
    case "extraction":
      formData.append(
        "fields",
        new Blob([JSON.stringify(parameters.extractionFields)], {
          type: "application/json",
        }),
      );
      break;
    case "translation":
      formData.append("targetLanguage", parameters.targetLanguage.trim());
      if (parameters.sourceLanguage.trim()) {
        formData.append("sourceLanguage", parameters.sourceLanguage.trim());
      }
      break;
  }
  const response = await apiClient.post<DocumentUnderstandingResponse>(
    ROUTES[parameters.mode],
    formData,
  );
  return response.data;
}
