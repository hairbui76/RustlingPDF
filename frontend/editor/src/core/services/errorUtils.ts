export const FILE_EVENTS = {
  markError: "files:markError",
} as const;

const UUID_REGEX =
  /[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/g;

export function tryParseJson<T = any>(input: unknown): T | undefined {
  if (typeof input !== "string") return input as T | undefined;
  try {
    return JSON.parse(input) as T;
  } catch {
    return undefined;
  }
}

function isReadableBody(data: any): boolean {
  return (
    typeof data?.text === "function" ||
    (typeof Blob !== "undefined" && data instanceof Blob)
  );
}

function readBodyText(data: any): Promise<string> {
  if (typeof data.text === "function") return data.text();
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result ?? ""));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(data);
  });
}

export async function normalizeAxiosErrorData(data: any): Promise<any> {
  if (!data) return undefined;
  if (isReadableBody(data)) {
    const text = await readBodyText(data);
    return tryParseJson(text) ?? text;
  }
  return data;
}

/**
 * Replaces a Blob error body with its parsed JSON or text, in
 * place. Requests made with `responseType: "blob"` receive the backend's JSON
 * `ErrorResponse` as a Blob, which every error-message extractor would
 * otherwise see as an opaque object.
 */
export async function normalizeAxiosErrorResponse(error: any): Promise<void> {
  const response = error?.response;
  if (!response || !isReadableBody(response.data)) return;
  try {
    response.data = await normalizeAxiosErrorData(response.data);
  } catch (e) {
    console.debug("normalizeAxiosErrorResponse", e);
  }
}

/** Returns the human-readable message carried by an error response body. */
export function messageFromErrorData(data: unknown): string | undefined {
  if (typeof data === "string") return data.trim() ? data : undefined;
  if (!data || typeof data !== "object") return undefined;
  for (const key of ["message", "detail", "error"] as const) {
    const value = (data as Record<string, unknown>)[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return undefined;
}

export function extractErrorFileIds(payload: any): string[] | undefined {
  if (!payload) return undefined;
  if (Array.isArray(payload?.errorFileIds))
    return payload.errorFileIds as string[];
  if (typeof payload === "string") {
    const matches = payload.match(UUID_REGEX);
    if (matches && matches.length > 0) return Array.from(new Set(matches));
  }
  return undefined;
}

export function broadcastErroredFiles(fileIds: string[]) {
  if (!fileIds || fileIds.length === 0) return;
  window.dispatchEvent(
    new CustomEvent(FILE_EVENTS.markError, { detail: { fileIds } }),
  );
}

export function isZeroByte(
  file: File | { size?: number } | null | undefined,
): boolean {
  if (!file) return true;
  const size = (file as any).size;
  return typeof size === "number" ? size <= 0 : true;
}

export function isEmptyOutput(files: File[] | null | undefined): boolean {
  if (!files || files.length === 0) return true;
  return files.every((f) => (f as any)?.size === 0);
}
