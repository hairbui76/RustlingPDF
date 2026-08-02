import { describe, expect, it } from "vitest";
import { createQuickKey } from "@app/types/fileContext";
import {
  peekLocalFilePath,
  rememberLocalFilePath,
  takeLocalFilePath,
} from "@app/services/localFilePathRegistry";

/**
 * Regression guard for a cross-write: file A's bytes landing on file B's path.
 *
 * The registry this replaces was a `Map` keyed by `quickKey`
 * (`name|size|lastModified`). These tests pin the two properties that made
 * that unsafe and that object identity makes unconditional.
 */
describe("localFilePathRegistry", () => {
  it("keeps two colliding files' paths apart", () => {
    // The exact shape that broke: same name, same size, same timestamp — which
    // is what `new File(...)` without an explicit lastModified produces for
    // every file read in one tick.
    const timestamp = 1_700_000_000_000;
    const a = new File([new Uint8Array(8)], "scan.pdf", {
      lastModified: timestamp,
    });
    const b = new File([new Uint8Array(8)], "scan.pdf", {
      lastModified: timestamp,
    });

    // Precondition: these are genuinely indistinguishable by metadata.
    expect(createQuickKey(a)).toBe(createQuickKey(b));
    expect(a).not.toBe(b);

    rememberLocalFilePath(a, "/inbox/scan.pdf");
    rememberLocalFilePath(b, "/archive/scan.pdf");

    // Under the old key-based map the second write won and both files
    // resolved to /archive/scan.pdf — so saving A overwrote B.
    expect(takeLocalFilePath(a)).toBe("/inbox/scan.pdf");
    expect(takeLocalFilePath(b)).toBe("/archive/scan.pdf");
  });

  it("consumes the entry so a later add cannot pick up a stale path", () => {
    const file = new File([new Uint8Array(4)], "a.pdf");
    rememberLocalFilePath(file, "/home/u/a.pdf");

    expect(takeLocalFilePath(file)).toBe("/home/u/a.pdf");
    expect(takeLocalFilePath(file)).toBeUndefined();
    expect(peekLocalFilePath(file)).toBeUndefined();
  });

  it("reports nothing for a file that never came from disk", () => {
    expect(takeLocalFilePath(new File([], "web-upload.pdf"))).toBeUndefined();
  });
});
