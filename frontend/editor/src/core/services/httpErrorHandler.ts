// frontend/src/services/httpErrorHandler.ts
import { alert } from "@app/components/toast";
import {
  broadcastErroredFiles,
  extractErrorFileIds,
  normalizeAxiosErrorData,
} from "@app/services/errorUtils";
import { showSpecialErrorToast } from "@app/services/specialErrorToasts";
import {
  clampText,
  extractAxiosErrorMessage,
} from "@app/services/httpErrorUtils";

// Module-scoped state to reduce global variable usage
const recentSpecialByEndpoint: Record<string, number> = {};
const SPECIAL_SUPPRESS_MS = 1500; // brief window to suppress generic duplicate after special toast

/**
 * Handles HTTP errors with toast notifications and file error broadcasting
 * Returns true if the error should be suppressed (deduplicated), false otherwise
 */
export async function handleHttpError(error: any): Promise<boolean> {
  // Check if this error should skip the global toast (component will handle it)
  if (error?.config?.suppressErrorToast === true) {
    return false; // Don't show global toast, but continue rejection
  }

  const status: number | undefined = error?.response?.status;
  // Compute title/body (friendly) from the error object
  const { title, body } = extractAxiosErrorMessage(error);

  // Normalize response data ONCE, reuse for both ID extraction and special-toast matching
  const raw = error?.response?.data as any;
  let normalized: unknown = raw;
  try {
    normalized = await normalizeAxiosErrorData(raw);
  } catch (e) {
    console.debug("normalizeAxiosErrorData", e);
  }

  // 1) If server sends structured file IDs for failures, also mark them errored in UI
  try {
    const ids = extractErrorFileIds(normalized);
    if (ids && ids.length > 0) {
      broadcastErroredFiles(ids);
    }
  } catch (e) {
    console.debug("extractErrorFileIds", e);
  }

  // 2) Generic-vs-special dedupe by endpoint
  const url: string | undefined = error?.config?.url;
  const now = Date.now();
  const isSpecial =
    status === 422 ||
    status === 409 || // often actionable conflicts
    /Failed files:/.test(body) ||
    /invalid\/corrupted file\(s\)/i.test(body);

  if (isSpecial && url) {
    recentSpecialByEndpoint[url] = now;
  }
  if (!isSpecial && url) {
    const last = recentSpecialByEndpoint[url] || 0;
    if (now - last < SPECIAL_SUPPRESS_MS) {
      return true; // Suppress this error (deduplicated)
    }
  }

  // 3) Show specialized friendly toasts if matched; otherwise show the generic one
  let rawString: string | undefined;
  try {
    rawString =
      typeof normalized === "string" ? normalized : JSON.stringify(normalized);
  } catch (e) {
    console.debug("extractErrorFileIds", e);
  }

  const handled = showSpecialErrorToast(rawString, { status });
  if (!handled) {
    const displayBody = clampText(body);
    alert({
      alertType: "error",
      title,
      body: displayBody,
      expandable: true,
      isPersistentPopup: false,
    });
  }

  return false; // Error was handled with toast, continue normal rejection
}
