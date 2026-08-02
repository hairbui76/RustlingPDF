/**
 * OS notification when a long tool run finishes and the window is in the
 * background.
 *
 * NOT restored. `@tauri-apps/plugin-notification` is still a dependency and
 * the desktop layer that used it was deleted with `src/desktop`, but a
 * notification needs a permission request flow and a "was the window actually
 * backgrounded" check to avoid notifying about something the user is looking
 * at. See the ledger in `services/desktop/desktopCommands.test.ts`.
 */
export async function notifyPdfProcessingComplete(
  _fileCount: number,
): Promise<void> {
  // No-op in every runtime today.
}
