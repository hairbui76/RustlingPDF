/**
 * Test utilities for creating RustlingFile objects in tests
 */

import { RustlingFile, createRustlingFile } from "@app/types/fileContext";

/**
 * Create a RustlingFile object for testing purposes
 */
export function createTestRustlingFile(
  name: string,
  content: string = "test content",
  type: string = "application/pdf",
): RustlingFile {
  const file = new File([content], name, { type });
  return createRustlingFile(file);
}

/**
 * Create multiple RustlingFile objects for testing
 */
export function createTestFilesWithId(
  files: Array<{ name: string; content?: string; type?: string }>,
): RustlingFile[] {
  return files.map(
    ({ name, content = "test content", type = "application/pdf" }) =>
      createTestRustlingFile(name, content, type),
  );
}
