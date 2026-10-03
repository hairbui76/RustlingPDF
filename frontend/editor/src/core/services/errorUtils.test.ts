import { describe, expect, it } from "vitest";
import { AxiosError, AxiosHeaders } from "axios";
import {
  messageFromErrorData,
  normalizeAxiosErrorResponse,
} from "@app/services/errorUtils";
import { extractAxiosErrorMessage } from "@app/services/httpErrorUtils";
import {
  createStandardErrorHandler,
  extractErrorMessage,
} from "@app/utils/toolErrorHandler";

/**
 * Tool requests use `responseType: "blob"`, so a failed request carries the
 * backend's JSON `ErrorResponse` (`{ status, message, path }`) as a Blob. The
 * user must see that `message`, not a generic fallback.
 */

const BACKEND_MESSAGE = "qpdf is not available on this server";

function blobAxiosError(status: number, body: string): AxiosError {
  const config = {
    headers: new AxiosHeaders(),
    url: "/api/v1/misc/compress-pdf",
  };
  return new AxiosError(
    `Request failed with status code ${status}`,
    "ERR_BAD_RESPONSE",
    config,
    undefined,
    {
      status,
      statusText: "",
      headers: {},
      config,
      data: new Blob([body], { type: "application/json" }),
    },
  );
}

const backendJson = (status: number) =>
  JSON.stringify({
    status,
    message: BACKEND_MESSAGE,
    path: "/api/v1/misc/compress-pdf",
  });

describe("normalizeAxiosErrorResponse", () => {
  it("replaces a JSON Blob body with the parsed object", async () => {
    const error = blobAxiosError(503, backendJson(503));
    await normalizeAxiosErrorResponse(error);
    expect(error.response?.data).toEqual({
      status: 503,
      message: BACKEND_MESSAGE,
      path: "/api/v1/misc/compress-pdf",
    });
  });

  it("replaces a non-JSON Blob body with its text", async () => {
    const error = blobAxiosError(500, "plain failure");
    await normalizeAxiosErrorResponse(error);
    expect(error.response?.data).toBe("plain failure");
  });

  it("ignores errors without a response", async () => {
    const error = new Error("offline");
    await expect(normalizeAxiosErrorResponse(error)).resolves.toBeUndefined();
  });
});

describe("messageFromErrorData", () => {
  it("reads message, detail, and error fields", () => {
    expect(messageFromErrorData({ message: "a" })).toBe("a");
    expect(messageFromErrorData({ detail: "b" })).toBe("b");
    expect(messageFromErrorData({ error: "c" })).toBe("c");
  });

  it("returns plain text and rejects empty or structured-only payloads", () => {
    expect(messageFromErrorData("text")).toBe("text");
    expect(messageFromErrorData("  ")).toBeUndefined();
    expect(messageFromErrorData({})).toBeUndefined();
    expect(messageFromErrorData(undefined)).toBeUndefined();
  });
});

describe("backend error message reaches the user", () => {
  it("global toast shows the backend message", async () => {
    const error = blobAxiosError(503, backendJson(503));
    await normalizeAxiosErrorResponse(error);
    expect(extractAxiosErrorMessage(error).body).toBe(BACKEND_MESSAGE);
  });

  it("tool panel error shows the backend message", async () => {
    const error = blobAxiosError(500, backendJson(500));
    await normalizeAxiosErrorResponse(error);
    expect(extractErrorMessage(error)).toBe(BACKEND_MESSAGE);
    expect(createStandardErrorHandler("fallback")(error)).toBe(BACKEND_MESSAGE);
  });

  it("tool panel falls back to the error message without a body", () => {
    expect(extractErrorMessage(new Error("boom"))).toBe("boom");
    expect(createStandardErrorHandler("fallback")({})).toBe("fallback");
  });
});
