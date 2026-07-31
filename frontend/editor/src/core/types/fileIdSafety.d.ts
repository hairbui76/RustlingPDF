/**
 * Type safety declarations to prevent file.name/UUID confusion
 */

import { FileId, RustlingFile } from "@app/types/fileContext";

declare global {
  namespace FileIdSafety {
    // Mark functions that should never accept file.name as parameters
    type SafeFileIdFunction<T extends (...args: any[]) => any> = T extends (
      ...args: infer P
    ) => infer _R
      ? P extends readonly [string, ...any[]]
        ? never // Reject string parameters in first position for FileId functions
        : T
      : T;

    // Mark functions that should only accept RustlingFile, not regular File
    type RustlingFileOnlyFunction<T extends (...args: any[]) => any> =
      T extends (...args: infer P) => infer _R
        ? P extends readonly [File, ...any[]]
          ? never // Reject File parameters in first position for RustlingFile functions
          : T
        : T;

    // Utility type to enforce RustlingFile usage
    type RequireRustlingFile<T> = T extends File ? RustlingFile : T;
  }

  // Extend Window interface for debugging
  interface Window {
    __FILE_ID_DEBUG?: boolean;
  }
}

// Augment FileContext types to prevent bypassing RustlingFile
declare module "../contexts/FileContext" {
  export interface StrictFileContextActions {
    pinFile: (file: RustlingFile) => void; // Must be RustlingFile
    unpinFile: (file: RustlingFile) => void; // Must be RustlingFile
    addFiles: (
      files: File[],
      options?: { insertAfterPageId?: string },
    ) => Promise<RustlingFile[]>; // Returns RustlingFile
    consumeFiles: (
      inputFileIds: FileId[],
      outputFiles: File[],
    ) => Promise<RustlingFile[]>; // Returns RustlingFile
  }

  export interface StrictFileContextSelectors {
    getFile: (id: FileId) => RustlingFile | undefined; // Returns RustlingFile
    getFiles: (ids?: FileId[]) => RustlingFile[]; // Returns RustlingFile[]
    isFilePinned: (file: RustlingFile) => boolean; // Must be RustlingFile
  }
}

export {};
