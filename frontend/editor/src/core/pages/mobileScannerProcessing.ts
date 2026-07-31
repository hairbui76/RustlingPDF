import { PDFDocument } from "@cantoo/pdf-lib";

export type ScanFilter = "color" | "clean" | "grayscale" | "blackWhite";

export interface NormalizedCorners {
  topLeftCorner: { x: number; y: number };
  topRightCorner: { x: number; y: number };
  bottomLeftCorner: { x: number; y: number };
  bottomRightCorner: { x: number; y: number };
}

export interface MobileScan {
  id: string;
  dataUrl: string;
}

export const FULL_IMAGE_CORNERS: NormalizedCorners = {
  topLeftCorner: { x: 0, y: 0 },
  topRightCorner: { x: 1, y: 0 },
  bottomLeftCorner: { x: 0, y: 1 },
  bottomRightCorner: { x: 1, y: 1 },
};

export function clampNormalized(value: number): number {
  return Math.max(0, Math.min(1, value));
}

export function fitScanDimensions(
  width: number,
  height: number,
  maximumDimension = 3_000,
): { width: number; height: number } {
  if (
    width <= 0 ||
    height <= 0 ||
    maximumDimension <= 0 ||
    (width <= maximumDimension && height <= maximumDimension)
  ) {
    return { width, height };
  }
  const scale = maximumDimension / Math.max(width, height);
  return {
    width: Math.max(1, Math.round(width * scale)),
    height: Math.max(1, Math.round(height * scale)),
  };
}

export function moveScan(
  scans: MobileScan[],
  index: number,
  direction: -1 | 1,
): MobileScan[] {
  const target = index + direction;
  if (
    index < 0 ||
    index >= scans.length ||
    target < 0 ||
    target >= scans.length
  ) {
    return scans;
  }
  const reordered = [...scans];
  [reordered[index], reordered[target]] = [reordered[target], reordered[index]];
  return reordered;
}

function loadImage(dataUrl: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () =>
      reject(new Error("The selected image could not be decoded."));
    image.src = dataUrl;
  });
}

function canvasDataUrl(canvas: HTMLCanvasElement): string {
  return canvas.toDataURL("image/jpeg", 0.94);
}

export async function applyScanFilter(
  dataUrl: string,
  filter: ScanFilter,
): Promise<string> {
  if (filter === "color") return dataUrl;
  const image = await loadImage(dataUrl);
  const canvas = document.createElement("canvas");
  canvas.width = image.naturalWidth || image.width;
  canvas.height = image.naturalHeight || image.height;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context)
    throw new Error("Image cleanup is unavailable in this browser.");

  if (filter === "clean") {
    context.filter = "contrast(1.2) saturate(0.9) brightness(1.04)";
  } else if (filter === "grayscale") {
    context.filter = "grayscale(1) contrast(1.12)";
  }
  context.drawImage(image, 0, 0, canvas.width, canvas.height);

  if (filter === "blackWhite") {
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height);
    for (let offset = 0; offset < pixels.data.length; offset += 4) {
      const luminance =
        pixels.data[offset] * 0.299 +
        pixels.data[offset + 1] * 0.587 +
        pixels.data[offset + 2] * 0.114;
      const value = luminance >= 170 ? 255 : 0;
      pixels.data[offset] = value;
      pixels.data[offset + 1] = value;
      pixels.data[offset + 2] = value;
    }
    context.putImageData(pixels, 0, 0);
  }
  return canvasDataUrl(canvas);
}

export async function rotateScanClockwise(dataUrl: string): Promise<string> {
  const image = await loadImage(dataUrl);
  const width = image.naturalWidth || image.width;
  const height = image.naturalHeight || image.height;
  const canvas = document.createElement("canvas");
  canvas.width = height;
  canvas.height = width;
  const context = canvas.getContext("2d");
  if (!context)
    throw new Error("Image rotation is unavailable in this browser.");
  context.translate(canvas.width, 0);
  context.rotate(Math.PI / 2);
  context.drawImage(image, 0, 0, width, height);
  return canvasDataUrl(canvas);
}

export async function dataUrlToFile(
  dataUrl: string,
  filename: string,
): Promise<File> {
  const response = await fetch(dataUrl);
  if (!response.ok) throw new Error("A scanned page could not be read.");
  const blob = await response.blob();
  return new File([blob], filename, {
    type: blob.type || "image/jpeg",
    lastModified: Date.now(),
  });
}

export async function createScansPdfBytes(
  dataUrls: string[],
): Promise<Uint8Array<ArrayBuffer>> {
  if (dataUrls.length === 0) {
    throw new Error("Add at least one page before exporting.");
  }
  const document = await PDFDocument.create();
  for (const dataUrl of dataUrls) {
    const response = await fetch(dataUrl);
    if (!response.ok) throw new Error("A scanned page could not be read.");
    const bytes = new Uint8Array(await response.arrayBuffer());
    const mimeType = dataUrl.slice(0, dataUrl.indexOf(";")).toLowerCase();
    const image = mimeType.includes("png")
      ? await document.embedPng(bytes)
      : await document.embedJpg(bytes);
    const page = document.addPage([image.width, image.height]);
    page.drawImage(image, {
      x: 0,
      y: 0,
      width: image.width,
      height: image.height,
    });
  }
  const bytes = await document.save();
  const fileBytes = new Uint8Array(bytes.byteLength);
  fileBytes.set(bytes);
  return fileBytes;
}

export async function createScansPdf(
  dataUrls: string[],
  filename = "rustlingpdf-scan.pdf",
): Promise<File> {
  const fileBytes = await createScansPdfBytes(dataUrls);
  return new File([fileBytes.buffer], filename, {
    type: "application/pdf",
    lastModified: Date.now(),
  });
}

export function downloadFile(file: File): void {
  const url = URL.createObjectURL(file);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = file.name;
  anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}
