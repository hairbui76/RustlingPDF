import { describe, expect, it } from "vitest";
import type { FileId, RustlingFileStub } from "@app/types/fileContext";
import {
  addLocalPath,
  buildLocalPathIndex,
  isDuplicateFile,
} from "@app/contexts/file/fileDeduplication";

/**
 * Dropping a file the user explicitly opened is data loss from their point of
 * view, so the interesting cases are all "must NOT be treated as a duplicate".
 */

const COLLIDING_KEY = "report.pdf|2048|1700000000000";

function stub(
  id: string,
  quickKey: string,
  localFilePath?: string,
): RustlingFileStub {
  return { id: id as FileId, quickKey, localFilePath } as RustlingFileStub;
}

describe("isDuplicateFile", () => {
  it("keeps two copies of one document that live at different paths", () => {
    // `cp -p` / `rsync -a` preserve mtime, so these are indistinguishable by
    // name, size and timestamp. Metadata-only dedup silently dropped one.
    const pathIndex = buildLocalPathIndex({
      f1: stub("f1", COLLIDING_KEY, "/work/report.pdf"),
    } as Record<FileId, RustlingFileStub>);

    expect(
      isDuplicateFile({
        quickKey: COLLIDING_KEY,
        localFilePath: "/backup/report.pdf",
        existingQuickKeys: new Set([COLLIDING_KEY]),
        pathIndex,
      }),
    ).toBe(false);
  });

  it("still skips the very same file opened twice", () => {
    const pathIndex = buildLocalPathIndex({
      f1: stub("f1", COLLIDING_KEY, "/work/report.pdf"),
    } as Record<FileId, RustlingFileStub>);

    expect(
      isDuplicateFile({
        quickKey: COLLIDING_KEY,
        localFilePath: "/work/report.pdf",
        existingQuickKeys: new Set([COLLIDING_KEY]),
        pathIndex,
      }),
    ).toBe(true);
  });

  it("falls back to metadata for files with no path (web uploads)", () => {
    expect(
      isDuplicateFile({
        quickKey: COLLIDING_KEY,
        existingQuickKeys: new Set([COLLIDING_KEY]),
        pathIndex: new Map(),
      }),
    ).toBe(true);
  });

  it("is not a duplicate when the metadata key is new", () => {
    expect(
      isDuplicateFile({
        quickKey: "other.pdf|10|1",
        localFilePath: "/work/other.pdf",
        existingQuickKeys: new Set([COLLIDING_KEY]),
        pathIndex: new Map(),
      }),
    ).toBe(false);
  });

  it("keeps a from-disk file when the existing match has no path", () => {
    // The workspace entry came from a browser upload or a tool output; it
    // cannot be shown to be the same file, so the explicit open wins.
    expect(
      isDuplicateFile({
        quickKey: COLLIDING_KEY,
        localFilePath: "/work/report.pdf",
        existingQuickKeys: new Set([COLLIDING_KEY]),
        pathIndex: new Map(),
      }),
    ).toBe(false);
  });

  it("recognises a repeat within the same batch once its path is recorded", () => {
    const pathIndex = new Map<string, Set<string>>();
    const check = (localFilePath: string) =>
      isDuplicateFile({
        quickKey: COLLIDING_KEY,
        localFilePath,
        existingQuickKeys: new Set([COLLIDING_KEY]),
        pathIndex,
      });

    expect(check("/work/report.pdf")).toBe(false);
    addLocalPath(pathIndex, COLLIDING_KEY, "/work/report.pdf");
    // A third copy of the one just added is a genuine duplicate.
    expect(check("/work/report.pdf")).toBe(true);
    expect(check("/backup/report.pdf")).toBe(false);
  });
});
