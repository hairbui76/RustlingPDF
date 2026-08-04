import { describe, it, expect, vi, beforeEach } from "vitest";

// The office-preview branch of generateThumbnailForFile: a Word document is
// converted through the backend, the resulting PDF is rendered locally, and
// every failure mode degrades to "" (the file-type icon) without surfacing
// an error toast.

const postMock = vi.hoisted(() => vi.fn());
vi.mock("@app/services/apiClient", () => ({
  default: { post: postMock },
}));

// pdfium is a wasm module; stand in for the local render step.
vi.mock("@app/services/pdfiumService", () => ({
  openRawDocumentSafe: vi.fn(async () => 1),
  closeRawDocument: vi.fn(async () => undefined),
  getPdfiumModule: vi.fn(async () => ({
    FPDF_GetPageCount: () => 1,
  })),
}));
vi.mock("@app/utils/pdfiumPageRender", () => ({
  renderPdfiumPageDataUrl: vi.fn(async () => "data:image/png;base64,MOCK"),
  readPdfiumPageMetadata: vi.fn(async () => ({
    rotation: 0,
    width: 612,
    height: 792,
  })),
}));

import { generateThumbnailForFile } from "@app/utils/thumbnailUtils";

function wordFile(name: string, bytes = 128): File {
  return new File([new Uint8Array(bytes)], name, {
    type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  });
}

describe("office document previews", () => {
  beforeEach(() => {
    postMock.mockReset();
    // The failure paths warn on purpose (degrade-to-icon is silent for the
    // user, loud for the console); keep the suite's console-clean gate happy.
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  it("converts a docx through the backend and renders the PDF", async () => {
    postMock.mockResolvedValueOnce({ data: new Uint8Array([1, 2, 3]).buffer });

    const thumbnail = await generateThumbnailForFile(wordFile("report.docx"));

    expect(thumbnail).toBe("data:image/png;base64,MOCK");
    expect(postMock).toHaveBeenCalledTimes(1);
    const [url, form, config] = postMock.mock.calls[0];
    expect(url).toBe("/api/v1/convert/file/pdf");
    expect(form).toBeInstanceOf(FormData);
    // A preview is decoration: conversion failures must never toast.
    expect(config).toMatchObject({
      responseType: "arraybuffer",
      suppressErrorToast: true,
    });
  });

  it("degrades to the icon (empty string) when conversion fails", async () => {
    postMock.mockRejectedValueOnce(new Error("engine unavailable"));

    await expect(
      generateThumbnailForFile(wordFile("legacy.doc")),
    ).resolves.toBe("");
  });

  it("skips conversion entirely for oversized documents", async () => {
    const big = wordFile("huge.docx", 26 * 1024 * 1024);

    await expect(generateThumbnailForFile(big)).resolves.toBe("");
    expect(postMock).not.toHaveBeenCalled();
  });

  it("does not touch the backend for non-office files", async () => {
    const text = new File([new Uint8Array(16)], "notes.txt", {
      type: "text/plain",
    });

    await expect(generateThumbnailForFile(text)).resolves.toBe("");
    expect(postMock).not.toHaveBeenCalled();
  });
});
