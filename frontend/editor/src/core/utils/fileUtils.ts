// Pure utility functions for file operations

/**
 * Consolidated file size formatting utility
 */
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

/**
 * Get file date as string
 */
export function getFileDate(file: File | { lastModified: number }): string {
  if (file.lastModified) {
    return new Date(file.lastModified).toLocaleString();
  }
  return "Unknown";
}

/**
 * Get file size as string (legacy method for backward compatibility)
 */
export function getFileSize(file: File | { size: number }): string {
  if (!file.size) return "Unknown";
  return formatFileSize(file.size);
}

/**
 * Detects and normalizes file extension from filename
 * @param filename - The filename to extract extension from
 * @returns Normalized file extension in lowercase, empty string if no extension
 */
export function detectFileExtension(filename: string): string {
  if (!filename || typeof filename !== "string") return "";

  const parts = filename.split(".");
  // If there's no extension (no dots or only one part), return empty string
  if (parts.length <= 1) return "";

  // Get the last part (extension) in lowercase
  let extension = parts[parts.length - 1].toLowerCase();

  // Normalize common extension variants
  if (extension === "jpeg") extension = "jpg";

  return extension;
}

/**
 * Removes the file extension from a filename
 * @param filename - The filename to process
 * @param options - Options for processing
 * @param options.preserveCase - If true, preserves original case. If false (default), converts to lowercase
 * @returns Filename without extension
 * @example
 * getFilenameWithoutExtension('document.pdf') // 'document'
 * getFilenameWithoutExtension('my.file.name.txt') // 'my.file.name'
 * getFilenameWithoutExtension('REPORT.PDF', { preserveCase: true }) // 'REPORT'
 */
export function getFilenameWithoutExtension(
  filename: string,
  options: { preserveCase?: boolean } = {},
): string {
  if (!filename || typeof filename !== "string") return "";

  const { preserveCase = false } = options;
  const withoutExtension = filename.replace(/\.[^.]+$/, "");

  return preserveCase ? withoutExtension : withoutExtension.toLowerCase();
}

/**
 * Checks if a file is a PDF based on extension and MIME type
 * @param file - File or file-like object with name and type properties
 * @returns true if the file appears to be a PDF
 */
export function isPdfFile(
  file: { name?: string; type?: string } | File | Blob | null | undefined,
): boolean {
  if (!file) return false;

  const name = "name" in file ? file.name : undefined;
  const type = file.type;

  // Check MIME type first (most reliable)
  if (type === "application/pdf") return true;

  // Check file extension as fallback
  if (name) {
    const ext = detectFileExtension(name);
    if (ext === "pdf") return true;
  }

  return false;
}

/**
 * Best-effort MIME type from a file name.
 *
 * A `File` built by the application carries whatever `type` we hand the
 * constructor, and several code paths had no better source than a guess — the
 * desktop open path labelled every file `application/pdf`, so a PNG opened by
 * double-click claimed to be a PDF and anything keyed on `file.type` treated it
 * as one. The browser's own file picker sets this from the OS; when we build a
 * `File` ourselves the extension is the only thing left to read.
 *
 * Falls back to `application/octet-stream`, which is the honest answer for an
 * extension we do not know — deliberately not a guess at the most likely type.
 */
const MIME_TYPES_BY_EXTENSION: Record<string, string> = {
  // Images
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  svg: "image/svg+xml",
  tiff: "image/tiff",
  tif: "image/tiff",

  // Documents
  pdf: "application/pdf",
  txt: "text/plain",
  html: "text/html",
  htm: "text/html",
  css: "text/css",
  js: "application/javascript",
  json: "application/json",
  xml: "application/xml",
  csv: "text/csv",
  md: "text/markdown",

  // Office documents
  doc: "application/msword",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  xls: "application/vnd.ms-excel",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ppt: "application/vnd.ms-powerpoint",
  pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  odt: "application/vnd.oasis.opendocument.text",
  ods: "application/vnd.oasis.opendocument.spreadsheet",
  odp: "application/vnd.oasis.opendocument.presentation",

  // Archives
  zip: "application/zip",
  rar: "application/x-rar-compressed",
};

export function mimeTypeForFileName(fileName: string): string {
  const ext = detectFileExtension(fileName);
  return MIME_TYPES_BY_EXTENSION[ext] || "application/octet-stream";
}

export type NonPdfFileType =
  "image" | "csv" | "json" | "text" | "markdown" | "html" | "unknown";

export const IMAGE_EXTENSIONS = new Set([
  "png",
  "jpg",
  "jpeg",
  "gif",
  "bmp",
  "svg",
  "tiff",
  "tif",
  "webp",
]);
const CSV_EXTENSIONS = new Set(["csv", "tsv"]);
const JSON_EXTENSIONS = new Set(["json"]);
const TEXT_EXTENSIONS = new Set(["txt"]);
const MARKDOWN_EXTENSIONS = new Set(["md", "markdown"]);
const HTML_EXTENSIONS = new Set(["html", "htm"]);

/** All file extensions that the built-in viewer can render. */
export const VIEWER_SUPPORTED_EXTENSIONS: string[] = [
  "pdf",
  ...IMAGE_EXTENSIONS,
  ...CSV_EXTENSIONS,
  ...JSON_EXTENSIONS,
  ...TEXT_EXTENSIONS,
  ...MARKDOWN_EXTENSIONS,
  ...HTML_EXTENSIONS,
];

/**
 * Detects the non-PDF file type category for viewer routing.
 * Returns 'unknown' for PDFs or unrecognized formats.
 */
export function detectNonPdfFileType(
  file: { name?: string; type?: string } | File | null | undefined,
): NonPdfFileType {
  if (!file) return "unknown";

  const name = "name" in file ? file.name : undefined;
  const mimeType = file.type ?? "";

  // Check MIME type first
  if (mimeType.startsWith("image/")) return "image";
  if (mimeType === "text/csv") return "csv";
  if (mimeType === "text/tab-separated-values") return "csv";
  if (mimeType === "application/json") return "json";
  if (mimeType === "text/html") return "html";
  if (mimeType === "text/markdown") return "markdown";

  // Fall back to extension
  if (name) {
    const ext = detectFileExtension(name);
    if (IMAGE_EXTENSIONS.has(ext)) return "image";
    if (CSV_EXTENSIONS.has(ext)) return "csv";
    if (JSON_EXTENSIONS.has(ext)) return "json";
    if (MARKDOWN_EXTENSIONS.has(ext)) return "markdown";
    if (TEXT_EXTENSIONS.has(ext)) return "text";
    if (HTML_EXTENSIONS.has(ext)) return "html";
  }

  return "unknown";
}
