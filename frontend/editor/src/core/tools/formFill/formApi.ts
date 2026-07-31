/**
 * API service for form-related backend calls.
 */
import apiClient from "@app/services/apiClient";
import type {
  FormField,
  FormFieldCreationRequest,
  FormFieldModificationRequest,
} from "@app/tools/formFill/types";

/**
 * Fetch form fields with coordinates from the backend.
 * Calls POST /api/v1/form/fields-with-coordinates
 */
export async function fetchFormFieldsWithCoordinates(
  file: File | Blob,
): Promise<FormField[]> {
  const formData = new FormData();
  formData.append("file", file);

  const response = await apiClient.post<FormField[]>(
    "/api/v1/form/fields-with-coordinates",
    formData,
  );
  return response.data;
}

/**
 * Fill form fields and get back a filled PDF blob.
 * Calls POST /api/v1/form/fill
 */
export async function fillFormFields(
  file: File | Blob,
  values: Record<string, string>,
  flatten: boolean = false,
): Promise<Blob> {
  const formData = new FormData();
  formData.append("file", file);
  formData.append(
    "data",
    new Blob([JSON.stringify(values)], { type: "application/json" }),
  );
  formData.append("flatten", String(flatten));

  const response = await apiClient.post("/api/v1/form/fill", formData, {
    responseType: "blob",
  });
  return response.data;
}

/**
 * Extract form fields as CSV.
 * Calls POST /api/v1/form/extract-csv
 */
export async function extractFormFieldsCsv(
  file: File | Blob,
  values?: Record<string, string>,
): Promise<Blob> {
  const formData = new FormData();
  formData.append("file", file);
  if (values) {
    formData.append(
      "data",
      new Blob([JSON.stringify(values)], { type: "application/json" }),
    );
  }

  const response = await apiClient.post("/api/v1/form/extract-csv", formData, {
    responseType: "blob",
  });
  return response.data;
}

/**
 * Extract form fields as XLSX.
 * Calls POST /api/v1/form/extract-xlsx
 */
export async function extractFormFieldsXlsx(
  file: File | Blob,
  values?: Record<string, string>,
): Promise<Blob> {
  const formData = new FormData();
  formData.append("file", file);
  if (values) {
    formData.append(
      "data",
      new Blob([JSON.stringify(values)], { type: "application/json" }),
    );
  }

  const response = await apiClient.post("/api/v1/form/extract-xlsx", formData, {
    responseType: "blob",
  });
  return response.data;
}

/** Create AcroForm fields and return the updated PDF. */
export async function createFormFields(
  file: File | Blob,
  fields: FormFieldCreationRequest[],
): Promise<Blob> {
  const formData = new FormData();
  formData.append("file", file);
  formData.append(
    "fields",
    new Blob([JSON.stringify(fields)], { type: "application/json" }),
  );
  const response = await apiClient.post(
    "/api/v1/form/create-fields",
    formData,
    { responseType: "blob" },
  );
  return response.data;
}

/** Update existing AcroForm field definitions and return the updated PDF. */
export async function modifyFormFields(
  file: File | Blob,
  updates: FormFieldModificationRequest[],
): Promise<Blob> {
  const formData = new FormData();
  formData.append("file", file);
  formData.append(
    "updates",
    new Blob([JSON.stringify(updates)], { type: "application/json" }),
  );
  const response = await apiClient.post(
    "/api/v1/form/modify-fields",
    formData,
    { responseType: "blob" },
  );
  return response.data;
}

/** Delete existing AcroForm fields and return the updated PDF. */
export async function deleteFormFields(
  file: File | Blob,
  names: string[],
): Promise<Blob> {
  const formData = new FormData();
  formData.append("file", file);
  formData.append(
    "names",
    new Blob([JSON.stringify(names)], { type: "application/json" }),
  );
  const response = await apiClient.post(
    "/api/v1/form/delete-fields",
    formData,
    { responseType: "blob" },
  );
  return response.data;
}

/** Fill one PDF per CSV/XLSX row and return the result ZIP. */
export async function batchFillFormFields(
  file: File | Blob,
  dataFile: File,
): Promise<Blob> {
  const formData = new FormData();
  formData.append("file", file);
  formData.append("dataFile", dataFile);
  const response = await apiClient.post("/api/v1/form/batch-fill", formData, {
    responseType: "blob",
  });
  return response.data;
}
