/**
 * File History Utilities
 *
 * Helper functions for IndexedDB-based file history management.
 * Handles file history operations and lineage tracking.
 */
import { RustlingFileStub } from "@app/types/fileContext";

/**
 * Group files by processing branches - each branch ends in a leaf file
 * Returns Map<fileId, lineagePath[]> where fileId is the leaf and lineagePath is the path back to original
 */
export function groupFilesByOriginal(
  RustlingFileStubs: RustlingFileStub[],
): Map<string, RustlingFileStub[]> {
  const groups = new Map<string, RustlingFileStub[]>();

  // Create a map for quick lookups
  const fileMap = new Map<string, RustlingFileStub>();
  for (const record of RustlingFileStubs) {
    fileMap.set(record.id, record);
  }

  // Find leaf files (files that are not parents of any other files AND have version history)
  // Original files (v0) should only be leaves if they have no processed versions at all
  const leafFiles = RustlingFileStubs.filter((stub) => {
    const isParentOfOthers = RustlingFileStubs.some(
      (otherStub) => otherStub.parentFileId === stub.id,
    );
    const isOriginalOfOthers = RustlingFileStubs.some(
      (otherStub) => otherStub.originalFileId === stub.id,
    );

    // A file is a leaf if:
    // 1. It's not a parent of any other files, AND
    // 2. It has processing history (versionNumber > 0) OR it's not referenced as original by others
    return (
      !isParentOfOthers &&
      ((stub.versionNumber && stub.versionNumber > 0) || !isOriginalOfOthers)
    );
  });

  // For each leaf file, build its complete lineage path back to original
  for (const leafFile of leafFiles) {
    const lineagePath: RustlingFileStub[] = [];
    let currentFile: RustlingFileStub | undefined = leafFile;

    // Trace back through parentFileId chain to build this specific branch
    while (currentFile) {
      lineagePath.push(currentFile);

      // Move to parent file in this branch
      let nextFile: RustlingFileStub | undefined = undefined;

      if (currentFile.parentFileId) {
        nextFile = fileMap.get(currentFile.parentFileId);
      } else if (
        currentFile.originalFileId &&
        currentFile.originalFileId !== currentFile.id
      ) {
        // For v1 files, the original file might be referenced by originalFileId
        nextFile = fileMap.get(currentFile.originalFileId);
      }

      // Check for infinite loops before moving to next
      if (nextFile && lineagePath.some((file) => file.id === nextFile!.id)) {
        break;
      }

      currentFile = nextFile;
    }

    // Sort lineage with latest version first (leaf at top)
    lineagePath.sort((a, b) => (b.versionNumber || 0) - (a.versionNumber || 0));

    // Use leaf file ID as the group key - each branch gets its own group
    groups.set(leafFile.id, lineagePath);
  }

  return groups;
}

/**
 * Check if a file has version history
 */
export function hasVersionHistory(fileStub: RustlingFileStub): boolean {
  return !!(
    fileStub.originalFileId &&
    fileStub.versionNumber &&
    fileStub.versionNumber > 0
  );
}
