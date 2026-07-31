// Centralized download page for published RustlingPDF builds.
const LATEST_RELEASE_URL =
  "https://github.com/hairbui76/RustlingPDF/releases/latest";

export const DOWNLOAD_URLS = {
  WINDOWS: LATEST_RELEASE_URL,
  MAC: LATEST_RELEASE_URL,
  LINUX_DOCS: LATEST_RELEASE_URL,
} as const;
