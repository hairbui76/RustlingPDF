import { RustlingFileStub } from "@app/types/fileContext";

export interface OpenInNewWindowApi {
  /** Whether this file can be opened in a separate window. */
  canOpenInNewWindow: (file: RustlingFileStub) => boolean;
  /** Open the file in a separate window. */
  openInNewWindow: (file: RustlingFileStub) => void;
}

/**
 * Core (web) build: multiple windows aren't a thing in the browser, so this is
 * a no-op. The desktop build overrides this file via path resolution to spawn
 * a real Tauri window.
 */
export function useOpenInNewWindow(): OpenInNewWindowApi {
  return {
    canOpenInNewWindow: () => false,
    openInNewWindow: () => {},
  };
}
