/**
 * Which `File` came from which absolute path on disk.
 *
 * Keyed by the `File` **object**, deliberately. The previous mechanism keyed a
 * plain `Map` by `quickKey` (`name|size|lastModified`), which is not an
 * identity: two files with the same name and size collide whenever their
 * `lastModified` matches, and files constructed without an explicit
 * `lastModified` get `Date.now()` — so two documents read in the same
 * millisecond, which is the normal case inside a `Promise.all`, produced the
 * *same key*. Last write won, so one file silently took the other's path, and
 * the next in-place save wrote one document's bytes over a different file on
 * the user's disk. Multi-selecting two copies of a document from different
 * folders and choosing "Merge with RustlingPDF" was enough to trigger it.
 *
 * Object identity cannot collide: the entry is reachable only through the very
 * `File` instance that was read from that path. Nothing about the file's
 * metadata participates.
 *
 * A `WeakMap` so a file that is opened and then dropped before it reaches the
 * workspace leaves nothing behind.
 */
const localFilePaths = new WeakMap<File, string>();

/** Record the absolute path `file` was read from. */
export function rememberLocalFilePath(file: File, path: string): void {
  localFilePaths.set(file, path);
}

/**
 * Read and forget the path `file` was loaded from.
 *
 * Consumed on use: the path belongs to the workspace stub from that point on,
 * and a stale registry entry could otherwise be picked up by a later add of
 * the same `File` instance.
 */
export function takeLocalFilePath(file: File): string | undefined {
  const path = localFilePaths.get(file);
  if (path !== undefined) {
    localFilePaths.delete(file);
  }
  return path;
}

/** Non-destructive read, for assertions and tests. */
export function peekLocalFilePath(file: File): string | undefined {
  return localFilePaths.get(file);
}
