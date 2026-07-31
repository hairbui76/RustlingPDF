import apiClient from "@app/services/apiClient";
import type {
  AccessibilityRepairs,
  AccessibilityReport,
} from "@app/tools/accessibility/types";

export async function checkPdfAccessibility(
  file: File | Blob,
): Promise<AccessibilityReport> {
  const formData = new FormData();
  formData.append("fileInput", file);
  const response = await apiClient.post<AccessibilityReport>(
    "/api/v1/accessibility/check",
    formData,
  );
  return response.data;
}

export async function remediatePdfAccessibility(
  file: File | Blob,
  repairs: AccessibilityRepairs,
): Promise<Blob> {
  const formData = new FormData();
  formData.append("fileInput", file);
  formData.append(
    "repairs",
    new Blob([JSON.stringify(repairs)], { type: "application/json" }),
  );
  const response = await apiClient.post(
    "/api/v1/accessibility/remediate",
    formData,
    { responseType: "blob" },
  );
  return response.data;
}
