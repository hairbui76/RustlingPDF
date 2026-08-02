import type { FileId, RustlingFileStub } from "@app/types/fileContext";

/**
 * Deciding whether a file being added is already in the workspace.
 *
 * The only key available for this is `quickKey` — `name|size|lastModified` —
 * and it is weaker than it looks. `cp -p`, `rsync -a`, and every file-sync
 * client preserve mtime, so a copy of a document in another folder is
 * byte-for-byte indistinguishable from the original by metadata alone.
 * Treating that as a duplicate silently discards a file the user explicitly
 * opened, and "silently" is the whole problem: nothing distinguishes it from a
 * file that failed to load.
 *
 * A path is stronger evidence. For files that came from disk, the question
 * "is this already here?" has an exact answer: is *this path* already here.
 * Files with no path (a browser upload, a tool output) fall back to the
 * metadata key exactly as before, so web behaviour is unchanged.
 */
export type LocalPathIndex = Map<string, Set<string>>;

/** Index the on-disk paths the workspace already holds, by quickKey. */
export function buildLocalPathIndex(
  stubsById: Record<FileId, RustlingFileStub>,
): LocalPathIndex {
  const index: LocalPathIndex = new Map();
  for (const stub of Object.values(stubsById)) {
    if (stub?.quickKey && stub.localFilePath) {
      addLocalPath(index, stub.quickKey, stub.localFilePath);
    }
  }
  return index;
}

/** Record a path against a quickKey, so later files in a batch see it too. */
export function addLocalPath(
  index: LocalPathIndex,
  quickKey: string,
  localFilePath: string,
): void {
  const paths = index.get(quickKey) ?? new Set<string>();
  paths.add(localFilePath);
  index.set(quickKey, paths);
}

export interface DuplicateCheck {
  quickKey: string;
  /** Path this file was read from, if it came from disk. */
  localFilePath?: string;
  /** quickKeys already present in the workspace. */
  existingQuickKeys: ReadonlySet<string>;
  pathIndex: LocalPathIndex;
}

/**
 * True when the file should be skipped as a duplicate.
 *
 * A metadata match is necessary but not sufficient: a file from disk whose
 * path the workspace does not already hold is a *different* document that
 * merely looks the same, and must be kept.
 */
export function isDuplicateFile({
  quickKey,
  localFilePath,
  existingQuickKeys,
  pathIndex,
}: DuplicateCheck): boolean {
  if (!existingQuickKeys.has(quickKey)) {
    return false;
  }
  if (localFilePath === undefined) {
    // No path to argue with — metadata is all there is.
    return true;
  }
  return pathIndex.get(quickKey)?.has(localFilePath) ?? false;
}
