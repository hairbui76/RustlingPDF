import { RustlingFileStub } from "@app/types/fileContext";

export interface OpenInNewWindowApi {
  /** Whether this file can be opened in a separate window. */
  canOpenInNewWindow: (file: RustlingFileStub) => boolean;
  /** Open the file in a separate window. */
  openInNewWindow: (file: RustlingFileStub) => void;
}

/**
 * Multiple windows aren't a thing in the browser, so this is a no-op and the
 * menu item that consumes it renders nothing.
 *
 * NOT restored for desktop. The Rust side still registers
 * `open_in_new_window`, `open_files_in_new_window` and `pop_window_file_ids`,
 * but the frontend half needs a stored-file materialisation path and a
 * per-platform capability gate (windows must share one persistent web store,
 * which they do not on Linux). See the ledger in
 * `services/desktop/desktopCommands.test.ts`.
 */
export function useOpenInNewWindow(): OpenInNewWindowApi {
  return {
    canOpenInNewWindow: () => false,
    openInNewWindow: () => {},
  };
}
